//! The pool: registered repositories and their worktrees.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::git::{Checkout, DefaultBackend, GitBackend, Head};
use crate::model::{
    Created, DropBlocker, DropOptions, Dropped, NewWorktree, Repo, RepoDetails, Worktree,
    WorktreeState,
};
use crate::name;
use crate::source::{SourceResolver, UrlResolver};

/// Config key in a bare repository naming its default remote.
const DEFAULT_REMOTE_KEY: &str = "wtm.defaultRemote";

/// A registered repository that was just created.
#[derive(Debug)]
pub struct Registered {
    pub repo: Repo,
    pub url: String,
    /// Other repositories with the same URL.
    pub same_url: Vec<String>,
    pub default_branch: Option<String>,
}

/// A clone turned into a registered repository and worktree.
#[derive(Debug)]
pub struct Adopted {
    pub repo: Repo,
    /// Where the worktree is now.
    pub path: PathBuf,
    pub head: Head,
}

/// Worktrees re-linked by [`Pool::repair`].
#[derive(Debug)]
pub struct Repaired {
    pub repo: String,
    pub worktrees: Vec<PathBuf>,
}

pub struct Pool<B: GitBackend = DefaultBackend> {
    config: Config,
    git: B,
    resolvers: Vec<Box<dyn SourceResolver>>,
}

impl<B: GitBackend> Pool<B> {
    pub fn new(config: Config, git: B) -> Self {
        Pool {
            config,
            git,
            resolvers: vec![Box::new(UrlResolver)],
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    fn repos_dir(&self) -> &Path {
        &self.config.repos_dir.value
    }

    fn checkouts_dir(&self) -> &Path {
        &self.config.checkouts_dir.value
    }

    // Repositories

    /// All registered repositories, sorted by name.
    pub fn repos(&self) -> Result<Vec<Repo>> {
        let entries = match std::fs::read_dir(self.repos_dir()) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::io(self.repos_dir())(e)),
        };
        let mut repos = Vec::new();
        for entry in entries {
            let entry = entry.map_err(Error::io(self.repos_dir()))?;
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str().and_then(|n| n.strip_suffix(".git")) else {
                continue;
            };
            let path = entry.path();
            if path.is_dir() && self.git.is_repository(&path) {
                repos.push(Repo {
                    name: name.to_string(),
                    git_dir: path,
                });
            }
        }
        repos.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(repos)
    }

    pub fn repo(&self, name: &str) -> Result<Repo> {
        name::validate(name)?;
        let git_dir = self.repo_path(name);
        if git_dir.is_dir() && self.git.is_repository(&git_dir) {
            Ok(Repo {
                name: name.to_string(),
                git_dir,
            })
        } else {
            Err(Error::RepoNotFound(name.to_string()))
        }
    }

    fn repo_path(&self, name: &str) -> PathBuf {
        self.repos_dir().join(format!("{name}.git"))
    }

    pub fn repo_details(&self, repo: &Repo) -> Result<RepoDetails> {
        let remotes = self.git.remotes(&repo.git_dir)?;
        let default_remote = self.default_remote(repo)?;
        let default_branch = self.default_branch(repo, default_remote.as_deref())?;
        Ok(RepoDetails {
            repo: repo.clone(),
            remotes,
            default_remote,
            default_branch,
        })
    }

    /// The remote set with `remote set-default`, else `origin`, else the
    /// first remote.
    pub fn default_remote(&self, repo: &Repo) -> Result<Option<String>> {
        let remotes = self.git.remotes(&repo.git_dir)?;
        let has = |name: &str| remotes.iter().any(|r| r.name == name);
        if let Some(configured) = self.git.config_get(&repo.git_dir, DEFAULT_REMOTE_KEY)?
            && has(&configured)
        {
            return Ok(Some(configured));
        }
        if has("origin") {
            return Ok(Some("origin".to_string()));
        }
        Ok(remotes.into_iter().next().map(|r| r.name))
    }

    /// The default branch of `remote`, or else the branch HEAD points at.
    pub fn default_branch(&self, repo: &Repo, remote: Option<&str>) -> Result<Option<String>> {
        if let Some(remote) = remote
            && let Some(branch) = self.git.remote_head(&repo.git_dir, remote)?
        {
            return Ok(Some(branch));
        }
        Ok(self.git.head_branch(&repo.git_dir)?)
    }

