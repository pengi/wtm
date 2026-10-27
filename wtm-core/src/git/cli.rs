//! Operations run through the `git` command.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{Checkout, Credential, GitError, Head, Result};

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

    /// Register a worktree at `path` without checking out any files.
    pub fn add_worktree_no_checkout(&self, git_dir: &Path, path: &Path, head: &Head) -> Result<()> {
        let mut args: Vec<&OsStr> = vec![
            "worktree".as_ref(),
            "add".as_ref(),
            "--no-checkout".as_ref(),
        ];
        let target = match head {
            Head::Branch(branch) => branch.as_str(),
            Head::Detached(oid) => {
                args.push("--detach".as_ref());
                oid.0.as_str()
            }
            Head::Unborn(branch) => {
                return Err(GitError::Library(format!(
                    "branch '{branch}' has no commits"
                )));
            }
        };
        args.push(path.as_os_str());
        args.push(target.as_ref());
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

    pub fn credential_fill(&self, url: &str, username: Option<&str>) -> Result<Option<Credential>> {
        let mut input = credential_line("url", url)?;
        if let Some(username) = username {
            input.push_str(&credential_line("username", username)?);
        }
        // Never prompt: no terminal, no askpass program, no Git Credential
        // Manager dialog.
        let mut cmd = self.command(None, ["-c", "core.askPass=", "credential", "fill"]);
        cmd.env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "")
            .env("SSH_ASKPASS", "")
            .env("GCM_INTERACTIVE", "never");
        let Some(output) = run_with_input(cmd, &input)? else {
            return Ok(None);
        };
        let mut found_username = None;
        let mut password = None;
        for line in output.lines() {
            if let Some(v) = line.strip_prefix("username=") {
                found_username = Some(v.to_string());
            } else if let Some(v) = line.strip_prefix("password=") {
                password = Some(v.to_string());
            }
        }
        Ok(password.map(|password| Credential {
            url: url.to_string(),
            username: found_username,
            password,
        }))
    }

    /// `git credential approve` or `reject`.
    pub fn credential_store(&self, action: &str, credential: &Credential) -> Result<()> {
        let mut input = credential_line("url", &credential.url)?;
        if let Some(username) = &credential.username {
            input.push_str(&credential_line("username", username)?);
        }
        input.push_str(&credential_line("password", &credential.password)?);
        let cmd = self.command(None, ["credential", action]);
        match run_with_input(cmd, &input)? {
            Some(_) => Ok(()),
            None => Err(GitError::Command {
                command: format!("credential {action}"),
                stderr: String::new(),
            }),
        }
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

/// One `key=value` line of git's credential protocol.
fn credential_line(key: &str, value: &str) -> Result<String> {
    if value.contains(['\n', '\0']) {
        return Err(GitError::Library(format!(
            "credential {key} contains a line break"
        )));
    }
    Ok(format!("{key}={value}\n"))
}

/// Run with `input` on standard input; standard output on success, None on
/// failure.
fn run_with_input(mut cmd: Command, input: &str) -> Result<Option<String>> {
    use std::io::Write;
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(GitError::Spawn)?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).map_err(GitError::Spawn)?;
    }
    let out = child.wait_with_output().map_err(GitError::Spawn)?;
    Ok(out
        .status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned()))
}
