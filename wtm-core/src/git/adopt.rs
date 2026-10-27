//! Turning a normal clone into a bare repository plus a linked worktree.
//!
//! Git has no command for this, so it is done in steps:
//!
//! 1. Move the clone's `.git` directory to the new bare repository and mark
//!    it bare.
//! 2. Register a worktree for the checked out branch with `git worktree add
//!    --no-checkout` at a temporary path whose last component is the
//!    worktree's name (git names the worktree after it).
//! 3. Move the clone's index into the worktree, so staged changes survive.
//! 4. Point the clone directory at the worktree with a `.git` file, and let
//!    `git worktree repair` update git's side of the link.

use std::path::{Path, PathBuf};

use super::cli::GitCli;
use super::{GitError, Head, Result, worktree_link};

pub fn convert_to_worktree(cli: &GitCli, path: &Path, git_dir: &Path, head: &Head) -> Result<()> {
    let dot_git = path.join(".git");
    std::fs::rename(&dot_git, git_dir).map_err(|e| io_error("move", &dot_git, e))?;

    let scratch = git_dir.with_extension("adopting");
    let result = attach(cli, path, git_dir, head, &scratch);
    let _ = std::fs::remove_dir_all(&scratch);

    if let Err(e) = result {
        restore(cli, path, git_dir);
        return Err(e);
    }
    Ok(())
}

fn attach(cli: &GitCli, path: &Path, git_dir: &Path, head: &Head, scratch: &Path) -> Result<()> {
    cli.config_set(git_dir, "core.bare", "true")?;

    let name = path
        .file_name()
        .ok_or_else(|| GitError::Library(format!("{} has no name", path.display())))?;
    let temporary = scratch.join(name);
    std::fs::create_dir_all(scratch).map_err(|e| io_error("create", scratch, e))?;
    cli.add_worktree_no_checkout(git_dir, &temporary, head)?;
    let admin = worktree_link(&temporary)
        .ok_or_else(|| GitError::Library("git did not create the worktree".into()))?;

    let index = git_dir.join("index");
    if index.exists() {
        let target = admin.join("index");
        std::fs::rename(&index, &target).map_err(|e| io_error("move", &index, e))?;
    }

    let dot_git = path.join(".git");
    std::fs::write(&dot_git, format!("gitdir: {}\n", admin.display()))
        .map_err(|e| io_error("write", &dot_git, e))?;
    cli.repair_worktrees(git_dir, &[path.to_path_buf()])
}

/// Undo a failed conversion as far as possible: put the `.git` directory
/// back and make it a normal repository again.
fn restore(cli: &GitCli, path: &Path, git_dir: &Path) {
    let dot_git = path.join(".git");
    if let Some(admin) = worktree_link(path) {
        let index = admin.join("index");
        if index.exists() {
            let _ = std::fs::rename(&index, git_dir.join("index"));
        }
    }
    if dot_git.is_file() {
        let _ = std::fs::remove_file(&dot_git);
    }
    let _ = cli.prune_worktrees(git_dir);
    let _ = cli.config_set(git_dir, "core.bare", "false");
    let _ = std::fs::rename(git_dir, &dot_git);
}

fn io_error(action: &str, path: &Path, e: std::io::Error) -> GitError {
    GitError::Library(format!(
        "cannot {action} {}: {e}",
        PathBuf::from(path).display()
    ))
}
