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
    Register {
        /// URL of the upstream repository, as given to `git clone`
        source: String,
        /// Name of the repository [default: derived from SOURCE]
        name: Option<String>,
        /// Name of the remote
        #[arg(long, default_value = "origin")]
        remote: String,
    },

    /// Remove a registered repository
    ///
    /// Deletes the bare repository. Refused while the repository has
    /// worktrees, or has branches with commits that are on no remote.
    Unregister {
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
        #[arg(add = ArgValueCandidates::new(complete::repos))]
        repo: String,
    },
    /// Add a remote and fetch it
    Add {
        #[arg(add = ArgValueCandidates::new(complete::repos))]
        repo: String,
        name: String,
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
        #[arg(add = ArgValueCandidates::new(complete::repos))]
        repo: String,
        #[arg(add = ArgValueCandidates::new(complete::remotes))]
        name: String,
    },
    /// Set the remote that `new` fetches and tracks by default
    SetDefault {
        #[arg(add = ArgValueCandidates::new(complete::repos))]
        repo: String,
        #[arg(add = ArgValueCandidates::new(complete::remotes))]
        name: String,
    },
}

#[derive(Subcommand)]
pub enum Generate {
    /// Man pages, into DIR
    Man { dir: PathBuf },
    /// Completion script for installing system-wide
    Completion { shell: Shell },
    /// Command reference in Markdown
    Markdown,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
}
