//! Command line arguments.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::engine::ArgValueCandidates;

use crate::complete;

/// Worktree manager: a pool of bare git repositories and their worktrees.
///
/// Registered repositories are kept as bare repositories in the repos
/// directory. Worktrees are created from them in the checkouts directory.
///
/// Standard output only carries data (paths, lists); messages go to
/// standard error.
#[derive(Parser)]
#[command(name = "wtm", version, propagate_version = true, max_term_width = 100)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args)]
#[command(next_help_heading = "Global options")]
pub struct GlobalArgs {
    /// Config file [env: WTM_CONFIG] [default: ~/.config/wtm/config.toml]
    #[arg(long, global = true, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Directory of the bare repositories [env: WTM_REPOS_DIR]
    #[arg(long, global = true, value_name = "DIR")]
    pub repos_dir: Option<PathBuf>,

    /// Directory of the worktrees [env: WTM_CHECKOUTS_DIR]
    #[arg(long, global = true, value_name = "DIR")]
    pub checkouts_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Register an upstream repository
    ///
    /// Creates a bare repository named NAME in the repos directory, with
    /// SOURCE as its remote, and fetches it. No worktree is created.
    ///
    /// SOURCE is a URL, or ACCOUNT:PATH for a repository on an account's
    /// service, e.g. `github:owner/repo`. A PATH without owner is the
    /// account user's own repository.
    Register {
        /// URL of the upstream repository, as given to `git clone`, or
        /// ACCOUNT:PATH
        #[arg(add = ArgValueCandidates::new(complete::account_sources))]
        source: String,
        /// Name of the repository [default: derived from SOURCE]
        name: Option<String>,
        /// Name of the remote
        #[arg(long, default_value = "origin")]
        remote: String,
    },

    /// Turn an existing clone into a registered repository and worktree
    ///
    /// Moves the clone's .git directory into the repos directory as the
    /// bare repository NAME, and makes the clone's directory a worktree of
    /// it. The directory is moved into the checkouts directory, unless
    /// --keep-path is given. Branches, stashes, remotes, hooks, uncommitted
    /// changes and untracked files are kept.
    ///
    /// Refused for clones with linked worktrees or submodules, or with an
    /// operation such as a rebase in progress.
    Adopt {
        /// Top directory of the clone
        #[arg(value_hint = clap::ValueHint::DirPath)]
        path: PathBuf,
        /// Name of the repository and worktree [default: the directory name]
        name: Option<String>,
        /// Leave the worktree where the clone is
        #[arg(long)]
        keep_path: bool,
    },

    /// Manage accounts on upstream services (GitHub, GitLab)
    ///
    /// Accounts are kept in the config file. Their tokens are kept by git's
    /// credential helpers, the same way git keeps HTTPS passwords.
    #[command(subcommand)]
    Auth(AuthCommand),

    /// Search repositories on an account's service
    ///
    /// Lists repositories you own or are a member of, matching QUERY if
    /// given. With --all, searches all repositories visible to you (QUERY
    /// is then required). Register one with `wtm register ACCOUNT:PATH`.
    Search {
        /// Account to search with
        #[arg(add = ArgValueCandidates::new(complete::accounts))]
        account: String,
        /// Text to look for in names and descriptions
        query: Option<String>,
        /// Search all visible repositories, not just your own
        #[arg(long)]
        all: bool,
    },

    /// Remove a registered repository
    ///
    /// Deletes the bare repository. Refused while the repository has
    /// worktrees, or has branches with commits that are on no remote.
    Unregister {
        /// Registered repository
        #[arg(add = ArgValueCandidates::new(complete::repos))]
        repo: String,
        /// Delete even if branches have commits that are on no remote
        #[arg(short, long)]
        force: bool,
    },

    /// List registered repositories and their remotes
    ///
    /// The default remote is marked with `*`.
    Repos,

    /// Manage the remotes of a repository
    #[command(subcommand)]
    Remote(RemoteCommand),

    /// Fetch registered repositories
    ///
    /// Fetches all remotes of the given repositories, or of all registered
    /// repositories if none are given. Branches deleted on a remote are
    /// removed from the remote-tracking branches.
    Fetch {
        /// Registered repositories [default: all]
        #[arg(add = ArgValueCandidates::new(complete::repos))]
        repos: Vec<String>,
    },

    /// Create a worktree
    ///
    /// Creates the worktree NAME in the checkouts directory and prints its
    /// path.
    ///
    /// The branch checked out is BRANCH if given, else NAME if given, else
    /// the default branch of the remote. An existing local branch is checked
    /// out as is. Otherwise, if the remote has the branch, a local branch
    /// tracking it is created. Otherwise a new branch is created from START
    /// (default: the remote's default branch), without upstream.
    New {
        /// Registered repository
        #[arg(add = ArgValueCandidates::new(complete::repos))]
        repo: String,
        /// Name of the worktree [default: REPO]
        name: Option<String>,
        /// Branch to check out
        #[arg(short, long)]
        branch: Option<String>,
        /// Start point of a new branch
        #[arg(long, value_name = "START")]
        from: Option<String>,
        /// Remote to fetch and track [default: the default remote]
        #[arg(long, add = ArgValueCandidates::new(complete::remotes))]
        remote: Option<String>,
        /// Don't fetch before creating the worktree
        #[arg(long)]
        no_fetch: bool,
    },

