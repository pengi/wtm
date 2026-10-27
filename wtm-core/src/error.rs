use std::path::PathBuf;

use crate::git::GitError;
use crate::model::DropBlocker;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid name '{name}': {reason}")]
    InvalidName { name: String, reason: &'static str },
    #[error("config file {path}: {message}")]
    Config { path: PathBuf, message: String },
    #[error("{0}")]
    InvalidPath(String),
    #[error("cannot tell what repository '{0}' refers to")]
    UnknownSource(String),
    #[error("repository '{0}' is not registered")]
    RepoNotFound(String),
    #[error("repository '{0}' is already registered")]
    RepoExists(String),
    #[error("repository '{repo}' still has worktrees: {}", .worktrees.join(", "))]
    RepoHasWorktrees {
        repo: String,
        worktrees: Vec<String>,
    },
    #[error("repository '{repo}' has branches with commits on no remote: {}", .branches.join(", "))]
    RepoHasUnpushed { repo: String, branches: Vec<String> },
    #[error("repository '{0}' has no remotes")]
    NoRemotes(String),
    #[error("repository '{repo}' has no remote '{remote}'")]
    RemoteNotFound { repo: String, remote: String },
    #[error("repository '{repo}' already has a remote '{remote}'")]
    RemoteExists { repo: String, remote: String },
    #[error("cannot tell the default branch of '{0}'; pass a branch")]
    NoDefaultBranch(String),
    #[error("worktree '{0}' not found")]
    WorktreeNotFound(String),
    #[error("'{name}' matches several worktrees: {}", .paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "))]
    AmbiguousWorktree { name: String, paths: Vec<PathBuf> },
    #[error("{} is not the top directory of a git clone", .0.display())]
    NotAClone(PathBuf),
    #[error("cannot adopt {}: {reason}", .path.display())]
    CannotAdopt { path: PathBuf, reason: String },
    #[error("{0} already exists")]
    PathExists(PathBuf),
    #[error("branch '{branch}' is already checked out in {}", .path.display())]
    BranchCheckedOut { branch: String, path: PathBuf },
    #[error("branch '{0}' already exists; a start point only applies to new branches")]
    BranchExists(String),
    #[error("'{0}' is not a commit")]
    UnknownRevision(String),
    #[error("worktree '{name}' is not clean")]
    NotClean {
        name: String,
        blockers: Vec<DropBlocker>,
    },
    #[error("{}: {error}", .path.display())]
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Upstream(#[from] crate::upstream::UpstreamError),
    #[error("no account '{0}'; see `wtm auth list`")]
    AccountNotFound(String),
    #[error("account '{0}' already exists")]
    AccountExists(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Error {
        let path = path.into();
        move |error| Error::Io { path, error }
    }
}
