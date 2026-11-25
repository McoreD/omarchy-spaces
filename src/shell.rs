//! `space shell`: what a terminal window Spaces opens runs. It asks
//! herdr-core for a shell in the window's folder, labelled with the window's
//! process so `space watch` can tell which window shows it, and attaches to
//! it. Core then knows what runs in it and what its agents report, for the
//! window's tab and its space. The window is the terminal: closing it ends
//! the shell, as closing any terminal window would.
//!
//! Without core (`hc` not installed, or core not starting), it runs your own
//! shell, as the terminal would have.

use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::agent;
use crate::config::{self, Config};
use crate::core;
use crate::terminal::shell_quote;

/// Attaches to the shell, then ends it however the window goes: the shell
/// exiting, `hc attach` detaching, or the window closing, which hangs up
/// both, and the trap ends the shell once `hc attach` has gone.
const ATTACH: &str = r#"end() { hc kill "$1" >/dev/null 2>&1; }
trap 'end "$1"; exit' HUP TERM
hc attach "$1"
end "$1""#;

/// The conversation to pick up before the shell, as `claude <id>`.
pub const RESUME: &str = "SPACES_RESUME";
/// Names the window to the `space restore` that opened it.
pub const RESTORE: &str = "SPACES_RESTORE";

pub fn shell() -> Result<()> {
    let argv = std::env::var(RESUME)
        .ok()
        .and_then(|resume| agent::parse_resume(&resume))
        .map(|command| agent_shell(&command))
        .unwrap_or_default();
    let wanted = Config::load_or_default().use_herdr_core();
    let error = if !wanted || !config::on_path("hc") {
        own_shell(&argv)
    } else {
        match core_shell(&argv) {
            Ok(address) => Command::new("sh")
                .arg("-c")
                .arg(ATTACH)
                .arg("omarchy-spaces-shell")
                .arg(address)
                .exec(),
            Err(e) => {
                eprintln!("space: no herdr-core, so a plain shell: {e:#}");
                own_shell(&argv)
            }
        }
    };
    Err(error).context("can't start a shell")
}

/// A new shell in core, here, for this window: its address.
fn core_shell(argv: &[String]) -> Result<String> {
    anyhow::ensure!(config::on_path("hc"), "hc isn't installed");
    let cwd = std::env::current_dir()?;
    let window = std::os::unix::process::parent_id();
    let env = forwarded_env(std::env::vars());
    let size = tty_size();
    match core::run_shell(argv, &cwd, &env, window, size) {
        Ok(address) => Ok(address),
        // Core isn't running: start it, as `hc` would, unless pointed at a
        // particular one.
        Err(_) if std::env::var_os("HC_SOCKET").is_none() => {
            Command::new("hc")
                .args(["server", "start"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            core::run_shell(argv, &cwd, &env, window, size)
        }
        Err(e) => Err(e),
    }
}

fn your_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into())
}

/// Your shell running `command`, with your aliases, and then being itself.
fn agent_shell(command: &str) -> Vec<String> {
    let shell = your_shell();
    let then = format!("{command}; exec {}", shell_quote(&shell));
    vec![shell, "-i".into(), "-c".into(), then]
}

/// Your login shell, or `argv` instead, in place of this process.
fn own_shell(argv: &[String]) -> std::io::Error {
    let shell = your_shell();
    let name = shell.rsplit('/').next().unwrap_or("sh");
    let mut command = Command::new(&shell);
    command.env_remove(RESUME).env_remove(RESTORE);
    match argv {
        [_, args @ ..] => command.args(args),
        [] => command.arg0(format!("-{name}")),
    };
    command.exec()
}

/// The environment the window's shell would have had, for core's shell:
/// this one, less what names the terminal (core's emulator answers as
/// itself) and what only made sense here.
fn forwarded_env(vars: impl Iterator<Item = (String, String)>) -> BTreeMap<String, String> {
    const TERMINAL: &[&str] = &[
        "TERM",
        "COLORTERM",
        "TERM_PROGRAM",
        "TERM_PROGRAM_VERSION",
        "TERMINFO",
        "WINDOWID",
        "VTE_VERSION",
        "PWD",
        "OLDPWD",
        "SHLVL",
        "_",
        RESUME,
        RESTORE,
    ];
    const TERMINAL_PREFIXES: &[&str] = &["KITTY_", "GHOSTTY_", "ALACRITTY_", "WEZTERM_", "HC_"];
    vars.filter(|(key, _)| {
        !TERMINAL.contains(&key.as_str()) && !TERMINAL_PREFIXES.iter().any(|p| key.starts_with(p))
    })
    .collect()
}

/// This terminal's size, columns by rows, so the shell starts at the
/// window's size rather than core's default.
fn tty_size() -> Option<(u16, u16)> {
    let out = Command::new("stty")
        .arg("size")
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut parts = text.split_whitespace();
    let rows: u16 = parts.next()?.parse().ok()?;
    let cols: u16 = parts.next()?.parse().ok()?;
    (rows > 0 && cols > 0).then_some((cols, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_the_session_but_not_the_terminal() {
        let vars = [
            ("PATH", "/usr/bin"),
            ("SSH_AUTH_SOCK", "/run/ssh"),
            ("SPACES_DIR", "/p/ren"),
            ("TERM", "xterm-kitty"),
            ("KITTY_WINDOW_ID", "3"),
            ("PWD", "/p"),
            ("HC_TERMINAL_ID", "t2"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let env = forwarded_env(vars.into_iter());
        assert_eq!(
            env.keys().map(String::as_str).collect::<Vec<_>>(),
            ["PATH", "SPACES_DIR", "SSH_AUTH_SOCK"]
        );
    }

    #[test]
    fn the_attach_script_is_valid_sh() {
        let out = Command::new("sh")
            .args(["-n", "-c", ATTACH])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
