//! What the bar and the sidebar show, as `space watch` prints it: a line is
//! all of it as of one moment, and everything in it comes from one snapshot,
//! so its parts can't disagree. The plugin draws it and works none of it out
//! again (`omarchy/Spaces.mjs` reads it).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::commands::{Live, Row, Screen, Snapshot};
use crate::monitors;
use crate::name::{self, HOME, Location};
use crate::status::Status;

/// Goes up when what's printed changes in a way that a plugin made for the
/// last one can't read, so that one updated without the other says so.
const VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub(crate) struct State<'a> {
    version: u32,
    /// The monitor you are on, by name.
    focused: Option<String>,
    monitors: Vec<Monitor>,
    /// As `space ls --json` lists them.
    spaces: &'a [Row],
}

/// A monitor, and the tabs its bar shows: those it has of the space it shows.
#[derive(Debug, Serialize)]
struct Monitor {
    name: String,
    /// The space it shows, by its id.
    space: String,
    /// The tab it shows, by its key. `None` on a workspace without a number.
    tab: Option<u8>,
    /// How many frames it shows a second.
    refresh: f64,
    /// Whether the workspace it shows has tiled windows.
    tiled: bool,
    tabs: Vec<Tab>,
}

#[derive(Debug, PartialEq, Serialize)]
struct Tab {
    /// What its workspace is numbered: 1-10 on the first monitor, 11-20 on
    /// the next.
    key: u8,
    /// What it shows as, 1-10; 0 for no number.
    slot: u8,
    /// The name you gave it, else what its last used window is called. One of
    /// `home`'s has only the name you gave it.
    label: String,
    /// Whether you gave it the name.
    named: bool,
    /// What most needs you among the agents in its windows.
    status: Option<Status>,
    windows: u32,
    /// Whether its monitor shows it.
    active: bool,
}

pub(crate) fn state<'a>(snapshot: &Snapshot, live: &Live, rows: &'a [Row]) -> State<'a> {
    let focused = snapshot.screens.iter().find(|screen| screen.focused);
    State {
        version: VERSION,
        focused: focused.map(|screen| screen.name.clone()),
        monitors: snapshot
            .screens
            .iter()
            .map(|screen| monitor(screen, snapshot, live, rows))
            .collect(),
        spaces: rows,
    }
}

fn monitor(screen: &Screen, snapshot: &Snapshot, live: &Live, rows: &[Row]) -> Monitor {
    let space = screen.shows.space();
    let row = rows.iter().find(|row| row.id == space);
    let tabs = slots(screen, snapshot)
        .into_iter()
        .map(|(key, slot)| tab(key, slot, screen, snapshot, live, row))
        .collect();
    Monitor {
        name: screen.name.clone(),
        space: space.to_string(),
        tab: screen.shows.key(),
        refresh: screen.refresh,
        tiled: snapshot
            .space_windows
            .iter()
            .any(|w| w.space == space && w.tab == screen.shows.key() && !w.floating),
        tabs,
    }
}

/// The tabs on a monitor and the slots they show as, in the order they show.
fn slots(screen: &Screen, snapshot: &Snapshot) -> Vec<(u8, u8)> {
    let space = screen.shows.space();
    let keys: BTreeSet<u8> = snapshot
        .places
        .iter()
        .filter(|((of, _), place)| of == space && place.monitor == screen.name)
        .map(|((_, key), _)| *key)
        .chain(screen.shows.key())
        .collect();
    let block = snapshot.blocks.block(&screen.name);
    let shown: BTreeMap<u8, u8> = match block {
        // Omarchy numbers `home`'s guests itself.
        Some(block) if space != HOME => monitors::slots(keys, block),
        _ => {
            let mut taken = BTreeSet::new();
            let numbered = keys.into_iter().map(|key| (key, name::slot_of(key)));
            numbered
                .map(|(key, slot)| (key, if taken.insert(slot) { slot } else { 0 }))
                .collect()
        }
    };
    let mut shown: Vec<(u8, u8)> = shown.into_iter().collect();
    shown.sort_by_key(|(key, slot)| (*slot == 0, *slot, *key));
    shown
}

