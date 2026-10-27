//! The git backend: the only place where wtm talks to git.
//!
//! Everything outside this module goes through [`GitBackend`]. No git2 types
//! appear in its signatures, so the implementation can change (or be
//! replaced) without touching the rest of wtm. `scripts/check-git-boundary.sh`
//! enforces that git2 and the git binary are not used elsewhere.
//!
//! [`DefaultBackend`] runs the `git` command for network and mutating
//! operations, so the user's ssh config, credential helpers, hooks and
//! filters (git-lfs) apply. It uses libgit2 for read-only queries.

mod adopt;
mod cli;
mod libgit;

use std::fmt;
use std::path::{Path, PathBuf};

pub use cli::{GitCli, GitOutput};

/// Errors from the git backend.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("could not run git: {0}")]
    Spawn(std::io::Error),
    #[error("git {version} is too old, wtm needs git {minimum} or newer")]
    TooOld { version: String, minimum: String },
    #[error("`git {command}` failed{}", fmt_stderr(.stderr))]
    Command { command: String, stderr: String },
    #[error("{0}")]
    Library(String),
}

fn fmt_stderr(stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        String::new()
    } else {
        format!(":\n{stderr}")
    }
}

pub type Result<T> = std::result::Result<T, GitError>;

/// A commit id, as a hex string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Oid(pub String);

impl Oid {
    /// The abbreviated form, for display.
    pub fn short(&self) -> &str {
        &self.0[..self.0.len().min(10)]
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteInfo {
    pub name: String,
    pub url: Option<String>,
    pub push_url: Option<String>,
}

/// What HEAD of a working tree points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    Branch(String),
    Detached(Oid),
    /// A branch without any commits yet.
    Unborn(String),
}

impl Head {
    pub fn branch(&self) -> Option<&str> {
        match self {
            Head::Branch(b) | Head::Unborn(b) => Some(b),
            Head::Detached(_) => None,
        }
    }
}

/// A linked worktree, as known by the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    /// Git's administrative name (the directory under `worktrees/`).
    pub admin_name: String,
    pub path: PathBuf,
    /// None when the working tree is missing.
    pub head: Option<Head>,
    pub locked: bool,
    /// The working tree directory no longer exists.
    pub missing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamInfo {
    /// Short name of the upstream, e.g. `origin/main`.
    pub name: String,
    /// None if the upstream is configured but does not exist.
    pub tip: Option<Oid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: String,
    pub tip: Oid,
    pub upstream: Option<UpstreamInfo>,
}

/// An operation in progress in a working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
    ApplyMailbox,
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Operation::Merge => "merge",
            Operation::Rebase => "rebase",
            Operation::CherryPick => "cherry-pick",
            Operation::Revert => "revert",
            Operation::Bisect => "bisect",
            Operation::ApplyMailbox => "am",
        })
    }
}

/// Summary of a working tree's status. Ignored files are not counted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusSummary {
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicted: usize,
    pub operation: Option<Operation>,
}

impl StatusSummary {
    pub fn is_clean(&self) -> bool {
        *self == StatusSummary::default()
    }
}

/// How `add_worktree` gets its branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checkout {
    /// Check out an existing local branch.
    Existing { branch: String },
    /// Create a local branch tracking a remote-tracking branch, e.g.
    /// `origin/feature`.
    Track { branch: String, upstream: String },
    /// Create a new branch at `start`, without upstream.
    New { branch: String, start: String },
}

impl Checkout {
    pub fn branch(&self) -> &str {
        match self {
            Checkout::Existing { branch }
            | Checkout::Track { branch, .. }
            | Checkout::New { branch, .. } => branch,
        }
    }
}

/// A credential as stored by git's credential helpers.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential {
    /// The service it is for, e.g. `https://github.com`.
    pub url: String,
    pub username: Option<String>,
    pub password: String,
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credential")
            .field("url", &self.url)
            .field("username", &self.username)
            .field("password", &"<hidden>")
            .finish()
    }
}

/// A normal clone: a working directory with a `.git` directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneInfo {
    pub head: Head,
    pub operation: Option<Operation>,
    /// Number of linked worktrees the clone already has.
    pub linked_worktrees: usize,
    pub submodules: usize,
    /// `core.worktree` is set, so the working directory is elsewhere.
    pub custom_worktree: bool,
}

/// All git access used by wtm.
///
/// `git_dir` is always the path of a bare repository in the pool.
pub trait GitBackend {
    // Repositories

    /// Create an empty bare repository.
    fn init_bare(&self, git_dir: &Path) -> Result<()>;
    /// Whether `path` is a git repository directory.
    fn is_repository(&self, path: &Path) -> bool;
    /// The branch HEAD points at, if any.
    fn head_branch(&self, git_dir: &Path) -> Result<Option<String>>;
    /// Point HEAD at a branch.
    fn set_head_branch(&self, git_dir: &Path, branch: &str) -> Result<()>;
    fn config_get(&self, git_dir: &Path, key: &str) -> Result<Option<String>>;
    fn config_set(&self, git_dir: &Path, key: &str, value: &str) -> Result<()>;
    fn config_unset(&self, git_dir: &Path, key: &str) -> Result<()>;

