//! Names of repositories, worktrees and remotes.

use crate::error::{Error, Result};

/// Check that `name` can be used as a repository, worktree or remote name.
///
/// Names become directory names, so they are restricted to letters, digits,
/// `.`, `_` and `-`, and may not start with `.` or `-`.
pub fn validate(name: &str) -> Result<()> {
    let invalid = |reason| {
        Err(Error::InvalidName {
            name: name.to_string(),
            reason,
        })
    };
    if name.is_empty() {
        return invalid("empty");
    }
    if name.starts_with('.') || name.starts_with('-') {
        return invalid("may not start with '.' or '-'");
    }
    if name.ends_with(".git") || name.ends_with(".lock") {
        return invalid("may not end with '.git' or '.lock'");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return invalid("only letters, digits, '.', '_' and '-' are allowed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate;

    #[test]
    fn accepts_plain_names() {
        for name in ["wtm", "my-repo", "my_repo.rs", "A1"] {
            assert!(validate(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn rejects_bad_names() {
        for name in ["", ".hidden", "-x", "a/b", "a b", "repo.git", "x.lock", "ö"] {
            assert!(validate(name).is_err(), "{name}");
        }
    }
}
