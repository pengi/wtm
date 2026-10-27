//! Turning what the user passes to `register` into a remote URL.
//!
//! Plain git URLs are handled by [`UrlResolver`]. Upstream services (e.g.
//! `github:owner/repo`) will add their own resolvers.

/// A source resolved to something git can fetch from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSource {
    pub url: String,
    /// Repository name to use when the user gives none.
    pub suggested_name: String,
}

pub trait SourceResolver {
    /// Resolve `source`, or return `None` if this resolver doesn't handle it.
    fn resolve(&self, source: &str) -> Option<ResolvedSource>;
}

/// Accepts anything git accepts as a URL: `https://…`, `ssh://…`,
/// `user@host:path`, `file://…` and local paths.
pub struct UrlResolver;

impl SourceResolver for UrlResolver {
    fn resolve(&self, source: &str) -> Option<ResolvedSource> {
        let suggested_name = name_from_url(source)?;
        Some(ResolvedSource {
            url: source.to_string(),
            suggested_name,
        })
    }
}

/// The last path component of a URL, without `.git`.
pub fn name_from_url(url: &str) -> Option<String> {
    let trimmed = url.trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::name_from_url;

    #[test]
    fn names_from_urls() {
        let cases = [
            ("git@github.com:pengi/wtm.git", "wtm"),
            ("https://github.com/pengi/wtm", "wtm"),
            ("https://github.com/pengi/wtm/", "wtm"),
            ("ssh://git@host:2222/group/sub/project.git", "project"),
            ("/srv/git/thing.git", "thing"),
            ("host:repo", "repo"),
        ];
        for (url, name) in cases {
            assert_eq!(name_from_url(url).as_deref(), Some(name), "{url}");
        }
        assert_eq!(name_from_url("https://host/.git"), None);
    }
}