    /// Register a repository: create a bare repository with `source` as its
    /// remote `remote_name`, and fetch it.
    pub fn register(
        &self,
        source: &str,
        name: Option<&str>,
        remote_name: &str,
    ) -> Result<Registered> {
        let resolved = self
            .resolvers
            .iter()
            .find_map(|r| r.resolve(source))
            .ok_or_else(|| Error::UnknownSource(source.to_string()))?;
        let name = name.unwrap_or(&resolved.suggested_name).to_string();
        name::validate(&name)?;
        name::validate(remote_name)?;

        let git_dir = self.repo_path(&name);
        if git_dir.exists() {
            return Err(Error::RepoExists(name));
        }

        let mut same_url = Vec::new();
        for repo in self.repos()? {
            let remotes = self.git.remotes(&repo.git_dir)?;
            if remotes
                .iter()
                .any(|r| r.url.as_deref() == Some(&resolved.url))
            {
                same_url.push(repo.name);
            }
        }

        std::fs::create_dir_all(self.repos_dir()).map_err(Error::io(self.repos_dir()))?;
        let result = self.init_repo(&git_dir, remote_name, &resolved.url);
        let default_branch = match result {
            Ok(branch) => branch,
            Err(e) => {
                // Leave nothing half-registered behind.
                let _ = std::fs::remove_dir_all(&git_dir);
                return Err(e);
            }
        };

        Ok(Registered {
            repo: Repo { name, git_dir },
            url: resolved.url,
            same_url,
            default_branch,
        })
    }

    fn init_repo(&self, git_dir: &Path, remote: &str, url: &str) -> Result<Option<String>> {
        self.git.init_bare(git_dir)?;
        self.git.add_remote(git_dir, remote, url)?;
        self.git.fetch(git_dir, remote)?;
        // An empty upstream has no default branch; that's not an error.
        if self.git.update_remote_head(git_dir, remote).is_err() {
            return Ok(None);
        }
        let branch = self.git.remote_head(git_dir, remote)?;
        if let Some(branch) = &branch {
            self.git.set_head_branch(git_dir, branch)?;
        }
        Ok(branch)
    }

    /// Remove a registered repository. It must have no worktrees, and unless
    /// `force`, all its branches must be on a remote.
    pub fn unregister(&self, name: &str, force: bool) -> Result<Repo> {
        let repo = self.repo(name)?;
        self.git.prune_worktrees(&repo.git_dir)?;
        let worktrees: Vec<String> = self
            .git
            .worktrees(&repo.git_dir)?
            .into_iter()
            .map(|w| w.path.display().to_string())
            .collect();
        if !worktrees.is_empty() {
            return Err(Error::RepoHasWorktrees {
                repo: repo.name,
                worktrees,
            });
        }
        if !force {
            let mut unpushed = Vec::new();
            for branch in self.git.local_branches(&repo.git_dir)? {
                if self
                    .git
                    .commits_not_on_remotes(&repo.git_dir, &branch.tip)?
                    > 0
                {
                    unpushed.push(branch.name);
                }
            }
            if !unpushed.is_empty() {
                return Err(Error::RepoHasUnpushed {
                    repo: repo.name,
                    branches: unpushed,
                });
            }
        }
        std::fs::remove_dir_all(&repo.git_dir).map_err(Error::io(&repo.git_dir))?;
        Ok(repo)
    }