    // Remotes

    fn remotes(&self, git_dir: &Path) -> Result<Vec<RemoteInfo>>;
    /// Add a remote with the standard fetch refspec
    /// `+refs/heads/*:refs/remotes/<name>/*`.
    fn add_remote(&self, git_dir: &Path, name: &str, url: &str) -> Result<()>;
    fn remove_remote(&self, git_dir: &Path, name: &str) -> Result<()>;
    /// Fetch a remote, pruning deleted branches.
    fn fetch(&self, git_dir: &Path, remote: &str) -> Result<()>;
    /// Ask the remote for its default branch and record it locally
    /// (`refs/remotes/<remote>/HEAD`).
    fn update_remote_head(&self, git_dir: &Path, remote: &str) -> Result<()>;
    /// The recorded default branch of a remote, without the remote prefix.
    fn remote_head(&self, git_dir: &Path, remote: &str) -> Result<Option<String>>;

    // Branches and commits

    fn local_branches(&self, git_dir: &Path) -> Result<Vec<BranchInfo>>;
    fn local_branch(&self, git_dir: &Path, name: &str) -> Result<Option<BranchInfo>>;
    /// Whether a remote-tracking branch such as `origin/main` exists.
    fn remote_branch_exists(&self, git_dir: &Path, name: &str) -> Result<bool>;
    /// Resolve a revision to a commit.
    fn resolve_commit(&self, git_dir: &Path, rev: &str) -> Result<Option<Oid>>;
    /// Delete a local branch, whether merged or not.
    fn delete_branch(&self, git_dir: &Path, name: &str) -> Result<()>;
    /// The number of commits reachable from `commit` but not from any
    /// remote-tracking branch.
    fn commits_not_on_remotes(&self, git_dir: &Path, commit: &Oid) -> Result<usize>;
    /// Like `commits_not_on_remotes`, but also counts commits on local
    /// branches as safe.
    fn commits_not_on_any_branch(&self, git_dir: &Path, commit: &Oid) -> Result<usize>;
    /// (ahead, behind) of `local` relative to `upstream`.
    fn ahead_behind(&self, git_dir: &Path, local: &Oid, upstream: &Oid) -> Result<(usize, usize)>;

    // Worktrees

    fn worktrees(&self, git_dir: &Path) -> Result<Vec<WorktreeInfo>>;
    fn add_worktree(&self, git_dir: &Path, path: &Path, checkout: &Checkout) -> Result<()>;
    /// Remove a worktree. Without `force`, git refuses if anything would be
    /// lost; with `force`, it removes even locked and dirty worktrees.
    fn remove_worktree(&self, git_dir: &Path, path: &Path, force: bool) -> Result<()>;
    /// Remove administrative data for worktrees whose directory is gone.
    fn prune_worktrees(&self, git_dir: &Path) -> Result<()>;
    /// Re-link worktrees after the repository or worktrees were moved.
    fn repair_worktrees(&self, git_dir: &Path, paths: &[PathBuf]) -> Result<()>;
    /// The repository a worktree directory links to, read from its `.git`
    /// file. Returns the path as written, even if it no longer exists.
    fn worktree_link(&self, path: &Path) -> Option<PathBuf>;
    fn worktree_status(&self, path: &Path) -> Result<StatusSummary>;

    // Adopting existing clones

    /// Inspect `path` as the top directory of a normal clone. None if it is
    /// not one (no repository, a bare repository, or a linked worktree).
    fn inspect_clone(&self, path: &Path) -> Result<Option<CloneInfo>>;
    /// Turn the normal clone at `path` into the bare repository `git_dir`,
    /// with `path` as a linked worktree of it checked out at `head`. The
    /// index, and with it staged changes, is kept. On failure, the clone is
    /// restored.
    fn convert_to_worktree(&self, path: &Path, git_dir: &Path, head: &Head) -> Result<()>;

    // Credentials, kept by the user's git credential helpers

    /// The stored credential for `url` (and `username`, if given). Never
    /// prompts; None if no helper has one.
    fn credential_fill(&self, url: &str, username: Option<&str>) -> Result<Option<Credential>>;
    /// Store a credential with the credential helpers.
    fn credential_approve(&self, credential: &Credential) -> Result<()>;
    /// Remove a credential from the credential helpers.
    fn credential_reject(&self, credential: &Credential) -> Result<()>;
}

/// The standard backend: git CLI for changes and network, libgit2 for reads.
pub struct DefaultBackend {
    cli: GitCli,
}

impl DefaultBackend {
    /// Check that a usable `git` is installed and create the backend.
    pub fn new(output: GitOutput) -> Result<Self> {
        let cli = GitCli::new(output);
        cli.check_version()?;
        Ok(DefaultBackend { cli })
    }
}

impl GitBackend for DefaultBackend {
    fn init_bare(&self, git_dir: &Path) -> Result<()> {
        self.cli.init_bare(git_dir)
    }

    fn is_repository(&self, path: &Path) -> bool {
        libgit::is_repository(path)
    }

