//! Values for dynamic shell completion.
//!
//! Completion runs without the command line's global options, so it uses
//! the configuration from the environment and the config file.

use clap_complete::engine::CompletionCandidate;
use wtm_core::git::{DefaultBackend, GitOutput};
use wtm_core::{Config, Pool};

fn pool() -> Option<Pool> {
    let config = Config::load(&Default::default()).ok()?;
    let git = DefaultBackend::new(GitOutput::Capture).ok()?;
    Some(Pool::new(config, git))
}

pub fn repos() -> Vec<CompletionCandidate> {
    let Some(pool) = pool() else {
        return Vec::new();
    };
    pool.repos()
        .unwrap_or_default()
        .into_iter()
        .map(|r| CompletionCandidate::new(r.name))
        .collect()
}

pub fn worktrees() -> Vec<CompletionCandidate> {
    let Some(pool) = pool() else {
        return Vec::new();
    };
    pool.worktrees()
        .unwrap_or_default()
        .into_iter()
        .map(|w| CompletionCandidate::new(w.name).help(Some(w.repo.into())))
        .collect()
}

/// Remote names of all repositories; the repository isn't known while
/// completing.
pub fn remotes() -> Vec<CompletionCandidate> {
    let Some(pool) = pool() else {
        return Vec::new();
    };
    let mut names: Vec<String> = pool
        .repos()
        .unwrap_or_default()
        .iter()
        .filter_map(|r| pool.repo_details(r).ok())
        .flat_map(|d| d.remotes.into_iter().map(|r| r.name))
        .collect();
    names.sort();
    names.dedup();
    names.into_iter().map(CompletionCandidate::new).collect()
}