    /// Turn an existing clone into a registered repository and a worktree
    /// of it. The clone's directory is moved into the checkouts directory,
    /// unless `keep_path`. Branches, stashes, remotes, uncommitted changes
    /// and untracked files are kept.
    pub fn adopt(&self, path: &Path, name: Option<&str>, keep_path: bool) -> Result<Adopted> {
        let path = path.canonicalize().map_err(Error::io(path))?;
        let info = self
            .git
            .inspect_clone(&path)?
            .ok_or_else(|| Error::NotAClone(path.clone()))?;
        let refuse = |reason: String| {
            Err(Error::CannotAdopt {
                path: path.clone(),
                reason,
            })
        };
        if info.linked_worktrees > 0 {
            return refuse("it has linked worktrees; remove them first".into());
        }
        if info.submodules > 0 {
            return refuse("it has submodules, which wtm doesn't support yet".into());
        }
        if info.custom_worktree {
            return refuse("core.worktree is set".into());
        }
        if let Some(op) = info.operation {
            return refuse(format!("a {op} is in progress"));
        }
        if let Head::Unborn(branch) = &info.head {
            return refuse(format!("branch '{branch}' has no commits"));
        }

        let name = match name {
            Some(name) => name.to_string(),
            None => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        name::validate(&name)?;
        let git_dir = self.repo_path(&name);
        if git_dir.exists() {
            return Err(Error::RepoExists(name));
        }
        // A clone already in place in the checkouts directory stays there.
        let keep_path = keep_path || canonical(&self.checkouts_dir().join(&name)) == path;
        let target = if keep_path {
            path.clone()
        } else {
            self.checkouts_dir().join(&name)
        };
        if !keep_path && target.symlink_metadata().is_ok() {
            return Err(Error::PathExists(target));
        }
        if canonical(self.checkouts_dir()).starts_with(&path)
            || canonical(self.repos_dir()).starts_with(&path)
        {
            return refuse("it contains the wtm directories".into());
        }

        std::fs::create_dir_all(self.repos_dir()).map_err(Error::io(self.repos_dir()))?;
        if !keep_path {
            std::fs::create_dir_all(self.checkouts_dir())
                .map_err(Error::io(self.checkouts_dir()))?;
            std::fs::rename(&path, &target).map_err(Error::io(&path))?;
        }
        if let Err(e) = self.git.convert_to_worktree(&target, &git_dir, &info.head) {
            if !keep_path {
                let _ = std::fs::rename(&target, &path);
            }
            return Err(e.into());
        }

        // Record the default branch, so `new` works as for registered
        // repositories. Not every remote is reachable now; that's fine.
        let repo = Repo { name, git_dir };
        if let Some(remote) = self.default_remote(&repo)? {
            let _ = self.git.update_remote_head(&repo.git_dir, &remote);
            if let Some(branch) = self.git.remote_head(&repo.git_dir, &remote)? {
                self.git.set_head_branch(&repo.git_dir, &branch)?;
            }
        }

        Ok(Adopted {
            repo,
            path: target,
            head: info.head,
        })
    }

    // Remotes

    fn require_remote(&self, repo: &Repo, remote: &str) -> Result<()> {
        if self
            .git
            .remotes(&repo.git_dir)?
            .iter()
            .any(|r| r.name == remote)
        {
            Ok(())
        } else {
            Err(Error::RemoteNotFound {
                repo: repo.name.clone(),
                remote: remote.to_string(),
            })
        }
    }

    pub fn add_remote(&self, repo: &Repo, remote: &str, url: &str, fetch: bool) -> Result<()> {
        name::validate(remote)?;
        if self.require_remote(repo, remote).is_ok() {
            return Err(Error::RemoteExists {
                repo: repo.name.clone(),
                remote: remote.to_string(),
            });
        }
        self.git.add_remote(&repo.git_dir, remote, url)?;
        if fetch {
            self.git.fetch(&repo.git_dir, remote)?;
            // Not every remote has a default branch (e.g. an empty fork).
            let _ = self.git.update_remote_head(&repo.git_dir, remote);
        }
        Ok(())
    }

    pub fn remove_remote(&self, repo: &Repo, remote: &str) -> Result<()> {
        self.require_remote(repo, remote)?;
        self.git.remove_remote(&repo.git_dir, remote)?;
        if self
            .git
            .config_get(&repo.git_dir, DEFAULT_REMOTE_KEY)?
            .as_deref()
            == Some(remote)
        {
            self.git.config_unset(&repo.git_dir, DEFAULT_REMOTE_KEY)?;
        }
        Ok(())
    }

    pub fn set_default_remote(&self, repo: &Repo, remote: &str) -> Result<()> {
        self.require_remote(repo, remote)?;
        self.git
            .config_set(&repo.git_dir, DEFAULT_REMOTE_KEY, remote)?;
        Ok(())
    }

    /// Fetch all remotes of a repository.
    pub fn fetch(&self, repo: &Repo) -> Result<()> {
        for remote in self.git.remotes(&repo.git_dir)? {
            self.git.fetch(&repo.git_dir, &remote.name)?;
        }
        Ok(())
    }

    // Worktrees

    /// All worktrees of all registered repositories, sorted by name.
    pub fn worktrees(&self) -> Result<Vec<Worktree>> {
        let checkouts = canonical(self.checkouts_dir());
        let mut worktrees = Vec::new();
        for repo in self.repos()? {
            for info in self.git.worktrees(&repo.git_dir)? {
                let name = info
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| info.admin_name.clone());
                let in_checkouts_dir = info
                    .path
                    .parent()
                    .is_some_and(|parent| canonical(parent) == checkouts);
                worktrees.push(Worktree {
                    name,
                    path: info.path,
                    repo: repo.name.clone(),
                    git_dir: repo.git_dir.clone(),
                    head: info.head,
                    locked: info.locked,
                    missing: info.missing,
                    in_checkouts_dir,
                });
            }
        }
        worktrees.sort_by(|a, b| (&a.name, &a.path).cmp(&(&b.name, &b.path)));
        Ok(worktrees)
    }

