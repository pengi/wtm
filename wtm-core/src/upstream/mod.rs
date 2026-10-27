//! Upstream services (GitHub, GitLab): looking up and searching
//! repositories through their REST APIs.

mod github;
mod gitlab;
pub mod http;

use serde::de::DeserializeOwned;

use crate::config::{Account, AccountKind};
use http::{HttpClient, Response};

pub use github::GitHub;
pub use gitlab::GitLab;

/// At most this many pages are fetched for a listing.
const MAX_PAGES: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum UpstreamError {
    #[error("could not run curl: {0}")]
    Spawn(std::io::Error),
    #[error("{url}: {message}")]
    Transport { url: String, message: String },
    #[error("authentication failed (HTTP {status}): {message}")]
    Unauthorized { status: u16, message: String },
    #[error("not found on the server: {0}")]
    NotFound(String),
    #[error("{url}: HTTP {status}: {message}")]
    Status {
        url: String,
        status: u16,
        message: String,
    },
    #[error("{url}: unexpected response: {message}")]
    Parse { url: String, message: String },
    #[error("searching all repositories needs a query")]
    QueryRequired,
    #[error("a token is needed for this; add one with `wtm auth add`")]
    TokenRequired,
}

pub type Result<T> = std::result::Result<T, UpstreamError>;

/// A repository on an upstream service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRepo {
    /// Full path, e.g. `owner/repo` or `group/subgroup/repo`.
    pub path: String,
    pub description: Option<String>,
    pub ssh_url: String,
    pub https_url: String,
    pub default_branch: Option<String>,
    pub archived: bool,
    pub fork: bool,
}

impl RemoteRepo {
    /// The last component of the path.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    fn matches(&self, query: Option<&str>) -> bool {
        let Some(query) = query else { return true };
        let query = query.to_lowercase();
        self.path.to_lowercase().contains(&query)
            || self
                .description
                .as_deref()
                .is_some_and(|d| d.to_lowercase().contains(&query))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchScope {
    /// Repositories the user owns or is a member of.
    Mine,
    /// All repositories visible to the user.
    All,
}

pub trait UpstreamProvider {
    /// The user name the token belongs to.
    fn current_user(&self) -> Result<String>;
    /// Look up a repository by its full path.
    fn repository(&self, path: &str) -> Result<RemoteRepo>;
    fn search(&self, query: Option<&str>, scope: SearchScope) -> Result<Vec<RemoteRepo>>;
}

/// The provider for an account's service.
pub fn provider<'a>(
    account: &Account,
    token: Option<String>,
    http: &'a dyn HttpClient,
) -> Box<dyn UpstreamProvider + 'a> {
    let api = account.api_url();
    match account.kind {
        AccountKind::GitHub => Box::new(GitHub::new(api, token, http)),
        AccountKind::GitLab => Box::new(GitLab::new(api, token, http)),
    }
}

/// Shared request handling: authentication header, status codes, JSON.
struct Api<'a> {
    base: String,
    token: Option<String>,
    http: &'a dyn HttpClient,
    /// Headers sent with every request.
    headers: &'static [(&'static str, &'static str)],
}

impl Api<'_> {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn get(&self, url: &str) -> Result<Response> {
        let auth = self.token.as_ref().map(|t| format!("Bearer {t}"));
        let mut headers: Vec<(&str, &str)> = self.headers.to_vec();
        if let Some(auth) = &auth {
            headers.push(("Authorization", auth));
        }
        let response = self.http.get(url, &headers)?;
        let message = || {
            serde_json::from_slice::<serde_json::Value>(&response.body)
                .ok()
                .and_then(|v| {
                    v.get("message")
                        .map(|m| m.to_string().trim_matches('"').to_string())
                })
                .unwrap_or_default()
        };
        match response.status {
            200..=299 => Ok(response),
            401 => Err(UpstreamError::Unauthorized {
                status: 401,
                message: message(),
            }),
            404 => Err(UpstreamError::NotFound(url.to_string())),
            status => Err(UpstreamError::Status {
                url: url.to_string(),
                status,
                message: message(),
            }),
        }
    }

    fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let response = self.get(url)?;
        parse(url, &response)
    }

    /// Follow `Link: rel="next"` pages, collecting what `items` extracts
    /// from each.
    fn get_pages<P: DeserializeOwned, T>(
        &self,
        url: &str,
        items: impl Fn(P) -> Vec<T>,
    ) -> Result<Vec<T>> {
        let mut all = Vec::new();
        let mut next = Some(url.to_string());
        for _ in 0..MAX_PAGES {
            let Some(url) = next.take() else { break };
            let response = self.get(&url)?;
            all.extend(items(parse(&url, &response)?));
            next = response.next_link();
        }
        Ok(all)
    }
}

fn parse<T: DeserializeOwned>(url: &str, response: &Response) -> Result<T> {
    serde_json::from_slice(&response.body).map_err(|e| UpstreamError::Parse {
        url: url.to_string(),
        message: e.to_string(),
    })
}

#[cfg(test)]
pub(crate) mod fake {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::UpstreamError;
    use super::http::{HttpClient, Response};

    type Headers = Vec<(String, String)>;

    /// Serves canned responses by URL and records the requests.
    #[derive(Default)]
    pub struct FakeHttp {
        /// URL to (status, headers, body).
        pub responses: HashMap<String, (u16, Headers, String)>,
        /// (URL, headers) of each request.
        pub requests: RefCell<Vec<(String, Headers)>>,
    }

    impl FakeHttp {
        pub fn add(&mut self, url: &str, status: u16, body: &str) {
            self.responses
                .insert(url.to_string(), (status, Vec::new(), body.to_string()));
        }

        pub fn add_paged(&mut self, url: &str, next: &str, body: &str) {
            let link = format!("<{next}>; rel=\"next\"");
            self.responses.insert(
                url.to_string(),
                (200, vec![("link".into(), link)], body.to_string()),
            );
        }
    }

    impl HttpClient for FakeHttp {
        fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, UpstreamError> {
            self.requests.borrow_mut().push((
                url.to_string(),
                headers
                    .iter()
                    .map(|(n, v)| (n.to_string(), v.to_string()))
                    .collect(),
            ));
            let (status, headers, body) = self.responses.get(url).cloned().unwrap_or((
                404,
                Vec::new(),
                "{\"message\":\"Not Found\"}".into(),
            ));
            Ok(Response {
                status,
                headers,
                body: body.into_bytes(),
            })
        }
    }
}
