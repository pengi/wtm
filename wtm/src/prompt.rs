//! Reading a secret from the terminal without echoing it.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::process::Command;

use anyhow::{Context, Result};

pub fn read_secret(prompt: &str) -> Result<String> {
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .context("no terminal to ask for the token on; use --token-stdin")?;
    write!(tty, "{prompt}")?;
    tty.flush()?;

    stty("-echo")?;
    let mut line = String::new();
    let read = BufReader::new(&tty).read_line(&mut line);
    // Restore echo even if reading failed.
    let restored = stty("echo");
    writeln!(tty)?;
    read?;
    restored?;
    Ok(line.trim().to_string())
}

fn stty(mode: &str) -> Result<()> {
    let status = Command::new("stty")
        .arg(mode)
        .stdin(File::open("/dev/tty")?)
        .status()
        .context("could not run stty")?;
    anyhow::ensure!(status.success(), "stty {mode} failed");
    Ok(())
}