    /// Find a worktree by name. Worktrees in the checkouts directory take
    /// precedence over ones elsewhere with the same name.
    pub fn worktree(&self, name: &str) -> Result<Worktree> {
        let matching: Vec<Worktree> = self
            .worktrees()?
            .into_iter()
            .filter(|w| w.name == name)
            .collect();
        if let Some(w) = matching.iter().find(|w| w.in_checkouts_dir) {
            return Ok(w.clone());
        }
        match matching.len() {
            0 => Err(Error::WorktreeNotFound(name.to_string())),
            1 => Ok(matching.into_iter().next().unwrap()),
            _ => Err(Error::AmbiguousWorktree {
                name: name.to_string(),
                paths: matching.into_iter().map(|w| w.path).collect(),
            }),
        }
    }

    pub fn worktree_state(&self, worktree: &Worktree) -> Result<WorktreeState> {
        let mut state = WorktreeState::default();
        if worktree.missing {
            return Ok(state);
        }
        state.status = Some(self.git.worktree_status(&worktree.path)?);
        if let Some(Head::Branch(branch)) = &worktree.head
            && let Some(info) = self.git.local_branch(&worktree.git_dir, branch)?
        {
            if let Some(upstream_tip) = info.upstream.as_ref().and_then(|u| u.tip.as_ref()) {
                state.ahead_behind = Some(self.git.ahead_behind(
                    &worktree.git_dir,
                    &info.tip,
                    upstream_tip,
                )?);
            }
            state.not_on_remotes = Some(
                self.git
                    .commits_not_on_remotes(&worktree.git_dir, &info.tip)?,
            );
            state.upstream = info.upstream;
        }
        Ok(state)
    }

    /// Create a worktree in the checkouts directory.
    pub fn new_worktree(&self, opts: &NewWorktree) -> Result<Created> {
        let repo = self.repo(&opts.repo)?;
        let name = opts.name.clone().unwrap_or_else(|| repo.name.clone());
        name::validate(&name)?;
        let path = self.checkouts_dir().join(&name);
        if path.symlink_metadata().is_ok() {
            return Err(Error::PathExists(path));
        }

        let remote = match &opts.remote {
            Some(remote) => {
                self.require_remote(&repo, remote)?;
                Some(remote.clone())
            }
            None => self.default_remote(&repo)?,
        };

        let mut fetch_error = None;
        if opts.fetch
            && let Some(remote) = &remote
            && let Err(e) = self.git.fetch(&repo.git_dir, remote)
        {
            fetch_error = Some(e.into());
        }

        let branch = match (&opts.branch, &opts.name) {
            (Some(branch), _) => branch.clone(),
            (None, Some(name)) => name.clone(),
            (None, None) => self
                .default_branch(&repo, remote.as_deref())?
                .ok_or_else(|| Error::NoDefaultBranch(repo.name.clone()))?,
        };

        let checkout = self.choose_checkout(&repo, &branch, remote.as_deref(), &opts.from)?;

        std::fs::create_dir_all(self.checkouts_dir()).map_err(Error::io(self.checkouts_dir()))?;
        self.git.add_worktree(&repo.git_dir, &path, &checkout)?;

        Ok(Created {
            name,
            path,
            checkout,
            fetch_error,
        })
    }

    fn choose_checkout(
        &self,
        repo: &Repo,
        branch: &str,
        remote: Option<&str>,
        from: &Option<String>,
    ) -> Result<Checkout> {
        let git_dir = &repo.git_dir;

        if self.git.local_branch(git_dir, branch)?.is_some() {
            if from.is_some() {
                return Err(Error::BranchExists(branch.to_string()));
            }
            // Forget worktrees whose directory was deleted by hand, so their
            // branches are free again.
            self.git.prune_worktrees(git_dir)?;
            if let Some(other) = self
                .git
                .worktrees(git_dir)?
                .into_iter()
                .find(|w| w.head.as_ref().and_then(|h| h.branch()) == Some(branch))
            {
                return Err(Error::BranchCheckedOut {
                    branch: branch.to_string(),
                    path: other.path,
                });
            }
            return Ok(Checkout::Existing {
                branch: branch.to_string(),
            });
        }

        if from.is_none()
            && let Some(remote) = remote
        {
            let upstream = format!("{remote}/{branch}");
            if self.git.remote_branch_exists(git_dir, &upstream)? {
                return Ok(Checkout::Track {
                    branch: branch.to_string(),
                    upstream,
                });
            }
        }

        let start = match from {
            Some(from) => from.clone(),
            None => {
                let default = self
                    .default_branch(repo, remote)?
                    .ok_or_else(|| Error::NoDefaultBranch(repo.name.clone()))?;
                match remote {
                    Some(remote) => format!("{remote}/{default}"),
                    None => default,
                }
            }
        };
        if self.git.resolve_commit(git_dir, &start)?.is_none() {
            return Err(Error::UnknownRevision(start));
        }
        Ok(Checkout::New {
            branch: branch.to_string(),
            start,
        })
    }