    /// List worktrees
    ///
    /// Shows each worktree's repository, branch and state: uncommitted
    /// changes, untracked files, commits ahead of or behind the upstream,
    /// and commits on no remote.
    #[command(visible_alias = "ls")]
    List {
        /// Only list worktrees of this repository
        #[arg(long, add = ArgValueCandidates::new(complete::repos))]
        repo: Option<String>,
        /// Show the path of each worktree
        #[arg(short, long)]
        paths: bool,
        /// Only print worktree names, one per line
        #[arg(long, conflicts_with = "paths")]
        names: bool,
    },

    /// Remove a worktree and its branch
    ///
    /// Removes the worktree directory, including ignored files, and deletes
    /// its local branch. Refused if anything else would be lost:
    /// uncommitted changes, untracked files that are not ignored, an
    /// operation in progress, a lock, or commits that are on no remote.
    Drop {
        /// Worktree
        #[arg(add = ArgValueCandidates::new(complete::worktrees))]
        name: String,
        /// Drop even if something would be lost
        #[arg(short, long)]
        force: bool,
        /// Keep the local branch
        #[arg(short, long)]
        keep_branch: bool,
        /// Fetch first, so recently pushed commits count as pushed
        #[arg(long)]
        fetch: bool,
    },

    /// Print the path of a worktree
    ///
    /// Without NAME, prints the checkouts directory. Meant for scripts and
    /// shell functions such as `cd "$(wtm locate NAME)"`.
    Locate {
        /// Worktree
        #[arg(add = ArgValueCandidates::new(complete::worktrees))]
        name: Option<String>,
        /// Print the path of the bare repository NAME instead (or of the
        /// repos directory, without NAME)
        #[arg(long)]
        repo: bool,
    },

    /// Re-link worktrees after moving directories
    ///
    /// Fixes the links between worktrees in the checkouts directory and
    /// their repositories after either directory was moved, and forgets
    /// worktrees whose directory was deleted.
    Repair,

    /// Show the effective configuration
    Config,

    /// Print shell integration code
    ///
    /// Defines completion for wtm and a function (default `wcd`) that
    /// changes directory to a worktree. Add to your shell's rc file, after
    /// compinit for zsh:
    ///
    ///     eval "$(wtm shell-init zsh)"
    ShellInit {
        /// Shell to integrate with
        shell: Shell,
        /// Name of the change-directory function
        #[arg(long, default_value = "wcd")]
        cd_command: String,
    },

    /// Generate files for packaging and documentation
    #[command(hide = true, subcommand)]
    Generate(Generate),
}

#[derive(Subcommand)]
pub enum RemoteCommand {
    /// List the remotes of a repository
    List {
        /// Registered repository
        #[arg(add = ArgValueCandidates::new(complete::repos))]
        repo: String,
    },
    /// Add a remote and fetch it
    Add {
        /// Registered repository
        #[arg(add = ArgValueCandidates::new(complete::repos))]
        repo: String,
        /// Name of the remote
        name: String,
        /// URL of the remote, as given to `git clone`
        url: String,
        /// Don't fetch the new remote
        #[arg(long)]
        no_fetch: bool,
        /// Make it the default remote
        #[arg(long)]
        default: bool,
    },
    /// Remove a remote and its remote-tracking branches
    Remove {
        /// Registered repository
        #[arg(add = ArgValueCandidates::new(complete::repos))]
        repo: String,
        /// Remote
        #[arg(add = ArgValueCandidates::new(complete::remotes))]
        name: String,
    },
    /// Set the remote that `new` fetches and tracks by default
    SetDefault {
        /// Registered repository
        #[arg(add = ArgValueCandidates::new(complete::repos))]
        repo: String,
        /// Remote
        #[arg(add = ArgValueCandidates::new(complete::remotes))]
        name: String,
    },
}

#[derive(Subcommand)]
pub enum AuthCommand {
    /// Add an account
    ///
    /// Asks for an access token, unless git's credential helper already has
    /// one for URL (for example from `gh auth login`). The token is checked
    /// against the service and stored with git's credential helper. Without
    /// a token, only public repositories can be looked up.
    ///
    /// GitHub tokens need the `repo` scope (or read access to the
    /// repositories, for fine-grained tokens); GitLab tokens `read_api`.
    Add {
        /// Name of the account, used as ACCOUNT in `ACCOUNT:PATH`
        name: String,
        /// Kind of service
        kind: AccountKind,
        /// Web address of the service [default: https://github.com or
        /// https://gitlab.com]
        #[arg(long)]
        url: Option<String>,
        /// Your user name [default: the token's user]
        #[arg(long)]
        user: Option<String>,
        /// Clone URL to register repositories with
        #[arg(long, value_enum, default_value_t = Protocol::Ssh)]
        protocol: Protocol,
        /// Read the token from standard input instead of asking
        #[arg(long)]
        token_stdin: bool,
    },
    /// List accounts
    List,
    /// Check that accounts' tokens work
    Check {
        /// Account [default: all]
        #[arg(add = ArgValueCandidates::new(complete::accounts))]
        name: Option<String>,
    },
    /// Remove an account from the config file
    ///
    /// The token stays with git's credential helper, since git may use it
    /// for HTTPS, unless --forget-token is given.
    Remove {
        #[arg(add = ArgValueCandidates::new(complete::accounts))]
        /// Account
        name: String,
        /// Also remove the token from git's credential helper
        #[arg(long)]
        forget_token: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum AccountKind {
    Github,
    Gitlab,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Protocol {
    Ssh,
    Https,
}

#[derive(Subcommand)]
pub enum Generate {
    /// Man pages, into DIR
    Man {
        /// Output directory
        dir: PathBuf,
    },
    /// Completion script for installing system-wide
    Completion {
        /// Shell to generate for
        shell: Shell,
    },
    /// Command reference in Markdown
    Markdown,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
}
