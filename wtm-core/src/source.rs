//! Turning what the user passes to `register` into a remote URL.
//!
//! Plain git URLs are handled by [`UrlResolver`]. Repositories on upstream
//! services (`ACCOUNT:owner/repo`) are resolved through the account's API by
//! the pool.

use crate::config::Account;
use crate::error::Result;

/// A source resolved to something git can fetch from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSource {
    pub url: String,
    /// Repository name to use when the user gives none.
    pub suggested_name: String,
}

pub trait SourceResolver {
    /// Resolve `source`, or return `None` if this resolver doesn't handle it.
    fn resolve(&self, source: &str) -> Result<Option<ResolvedSource>>;
}

/// Accepts anything git accepts as a URL: `https://…`, `ssh://…`,
/// `user@host:path`, `file://…` and local paths.
pub struct UrlResolver;

impl SourceResolver for UrlResolver {
    fn resolve(&self, source: &str) -> Result<Option<ResolvedSource>> {
        Ok(name_from_url(source).map(|suggested_name| ResolvedSource {
            url: source.to_string(),
            suggested_name,
        }))
    }
}

/// Split `ACCOUNT:PATH` into the account and the path, if `ACCOUNT` is the
/// name of a configured account. Anything else is left to be a URL (such as
/// scp-style `host:path`).
pub fn split_account_source<'a, 's>(
    source: &'s str,
    accounts: &'a [Account],
) -> Option<(&'a Account, &'s str)> {
    if source.contains("://") {
        return None;
    }
    let (name, path) = source.split_once(':')?;
    let path = path.trim_matches('/');
    let account = accounts.iter().find(|a| a.name == name)?;
    (!path.is_empty()).then_some((account, path))
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

    #[test]
    fn account_sources() {
        use super::split_account_source;
        use crate::config::{Account, AccountKind, CloneProtocol};
        let accounts =
            [Account::new("gh", AccountKind::GitHub, None, None, CloneProtocol::Ssh).unwrap()];
        let split = |s| split_account_source(s, &accounts).map(|(a, p)| (a.name.clone(), p));
        assert_eq!(split("gh:pengi/wtm"), Some(("gh".into(), "pengi/wtm")));
        assert_eq!(split("gh:wtm"), Some(("gh".into(), "wtm")));
        assert_eq!(split("gh:"), None);
        assert_eq!(split("other:pengi/wtm"), None);
        assert_eq!(split("ssh://gh:22/x"), None);
        assert_eq!(split("/local/path"), None);
    }
}