    /// What would be lost by dropping `worktree`.
    pub fn drop_blockers(
        &self,
        worktree: &Worktree,
        keep_branch: bool,
    ) -> Result<Vec<DropBlocker>> {
        let mut blockers = Vec::new();
        if worktree.locked {
            blockers.push(DropBlocker::Locked);
        }
        if worktree.missing {
            return Ok(blockers);
        }

        let status = self.git.worktree_status(&worktree.path)?;
        if status.staged + status.unstaged + status.conflicted > 0 {
            blockers.push(DropBlocker::Uncommitted {
                staged: status.staged,
                unstaged: status.unstaged,
                conflicted: status.conflicted,
            });
        }
        if status.untracked > 0 {
            blockers.push(DropBlocker::Untracked(status.untracked));
        }
        if let Some(op) = status.operation {
            blockers.push(DropBlocker::InProgress(op));
        }

        match &worktree.head {
            Some(Head::Branch(branch)) if !keep_branch => {
                if let Some(info) = self.git.local_branch(&worktree.git_dir, branch)? {
                    let commits = self
                        .git
                        .commits_not_on_remotes(&worktree.git_dir, &info.tip)?;
                    if commits > 0 {
                        blockers.push(DropBlocker::Unpushed {
                            branch: branch.clone(),
                            commits,
                        });
                    }
                }
            }
            Some(Head::Detached(commit)) => {
                let commits = self
                    .git
                    .commits_not_on_any_branch(&worktree.git_dir, commit)?;
                if commits > 0 {
                    blockers.push(DropBlocker::DetachedCommits(commits));
                }
            }
            _ => {}
        }
        Ok(blockers)
    }

    /// Remove a worktree and, unless `keep_branch`, its local branch.
    pub fn drop_worktree(&self, name: &str, opts: &DropOptions) -> Result<Dropped> {
        let worktree = self.worktree(name)?;
        if opts.fetch {
            self.fetch(&self.repo(&worktree.repo)?)?;
        }

        let blockers = self.drop_blockers(&worktree, opts.keep_branch)?;
        if !blockers.is_empty() && !opts.force {
            return Err(Error::NotClean {
                name: worktree.name,
                blockers,
            });
        }

        if worktree.missing {
            self.git.prune_worktrees(&worktree.git_dir)?;
        } else {
            self.git
                .remove_worktree(&worktree.git_dir, &worktree.path, opts.force)?;
        }

        let mut deleted_branch = None;
        if !opts.keep_branch
            && let Some(Head::Branch(branch)) = &worktree.head
            && self.git.local_branch(&worktree.git_dir, branch)?.is_some()
        {
            self.git.delete_branch(&worktree.git_dir, branch)?;
            deleted_branch = Some(branch.clone());
        }

        Ok(Dropped {
            worktree,
            deleted_branch,
        })
    }

    /// Re-link worktrees in the checkouts directory with their repositories
    /// after either was moved, and forget worktrees that no longer exist.
    pub fn repair(&self) -> Result<Vec<Repaired>> {
        let mut found: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
        if let Ok(entries) = std::fs::read_dir(self.checkouts_dir()) {
            for entry in entries.flatten() {
                let path = entry.path();
                // A link looks like <repos_dir>/<repo>.git/worktrees/<admin>.
                let repo_name = self.git.worktree_link(&path).and_then(|link| {
                    let repo_dir = link.parent()?.parent()?;
                    let file_name = repo_dir.file_name()?.to_str()?;
                    Some(file_name.strip_suffix(".git")?.to_string())
                });
                if let Some(repo_name) = repo_name {
                    found.entry(repo_name).or_default().push(path);
                }
            }
        }

        let mut repaired = Vec::new();
        for repo in self.repos()? {
            let paths = found.remove(&repo.name).unwrap_or_default();
            // Repair before pruning: a moved worktree looks missing until
            // it is re-linked.
            if !paths.is_empty() {
                self.git.repair_worktrees(&repo.git_dir, &paths)?;
            }
            self.git.prune_worktrees(&repo.git_dir)?;
            repaired.push(Repaired {
                repo: repo.name,
                worktrees: paths,
            });
        }
        Ok(repaired)
    }
}

/// Resolve symlinks where possible, so paths compare the way git stores them.
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
