use clap::Parser;

/// Worktree manager: a pool of bare git repositories and their worktrees.
#[derive(Parser)]
#[command(version)]
struct Cli {}

fn main() {
    Cli::parse();
}
