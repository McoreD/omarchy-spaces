use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use clap::{Parser, Subcommand};

use renspace::{commands, name};

/// Spaces for Hyprland: switch projects, not workspaces.
#[derive(Parser)]
#[command(name = "space", version)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Make a folder a space and go to it.
    New {
        /// Lowercase letters, digits and '-'. Defaults to the folder name.
        name: Option<String>,
        /// Defaults to the current folder. Created if it doesn't exist.
        dir: Option<PathBuf>,
    },
    /// Go to a space where you left it. With no name, pick one.
    Go {
        /// A space, or 'home' for the plain workspaces.
        name: Option<String>,
    },
    /// List spaces, the active one marked, with their window counts.
    Ls {
        /// Print JSON, for scripts.
        #[arg(long)]
        json: bool,
    },
    /// Go to workspace N of the active space.
    Ws {
        #[arg(value_parser = slot)]
        n: u8,
    },
    /// Move the focused window to workspace N of the active space.
    Move {
        #[arg(value_parser = slot)]
        n: u8,
        /// Stay where you are instead of following the window.
        #[arg(long)]
        silent: bool,
    },
}

fn slot(raw: &str) -> Result<u8, String> {
    name::parse_slot(raw).ok_or_else(|| format!("'{raw}' isn't a workspace: use 1-{}", name::SLOTS))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Run from a keybind, nobody sees stderr, so say it in a notification.
    let from_keybind = matches!(
        cli.command,
        Cmd::Go { .. } | Cmd::Ws { .. } | Cmd::Move { .. }
    ) && !std::io::stderr().is_terminal();

    let result = match cli.command {
        Cmd::New { name, dir } => commands::new(name, dir),
        Cmd::Go { name } => commands::go(name),
        Cmd::Ls { json } => commands::ls(json),
        Cmd::Ws { n } => commands::ws(n),
        Cmd::Move { n, silent } => commands::move_window(n, silent),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let message = format!("{e:#}");
            eprintln!("space: {message}");
            if from_keybind {
                notify(&message);
            }
            ExitCode::FAILURE
        }
    }
}

fn notify(message: &str) {
    let _ = Command::new("notify-send")
        .args(["--app-name=renSpace", "renSpace", message])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}
