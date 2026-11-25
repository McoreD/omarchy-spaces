//! `space shell`: what a terminal that `space restore` opens runs, to pick up
//! the conversation that was in it. It's your own shell, with the agent
//! started in it, so leaving the agent leaves you at your prompt.

use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::{Context, Result};

use crate::agent;
use crate::terminal::shell_quote;

/// The conversation to pick up before the shell, as `claude <id>`.
pub const RESUME: &str = "SPACES_RESUME";
/// Names the window to the `space restore` that opened it.
pub const RESTORE: &str = "SPACES_RESTORE";

/// Your login shell, in place of this process, or first what `RESUME` says.
pub fn shell() -> Result<()> {
    let resume = std::env::var(RESUME)
        .ok()
        .and_then(|resume| agent::parse_resume(&resume));
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into());
    let mut command = Command::new(&shell);
    command.env_remove(RESUME).env_remove(RESTORE);
    match resume {
        // Interactive, so with your aliases, and then being itself.
        Some(resume) => command
            .args(["-i", "-c"])
            .arg(format!("{resume}; exec {}", shell_quote(&shell))),
        None => command.arg0(format!("-{}", shell.rsplit('/').next().unwrap_or("sh"))),
    };
    Err(command.exec()).context("can't start a shell")
}
