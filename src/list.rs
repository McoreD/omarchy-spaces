//! What `space ls --json` prints, for scripts. It's theirs to rely on, so it's
//! apart from what the plugin reads (`view.rs`), which changes as the bar and
//! the sidebar do, and from how spaces are kept.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::folder::{self, Folder};
use crate::live::{Live, Row, Snapshot, TerminalRow};
use crate::name;
use crate::status::Status;
use crate::tabs;

/// Goes up when it changes in a way that breaks a script made for the last
/// one; a new field doesn't.
const VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub(crate) struct List<'a> {
    version: u32,
    /// `home` first, then in the sidebar's order.
    spaces: Vec<Space<'a>>,
}

#[derive(Debug, Serialize)]
struct Space<'a> {
    /// What commands take it by.
    id: &'a str,
    /// What to call it: the name you gave it, or its folder's.
    label: &'a str,
    /// Whether you gave it the name; otherwise it follows its folder.
    named: bool,
    /// The glyph or emoji you picked for its badge, if any.
    icon: Option<&'a str>,
    /// The checkout (or folder) its terminal is in. `null` for `home`, and
    /// for a space that never had a terminal.
    #[serde(flatten, serialize_with = "folder::or_null")]
    folder: Option<Folder>,
    /// Whether it's the one you are in.
    active: bool,
    /// `null` when Hyprland can't be reached.
    windows: Option<u32>,
    /// What most needs you among the agents in its windows, if any is
    /// working, done or waiting.
    status: Option<Status>,
    /// When it was last left, in Unix milliseconds, or 0 if never.
    left_at: u64,
    /// Its tabs, monitor by monitor, in the order they show. `null` when
    /// Hyprland can't be reached.
    tabs: Option<Vec<Tab>>,
    /// Its terminal windows, oldest first.
    terminals: Vec<Terminal<'a>>,
}

#[derive(Debug, Serialize)]
struct Tab {
    /// What commands take it by. Not to be taken apart, and another once the
    /// tab is moved.
    id: String,
    /// The number it shows on its monitor, 1-10. `null` for none.
    slot: Option<u8>,
    /// The monitor it's on, by name.
    monitor: String,
    /// The name you gave it, else what its last used window is called. Empty
    /// for an empty tab, and for one of `home`'s that you haven't named.
    label: String,
    /// Whether you gave it the name.
    named: bool,
    /// Whether its monitor shows it.
    shown: bool,
    windows: u32,
    /// What most needs you among the agents in its windows.
    status: Option<Status>,
}

#[derive(Debug, Serialize)]
struct Terminal<'a> {
    /// Hyprland's address for the window: `0x55a4...`.
    address: &'a str,
    /// The tab it's on, by its id. `null` on a workspace without a number.
    tab: Option<String>,
    /// The folder its shell is in.
    #[serde(flatten)]
    folder: &'a Folder,
    /// What runs in it (`claude`, `nvim`), or `null` at the prompt.
    program: Option<&'a str>,
    /// What its tab shows: the program, or else the folder's name.
    label: &'a str,
    /// What the agent in it is doing, if one reports it and isn't idle.
    status: Option<Status>,
}

/// The spaces in `rows`, with their tabs if Hyprland can be reached.
pub(crate) fn list<'a>(rows: &'a [Row], hyprland: Option<(&Snapshot, &Live)>) -> List<'a> {
    List {
        version: VERSION,
        spaces: rows.iter().map(|row| space(row, hyprland)).collect(),
    }
}

fn space<'a>(row: &'a Row, hyprland: Option<(&Snapshot, &Live)>) -> Space<'a> {
    Space {
        id: &row.id,
        label: &row.label,
        named: row.named,
        icon: row.icon.as_deref(),
        folder: row.folder.clone(),
        active: row.active,
        windows: row.windows,
        status: row.status,
        left_at: row.left_at,
        tabs: hyprland.map(|(snapshot, live)| tabs_of(row, snapshot, live)),
        terminals: row.terminals.iter().map(|t| terminal(&row.id, t)).collect(),
    }
}

fn tabs_of(row: &Row, snapshot: &Snapshot, live: &Live) -> Vec<Tab> {
    let screens = snapshot.screens.iter().map(|screen| screen.name.as_str());
    let places = snapshot.places.values().map(|place| place.monitor.as_str());
    let mut seen = BTreeSet::new();
    screens
        .chain(places)
        .filter(|monitor| seen.insert(*monitor))
        .flat_map(|monitor| {
            let tabs = tabs::on_monitor(&row.id, monitor, snapshot, live, Some(row));
            tabs.into_iter().map(move |tab| Tab {
                id: tab.id,
                slot: tab.slot,
                monitor: monitor.to_string(),
                label: tab.label,
                named: tab.named,
                shown: tab.shown,
                windows: tab.windows,
                status: tab.status,
            })
        })
        .collect()
}

fn terminal<'a>(space: &str, terminal: &'a TerminalRow) -> Terminal<'a> {
    Terminal {
        address: &terminal.address,
        tab: terminal.tab.map(|key| name::tab_id(space, key)),
        folder: &terminal.folder,
        program: terminal.program.as_deref(),
        label: &terminal.label,
        status: terminal.status,
    }
}
