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
        /// Instead of a folder, use the one of that name in your roots, or
        /// make one in the first root (~/Projects by default).
        #[arg(long, requires = "name", conflicts_with = "dir")]
        in_roots: bool,
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
    /// Rename a space, the active one unless you name it. Its windows stay.
    #[command(override_usage = "space rename [OLD] <NEW>")]
    Rename {
        #[arg(value_name = "OLD")]
        first: String,
        #[arg(value_name = "NEW")]
        second: Option<String>,
    },
    /// Close every window of a space, the active one unless you name it.
    Close { name: Option<String> },
    /// Stop a folder being a space: removes its .renspace.toml, not the folder.
    Rm { name: String },
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
    let unseen = !matches!(cli.command, Cmd::Ls { .. }) && !std::io::stderr().is_terminal();

    let result = match cli.command {
        Cmd::New {
            name,
            dir,
            in_roots,
        } => commands::new(
            name,
            if in_roots {
                commands::NewDir::InRoots
            } else {
                commands::NewDir::Path(dir)
            },
        ),
        Cmd::Rename { first, second } => match second {
            Some(new) => commands::rename(Some(first), new),
            None => commands::rename(None, first),
        },
        Cmd::Close { name } => commands::close(name),
        Cmd::Rm { name } => commands::remove(name),
        Cmd::Go { name } => commands::go(name),
        Cmd::Ls { json } => commands::ls(json),
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
