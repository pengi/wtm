//! The things in a pool.

use std::fmt;
use std::path::PathBuf;

use crate::git::{Checkout, Head, Operation, RemoteInfo, StatusSummary, UpstreamInfo};

/// A registered repository: a bare repository in the repos directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub name: String,
    pub git_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RepoDetails {
    pub repo: Repo,
    pub remotes: Vec<RemoteInfo>,
    pub default_remote: Option<String>,
    pub default_branch: Option<String>,
}

/// A worktree of a registered repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// The directory name.
    pub name: String,
    pub path: PathBuf,
    /// Name of the repository it belongs to.
    pub repo: String,
    pub git_dir: PathBuf,
    /// None if the directory is missing.
    pub head: Option<Head>,
    pub locked: bool,
    /// The directory no longer exists.
    pub missing: bool,
    /// Whether it lives in the checkouts directory. Worktrees created with
    /// plain `git worktree add` may live elsewhere.
    pub in_checkouts_dir: bool,
}

/// Status of a worktree, for listing.
#[derive(Debug, Clone, Default)]
pub struct WorktreeState {
    /// None if the directory is missing.
    pub status: Option<StatusSummary>,
    pub upstream: Option<UpstreamInfo>,
    /// (ahead, behind) relative to the upstream.
    pub ahead_behind: Option<(usize, usize)>,
    /// Commits on the checked out branch that are on no remote.
    pub not_on_remotes: Option<usize>,
}

/// Options for creating a worktree.
#[derive(Debug, Clone, Default)]
pub struct NewWorktree {
    pub repo: String,
    /// Worktree name; defaults to the repository name.
    pub name: Option<String>,
    /// Branch; defaults to `name`, or the default branch if `name` is not
    /// given either.
    pub branch: Option<String>,
    /// Start point for a new branch.
    pub from: Option<String>,
    /// Remote to fetch from and track; defaults to the default remote.
    pub remote: Option<String>,
    pub fetch: bool,
}

/// A worktree that was just created.
#[derive(Debug)]
pub struct Created {
    pub name: String,
    pub path: PathBuf,
    pub checkout: Checkout,
    /// Fetching failed, and the worktree was created from what was already
    /// fetched.
    pub fetch_error: Option<crate::Error>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DropOptions {
    /// Keep the local branch.
    pub keep_branch: bool,
    /// Drop even if something would be lost.
    pub force: bool,
    /// Fetch before checking what is pushed.
    pub fetch: bool,
}

#[derive(Debug)]
pub struct Dropped {
    pub worktree: Worktree,
    pub deleted_branch: Option<String>,
}

/// Why a worktree can't be dropped without losing something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropBlocker {
    Locked,
    Uncommitted {
        staged: usize,
        unstaged: usize,
        conflicted: usize,
    },
    Untracked(usize),
    InProgress(Operation),
    Unpushed {
        branch: String,
        commits: usize,
    },
    DetachedCommits(usize),
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}

impl fmt::Display for DropBlocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DropBlocker::Locked => f.write_str("the worktree is locked"),
            DropBlocker::Uncommitted {
                staged,
                unstaged,
                conflicted,
            } => {
                let parts: Vec<String> = [
                    (*staged, "staged"),
                    (*unstaged, "unstaged"),
                    (*conflicted, "conflicted"),
                ]
                .into_iter()
                .filter(|(n, _)| *n > 0)
                .map(|(n, what)| format!("{n} {what}"))
                .collect();
                write!(f, "uncommitted changes ({})", parts.join(", "))
            }
            DropBlocker::Untracked(n) => write!(f, "{} not ignored", plural(*n, "untracked file")),
            DropBlocker::InProgress(op) => write!(f, "a {op} is in progress"),
            DropBlocker::Unpushed { branch, commits } => write!(
                f,
                "branch '{branch}' has {} on no remote",
                plural(*commits, "commit")
            ),
            DropBlocker::DetachedCommits(n) => {
                write!(f, "detached HEAD has {} on no branch", plural(*n, "commit"))
            }
        }
    }
}