    fn head_branch(&self, git_dir: &Path) -> Result<Option<String>> {
        libgit::head_branch(git_dir)
    }

    fn set_head_branch(&self, git_dir: &Path, branch: &str) -> Result<()> {
        self.cli.set_head_branch(git_dir, branch)
    }

    fn config_get(&self, git_dir: &Path, key: &str) -> Result<Option<String>> {
        libgit::config_get(git_dir, key)
    }

    fn config_set(&self, git_dir: &Path, key: &str, value: &str) -> Result<()> {
        self.cli.config_set(git_dir, key, value)
    }

    fn config_unset(&self, git_dir: &Path, key: &str) -> Result<()> {
        self.cli.config_unset(git_dir, key)
    }

    fn remotes(&self, git_dir: &Path) -> Result<Vec<RemoteInfo>> {
        libgit::remotes(git_dir)
    }

    fn add_remote(&self, git_dir: &Path, name: &str, url: &str) -> Result<()> {
        self.cli.add_remote(git_dir, name, url)
    }

    fn remove_remote(&self, git_dir: &Path, name: &str) -> Result<()> {
        self.cli.remove_remote(git_dir, name)
    }

    fn fetch(&self, git_dir: &Path, remote: &str) -> Result<()> {
        self.cli.fetch(git_dir, remote)
    }

    fn update_remote_head(&self, git_dir: &Path, remote: &str) -> Result<()> {
        self.cli.update_remote_head(git_dir, remote)
    }

    fn remote_head(&self, git_dir: &Path, remote: &str) -> Result<Option<String>> {
        libgit::remote_head(git_dir, remote)
    }

    fn local_branches(&self, git_dir: &Path) -> Result<Vec<BranchInfo>> {
        libgit::local_branches(git_dir)
    }

    fn local_branch(&self, git_dir: &Path, name: &str) -> Result<Option<BranchInfo>> {
        libgit::local_branch(git_dir, name)
    }

    fn remote_branch_exists(&self, git_dir: &Path, name: &str) -> Result<bool> {
        libgit::remote_branch_exists(git_dir, name)
    }

    fn resolve_commit(&self, git_dir: &Path, rev: &str) -> Result<Option<Oid>> {
        libgit::resolve_commit(git_dir, rev)
    }

    fn delete_branch(&self, git_dir: &Path, name: &str) -> Result<()> {
        self.cli.delete_branch(git_dir, name)
    }

    fn commits_not_on_remotes(&self, git_dir: &Path, commit: &Oid) -> Result<usize> {
        libgit::commits_not_in(git_dir, commit, false)
    }

    fn commits_not_on_any_branch(&self, git_dir: &Path, commit: &Oid) -> Result<usize> {
        libgit::commits_not_in(git_dir, commit, true)
    }

    fn ahead_behind(&self, git_dir: &Path, local: &Oid, upstream: &Oid) -> Result<(usize, usize)> {
        libgit::ahead_behind(git_dir, local, upstream)
    }

    fn worktrees(&self, git_dir: &Path) -> Result<Vec<WorktreeInfo>> {
        libgit::worktrees(git_dir)
    }

    fn add_worktree(&self, git_dir: &Path, path: &Path, checkout: &Checkout) -> Result<()> {
        self.cli.add_worktree(git_dir, path, checkout)
    }

    fn remove_worktree(&self, git_dir: &Path, path: &Path, force: bool) -> Result<()> {
        self.cli.remove_worktree(git_dir, path, force)
    }

    fn prune_worktrees(&self, git_dir: &Path) -> Result<()> {
        self.cli.prune_worktrees(git_dir)
    }

    fn repair_worktrees(&self, git_dir: &Path, paths: &[PathBuf]) -> Result<()> {
        self.cli.repair_worktrees(git_dir, paths)
    }

    fn worktree_link(&self, path: &Path) -> Option<PathBuf> {
        worktree_link(path)
    }

    fn worktree_status(&self, path: &Path) -> Result<StatusSummary> {
        libgit::worktree_status(path)
    }

    fn inspect_clone(&self, path: &Path) -> Result<Option<CloneInfo>> {
        libgit::inspect_clone(path)
    }

    fn convert_to_worktree(&self, path: &Path, git_dir: &Path, head: &Head) -> Result<()> {
        adopt::convert_to_worktree(&self.cli, path, git_dir, head)
    }

    fn credential_fill(&self, url: &str, username: Option<&str>) -> Result<Option<Credential>> {
        self.cli.credential_fill(url, username)
    }

    fn credential_approve(&self, credential: &Credential) -> Result<()> {
        self.cli.credential_store("approve", credential)
    }

    fn credential_reject(&self, credential: &Credential) -> Result<()> {
        self.cli.credential_store("reject", credential)
    }
}

/// Read the `gitdir:` line of a worktree's `.git` file.
fn worktree_link(path: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(path.join(".git")).ok()?;
    let target = content.strip_prefix("gitdir:")?.trim();
    let target = PathBuf::from(target);
    Some(if target.is_relative() {
        path.join(target)
    } else {
        target
    })
}
