//! What opens a terminal, or runs an app, as Omarchy does. Which terminal is
//! for Omarchy to say (`omarchy default terminal`), so there's no setting for
//! it here.

use std::fs;
use std::path::Path;

use crate::paths;

/// Opens a terminal in `$SPACES_DIR` the way Omarchy's `super+enter`
/// does (`omarchy-launch-terminal`), so it is the terminal you chose in
/// Omarchy.
const XDG_TERMINAL: &str = "xdg-terminal-exec --dir=\"$SPACES_DIR\"";

/// Ghostty opens all its windows from one process, which Hyprland then can't
/// place, nor we tell apart. So ours are a process each, ending with its
/// window.
const GHOSTTY: &str = "ghostty --gtk-single-instance=false --quit-after-last-window-closed=true \
     --working-directory=\"$SPACES_DIR\"";
const GHOSTTY_ENTRY: &str = "com.mitchellh.ghostty.desktop";

/// The command that opens a terminal: for `sh -c`, with `SPACES_DIR` set to
/// its folder.
pub fn terminal() -> String {
    terminal_running(None)
}

/// The same, with the terminal picking up a conversation, which `space shell`
/// does.
pub fn resuming_terminal() -> String {
    terminal_running(Some(&space_shell()))
}

fn terminal_running(program: Option<&str>) -> String {
    let ghostty = prefers_ghostty(&paths::config_dir()) && on_path("ghostty");
    omarchy_terminal(ghostty, program)
}

/// The command for Omarchy's terminal, running `program` in place of your
/// shell.
fn omarchy_terminal(ghostty: bool, program: Option<&str>) -> String {
    let launcher = app_launcher();
    let (terminal, run) = match ghostty {
        true => (GHOSTTY, " -e "),
        false => (XDG_TERMINAL, " "),
    };
    match program {
        Some(program) => format!("{launcher} {terminal}{run}{program}"),
        None => format!("{launcher} {terminal}"),
    }
}

/// Whether the terminal you chose in Omarchy is Ghostty: the first in the list
/// that `xdg-terminal-exec` picks from, read here as asking it would hold up
/// each terminal.
fn prefers_ghostty(config_dir: &Path) -> bool {
    ["hyprland-xdg-terminals.list", "xdg-terminals.list"]
        .iter()
        .filter_map(|name| fs::read_to_string(config_dir.join(name)).ok())
        .find_map(|list| {
            let first = list
                .lines()
                .map(str::trim)
                .find(|line| line.ends_with(".desktop") && !line.starts_with(['#', '-', '/']))?;
            Some(first.trim_start_matches('+') == GHOSTTY_ENTRY)
        })
        .unwrap_or(false)
}

/// Runs an app as Omarchy does, in a scope of its own. Outside a uwsm session,
/// without it.
pub fn app_launcher() -> &'static str {
    if on_path("uwsm-app") {
        "exec setsid uwsm-app --"
    } else {
        "exec"
    }
}

/// This program, to run as `space shell`, by its path so it needn't be on
/// the terminal's PATH.
fn space_shell() -> String {
    let exe = std::env::current_exe()
        .map(|exe| exe.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "space".into());
    format!("'{}' shell", exe.replace('\'', "'\\''"))
}

pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_ghostty_a_process_a_window() {
        let plain = omarchy_terminal(false, None);
        assert!(
            plain.ends_with(r#"exec setsid uwsm-app -- xdg-terminal-exec --dir="$SPACES_DIR""#)
                || plain == r#"exec xdg-terminal-exec --dir="$SPACES_DIR""#
        );
        let running = omarchy_terminal(false, Some("'/b/space' shell"));
        assert!(
            running.ends_with(r#" --dir="$SPACES_DIR" '/b/space' shell"#),
            "{running}"
        );
        let ghostty = omarchy_terminal(true, Some("'/b/space' shell"));
        assert!(
            ghostty.contains(" --gtk-single-instance=false "),
            "{ghostty}"
        );
        assert!(
            ghostty.ends_with(r#" --working-directory="$SPACES_DIR" -e '/b/space' shell"#),
            "{ghostty}"
        );

        let dir = tempfile::tempdir().unwrap();
        let list = |text: &str| {
            fs::write(dir.path().join("xdg-terminals.list"), text).unwrap();
            prefers_ghostty(dir.path())
        };
        assert!(!prefers_ghostty(&dir.path().join("nowhere")));
        assert!(list(
            "# Preferred\ncom.mitchellh.ghostty.desktop\nkitty.desktop\n"
        ));
        assert!(!list("kitty.desktop\ncom.mitchellh.ghostty.desktop\n"));
        assert!(!list("# com.mitchellh.ghostty.desktop\n"));
        assert!(list(
            "/execarg_default:x.desktop:-e\n-kitty.desktop\n+com.mitchellh.ghostty.desktop\n"
        ));
    }
}
