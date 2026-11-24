use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use clap::{Parser, Subcommand};

use renspace::{commands, name, shell, watch};

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
    /// Go to the next space down the sidebar, wrapping around to `home`.
    Next,
    /// Go to the space before, up the sidebar, wrapping around to the last.
    Prev,
    /// List spaces, the active one marked, with their window counts.
    Ls {
        /// Print JSON, for scripts.
        #[arg(long)]
        json: bool,
    },
    /// Name a space, the active one unless you say which, or a tab, as
    /// `<space>:<n>`. An empty name has a space called after its folder
    /// again, and a tab after its window (in `home`, just its number).
    #[command(override_usage = "space rename [SPACE | SPACE:N] <NAME>")]
    Rename {
        #[arg(value_name = "SPACE")]
        first: String,
        #[arg(value_name = "NAME")]
        second: Option<String>,
    },
    /// Give a space, the active one unless you say which, a glyph or an emoji
    /// for its badge, in place of its letters. An empty icon brings the
    /// letters back.
    #[command(override_usage = "space icon [SPACE] <ICON>")]
    Icon {
        #[arg(value_name = "SPACE")]
        first: String,
        #[arg(value_name = "ICON")]
        second: Option<String>,
    },
    /// Move a space up or down the sidebar, to a position counting from 1
    /// below `home`, which stays first.
    Reorder {
        /// What it's called, or its id.
        space: String,
        position: usize,
    },
    /// Close every window of a space, the active one unless you say which.
    Close { space: Option<String> },
    /// Open a terminal in the folder you are in, or the active space's.
    Terminal,
    /// Print the spaces as `ls --json` does, a line each time they change.
    Watch {
        /// Play a sound when an agent needs you or finishes unseen.
        #[arg(long)]
        sounds: bool,
    },
    /// Run as a terminal window's program: a shell in herdr-core, attached
    /// here and ended with the window, else your own shell.
    #[command(hide = true)]
    Shell,
    /// Run by Hyprland when closing a window empties the workspace you are
    /// on: go to another tab, or end the space.
    #[command(hide = true)]
    Emptied { workspace: String },
    /// Go to a workspace (tab) of the active space: 1-10, new, next or prev.
    Ws {
        #[arg(value_parser = target)]
        target: commands::Target,
    },
    /// Send the focused window to another space, on the tab you'd land on
    /// going there, and go with it.
    Send {
        /// What it's called, or its id; 'home' for the plain workspaces;
        /// next or prev for the space below or above, wrapping around.
        #[arg(value_parser = space_target)]
        space: commands::SpaceTarget,
        /// Stay where you are instead of following the window.
        #[arg(long)]
        silent: bool,
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

fn space_target(raw: &str) -> Result<commands::SpaceTarget, String> {
    Ok(match raw {
        "next" => commands::SpaceTarget::Next,
        "prev" => commands::SpaceTarget::Prev,
        _ => commands::SpaceTarget::Named(raw.to_string()),
    })
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
    let unseen = !matches!(cli.command, Cmd::Ls { .. } | Cmd::Watch { .. } | Cmd::Shell)
        && !std::io::stderr().is_terminal();

    let result = match cli.command {
        Cmd::New { dir } => commands::new(dir),
        Cmd::Go { space } => commands::go(space),
        Cmd::Next => commands::step(true),
        Cmd::Prev => commands::step(false),
        Cmd::Ls { json } => commands::ls(json),
        Cmd::Rename { first, second } => match second {
            Some(new_name) => commands::rename(Some(first), new_name),
            None => commands::rename(None, first),
        },
        Cmd::Icon { first, second } => match second {
            Some(icon) => commands::icon(Some(first), icon),
            None => commands::icon(None, first),
        },
        Cmd::Reorder { space, position } => commands::reorder(space, position),
        Cmd::Close { space } => commands::close(space),
        Cmd::Terminal => commands::open_terminal(),
        Cmd::Watch { sounds } => watch::watch(sounds),
        Cmd::Shell => shell::shell(),
        Cmd::Emptied { workspace } => commands::emptied(&workspace),
        Cmd::Ws { target } => commands::ws(target),
        Cmd::Send { space, silent } => commands::send(space, silent),
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
