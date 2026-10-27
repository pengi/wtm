//! Read-only queries through libgit2.

use std::path::{Path, PathBuf};

use git2::{BranchType, ErrorCode, Repository, RepositoryState, Status, StatusOptions};

use super::{
    BranchInfo, CloneInfo, GitError, Head, Oid, Operation, RemoteInfo, Result, StatusSummary,
    UpstreamInfo, WorktreeInfo,
};

impl From<git2::Error> for GitError {
    fn from(e: git2::Error) -> Self {
        GitError::Library(e.message().to_string())
    }
}

fn open(git_dir: &Path) -> Result<Repository> {
    Ok(Repository::open_bare(git_dir)?)
}

fn not_found(e: &git2::Error) -> bool {
    matches!(e.code(), ErrorCode::NotFound | ErrorCode::InvalidSpec)
}

/// Turn a "not found" error into `None`.
fn optional<T>(r: std::result::Result<T, git2::Error>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if not_found(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn oid(id: git2::Oid) -> Oid {
    Oid(id.to_string())
}

fn git_oid(id: &Oid) -> Result<git2::Oid> {
    Ok(git2::Oid::from_str(&id.0)?)
}

pub fn is_repository(path: &Path) -> bool {
    Repository::open_bare(path).is_ok()
}

pub fn head_branch(git_dir: &Path) -> Result<Option<String>> {
    let repo = open(git_dir)?;
    let head = repo.find_reference("HEAD")?;
    Ok(head
        .symbolic_target()?
        .and_then(|t| t.strip_prefix("refs/heads/"))
        .map(str::to_string))
}

pub fn config_get(git_dir: &Path, key: &str) -> Result<Option<String>> {
    let config = open(git_dir)?.config()?.snapshot()?;
    optional(config.get_string(key))
}

pub fn remotes(git_dir: &Path) -> Result<Vec<RemoteInfo>> {
    let repo = open(git_dir)?;
    let names = repo.remotes()?;
    let mut remotes = Vec::new();
    for name in names.iter() {
        let Some(name) = name? else { continue };
        let remote = repo.find_remote(name)?;
        remotes.push(RemoteInfo {
            name: name.to_string(),
            url: remote.url().ok().map(str::to_string),
            push_url: remote.pushurl().ok().flatten().map(str::to_string),
        });
    }
    Ok(remotes)
}

pub fn remote_head(git_dir: &Path, remote: &str) -> Result<Option<String>> {
    let repo = open(git_dir)?;
    let Some(head) = optional(repo.find_reference(&format!("refs/remotes/{remote}/HEAD")))? else {
        return Ok(None);
    };
    let prefix = format!("refs/remotes/{remote}/");
    Ok(head
        .symbolic_target()?
        .and_then(|t| t.strip_prefix(prefix.as_str()))
        .map(str::to_string))
}

fn branch_info(repo: &Repository, branch: &git2::Branch) -> Result<Option<BranchInfo>> {
    let reference = branch.get();
    let (Ok(name), Ok(refname)) = (reference.shorthand(), reference.name()) else {
        return Ok(None);
    };
    let tip = oid(reference.peel_to_commit()?.id());
    let upstream = match optional(repo.branch_upstream_name(refname))? {
        None => None,
        Some(buf) => {
            let full = buf.as_str().unwrap_or_default().to_string();
            let tip = optional(repo.refname_to_id(&full))?.map(oid);
            let name = full
                .strip_prefix("refs/remotes/")
                .or_else(|| full.strip_prefix("refs/heads/"))
                .unwrap_or(&full)
                .to_string();
            Some(UpstreamInfo { name, tip })
        }
    };
    Ok(Some(BranchInfo {
        name: name.to_string(),
        tip,
        upstream,
    }))
}

pub fn local_branches(git_dir: &Path) -> Result<Vec<BranchInfo>> {
    let repo = open(git_dir)?;
    let mut branches = Vec::new();
    for item in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = item?;
        if let Some(info) = branch_info(&repo, &branch)? {
            branches.push(info);
        }
    }
    Ok(branches)
}

pub fn local_branch(git_dir: &Path, name: &str) -> Result<Option<BranchInfo>> {
    let repo = open(git_dir)?;
    match optional(repo.find_branch(name, BranchType::Local))? {
        Some(branch) => branch_info(&repo, &branch),
        None => Ok(None),
    }
}

pub fn remote_branch_exists(git_dir: &Path, name: &str) -> Result<bool> {
    let repo = open(git_dir)?;
    Ok(optional(repo.find_reference(&format!("refs/remotes/{name}")))?.is_some())
}

pub fn resolve_commit(git_dir: &Path, rev: &str) -> Result<Option<Oid>> {
    let repo = open(git_dir)?;
    let Some(object) = optional(repo.revparse_single(rev))? else {
        return Ok(None);
    };
    Ok(optional(object.peel_to_commit())?.map(|c| oid(c.id())))
}

/// Count commits reachable from `commit` but not from remote-tracking
/// branches (and local branches, if `include_local`).
pub fn commits_not_in(git_dir: &Path, commit: &Oid, include_local: bool) -> Result<usize> {
    let repo = open(git_dir)?;
    let mut walk = repo.revwalk()?;
    walk.push(git_oid(commit)?)?;
    let mut globs = vec!["refs/remotes/*"];
    if include_local {
        globs.push("refs/heads/*");
    }
    for glob in globs {
        for reference in repo.references_glob(glob)? {
            let reference = reference?;
            // Symbolic refs (origin/HEAD) point at refs that are hidden anyway.
            if let Some(target) = reference.target() {
                walk.hide(target)?;
            }
        }
    }
    let mut count = 0;
    for id in walk {
        id?;
        count += 1;
    }
    Ok(count)
}

pub fn ahead_behind(git_dir: &Path, local: &Oid, upstream: &Oid) -> Result<(usize, usize)> {
    let repo = open(git_dir)?;
    Ok(repo.graph_ahead_behind(git_oid(local)?, git_oid(upstream)?)?)
}

fn head_of(repo: &Repository) -> Result<Head> {
    let head = repo.find_reference("HEAD")?;
    match head.symbolic_target()? {
        Some(target) => {
            let branch = target
                .strip_prefix("refs/heads/")
                .unwrap_or(target)
                .to_string();
            if head.resolve().is_ok() {
                Ok(Head::Branch(branch))
            } else {
                Ok(Head::Unborn(branch))
            }
        }
        None => match head.target() {
            Some(id) => Ok(Head::Detached(oid(id))),
            None => Err(GitError::Library("HEAD has no target".into())),
        },
    }
}

pub fn worktrees(git_dir: &Path) -> Result<Vec<WorktreeInfo>> {
    let repo = open(git_dir)?;
    let mut worktrees = Vec::new();
    for name in repo.worktrees()?.iter() {
        let Some(name) = name? else { continue };
        let worktree = repo.find_worktree(name)?;
        // Normalise away a trailing separator.
        let path: PathBuf = worktree.path().components().collect();
        let locked = matches!(worktree.is_locked()?, git2::WorktreeLockStatus::Locked(_));
        let missing = !path.is_dir();
        let head = if missing {
            None
        } else {
            Repository::open_from_worktree(&worktree)
                .ok()
                .and_then(|wt_repo| head_of(&wt_repo).ok())
        };
        worktrees.push(WorktreeInfo {
            admin_name: name.to_string(),
            path,
            head,
            locked,
            missing,
        });
    }
    Ok(worktrees)
}

pub fn worktree_status(path: &Path) -> Result<StatusSummary> {
    let repo = Repository::open(path)?;
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(false)
        .include_ignored(false)
        .exclude_submodules(false);

    let staged_flags = Status::INDEX_NEW
        | Status::INDEX_MODIFIED
        | Status::INDEX_DELETED
        | Status::INDEX_RENAMED
        | Status::INDEX_TYPECHANGE;
    let unstaged_flags =
        Status::WT_MODIFIED | Status::WT_DELETED | Status::WT_RENAMED | Status::WT_TYPECHANGE;

    let mut summary = StatusSummary::default();
    for entry in repo.statuses(Some(&mut options))?.iter() {
        let status = entry.status();
        if status.is_conflicted() {
            summary.conflicted += 1;
            continue;
        }
        if status.intersects(staged_flags) {
            summary.staged += 1;
        }
        if status.intersects(unstaged_flags) {
            summary.unstaged += 1;
        }
        if status.contains(Status::WT_NEW) {
            summary.untracked += 1;
        }
    }
    summary.operation = operation(&repo);
    Ok(summary)
}

fn operation(repo: &Repository) -> Option<Operation> {
    match repo.state() {
        RepositoryState::Clean => None,
        RepositoryState::Merge => Some(Operation::Merge),
        RepositoryState::Revert | RepositoryState::RevertSequence => Some(Operation::Revert),
        RepositoryState::CherryPick | RepositoryState::CherryPickSequence => {
            Some(Operation::CherryPick)
        }
        RepositoryState::Bisect => Some(Operation::Bisect),
        RepositoryState::Rebase
        | RepositoryState::RebaseInteractive
        | RepositoryState::RebaseMerge => Some(Operation::Rebase),
        RepositoryState::ApplyMailbox | RepositoryState::ApplyMailboxOrRebase => {
            Some(Operation::ApplyMailbox)
        }
    }
}

pub fn inspect_clone(path: &Path) -> Result<Option<CloneInfo>> {
    let repo = match Repository::open(path) {
        Ok(repo) => repo,
        Err(e) if not_found(&e) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if repo.is_bare() || repo.is_worktree() {
        return Ok(None);
    }
    // Must be the top directory, with the repository in its .git directory.
    let git_dir: PathBuf = repo.path().components().collect();
    let expected = path.join(".git");
    if git_dir.canonicalize().ok() != expected.canonicalize().ok() || !expected.is_dir() {
        return Ok(None);
    }
    let custom_worktree = repo
        .config()?
        .open_level(git2::ConfigLevel::Local)?
        .get_string("core.worktree")
        .is_ok();
    Ok(Some(CloneInfo {
        head: head_of(&repo)?,
        operation: operation(&repo),
        linked_worktrees: repo.worktrees()?.len(),
        submodules: repo.submodules()?.len(),
        custom_worktree,
    }))
}
