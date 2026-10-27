//! GitHub and GitHub Enterprise, REST API v3.

use serde::Deserialize;

use super::http::{HttpClient, encode};
use super::{Api, RemoteRepo, Result, SearchScope, UpstreamError, UpstreamProvider};

const HEADERS: &[(&str, &str)] = &[
    ("Accept", "application/vnd.github+json"),
    ("X-GitHub-Api-Version", "2022-11-28"),
];

pub struct GitHub<'a> {
    api: Api<'a>,
}

impl<'a> GitHub<'a> {
    pub fn new(base: String, token: Option<String>, http: &'a dyn HttpClient) -> Self {
        GitHub {
            api: Api {
                base,
                token,
                http,
                headers: HEADERS,
            },
        }
    }
}

#[derive(Deserialize)]
struct User {
    login: String,
}

#[derive(Deserialize)]
struct Repo {
    full_name: String,
    description: Option<String>,
    ssh_url: String,
    clone_url: String,
    default_branch: Option<String>,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    fork: bool,
}

impl From<Repo> for RemoteRepo {
    fn from(r: Repo) -> Self {
        RemoteRepo {
            path: r.full_name,
            description: r.description,
            ssh_url: r.ssh_url,
            https_url: r.clone_url,
            default_branch: r.default_branch,
            archived: r.archived,
            fork: r.fork,
        }
    }
}

#[derive(Deserialize)]
struct SearchResult {
    items: Vec<Repo>,
}

impl UpstreamProvider for GitHub<'_> {
    fn current_user(&self) -> Result<String> {
        if self.api.token.is_none() {
            return Err(UpstreamError::TokenRequired);
        }
        let user: User = self.api.get_json(&self.api.url("/user"))?;
        Ok(user.login)
    }

    fn repository(&self, path: &str) -> Result<RemoteRepo> {
        let encoded: Vec<String> = path.split('/').map(encode).collect();
        let url = self.api.url(&format!("/repos/{}", encoded.join("/")));
        match self.api.get_json::<Repo>(&url) {
            Err(UpstreamError::NotFound(_)) => Err(UpstreamError::NotFound(path.to_string())),
            result => result.map(RemoteRepo::from),
        }
    }

    fn search(&self, query: Option<&str>, scope: SearchScope) -> Result<Vec<RemoteRepo>> {
        match scope {
            SearchScope::Mine => {
                if self.api.token.is_none() {
                    return Err(UpstreamError::TokenRequired);
                }
                let url = self.api.url(
                    "/user/repos?per_page=100&sort=full_name\
                     &affiliation=owner,collaborator,organization_member",
                );
                let repos: Vec<RemoteRepo> = self.api.get_pages(&url, |page: Vec<Repo>| {
                    page.into_iter().map(Into::into).collect()
                })?;
                Ok(repos.into_iter().filter(|r| r.matches(query)).collect())
            }
            SearchScope::All => {
                let query = query.ok_or(UpstreamError::QueryRequired)?;
                let url = self.api.url(&format!(
                    "/search/repositories?per_page=100&q={}",
                    encode(query)
                ));
                self.api.get_pages(&url, |page: SearchResult| {
                    page.items.into_iter().map(Into::into).collect()
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upstream::fake::FakeHttp;

    const API: &str = "https://api.github.com";

    fn repo_json(name: &str) -> String {
        format!(
            r#"{{"full_name":"{name}","description":"About {name}","ssh_url":"git@github.com:{name}.git",
                "clone_url":"https://github.com/{name}.git","default_branch":"main","archived":false,"fork":false}}"#
        )
    }

    #[test]
    fn looks_up_a_repository() {
        let mut http = FakeHttp::default();
        http.add(
            &format!("{API}/repos/pengi/wtm"),
            200,
            &repo_json("pengi/wtm"),
        );
        let github = GitHub::new(API.into(), Some("tok".into()), &http);

        let repo = github.repository("pengi/wtm").unwrap();
        assert_eq!(repo.path, "pengi/wtm");
        assert_eq!(repo.name(), "wtm");
        assert_eq!(repo.ssh_url, "git@github.com:pengi/wtm.git");
        assert_eq!(repo.https_url, "https://github.com/pengi/wtm.git");

        let headers = http.requests.borrow()[0].1.clone();
        assert!(headers.contains(&("Authorization".into(), "Bearer tok".into())));

        assert!(matches!(
            github.repository("pengi/nope"),
            Err(UpstreamError::NotFound(p)) if p == "pengi/nope"
        ));
    }

    #[test]
    fn lists_own_repositories_across_pages() {
        let mut http = FakeHttp::default();
        let first = format!(
            "{API}/user/repos?per_page=100&sort=full_name&affiliation=owner,collaborator,organization_member"
        );
        let second = format!("{API}/user/repos?page=2");
        http.add_paged(&first, &second, &format!("[{}]", repo_json("me/one")));
        http.add(&second, 200, &format!("[{}]", repo_json("org/two")));
        let github = GitHub::new(API.into(), Some("tok".into()), &http);

        let all = github.search(None, SearchScope::Mine).unwrap();
        let paths: Vec<_> = all.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, ["me/one", "org/two"]);

        let filtered = github.search(Some("TWO"), SearchScope::Mine).unwrap();
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn searches_everything() {
        let mut http = FakeHttp::default();
        http.add(
            &format!("{API}/search/repositories?per_page=100&q=git%20tool"),
            200,
            &format!("{{\"items\":[{}]}}", repo_json("x/y")),
        );
        let github = GitHub::new(API.into(), None, &http);
        let found = github.search(Some("git tool"), SearchScope::All).unwrap();
        assert_eq!(found[0].path, "x/y");
        assert!(matches!(
            github.search(None, SearchScope::All),
            Err(UpstreamError::QueryRequired)
        ));
        assert!(matches!(
            github.search(None, SearchScope::Mine),
            Err(UpstreamError::TokenRequired)
        ));
    }

    #[test]
    fn reports_authentication_failures() {
        let mut http = FakeHttp::default();
        http.add(
            &format!("{API}/user"),
            401,
            r#"{"message":"Bad credentials"}"#,
        );
        let github = GitHub::new(API.into(), Some("bad".into()), &http);
        let err = github.current_user().unwrap_err();
        assert!(err.to_string().contains("Bad credentials"), "{err}");
    }
}
