use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use clap::{Parser, Subcommand};

use renspace::{commands, name, watch};

/// Spaces for Hyprland: a set of tabs for each thing you work on.
#[derive(Parser)]
#[command(name = "space", version)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open a new space with a terminal, in the folder you are in (the
    /// focused terminal's, else the active space's, else home). It is called
    /// after the folder its terminal is in.
    New {
        /// Open it here instead.
        dir: Option<PathBuf>,
    },
    /// Go to a space where you left it. With none named, pick one.
    Go {
        /// What it's called, or its id; 'home' for the plain workspaces.
        space: Option<String>,
    },
    /// List spaces, the active one marked, with their window counts.
    Ls {
        /// Print JSON, for scripts.
        #[arg(long)]
        json: bool,
    },
    /// Name a space, the active one unless you say which. An empty name has
    /// it called after its folder again.
    #[command(override_usage = "space rename [SPACE] <NAME>")]
    Rename {
        #[arg(value_name = "SPACE")]
        first: String,
        #[arg(value_name = "NAME")]
        second: Option<String>,
    },
    /// Close every window of a space, the active one unless you say which.
    Close { space: Option<String> },
    /// Open a terminal in the folder you are in, or the active space's.
    Terminal,
    /// Print the spaces as `ls --json` does, a line each time they change.
    Watch,
    /// Run by Hyprland when closing a window empties the workspace you are
    /// on: go to another tab, or end the space.
    #[command(hide = true)]
    Emptied { workspace: String },
    /// Go to a workspace (tab) of the active space: 1-10, new, next or prev.
    Ws {
        #[arg(value_parser = target)]
        target: commands::Target,
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

fn target(raw: &str) -> Result<commands::Target, String> {
    match raw {
        "new" => Ok(commands::Target::New),
        "next" => Ok(commands::Target::Next),
        "prev" => Ok(commands::Target::Prev),
        _ => name::parse_slot(raw)
            .map(commands::Target::Slot)
            .ok_or_else(|| {
                format!(
                    "'{raw}' isn't a workspace: use 1-{}, new, next or prev",
                    name::SLOTS
                )
            }),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Run from a keybind or the sidebar, nobody sees stderr, so say it in a
    // notification.
    // `watch` ends whenever Hyprland restarts, and the plugin restarts it: not
    // worth a notification.
    let unseen =
        !matches!(cli.command, Cmd::Ls { .. } | Cmd::Watch) && !std::io::stderr().is_terminal();

    let result = match cli.command {
        Cmd::New { dir } => commands::new(dir),
        Cmd::Go { space } => commands::go(space),
        Cmd::Ls { json } => commands::ls(json),
        Cmd::Rename { first, second } => match second {
            Some(new_name) => commands::rename(Some(first), new_name),
            None => commands::rename(None, first),
        },
        Cmd::Close { space } => commands::close(space),
        Cmd::Terminal => commands::open_terminal(),
        Cmd::Watch => watch::watch(),
        Cmd::Emptied { workspace } => commands::emptied(&workspace),
        Cmd::Ws { target } => commands::ws(target),
        Cmd::Move { n, silent } => commands::move_window(n, silent),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        // `space ls | head` closing the pipe early isn't an error.
        Err(e)
            if e.chain().any(|cause| {
                cause
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::BrokenPipe)
            }) =>
        {
            ExitCode::SUCCESS
        }
        Err(e) => {
            let message = format!("{e:#}");
            eprintln!("space: {message}");
            if unseen {
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
