//! Core library of wtm: a pool of bare git repositories and their worktrees.
//!
//! This crate never prints or exits. Front ends (the `wtm` CLI, `wtmgui`)
//! present its results.

pub mod config;
mod error;
pub mod git;
pub mod model;
pub mod name;
pub mod pool;
pub mod source;

pub use config::Config;
pub use error::{Error, Result};
pub use pool::Pool;
