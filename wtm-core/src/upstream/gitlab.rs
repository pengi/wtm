//! GitLab, REST API v4.

use serde::Deserialize;

use super::http::{HttpClient, encode};
use super::{Api, RemoteRepo, Result, SearchScope, UpstreamError, UpstreamProvider};

pub struct GitLab<'a> {
    api: Api<'a>,
}

impl<'a> GitLab<'a> {
    pub fn new(base: String, token: Option<String>, http: &'a dyn HttpClient) -> Self {
        GitLab {
            api: Api {
                base,
                token,
                http,
                headers: &[("Accept", "application/json")],
            },
        }
    }
}

#[derive(Deserialize)]
struct User {
    username: String,
}

#[derive(Deserialize)]
struct Project {
    path_with_namespace: String,
    description: Option<String>,
    ssh_url_to_repo: String,
    http_url_to_repo: String,
    default_branch: Option<String>,
    #[serde(default)]
    archived: bool,
    forked_from_project: Option<serde_json::Value>,
}

impl From<Project> for RemoteRepo {
    fn from(p: Project) -> Self {
        RemoteRepo {
            path: p.path_with_namespace,
            description: p.description.filter(|d| !d.is_empty()),
            ssh_url: p.ssh_url_to_repo,
            https_url: p.http_url_to_repo,
            default_branch: p.default_branch,
            archived: p.archived,
            fork: p.forked_from_project.is_some(),
        }
    }
}

impl UpstreamProvider for GitLab<'_> {
    fn current_user(&self) -> Result<String> {
        if self.api.token.is_none() {
            return Err(UpstreamError::TokenRequired);
        }
        let user: User = self.api.get_json(&self.api.url("/user"))?;
        Ok(user.username)
    }

    fn repository(&self, path: &str) -> Result<RemoteRepo> {
        let url = self.api.url(&format!("/projects/{}", encode(path)));
        match self.api.get_json::<Project>(&url) {
            Err(UpstreamError::NotFound(_)) => Err(UpstreamError::NotFound(path.to_string())),
            result => result.map(RemoteRepo::from),
        }
    }

    fn search(&self, query: Option<&str>, scope: SearchScope) -> Result<Vec<RemoteRepo>> {
        let mut url = self
            .api
            .url("/projects?per_page=100&order_by=path&sort=asc");
        match scope {
            SearchScope::Mine => {
                if self.api.token.is_none() {
                    return Err(UpstreamError::TokenRequired);
                }
                url.push_str("&membership=true");
            }
            SearchScope::All if query.is_none() => return Err(UpstreamError::QueryRequired),
            SearchScope::All => {}
        }
        if let Some(query) = query {
            url.push_str(&format!("&search_namespaces=true&search={}", encode(query)));
        }
        self.api.get_pages(&url, |page: Vec<Project>| {
            page.into_iter().map(Into::into).collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upstream::fake::FakeHttp;

    const API: &str = "https://git.example.com/api/v4";

    fn project_json(path: &str) -> String {
        format!(
            r#"{{"path_with_namespace":"{path}","description":"","ssh_url_to_repo":"git@git.example.com:{path}.git",
                "http_url_to_repo":"https://git.example.com/{path}.git","default_branch":"main","archived":true}}"#
        )
    }

    #[test]
    fn looks_up_a_project_in_a_subgroup() {
        let mut http = FakeHttp::default();
        http.add(
            &format!("{API}/projects/group%2Fsub%2Fproj"),
            200,
            &project_json("group/sub/proj"),
        );
        let gitlab = GitLab::new(API.into(), Some("tok".into()), &http);
        let repo = gitlab.repository("group/sub/proj").unwrap();
        assert_eq!(repo.name(), "proj");
        assert_eq!(repo.description, None);
        assert!(repo.archived);
        assert!(!repo.fork);
        assert_eq!(repo.ssh_url, "git@git.example.com:group/sub/proj.git");
    }

    #[test]
    fn searches_memberships() {
        let mut http = FakeHttp::default();
        http.add(
            &format!(
                "{API}/projects?per_page=100&order_by=path&sort=asc&membership=true\
                 &search_namespaces=true&search=proj"
            ),
            200,
            &format!("[{}]", project_json("me/proj")),
        );
        let gitlab = GitLab::new(API.into(), Some("tok".into()), &http);
        let found = gitlab.search(Some("proj"), SearchScope::Mine).unwrap();
        assert_eq!(found[0].path, "me/proj");
        assert!(matches!(
            gitlab.search(None, SearchScope::All),
            Err(UpstreamError::QueryRequired)
        ));
    }
}
