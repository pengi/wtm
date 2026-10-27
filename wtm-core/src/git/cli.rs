//! Operations run through the `git` command.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{Checkout, GitError, Result};

/// The oldest git with everything wtm uses (`git worktree repair`).
const MINIMUM_VERSION: (u32, u32) = (2, 30);

/// Environment variables that would redirect git away from the repository
/// wtm points it at.
const REPO_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_PREFIX",
    "GIT_NAMESPACE",
];

/// What happens to git's own output for long-running operations (fetch,
/// checkout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitOutput {
    /// Capture it. Git may not prompt for credentials; failures report
    /// git's error output.
    Capture,
    /// Let git write progress and prompt on the terminal. Git's standard
    /// output goes to standard error, so wtm's own output stays clean.
    Inherit,
}

pub struct GitCli {
    output: GitOutput,
}

impl GitCli {
    pub fn new(output: GitOutput) -> Self {
        GitCli { output }
    }

    pub fn check_version(&self) -> Result<()> {
        let out = self.run_captured(None, ["--version"])?;
        let version = out
            .trim()
            .strip_prefix("git version ")
            .unwrap_or(out.trim());
        let mut numbers = version
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|n| n.parse::<u32>().ok());
        let found = (numbers.next().unwrap_or(0), numbers.next().unwrap_or(0));
        if found < MINIMUM_VERSION {
            return Err(GitError::TooOld {
                version: version.to_string(),
                minimum: format!("{}.{}", MINIMUM_VERSION.0, MINIMUM_VERSION.1),
            });
        }
        Ok(())
    }

    pub fn init_bare(&self, git_dir: &Path) -> Result<()> {
        self.run_captured(
            None,
            [OsStr::new("init"), "--bare".as_ref(), git_dir.as_os_str()],
        )?;
        Ok(())
    }

    pub fn set_head_branch(&self, git_dir: &Path, branch: &str) -> Result<()> {
        let target = format!("refs/heads/{branch}");
        self.run_captured(Some(git_dir), ["symbolic-ref", "HEAD", &target])?;
        Ok(())
    }

    pub fn config_set(&self, git_dir: &Path, key: &str, value: &str) -> Result<()> {
        self.run_captured(Some(git_dir), ["config", key, value])?;
        Ok(())
    }

    pub fn config_unset(&self, git_dir: &Path, key: &str) -> Result<()> {
        // Exit status 5 means the key was not set, which is fine.
        let out = self
            .command(Some(git_dir), ["config", "--unset", key])
            .output();
        let out = out.map_err(GitError::Spawn)?;
        if out.status.success() || out.status.code() == Some(5) {
            Ok(())
        } else {
            Err(command_error(["config", "--unset", key], &out.stderr))
        }
    }

    pub fn add_remote(&self, git_dir: &Path, name: &str, url: &str) -> Result<()> {
        self.run_captured(Some(git_dir), ["remote", "add", name, url])?;
        Ok(())
    }

    pub fn remove_remote(&self, git_dir: &Path, name: &str) -> Result<()> {
        self.run_captured(Some(git_dir), ["remote", "remove", name])?;
        Ok(())
    }

    pub fn fetch(&self, git_dir: &Path, remote: &str) -> Result<()> {
        self.run_long(git_dir, ["fetch", "--prune", remote])
    }

    pub fn update_remote_head(&self, git_dir: &Path, remote: &str) -> Result<()> {
        self.run_long(git_dir, ["remote", "set-head", remote, "--auto"])
    }

    pub fn delete_branch(&self, git_dir: &Path, name: &str) -> Result<()> {
        self.run_captured(Some(git_dir), ["branch", "-D", name])?;
        Ok(())
    }

    pub fn add_worktree(&self, git_dir: &Path, path: &Path, checkout: &Checkout) -> Result<()> {
        let path = path.as_os_str();
        let args: Vec<&OsStr> = match checkout {
            Checkout::Existing { branch } => {
                vec!["worktree".as_ref(), "add".as_ref(), path, branch.as_ref()]
            }
            Checkout::Track { branch, upstream } => vec![
                "worktree".as_ref(),
                "add".as_ref(),
                "--track".as_ref(),
                "-b".as_ref(),
                branch.as_ref(),
                path,
                upstream.as_ref(),
            ],
            Checkout::New { branch, start } => vec![
                "worktree".as_ref(),
                "add".as_ref(),
                "--no-track".as_ref(),
                "-b".as_ref(),
                branch.as_ref(),
                path,
                start.as_ref(),
            ],
        };
        self.run_long(git_dir, args)
    }

    pub fn remove_worktree(&self, git_dir: &Path, path: &Path, force: bool) -> Result<()> {
        let mut args: Vec<&OsStr> = vec!["worktree".as_ref(), "remove".as_ref()];
        if force {
            // Twice, to also remove locked worktrees.
            args.extend([OsStr::new("--force"), OsStr::new("--force")]);
        }
        args.push(path.as_os_str());
        self.run_captured(Some(git_dir), args)?;
        Ok(())
    }

    pub fn prune_worktrees(&self, git_dir: &Path) -> Result<()> {
        self.run_captured(Some(git_dir), ["worktree", "prune"])?;
        Ok(())
    }

    pub fn repair_worktrees(&self, git_dir: &Path, paths: &[PathBuf]) -> Result<()> {
        let mut args: Vec<&OsStr> = vec!["worktree".as_ref(), "repair".as_ref()];
        args.extend(paths.iter().map(|p| p.as_os_str()));
        self.run_captured(Some(git_dir), args)?;
        Ok(())
    }

    fn command<I, S>(&self, git_dir: Option<&Path>, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new("git");
        for var in REPO_ENV {
            cmd.env_remove(var);
        }
        if let Some(git_dir) = git_dir {
            cmd.arg("--git-dir").arg(git_dir);
        }
        cmd.args(args);
        cmd
    }

    /// Run git with all output captured; returns standard output.
    fn run_captured<I, S>(&self, git_dir: Option<&Path>, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S> + Clone,
        S: AsRef<OsStr>,
    {
        let out = self
            .command(git_dir, args.clone())
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .output()
            .map_err(GitError::Spawn)?;
        if !out.status.success() {
            return Err(command_error(args, &out.stderr));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Run a long operation according to the configured [`GitOutput`].
    fn run_long<I, S>(&self, git_dir: &Path, args: I) -> Result<()>
    where
        I: IntoIterator<Item = S> + Clone,
        S: AsRef<OsStr>,
    {
        match self.output {
            GitOutput::Capture => self.run_captured(Some(git_dir), args).map(|_| ()),
            GitOutput::Inherit => {
                let status = self
                    .command(Some(git_dir), args.clone())
                    .stdout(std::io::stderr())
                    .status()
                    .map_err(GitError::Spawn)?;
                if status.success() {
                    Ok(())
                } else {
                    Err(command_error(args, b""))
                }
            }
        }
    }
}

fn command_error<I, S>(args: I, stderr: &[u8]) -> GitError
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let command = args
        .into_iter()
        .map(|a| a.as_ref().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    GitError::Command {
        command,
        stderr: String::from_utf8_lossy(stderr).into_owned(),
    }
}
