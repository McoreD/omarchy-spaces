//! `space shell`: what a terminal runs when `space` opens it with a command
//! (`space new -- <cmd>`, `space tab new -- <cmd>`, `space terminal -- <cmd>`)
//! or when `space restore` picks up a saved agent conversation. It runs inside
//! your own interactive shell and hands back to it when the command exits, so
//! leaving the program leaves you at your prompt.

use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::agent;
use crate::terminal::shell_quote;

/// The conversation to pick up before the shell, as `claude <id>`.
pub const RESUME: &str = "SPACES_RESUME";
/// Names the window to the `space restore` that opened it.
pub const RESTORE: &str = "SPACES_RESTORE";
/// An explicit command argv to run before the shell, as a JSON string array.
pub const RUN: &str = "SPACES_RUN";

/// Encodes a command argv for `SPACES_RUN`.
pub fn encode_run(argv: &[String]) -> Result<String> {
    if argv.first().is_none_or(|arg| arg.is_empty()) {
        bail!("command can't be empty");
    }
    Ok(serde_json::to_string(argv)?)
}

/// Decodes a `SPACES_RUN` JSON argv, returning `None` if malformed or empty.
pub fn parse_run(value: &str) -> Option<Vec<String>> {
    let argv: Vec<String> = serde_json::from_str(value).ok()?;
    if argv.first().is_none_or(|arg| arg.is_empty()) {
        return None;
    }
    Some(argv)
}

/// Builds the interactive shell `Command` that `space shell` execs.
fn build_command(shell: &str, run: Option<Vec<String>>, resume: Option<String>) -> Command {
    let mut command = Command::new(shell);
    command
        .env_remove(RUN)
        .env_remove(RESUME)
        .env_remove(RESTORE);
    let shell_name = shell.rsplit('/').next().unwrap_or("sh");
    if let Some(argv) = run {
        // Interactive, so your PATH/aliases load; argv is passed as positional
        // parameters so no argument is ever parsed or expanded by the shell.
        if shell_name == "fish" {
            command
                .args(["-i", "-c", "$argv[2..]; exec $argv[1]", "--", shell])
                .args(&argv);
        } else {
            command
                .args(["-i", "-c", "\"$@\"; exec \"$0\"", shell])
                .args(&argv);
        }
    } else if let Some(resume) = resume {
        // Interactive, so with your aliases, and then being itself.
        command
            .args(["-i", "-c"])
            .arg(format!("{resume}; exec {}", shell_quote(shell)));
    } else {
        command.arg0(format!("-{shell_name}"));
    }
    command
}

/// Your login shell, in place of this process, or first what `RUN` or `RESUME`
/// says.
pub fn shell() -> Result<()> {
    let run = std::env::var(RUN).ok().and_then(|raw| parse_run(&raw));
    let resume = if run.is_none() {
        std::env::var(RESUME)
            .ok()
            .and_then(|resume| agent::parse_resume(&resume))
    } else {
        None
    };
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into());
    let mut command = build_command(&shell, run, resume);
    Err(command.exec()).context("can't start a shell")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_run_argv() {
        let argv = vec![
            "claude".into(),
            "fix $HOME; 'quotes'".into(),
            "".into(),
            "--model".into(),
        ];
        let encoded = encode_run(&argv).unwrap();
        assert_eq!(parse_run(&encoded), Some(argv));
        assert!(encode_run(&[]).is_err());
        assert!(encode_run(&["".into()]).is_err());
        assert_eq!(parse_run("[]"), None);
        assert_eq!(parse_run("[\"\"]"), None);
        assert_eq!(parse_run("not json"), None);
    }

    #[test]
    fn builds_safe_shell_invocations() {
        let argv = vec!["printf".into(), "a b".into(), "$HOME".into()];
        let bash = build_command("/bin/bash", Some(argv.clone()), None);
        let args: Vec<_> = bash.get_args().map(|a| a.to_string_lossy()).collect();
        assert_eq!(
            args,
            [
                "-i",
                "-c",
                "\"$@\"; exec \"$0\"",
                "/bin/bash",
                "printf",
                "a b",
                "$HOME"
            ]
        );

        let fish = build_command("/usr/bin/fish", Some(argv), None);
        let args: Vec<_> = fish.get_args().map(|a| a.to_string_lossy()).collect();
        assert_eq!(
            args,
            [
                "-i",
                "-c",
                "$argv[2..]; exec $argv[1]",
                "--",
                "/usr/bin/fish",
                "printf",
                "a b",
                "$HOME"
            ]
        );
    }
}