fn tab(
    key: u8,
    slot: u8,
    screen: &Screen,
    snapshot: &Snapshot,
    live: &Live,
    row: Option<&Row>,
) -> Tab {
    let space = screen.shows.space();
    let windows: Vec<_> = snapshot
        .space_windows
        .iter()
        .filter(|w| w.space == space && w.tab == Some(key))
        .collect();
    let name = row.and_then(|row| row.tabs.get(&key)).cloned();
    let label = || {
        // Windows that never had focus say -1, and go last.
        let last_used = windows
            .iter()
            .min_by_key(|w| u64::try_from(w.focus).unwrap_or(u64::MAX))?;
        let terminal = row?
            .terminals
            .iter()
            .find(|t| t.address == last_used.address);
        Some(terminal.map_or_else(|| app_name(&last_used.class), |t| t.label.clone()))
    };
    Tab {
        key,
        slot,
        named: name.is_some(),
        label: match name {
            Some(name) => name,
            None if matches!(screen.shows, Location::Home { .. }) => String::new(),
            None => label().unwrap_or_default(),
        },
        status: windows
            .iter()
            .filter_map(|w| live.statuses.get(&w.address).copied())
            .max(),
        windows: windows.len().try_into().unwrap_or(u32::MAX),
        active: screen.shows.key() == Some(key),
    }
}

/// A short app name from a window's class: `org.omarchy.quick-terminal` is
/// `Quick-terminal`, Omarchy's `chrome-app.hey.com__-Default` web apps are
/// `Hey.com`.
fn app_name(class: &str) -> String {
    let web_app = ["chrome-", "chromium-", "brave-"]
        .iter()
        .find_map(|browser| class.strip_prefix(browser))
        .and_then(|rest| rest.split_once("__"))
        .map(|(site, _)| site)
        .filter(|site| !site.is_empty());
    let name = match web_app {
        Some(site) => ["app.", "www."]
            .iter()
            .find_map(|prefix| site.strip_prefix(prefix))
            .unwrap_or(site),
        None => class.rsplit('.').next().unwrap_or(class),
    };
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_apps_from_window_classes() {
        assert_eq!(app_name("chromium"), "Chromium");
        assert_eq!(app_name("org.omarchy.quick-terminal"), "Quick-terminal");
        assert_eq!(app_name("com.mitchellh.ghostty"), "Ghostty");
        assert_eq!(app_name("chrome-app.hey.com__-Default"), "Hey.com");
        assert_eq!(app_name("chromium-www.x.com__-Default"), "X.com");
        assert_eq!(app_name(""), "");
    }

    fn screen(name: &str, focused: bool, id: i64, workspace: &str) -> Screen {
        Screen {
            name: name.into(),
            focused,
            refresh: 60.0,
            shows: name::locate(id, workspace),
        }
    }

    #[test]
    fn home_tabs_with_the_same_number_show_it_once() {
        let mut snapshot = Snapshot::default();
        snapshot.screens.push(screen("eDP-1", true, 3, "3"));
        for key in [2, 12] {
            let place = snapshot.places.entry((HOME.into(), key)).or_default();
            place.monitor = "eDP-1".into();
        }
        let shown = slots(&snapshot.screens[0], &snapshot);
        assert_eq!(shown, [(2, 2), (3, 3), (12, 0)]);
    }

    #[test]
    fn a_workspace_without_a_number_is_homes_but_no_tab() {
        let mut snapshot = Snapshot::default();
        snapshot.screens.push(screen("eDP-1", true, -5, "chat"));
        let state = state(&snapshot, &Live::default(), &[]);
        assert_eq!(state.focused.as_deref(), Some("eDP-1"));
        assert_eq!(state.monitors[0].space, HOME);
        assert_eq!(state.monitors[0].tab, None);
        assert_eq!(state.monitors[0].tabs, []);
    }
}
