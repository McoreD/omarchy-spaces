//! What the bar and the sidebar show, as `space watch` prints it: a line is
//! all of it as of one moment, and everything in it comes from one snapshot,
//! so its parts can't disagree. The plugin draws it and works none of it out
//! again (`omarchy/Spaces.mjs` reads it).
//!
//! It's between `space` and its plugin, and changes as what they show does.
//! What scripts read is `space ls --json` (`list.rs`).

use serde::Serialize;

use crate::commands::{Live, Row, Screen, Snapshot};
use crate::name;
use crate::status::Status;
use crate::tabs;

/// Goes up when what's printed changes in a way that a reader made for the
/// last one can't read, so that the plugin updated without `space`, or the
/// other way around, says so.
const VERSION: u32 = 2;

#[derive(Debug, Serialize)]
pub(crate) struct State<'a> {
    version: u32,
    /// The monitor you are on, by name.
    focused: Option<&'a str>,
    monitors: Vec<Monitor<'a>>,
    /// In the sidebar's order.
    spaces: Vec<Space<'a>>,
}

/// A monitor, and the tabs its bar shows: those it has of the space it shows.
#[derive(Debug, Serialize)]
struct Monitor<'a> {
    name: &'a str,
    /// The space it shows, by its id.
    space: &'a str,
    /// The tab it shows, by its id. `None` on a workspace without a number.
    tab: Option<String>,
    /// How many frames it shows a second.
    refresh: f64,
    /// Whether the workspace it shows has tiled windows.
    tiled: bool,
    tabs: Vec<Tab>,
}

#[derive(Debug, PartialEq, Serialize)]
struct Tab {
    id: String,
    /// The number it shows, 1-10. `None` for none.
    slot: Option<u8>,
    label: String,
    named: bool,
    status: Option<Status>,
    windows: u32,
    /// Whether its monitor shows it.
    active: bool,
}

/// A space, as the sidebar shows it.
#[derive(Debug, Serialize)]
struct Space<'a> {
    id: &'a str,
    label: &'a str,
    named: bool,
    icon: Option<&'a str>,
    active: bool,
    windows: Option<u32>,
    left_at: u64,
    status: Option<Status>,
}

pub(crate) fn state<'a>(snapshot: &'a Snapshot, live: &Live, rows: &'a [Row]) -> State<'a> {
    let focused = snapshot.screens.iter().find(|screen| screen.focused);
    State {
        version: VERSION,
        focused: focused.map(|screen| screen.name.as_str()),
        monitors: snapshot
            .screens
            .iter()
            .map(|screen| monitor(screen, snapshot, live, rows))
            .collect(),
        spaces: rows.iter().map(space).collect(),
    }
}

fn monitor<'a>(screen: &'a Screen, snapshot: &Snapshot, live: &Live, rows: &[Row]) -> Monitor<'a> {
    let space = screen.shows.space();
    let row = rows.iter().find(|row| row.id == space);
    let tabs = tabs::on_monitor(space, &screen.name, snapshot, live, row);
    Monitor {
        name: &screen.name,
        space,
        tab: screen.shows.key().map(|key| name::tab_id(space, key)),
        refresh: screen.refresh,
        tiled: snapshot
            .space_windows
            .iter()
            .any(|w| w.space == space && w.tab == screen.shows.key() && !w.floating),
        tabs: tabs
            .into_iter()
            .map(|tab| Tab {
                id: tab.id,
                slot: tab.slot,
                label: tab.label,
                named: tab.named,
                status: tab.status,
                windows: tab.windows,
                active: tab.shown,
            })
            .collect(),
    }
}

fn space(row: &Row) -> Space<'_> {
    Space {
        id: &row.id,
        label: &row.label,
        named: row.named,
        icon: row.icon.as_deref(),
        active: row.active,
        windows: row.windows,
        left_at: row.left_at,
        status: row.status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name::HOME;

    #[test]
    fn a_workspace_without_a_number_is_homes_but_no_tab() {
        let mut snapshot = Snapshot::default();
        snapshot.screens.push(Screen {
            name: "eDP-1".into(),
            focused: true,
            refresh: 60.0,
            shows: name::locate(-5, "chat"),
        });
        let live = Live::default();
        let state = state(&snapshot, &live, &[]);
        assert_eq!(state.focused, Some("eDP-1"));
        assert_eq!(state.monitors[0].space, HOME);
        assert_eq!(state.monitors[0].tab, None);
        assert_eq!(state.monitors[0].tabs, []);
    }
}
