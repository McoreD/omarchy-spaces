//! `space watch`: the spaces as `space ls --json` lists them, printed again,
//! one line of JSON each time, whenever they change. The shell plugin reads
//! it for the bar and the sidebar instead of asking `space ls` over and over.
//!
//! Hyprland says when windows and workspaces change, on its event socket, so
//! its state is read again only then. Nothing says when a shell changes
//! folder, so the terminals' folders are read from `/proc`, which costs a few
//! `readlink`s: the active space's every `FAST`, the rest every `SLOW`, and
//! all of them at once when a window's title changes, as terminals retitle
//! themselves on `cd`.
//!
//! It exits when Hyprland's event socket closes, when nobody reads what it
//! prints, and when its binary has been replaced (`cargo install`), so that
//! whatever started it can start it again: the plugin restarts it.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Result, bail};

use crate::commands;
use crate::hypr::Hypr;
use crate::paths;
use crate::spaces::Store;
use crate::state::Last;
use crate::terminal;

/// How often the active space's terminals' folders are read.
const FAST: Duration = Duration::from_millis(200);
/// How often every other terminal's folder is read, and the binary checked.
const SLOW: Duration = Duration::from_secs(2);
/// How long to let a burst of Hyprland events settle before reading its
/// state again: opening a window sends several.
const SETTLE: Duration = Duration::from_millis(20);

pub fn watch() -> Result<()> {
    let hypr = Hypr::from_env()?;
    let events = hypr.events()?;
    let state_dir = paths::state_dir();

    let mut snapshot = commands::snapshot(&hypr)?;
    let mut cwds: HashMap<u32, Option<PathBuf>> = HashMap::new();
    let mut store = Store::load(&state_dir)?;
    let mut last = Last::load(&state_dir)?;
    let (mut store_seen, mut last_seen) = (
        stamp(&state_dir.join("spaces.json")),
        stamp(&state_dir.join("last")),
    );
    let mut read_all = true;
    let mut slow_at = Instant::now();
    let mut printed = String::new();
    let mut out = std::io::stdout().lock();

    loop {
        // The terminals' folders: the active space's now, the rest when due.
        let now = Instant::now();
        if now >= slow_at {
            read_all = true;
            slow_at = now + SLOW;
            if replaced() {
                return Ok(());
            }
        }
        let pids: HashSet<u32> = snapshot.space_windows.iter().map(|w| w.pid).collect();
        cwds.retain(|pid, _| pids.contains(pid));
        for window in &snapshot.space_windows {
            if read_all || snapshot.active.as_deref() == Some(window.space.as_str()) {
                cwds.insert(window.pid, terminal::shell_cwd(window.pid));
            }
        }
        read_all = false;

        // Names and visits change from other `space` commands.
        // (The store's stamp is taken after its own saves, below.)
        if stamp(&state_dir.join("spaces.json")) != store_seen {
            store = Store::load(&state_dir)?;
        }
        let seen = stamp(&state_dir.join("last"));
        if seen != last_seen {
            last = Last::load(&state_dir)?;
            last_seen = seen;
        }

        let live =
            commands::live_from(&snapshot, |window| cwds.get(&window.pid).cloned().flatten());
        let rows = commands::rows(&mut store, Some(&live), &last);
        if let Err(e) = store.save() {
            eprintln!("space: warning: {e:#}");
        }
        store_seen = stamp(&state_dir.join("spaces.json"));
        let json = serde_json::to_string(&rows)?;
        if json != printed {
            // Nobody reading any more: done.
            if writeln!(out, "{json}").and_then(|()| out.flush()).is_err() {
                return Ok(());
            }
            printed = json;
        }

        match events.recv_timeout(FAST) {
            Ok(event) => {
                let mut changed = Change::of(&event);
                let settle = Instant::now() + SETTLE;
                while let Some(wait) = settle.checked_duration_since(Instant::now()) {
                    match events.recv_timeout(wait) {
                        Ok(event) => changed = changed.max(Change::of(&event)),
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => bail!("Hyprland's events stopped"),
                    }
                }
                match changed {
                    Change::Nothing => {}
                    Change::Folders => read_all = true,
                    Change::Windows => {
                        snapshot = commands::snapshot(&hypr)?;
                        read_all = true;
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => bail!("Hyprland's events stopped"),
        }
    }
}

/// What a Hyprland event can change about the spaces, least to most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Change {
    Nothing,
    /// A title changed, which terminals do on `cd`: read their folders.
    Folders,
    /// Windows or workspaces came, went or moved: read Hyprland's state.
    Windows,
}

impl Change {
    fn of(event: &str) -> Self {
        let name = event.split_once(">>").map_or(event, |(name, _)| name);
        match name {
            "windowtitle" | "windowtitlev2" => Change::Folders,
            "openwindow" | "closewindow" | "movewindow" | "movewindowv2" | "workspace"
            | "workspacev2" | "createworkspace" | "createworkspacev2" | "destroyworkspace"
            | "destroyworkspacev2" | "renameworkspace" | "moveworkspace" | "moveworkspacev2"
            | "focusedmon" | "focusedmonv2" | "configreloaded" => Change::Windows,
            _ => Change::Nothing,
        }
    }
}

/// When a file last changed, and its size, to notice another `space`
/// writing it.
fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Whether this program's binary has been replaced since it started, as
/// `cargo install` does: the kernel marks the running one deleted.
fn replaced() -> bool {
    std::fs::read_link("/proc/self/exe")
        .is_ok_and(|exe| exe.to_string_lossy().ends_with(" (deleted)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_events_by_what_they_change() {
        assert_eq!(Change::of("windowtitlev2>>55a4,~/ren"), Change::Folders);
        assert_eq!(
            Change::of("openwindow>>55a4,s1:1,kitty,zsh"),
            Change::Windows
        );
        assert_eq!(Change::of("workspacev2>>-1337,s1:2"), Change::Windows);
        assert_eq!(Change::of("activewindowv2>>55a4"), Change::Nothing);
        assert_eq!(Change::of("garbage"), Change::Nothing);
        assert!(Change::Windows > Change::Folders && Change::Folders > Change::Nothing);
    }

    #[test]
    fn this_binary_is_not_replaced() {
        assert!(!replaced());
    }
}
