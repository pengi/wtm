mod args;
mod commands;
mod complete;
mod generate;
mod prompt;
mod shell;
mod table;

use std::process::ExitCode;

use clap::{CommandFactory, Parser};
use clap_complete::CompleteEnv;

fn main() -> ExitCode {
    CompleteEnv::with_factory(args::Cli::command)
        .var(shell::COMPLETE_VAR)
        .complete();

    let cli = args::Cli::parse();
    match commands::run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            report(&e);
            ExitCode::FAILURE
        }
    }
}

fn report(e: &anyhow::Error) {
    eprintln!("wtm: {e:#}");
    if let Some(wtm_core::Error::NotClean { blockers, .. }) = e.downcast_ref() {
        for blocker in blockers {
            eprintln!("  - {blocker}");
        }
        let only_unpushed = blockers
            .iter()
            .all(|b| matches!(b, wtm_core::model::DropBlocker::Unpushed { .. }));
        if only_unpushed {
            eprintln!("Push the branch, keep it with --keep-branch, or drop anyway with --force.");
        } else {
            eprintln!("Use --force to drop anyway.");
        }
    }
}
