//! The commands. None of them keeps state about windows: each asks Hyprland
//! where things are, then dispatches. What Hyprland can't hold about a space,
//! its name and the terminal it follows, is in the store (`spaces.rs`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::browser::{self, Browser, Browsers};
use crate::config::{self, Config};
use crate::core;
use crate::folder::{self, Folder};
use crate::hypr::{self, Hypr};
use crate::layout::{self, Rect, Tree};
use crate::monitors::{self, Blocks, Strip};
use crate::name::{self, HOME, Location};
use crate::paths;
use crate::reserve;
use crate::session::{self, Session};
use crate::shell;
use crate::spaces::{self, Store};
use crate::state::{self, Last};
use crate::status::{self, Status};
use crate::terminal;
use crate::view;

/// Where `space tab go`, `new`, `next` and `prev` go. A space's workspaces
/// are its tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Slot(u8),
    /// A new tab, after the last one.
    New,
    Next,
    Prev,
}

/// `space tab go <n>` and `space tab new`, `next` and `prev`: a tab of the
/// active space. A space's new tab opens with a terminal in the space's folder, as a new space does,
/// so it never shows empty; `home`'s is Omarchy's next workspace, empty.
pub fn go_to_tab(target: Target) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let _turn = matches!(target, Target::Next | Target::Prev)
        .then(take_turn)
        .transpose()?;
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    let space = here.space();
    if target == Target::New && space != HOME {
        return new_tab(&hypr, &active, &here);
    }
    let strip = Strip::new(&hypr.workspaces()?, space, &active, &Blocks::load());
    let key = match target {
        Target::Slot(slot) => strip.at(slot),
        Target::New => new_tab_in(&strip, [])?,
        Target::Next | Target::Prev => {
            match step_tab(target == Target::Next, &strip.keys(), here.key()) {
                Some(key) => key,
                None => return Ok(()),
            }
        }
    };
    if here.key() == Some(key) {
        return Ok(());
    }

    focus(&hypr, space, key)?;
    remember(space, key);
    Ok(())
}

/// `space tab go <space>:<n>`: a tab of any space, on whichever monitor it is.
pub fn go_to_named_tab(tab: String) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (rows, _) = load_rows(Some(&hypr))?;
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    let Tab { id, key, .. } = tab_or_active(&hypr, &rows, Some(&tab), &active)?;
    if is_tab(&id, key, active.id, &active.name) {
        return Ok(());
    }
    if here.space() != id
        && let Some(left_at) = here.key()
    {
        remember(here.space(), left_at);
    }
    hypr.dispatch(&hypr::focus_workspace(&session_selector(&id, key)))?;
    remember(&id, key);
    Ok(())
}

/// A new tab in the active space, reserved until its terminal is up: the one
/// you are on, if nothing is open there, as on a monitor the space hasn't been
/// used on.
fn new_tab(hypr: &Hypr, active: &hypr::Workspace, here: &Location) -> Result<()> {
    let config = Config::load()?;
    let space = here.space();
    let dir = space_folder(hypr, space)?;
    let blocks = Blocks::load();
    let (_, key) = reserve::reserve(&paths::reserved_dir(), |reserved| {
        let workspaces = hypr.workspaces()?;
        let opening: Vec<u8> = reserved
            .iter()
            .filter(|(of, _)| of == space)
            .map(|(_, key)| *key)
            .collect();
        let empty = workspaces
            .iter()
            .any(|ws| ws.name == active.name && ws.windows == 0);
        if let Some(key) = here.key()
            && empty
            && !opening.contains(&key)
        {
            return Ok((space.to_string(), key));
        }
        let strip = Strip::new(&workspaces, space, active, &blocks);
        let taken = open_in(&workspaces, space).into_iter().chain(opening);
        Ok((space.to_string(), new_tab_in(&strip, taken)?))
    })?;
    let workspace = name::workspace_name(space, key);
    open_terminal_then(hypr, &config, &dir, &workspace, || focus(hypr, space, key))?;
    remember(space, key);
    Ok(())
}

/// A space's workspaces on a monitor.
fn tabs_on<'a>(
    workspaces: &'a [hypr::Workspace],
    space: &'a str,
    monitor: &'a str,
) -> impl Iterator<Item = &'a hypr::Workspace> {
    workspaces
        .iter()
        .filter(move |ws| ws.monitor == monitor && name::locate(ws.id, &ws.name).space() == space)
}

/// A new tab for a monitor: see `Strip::new_tab`.
fn new_tab_in(strip: &Strip, taken: impl IntoIterator<Item = u8>) -> Result<u8> {
    strip
        .new_tab(taken)
        .with_context(|| format!("all {} tabs are open; close one first", name::SLOTS))
}

/// The tab after or before `current` among the open ones; `None` when there's
/// nowhere else to go.
fn step_tab(forward: bool, open: &[u8], current: Option<u8>) -> Option<u8> {
    let Some(i) = current.and_then(|c| open.iter().position(|&key| key == c)) else {
        return if forward { open.first() } else { open.last() }.copied();
    };
    let step = if forward { 1 } else { open.len() - 1 };
    (open.len() > 1).then(|| open[(i + step) % open.len()])
}

/// `space window tab <n>`: the focused window to tab `n` of the active space,
/// following it unless `silent`.
pub fn window_to_tab(slot: u8, silent: bool) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    let workspaces = hypr.workspaces()?;
    let key = Strip::new(&workspaces, here.space(), &active, &Blocks::load()).at(slot);
    if here.key() == Some(key) || !hypr.has_active_window()? {
        return Ok(());
    }

    match &here {
        Location::Space { name, .. } => {
            hypr.dispatch(&hypr::move_window(&selector(name, key), !silent))?
        }
        Location::Home { .. } => {
            let omarchy = if silent {
                format!("o.workspaces.move_window({slot}, false)")
            } else {
                format!("o.workspaces.move_window({slot})")
            };
            hypr.dispatch(&omarchy)
                .or_else(|_| hypr.dispatch(&hypr::move_window(&slot.to_string(), !silent)))?
        }
    }
    if !silent {
        remember(here.space(), key);
    }
    Ok(())
}

// ---------------------------------------------------------------- spaces

/// A terminal window in a space, the folder its shell is in and what it
/// runs.
#[derive(Debug, Clone)]
pub(crate) struct Terminal {
    address: String,
    /// The tab it's on; 0 for a `home` workspace that has no number.
    tab: u8,
    pid: u32,
    shell: terminal::Shell,
}

/// A window on one of a space's workspaces, `home`'s included, which may be
/// a terminal.
#[derive(Debug, Clone)]
pub(crate) struct SpaceWindow {
    pub address: String,
    /// The process that runs it, and which of its windows it is.
    pub place: terminal::Place,
    pub space: String,
    /// The tab it's on, by its key.
    pub tab: Option<u8>,
    pub class: String,
    rect: Rect,
    pub floating: bool,
    fullscreen: u8,
    grouped: Vec<String>,
    /// How long ago it had focus.
    pub focus: i64,
}

/// A monitor and the workspace it shows.
#[derive(Debug, Clone)]
pub(crate) struct Screen {
    pub name: String,
    /// Whether it's the one you are on.
    pub focused: bool,
    pub refresh: f64,
    pub shows: Location,
}

/// What a session saves about a tab, besides its windows.
#[derive(Debug, Default)]
pub(crate) struct TabPlace {
    pub monitor: String,
    /// Its monitor's top left corner.
    corner: (i32, i32),
    dwindle: bool,
}

/// What Hyprland says about every space, before any terminal's folder is
/// read: the monitors, and the windows and workspaces each space holds, all
/// as of one moment. Where you are follows from it rather than being kept
/// beside it.
#[derive(Debug, Default)]
pub(crate) struct Snapshot {
    pub screens: Vec<Screen>,
    /// Which block of ten tabs is each monitor's.
    pub blocks: Blocks,
    windows: BTreeMap<String, u32>,
    /// Each space's tabs, by their keys.
    workspaces: BTreeMap<String, BTreeSet<u8>>,
    /// Where a tab shows as another number than its own, by space and key:
    /// an unplugged monitor's, on the one that's left (`monitors::slots`).
    guests: BTreeMap<String, BTreeMap<u8, u8>>,
    /// The windows on spaces' workspaces, `home`'s included.
    pub space_windows: Vec<SpaceWindow>,
    /// Every window, spaces' or not, for the browser's to be told apart.
    pub browser_windows: Vec<browser::Window>,
    /// Where the tabs are, by space and key.
    pub places: BTreeMap<(String, u8), TabPlace>,
    /// How far a window keeps from its tile's edge.
    border: i32,
    /// What grouped windows give up at the top for the group's bar.
    group_bar: i32,
}

/// The snapshot, which of its windows are terminals and where, and what
/// the agents in them are doing.
#[derive(Default)]
pub(crate) struct Live {
    active: Option<String>,
    windows: BTreeMap<String, u32>,
    workspaces: BTreeMap<String, BTreeSet<u8>>,
    guests: BTreeMap<String, BTreeMap<u8, u8>>,
    terminals: BTreeMap<String, Vec<Terminal>>,
    /// Windows that reported a native status via `space status` without being
    /// recognized as a terminal in `/proc`.
    extra_agents: Vec<(String, terminal::Agent)>,
    /// Every window's address, by space.
    addresses: BTreeMap<String, Vec<String>>,
    /// Agents' statuses, by window address.
    pub statuses: HashMap<String, Status>,
}

impl Live {
    /// Each window's status from what its agent reported, `seen` saying
    /// whether you've seen its turn end.
    pub fn set_statuses(&mut self, seen: impl Fn(&terminal::Agent) -> bool) {
        self.statuses = self
            .terminals
            .values()
            .flatten()
            .filter_map(|t| Some((t.address.as_str(), t.shell.agent.as_ref()?)))
            .chain(
                self.extra_agents
                    .iter()
                    .map(|(address, agent)| (address.as_str(), agent)),
            )
            .filter_map(|(address, agent)| {
                let status = status::from_report(&agent.word, seen(agent))?;
                Some((address.to_string(), status))
            })
            .collect();
    }

    /// The agents of the windows on screen, by window address.
    pub fn agents(&self) -> impl Iterator<Item = (&str, &terminal::Agent)> {
        self.terminals
            .values()
            .flatten()
            .filter_map(|t| Some((t.address.as_str(), t.shell.agent.as_ref()?)))
            .chain(
                self.extra_agents
                    .iter()
                    .map(|(address, agent)| (address.as_str(), agent)),
            )
    }

    /// The spaces Hyprland has, `home` aside.
    fn spaces(&self) -> BTreeSet<String> {
        space_ids(&self.workspaces, &self.windows)
    }
}

impl Snapshot {
    /// The workspace you are on.
    fn here(&self) -> Option<&Location> {
        self.screens.iter().find(|s| s.focused).map(|s| &s.shows)
    }

    /// The space you are in.
    pub fn active(&self) -> Option<&str> {
        self.here().map(Location::space)
    }

    /// The tab you are on.
    fn active_tab(&self) -> Option<u8> {
        self.here().and_then(Location::key)
    }

    /// The spaces Hyprland has, `home` aside.
    fn spaces(&self) -> BTreeSet<String> {
        space_ids(&self.workspaces, &self.windows)
    }
}

/// The spaces with a workspace or a window, `home` aside.
fn space_ids<A, B>(
    workspaces: &BTreeMap<String, A>,
    windows: &BTreeMap<String, B>,
) -> BTreeSet<String> {
    workspaces
        .keys()
        .chain(windows.keys())
        .filter(|space| *space != HOME)
        .cloned()
        .collect()
}

pub(crate) fn snapshot(hypr: &Hypr) -> Result<Snapshot> {
    let mut snapshot = Snapshot {
        border: hypr.option("general:border_size").unwrap_or(0),
        blocks: Blocks::load(),
        ..Default::default()
    };
    let hypr::Everything {
        monitors,
        workspaces,
        clients,
    } = hypr.everything()?;
    snapshot.screens = monitors
        .iter()
        .filter_map(|monitor| {
            let shows = monitor.shows.as_ref()?;
            Some(Screen {
                name: monitor.name.clone(),
                focused: monitor.focused,
                refresh: monitor.refresh,
                shows: name::locate(shows.id, &shows.name),
            })
        })
        .collect();
    for ws in workspaces {
        // Special workspaces (scratchpad and the like) belong to no space.
        if ws.name.starts_with("special:") {
            continue;
        }
        let location = name::locate(ws.id, &ws.name);
        let entry = snapshot
            .workspaces
            .entry(location.space().to_string())
            .or_default();
        if let Some(tab) = location.key() {
            entry.insert(tab);
            let monitor = monitors.iter().find(|m| m.name == ws.monitor);
            snapshot.places.insert(
                (location.space().to_string(), tab),
                TabPlace {
                    corner: monitor.map_or((0, 0), |m| (m.x, m.y)),
                    dwindle: ws.layout == "dwindle",
                    monitor: ws.monitor,
                },
            );
        }
    }
    snapshot.guests = guests(&snapshot.places, &snapshot.blocks);
    if clients.iter().any(|c| c.grouped.len() > 1) {
        snapshot.group_bar = group_bar(hypr);
    }
    let mut whose = terminal::places(&clients);
    for client in clients {
        if let Ok(pid) = u32::try_from(client.pid) {
            snapshot.browser_windows.push(browser::Window {
                address: client.address.clone(),
                pid,
                class: client.class.clone(),
                title: client.title.clone(),
                opened: client.opened(),
            });
        }
        // Windows not on any workspace yet report id -1 and no name.
        if client.workspace.name.is_empty() || client.workspace.name.starts_with("special:") {
            continue;
        }
        let location = name::locate(client.workspace.id, &client.workspace.name);
        *snapshot
            .windows
            .entry(location.space().to_string())
            .or_default() += 1;
        if let Some(place) = whose.remove(&client.address) {
            snapshot.space_windows.push(SpaceWindow {
                address: client.address,
                place,
                space: location.space().to_string(),
                tab: location.key(),
                class: client.class,
                rect: Rect {
                    x: client.at.0,
                    y: client.at.1,
                    w: client.size.0,
                    h: client.size.1,
                },
                floating: client.floating,
                fullscreen: client.fullscreen,
                grouped: client.grouped,
                focus: client.focus,
            });
        }
    }
    Ok(snapshot)
}

/// The tabs that show as another number than their own.
fn guests(
    places: &BTreeMap<(String, u8), TabPlace>,
    blocks: &Blocks,
) -> BTreeMap<String, BTreeMap<u8, u8>> {
    let mut strips: BTreeMap<(&str, u8), Vec<u8>> = BTreeMap::new();
    for ((space, key), place) in places {
        if space != HOME
            && let Some(block) = blocks.block(&place.monitor)
        {
            strips.entry((space, block)).or_default().push(*key);
        }
    }
    let mut guests: BTreeMap<String, BTreeMap<u8, u8>> = BTreeMap::new();
    for ((space, block), keys) in strips {
        for (key, slot) in monitors::slots(keys, block) {
            if name::block_of(key) != block {
                let of = guests.entry(space.to_string()).or_default();
                of.insert(key, slot);
            }
        }
    }
    guests
}

/// How high a group's bar is, as Hyprland works it out. A stacked one depends
/// on how many windows it has, and isn't accounted for.
fn group_bar(hypr: &Hypr) -> i32 {
    let option = |name: &str| hypr.option(&format!("group:groupbar:{name}")).unwrap_or(0);
    if option("enabled") == 0 || option("stacked") == 1 {
        return 0;
    }
    let text = option("gradients") == 1 || option("render_titles") == 1;
    option("gaps_out") * (1 + option("keep_upper_gap"))
        + option("indicator_height")
        + option("indicator_gap")
        + if text { option("height") } else { 0 }
}

/// Makes the live view from a snapshot, with `shell` saying which windows
/// are terminals, which folder each one's shell is in and what it runs, and
/// `reports` holding native agent statuses reported through `space status`.
pub(crate) fn live_from(
    snapshot: &Snapshot,
    mut shell: impl FnMut(&SpaceWindow) -> Option<terminal::Shell>,
    reports: &HashMap<String, status::Report>,
) -> Live {
    let mut terminals: BTreeMap<String, Vec<Terminal>> = BTreeMap::new();
    let mut extra_agents: Vec<(String, terminal::Agent)> = Vec::new();
    let mut addresses: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for window in &snapshot.space_windows {
        addresses
            .entry(window.space.clone())
            .or_default()
            .push(window.address.clone());
        let report = reports.get(&window.address);
        if let Some(mut shell) = shell(window) {
            if let Some(report) = report
                && shell.agent.as_ref().is_none_or(|a| a.word.is_empty())
            {
                shell.agent = Some(terminal::Agent::from_native(&window.address, report));
            }
            terminals
                .entry(window.space.clone())
                .or_default()
                .push(Terminal {
                    address: window.address.clone(),
                    tab: window.tab.unwrap_or(0),
                    pid: window.place.pid,
                    shell,
                });
        } else if let Some(report) = report {
            extra_agents.push((
                window.address.clone(),
                terminal::Agent::from_native(&window.address, report),
            ));
        }
    }
    // Oldest first, as far as Hyprland lets us tell: by tab, then process.
    for list in terminals.values_mut() {
        list.sort_by_key(|t| (t.tab, t.pid));
    }
    Live {
        active: snapshot.active().map(str::to_string),
        windows: snapshot.windows.clone(),
        workspaces: snapshot.workspaces.clone(),
        guests: snapshot.guests.clone(),
        terminals,
        extra_agents,
        addresses,
        statuses: HashMap::new(),
    }
}

fn live(hypr: &Hypr) -> Result<Live> {
    let core = core::list().ok().map(core::Terminals::new);
    let reports = status::load_reports(&paths::status_dir());
    let mut live = live_from(
        &snapshot(hypr)?,
        |window| terminal::shell_of(window.place, core.as_ref(), None),
        &reports,
    );
    // What you've seen, as `watch` notes it on core's terminals and native reports.
    live.set_statuses(terminal::Agent::seen);
    Ok(live)
}

/// One space, as `space ls` and the sidebar show it.
#[derive(Debug, Serialize)]
pub struct Row {
    /// What Hyprland's workspaces are named after: `s1` for `s1:1`.
    pub id: String,
    /// What to call it: the name you gave it, or its folder's.
    pub label: String,
    /// Whether you gave it the name; otherwise it follows its folder.
    pub named: bool,
    /// The glyph or emoji you picked for its badge, if any.
    pub icon: Option<String>,
    /// The checkout (or folder) its terminal is in. `None` for `home`, and
    /// for a space that never had a terminal.
    #[serde(flatten, serialize_with = "folder::or_null")]
    pub folder: Option<Folder>,
    pub active: bool,
    /// `None` when Hyprland can't be reached.
    pub windows: Option<u32>,
    /// What the space's workspaces that exist right now are numbered: 1-10 on
    /// the first monitor, 11-20 on the next.
    pub workspaces: Option<Vec<u8>>,
    /// The workspace `space go` returns to.
    pub last: u8,
    /// When the space was last left, in Unix milliseconds, or 0 if never.
    /// Larger is more recent; the sidebar starts on the most recent one.
    pub left_at: u64,
    /// Its terminal windows, oldest first. Empty without Hyprland.
    pub terminals: Vec<TerminalRow>,
    /// What most needs you among the agents in its windows, if any is
    /// working, done or waiting.
    pub status: Option<Status>,
    /// The names you gave its tabs, by what their workspaces are numbered.
    pub tabs: BTreeMap<u8, String>,
    /// What its tabs that an unplugged monitor left on another show as there,
    /// by what their workspaces are numbered; 0 for no number.
    pub guests: BTreeMap<u8, u8>,
}

/// A terminal window, as the bar names its tab.
#[derive(Debug, Serialize)]
pub struct TerminalRow {
    /// Hyprland's address for the window: `0x55a4...`.
    pub address: String,
    /// The tab it's on, by its key: what its workspace is numbered. `None` on
    /// a workspace without a number.
    pub tab: Option<u8>,
    /// The folder its shell is in.
    #[serde(flatten)]
    pub folder: Folder,
    /// What runs in it (`claude`, `nvim`), or `None` at the prompt.
    pub program: Option<String>,
    /// What its tab shows: the program, or else the folder's name.
    pub label: String,
    /// What the agent in it is doing, if one reports it and isn't idle.
    pub status: Option<Status>,
}

impl TerminalRow {
    fn new(t: &Terminal, live: &Live) -> Self {
        Self {
            address: t.address.clone(),
            tab: (t.tab > 0).then_some(t.tab),
            folder: t.shell.folder.clone(),
            program: t.shell.program.clone(),
            label: t.shell.label(),
            status: live.statuses.get(&t.address).copied(),
        }
    }
}

/// Brings what the store knows about a space up to date with its terminals,
/// and says what to call it and where it is. A space follows one terminal,
/// its first, for as long as that window lives, then the next oldest; with
/// none left it keeps the last folder it saw.
fn describe(
    id: &str,
    entry: &mut spaces::Entry,
    terminals: &[Terminal],
) -> (String, Option<Folder>) {
    let followed = entry
        .anchor
        .as_ref()
        .and_then(|anchor| terminals.iter().find(|t| &t.address == anchor))
        .or_else(|| terminals.first());
    if let Some(terminal) = followed {
        entry.anchor = Some(terminal.address.clone());
        entry.folder = Some(terminal.shell.folder.clone());
    } else {
        entry.anchor = None;
    }
    let folder = entry.folder.as_ref().map(Folder::checkout);
    let label = match (&entry.name, &folder) {
        (Some(name), _) => name.clone(),
        (None, Some(folder)) => folder.name(),
        (None, None) => id.to_string(),
    };
    (label, folder)
}

/// Every space: `home` first, then the rest in the order they were made.
/// Without `live` (no Hyprland), the spaces the store remembers. Updates the
/// store: new spaces, the terminals they follow, and gone ones pruned.
pub(crate) fn rows(store: &mut Store, live: Option<&Live>, last: &Last) -> Vec<Row> {
    let ids: Vec<String> = match live {
        Some(live) => {
            let spaces = live.spaces();
            store.prune(&spaces);
            for id in &spaces {
                let open = live.workspaces.get(id);
                let entry = store.entry(id);
                if entry.order == 0 {
                    entry.order = state::now();
                }
                // A tab's name goes with its tab.
                entry
                    .tabs
                    .retain(|tab, _| open.is_some_and(|open| open.contains(tab)));
            }
            if let Some(home) = store.spaces.get_mut(HOME) {
                let open = live.workspaces.get(HOME);
                home.tabs
                    .retain(|tab, _| open.is_some_and(|open| open.contains(tab)));
            }
            let mut ids: Vec<String> = spaces.into_iter().collect();
            ids.sort_by_key(|id| (store.get(id).map_or(0, |e| e.order), id.clone()));
            ids
        }
        None => {
            let mut ids: Vec<String> = store
                .spaces
                .keys()
                .filter(|id| *id != HOME)
                .cloned()
                .collect();
            ids.sort_by_key(|id| (store.get(id).map_or(0, |e| e.order), id.clone()));
            ids
        }
    };

    let row = |id: &str,
               label: String,
               named: bool,
               icon: Option<String>,
               folder: Option<Folder>,
               tabs| {
        let visit = last.get(id);
        Row {
            id: id.to_string(),
            label,
            named,
            icon,
            folder,
            active: live.is_some_and(|l| l.active.as_deref() == Some(id)),
            windows: live.map(|l| l.windows.get(id).copied().unwrap_or(0)),
            workspaces: live.map(|l| {
                l.workspaces
                    .get(id)
                    .map(|slots| slots.iter().copied().collect())
                    .unwrap_or_default()
            }),
            last: visit.map(|v| v.key).unwrap_or(1),
            left_at: visit.map(|v| v.at).unwrap_or(0),
            terminals: live
                .and_then(|l| Some((l, l.terminals.get(id)?)))
                .map(|(l, list)| list.iter().map(|t| TerminalRow::new(t, l)).collect())
                .unwrap_or_default(),
            status: live.and_then(|l| {
                l.addresses
                    .get(id)?
                    .iter()
                    .filter_map(|address| l.statuses.get(address).copied())
                    .max()
            }),
            tabs,
            guests: live
                .and_then(|l| l.guests.get(id))
                .cloned()
                .unwrap_or_default(),
        }
    };

    let home_tabs = store.get(HOME).map(|e| e.tabs.clone()).unwrap_or_default();
    let mut out = vec![row(HOME, HOME.to_string(), false, None, None, home_tabs)];
    for id in ids {
        let terminals = live
            .and_then(|l| l.terminals.get(&id))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let entry = store.entry(&id);
        let named = entry.name.is_some();
        let (label, folder) = match live {
            Some(_) => describe(&id, entry, terminals),
            // Without Hyprland, as last seen, changing nothing.
            None => describe(&id, &mut entry.clone(), &[]),
        };
        let icon = entry.icon.clone();
        let tabs = entry.tabs.clone();
        out.push(row(&id, label, named, icon, folder, tabs));
    }
    out
}

/// Reads the spaces and saves what changed about them. A failure to save is
/// only a warning: the list itself is right.
fn load_rows(hypr: Option<&Hypr>) -> Result<(Vec<Row>, Option<Live>)> {
    let (_, rows, live) = lock_rows(hypr)?;
    Ok((rows, live))
}

/// `load_rows`, with the store it read still locked, to change it further.
/// The windows and their terminals are read before it's locked, so a slow one
/// holds up only this command.
fn lock_rows(hypr: Option<&Hypr>) -> Result<(Store, Vec<Row>, Option<Live>)> {
    let state_dir = paths::state_dir();
    let mut known = Store::load(&state_dir)?.spaces;
    loop {
        let live = match hypr {
            Some(hypr) => Some(live(hypr)?),
            None => None,
        };
        let mut store = Store::lock(&state_dir)?;
        // Another command changed the spaces meanwhile, perhaps adding one
        // that `live` is too old to have, which it would then prune.
        if store.spaces != known {
            known = store.spaces;
            continue;
        }
        let last = Last::load(&state_dir)?;
        let rows = rows(&mut store, live.as_ref(), &last);
        if let Err(e) = store.save() {
            eprintln!("space: warning: {e:#}");
        }
        return Ok((store, rows, live));
    }
}

/// The space `target` means: `home`, an id, or what a space is called (its
/// name, or its folder's), exactly or else ignoring case.
fn resolve(rows: &[Row], target: &str) -> Result<String> {
    if let Some(row) = rows.iter().find(|r| r.id == target) {
        return Ok(row.id.clone());
    }
    for same in [
        (|a: &str, b: &str| a == b) as fn(&str, &str) -> bool,
        |a, b| a.to_lowercase() == b.to_lowercase(),
    ] {
        let found: Vec<&Row> = rows.iter().filter(|r| same(&r.label, target)).collect();
        match found.as_slice() {
            [] => continue,
            [row] => return Ok(row.id.clone()),
            many => bail!(
                "{} spaces are called '{target}'; pick one by id: {}",
                many.len(),
                many.iter()
                    .map(|r| r.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
    let mut message = format!("no space called '{target}'");
    if let Some(close) = name::suggest(target, rows.iter().map(|r| r.label.as_str())) {
        message.push_str(&format!(". Did you mean {close}?"));
    }
    bail!(message)
}

/// The folder you are in: the focused terminal's, else the active space's,
/// else home. One on another machine is passed over.
fn here_folder(hypr: &Hypr) -> Result<PathBuf> {
    if let Some(address) = hypr.active_window_address()?
        && let Some(place) = terminal::places(&hypr.clients()?).get(&address)
        && let Some(folder) = terminal::shell_folder(*place)
        && let Some(dir) = folder.local()
    {
        return Ok(dir.to_path_buf());
    }
    let (store, rows, _) = lock_rows(Some(hypr))?;
    Ok(rows
        .iter()
        .find(|r| r.active)
        .and_then(|r| store.get(&r.id))
        .and_then(|entry| entry.folder.as_ref()?.local())
        .filter(|dir| dir.is_dir())
        .map_or_else(paths::home, Path::to_path_buf))
}

/// The folder a space's new tab opens in: the space's own, its checkout's
/// when it's in one, else the folder you are in. One on another machine is
/// passed over.
fn space_folder(hypr: &Hypr, space: &str) -> Result<PathBuf> {
    let (rows, _) = load_rows(Some(hypr))?;
    match rows
        .iter()
        .find(|r| r.id == space)
        .and_then(|r| r.folder.as_ref()?.local())
        .filter(|dir| dir.is_dir())
    {
        Some(dir) => Ok(dir.to_path_buf()),
        None => here_folder(hypr),
    }
}

/// An id for a new space whose first terminal opens on tab `key`, reserved
/// until that terminal is up.
fn fresh_space(hypr: &Hypr, key: u8) -> Result<String> {
    let (id, _) = reserve::reserve(&paths::reserved_dir(), |reserved| {
        let mut taken = snapshot(hypr)?.spaces();
        let mut store = Store::load(&paths::state_dir())?;
        store.prune(&taken);
        taken.extend(reserved.iter().map(|(space, _)| space.clone()));
        Ok((store.fresh_id(&taken), key))
    })?;
    // A space by this id before may have left a visit behind.
    forget(&id);
    Ok(id)
}

/// `space new [dir]`: a new space in `dir`, by default the folder you are
/// in, with a terminal open there. It is called after the folder that
/// terminal is in, and follows it.
pub fn new(dir: Option<PathBuf>) -> Result<()> {
    let config = Config::load()?;
    let hypr = Hypr::from_env()?;
    let dir = match dir {
        Some(dir) => {
            let cwd = std::env::current_dir().context("can't read the current folder")?;
            let dir = cwd.join(dir);
            let dir = std::fs::canonicalize(&dir)
                .with_context(|| format!("no folder {}", paths::tilde(&dir)))?;
            if !dir.is_dir() {
                bail!("{} is not a folder", paths::tilde(&dir));
            }
            dir
        }
        None => here_folder(&hypr)?,
    };

    // On the monitor you are on.
    let first = name::key_in(Blocks::load().of(&hypr.active_workspace()?), 1);
    let id = fresh_space(&hypr, first)?;
    let workspace = name::workspace_name(&id, first);
    open_terminal_then(&hypr, &config, &dir, &workspace, || switch(&hypr, &id))?;

    // The watcher may have seen the space by now, and made its entry.
    let mut store = Store::lock(&paths::state_dir())?;
    let entry = store.entry(&id);
    entry.folder.get_or_insert(Folder::here(&dir));
    if entry.order == 0 {
        entry.order = state::now();
    }
    store.save()?;
    println!("Opened a space in {}", paths::tilde(&dir));
    Ok(())
}

/// The browser's windows in sessions, latest first, by address.
pub(crate) fn known_browsers(sessions: &[&Session]) -> HashMap<String, Browser> {
    sessions
        .iter()
        .rev()
        .flat_map(|session| &session.spaces)
        .flat_map(|space| &space.windows)
        .filter_map(|w| Some((w.address.clone(), w.browser.clone()?)))
        .collect()
}

/// The session to save: every space with a terminal or a browser's window, in
/// the sidebar's order.
/// `before` are the ones before it, latest first, for how windows were
/// arranged before one of them went fullscreen.
pub(crate) fn session_of(
    instance: &str,
    rows: &[Row],
    live: &Live,
    snapshot: &Snapshot,
    browsers: &Browsers,
    before: &[&Session],
) -> Session {
    let spaces = rows
        .iter()
        .filter_map(|row| {
            let terminals = live.terminals.get(&row.id).into_iter().flatten();
            let mut windows: Vec<session::Window> = terminals
                .filter_map(|t| {
                    let window = snapshot
                        .space_windows
                        .iter()
                        .find(|w| w.address == t.address)?;
                    let conversation = t.shell.conversation.clone();
                    let folder = match &conversation {
                        Some(conversation) => Folder::here(&conversation.folder),
                        None => t.shell.folder.clone(),
                    };
                    Some(session::Window {
                        folder: Some(folder),
                        conversation,
                        ..session_window(snapshot, window, before)?
                    })
                })
                .collect();
            for window in snapshot.space_windows.iter().filter(|w| w.space == row.id) {
                if let Some(browser) = browsers.get(&window.address)
                    && let Some(place) = session_window(snapshot, window, before)
                {
                    windows.push(session::Window {
                        browser: Some(browser.clone()),
                        ..place
                    });
                }
            }
            if windows.is_empty() {
                return None;
            }
            let tabs: BTreeSet<u8> = windows.iter().map(|w| w.tab).collect();
            let layouts = tabs
                .into_iter()
                .map(|tab| {
                    (
                        tab,
                        session_layout(snapshot, &row.id, tab, &windows, before),
                    )
                })
                .collect();
            let last = if row.active {
                snapshot.active_tab()
            } else {
                (row.id != HOME).then_some(row.last)
            };
            Some(session::Space {
                home: row.id == HOME,
                name: row.named.then(|| row.label.clone()),
                icon: row.icon.clone(),
                active: row.active,
                last,
                tabs: row.tabs.clone(),
                layouts,
                folder: row.folder.clone(),
                windows,
            })
        })
        .collect();
    Session {
        instance: instance.to_string(),
        spaces,
    }
}

/// Where a window is, as a session has it.
fn session_window(
    snapshot: &Snapshot,
    window: &SpaceWindow,
    before: &[&Session],
) -> Option<session::Window> {
    let tab = window.tab?;
    let corner = snapshot
        .places
        .get(&(window.space.clone(), tab))
        .map_or((0, 0), |place| place.corner);
    Some(session::Window {
        tab,
        address: window.address.clone(),
        floating: match window.fullscreen {
            _ if !window.floating => None,
            0 => Some(Rect {
                x: window.rect.x - corner.0,
                y: window.rect.y - corner.1,
                ..window.rect
            }),
            // It says where the screen is, not where it goes back to.
            _ => before
                .iter()
                .find_map(|session| session.find(&window.address)?.1.floating),
        },
        fullscreen: window.fullscreen,
        ..Default::default()
    })
}

/// How a tab's windows are arranged: how its tiles are split, less those that
/// aren't saved.
fn session_layout(
    snapshot: &Snapshot,
    space: &str,
    tab: u8,
    saved: &[session::Window],
    before: &[&Session],
) -> session::Layout {
    let on_tab = |w: &&SpaceWindow| w.space == space && w.tab == Some(tab);
    let index = |address: &str| saved.iter().position(|t| t.address == address);
    let mut windows: Vec<&SpaceWindow> = snapshot.space_windows.iter().filter(on_tab).collect();
    // The one that shows comes first in its group.
    windows.sort_by_key(|w| w.focus);
    let place = snapshot.places.get(&(space.to_string(), tab));

    let tiled: Vec<&&SpaceWindow> = windows.iter().filter(|w| !w.floating).collect();
    let tiles = if !place.is_some_and(|p| p.dwindle) {
        None
    } else if tiled.iter().any(|w| w.fullscreen != 0) {
        // It says where the screen is, not its tile. What it's among hasn't
        // changed as long as the same windows are.
        before
            .iter()
            .find_map(|session| layout_before(session, tab, saved, &tiled))
    } else {
        let mut groups: Vec<(Vec<String>, Rect)> = Vec::new();
        for window in &tiled {
            match groups
                .iter_mut()
                .find(|(shown, _)| window.grouped.contains(&shown[0]))
            {
                Some((group, _)) => group.push(window.address.clone()),
                None => {
                    let mut rect = window.rect;
                    if window.grouped.len() > 1 {
                        rect.y -= snapshot.group_bar;
                        rect.h += snapshot.group_bar;
                    }
                    groups.push((vec![window.address.clone()], rect));
                }
            }
        }
        layout::find(&groups, snapshot.border)
            .and_then(|tree| tree.filter_map(&|address| index(address)))
    };
    session::Layout {
        tiles,
        focused: windows.iter().find_map(|w| index(&w.address)),
        monitor: place.map(|p| p.monitor.clone()).filter(|m| !m.is_empty()),
    }
}

/// How a tab's tiles were split before, if by the same windows.
fn layout_before(
    before: &Session,
    tab: u8,
    saved: &[session::Window],
    tiled: &[&&SpaceWindow],
) -> Option<Tree<usize>> {
    let there: BTreeSet<&str> = tiled
        .iter()
        .map(|w| w.address.as_str())
        .filter(|address| saved.iter().any(|t| t.address == *address))
        .collect();
    let (space, _) = before.find(there.first()?)?;
    let tiles = space.layouts.get(&tab)?.tiles.as_ref()?;
    let address = |window: &usize| space.windows.get(*window).map(|t| t.address.as_str());
    let were: BTreeSet<&str> = tiles.windows().into_iter().filter_map(address).collect();
    if were != there {
        return None;
    }
    tiles.filter_map(&|window| {
        let address = address(window)?;
        saved.iter().position(|t| t.address == address)
    })
}

/// `space restore`: the spaces that were up before Hyprland last restarted,
/// in new windows arranged as they were, or those of an earlier session
/// kept in `file`. Each is struck off as it opens, so none opens twice. It
/// takes you back to the space you were in, unless you have started on
/// something else. The plugin runs it as it starts.
pub fn restore(file: Option<PathBuf>, auto: bool) -> Result<()> {
    // A mistake in the config shouldn't cost you your spaces.
    let config = Config::load_or_default();
    if auto && !config.restore.enabled {
        return Ok(());
    }
    let hypr = Hypr::from_env()?;
    let state_dir = paths::state_dir();
    let Some(_lock) = session::lock_restore(&state_dir)? else {
        println!("Already restoring");
        return Ok(());
    };
    session::set_aside(&state_dir, &hypr.instance)?;
    if let Some(file) = file {
        session::set_previous_from(&state_dir, &file)?;
    }
    let Some(mut session) = session::previous(&state_dir)? else {
        println!("Nothing to restore");
        return Ok(());
    };

    let start = hypr.active_workspace()?;
    let mut opened: HashSet<String> = HashSet::new();
    // The terminals of every space first: a browser takes a while to start.
    let mut spaces: Vec<Restoring> = Vec::new();
    let now = state::now();
    while !session.spaces.is_empty() {
        let mut space = Restoring {
            addresses: vec![None; session.spaces[0].windows.len()],
            saved: session.spaces[0].clone(),
            id: String::new(),
            // In the sidebar, they keep their order.
            order: now + spaces.len() as u64,
        };
        if !space.saved.windows.is_empty() {
            space.open_terminals(&hypr, &config, &mut opened, || {
                let mut left = session.clone();
                left.spaces.remove(0);
                session::set_previous(&state_dir, &left)
            })?;
            if !space.has_browser(None) {
                space.note_restored(&hypr)?;
            }
            for key in space.tabs() {
                if !space.has_browser(Some(key)) {
                    space.arrange(&hypr, key);
                }
            }
            if space.addresses.iter().any(Option::is_some) {
                space.describe()?;
            }
            spaces.push(space);
        }
        session.spaces.remove(0);
        session::set_previous(&state_dir, &session)?;
    }

    let active = spaces.iter().find(|space| space.saved.active);
    let back = active.and_then(Restoring::back);
    let mut landed = go_back(&hypr, &start, &opened, back.as_deref(), false)?;
    let mut browser_works = config.restore.browser;
    for space in spaces.iter_mut().filter(|s| s.has_browser(None)) {
        space.open_browsers(&hypr, &mut opened, &mut browser_works);
        space.note_restored(&hypr)?;
        for key in space.tabs() {
            if space.has_browser(Some(key)) {
                space.arrange(&hypr, key);
            }
        }
        if space.addresses.iter().any(Option::is_some) {
            space.describe()?;
        }
        if space.saved.active && !landed {
            landed = go_back(&hypr, &start, &opened, space.back().as_deref(), true)?;
        }
    }
    let count = spaces.len();
    let plural = if count == 1 { "" } else { "s" };
    println!("Restored {count} space{plural}");
    Ok(())
}

/// Takes you to the tab you were on, unless you have started on something
/// else or, unless `empty` will do, nothing is open there yet. Whether it did.
fn go_back(
    hypr: &Hypr,
    start: &hypr::Workspace,
    opened: &HashSet<String>,
    back: Option<&str>,
    empty: bool,
) -> Result<bool> {
    let Some(back) = back else {
        return Ok(false);
    };
    let name = back.strip_prefix("name:").unwrap_or(back);
    let there = |ws: &hypr::Workspace| ws.name == name && ws.windows > 0;
    if !empty && !hypr.workspaces()?.iter().any(there) {
        return Ok(false);
    }
    if still_at(hypr, start, opened)? {
        // A tab that isn't there yet is made on the monitor you are on.
        let blocks = Blocks::load();
        let key = name::locate(back.parse().unwrap_or(0), name).key();
        if let Some(monitor) = key.and_then(|key| blocks.monitor(key))
            && monitor != start.monitor
        {
            hypr.dispatch(&hypr::focus_monitor(monitor))?;
        }
        hypr.dispatch(&hypr::focus_workspace(back))?;
    }
    Ok(true)
}

/// Whether you are where you started, on a workspace that had nothing on it
/// and has nothing but what was `opened` now.
fn still_at(hypr: &Hypr, start: &hypr::Workspace, opened: &HashSet<String>) -> Result<bool> {
    let now = hypr.active_workspace()?;
    Ok(now.name == start.name
        && hypr
            .clients()?
            .iter()
            .filter(|c| c.workspace.name == now.name)
            .all(|c| opened.contains(&c.address)))
}

/// Hyprland's selector for a tab, as a session has it.
fn session_selector(space: &str, tab: u8) -> String {
    format!(
        "{}{}",
        if space == HOME { "" } else { "name:" },
        session_workspace(space, tab)
    )
}

/// Hyprland's name for a tab, as a session has it.
fn session_workspace(space: &str, tab: u8) -> String {
    if space == HOME {
        tab.to_string()
    } else {
        name::workspace_name(space, tab)
    }
}

/// A space being restored.
struct Restoring {
    saved: session::Space,
    id: String,
    /// The windows of `saved`, those open by now.
    addresses: Vec<Option<String>>,
    order: u64,
}

impl Restoring {
    /// Whether any of its windows, or of those on a tab, is a browser's.
    fn has_browser(&self, tab: Option<u8>) -> bool {
        self.saved
            .windows
            .iter()
            .any(|w| w.browser.is_some() && tab.is_none_or(|tab| w.tab == tab))
    }

    fn tabs(&self) -> BTreeSet<u8> {
        self.saved.windows.iter().map(|w| w.tab).collect()
    }

    /// The tab you were on.
    fn back(&self) -> Option<String> {
        let first = self.saved.windows.first()?.tab;
        Some(session_selector(&self.id, self.saved.last.unwrap_or(first)))
    }

    /// Opens its terminals, calling `opening` once the first is on its way, or
    /// when there are none. Their windows are added to `opened`.
    fn open_terminals(
        &mut self,
        hypr: &Hypr,
        config: &Config,
        opened: &mut HashSet<String>,
        opening: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.id = match self.saved.windows.first() {
            Some(first) if !self.saved.home => fresh_space(hypr, first.tab)?,
            _ => HOME.to_string(),
        };
        let mut opening = Some(opening);
        for n in 0..self.saved.windows.len() {
            let window = &self.saved.windows[n];
            if window.browser.is_some() {
                continue;
            }
            let token = format!("{}-{}-{n}", std::process::id(), self.id);
            let opening = || opening.take().map_or(Ok(()), |opening| opening());
            let address = reopen_terminal(hypr, config, &self.id, window, &token, opening)?;
            self.opened(n, address, opened);
        }
        opening.take().map_or(Ok(()), |opening| opening())
    }

    /// Opens its browser windows, for as long as the browser `works`.
    fn open_browsers(&mut self, hypr: &Hypr, opened: &mut HashSet<String>, works: &mut bool) {
        for n in 0..self.saved.windows.len() {
            let window = &self.saved.windows[n];
            let Some(browser) = window.browser.as_ref().filter(|_| *works) else {
                continue;
            };
            // Nothing it can open: no fault of the browser's.
            let Some(command) = browser.command(config::app_launcher()) else {
                continue;
            };
            // Spaces are worth having without it.
            let address = open_browser(hypr, &self.id, window, browser, &command)
                .inspect_err(|e| eprintln!("space: warning: {e:#}"))
                .unwrap_or(None);
            *works = address.is_some();
            self.opened(n, address, opened);
        }
    }

    fn opened(&mut self, n: usize, address: Option<String>, opened: &mut HashSet<String>) {
        if let Some(address) = &address {
            let workspace = session_workspace(&self.id, self.saved.windows[n].tab);
            reserve::release(&paths::reserved_dir(), &workspace);
            opened.insert(address.clone());
        }
        self.addresses[n] = address;
    }

    fn note_restored(&self, hypr: &Hypr) -> Result<()> {
        let mut restored = self.saved.clone();
        for (window, address) in restored.windows.iter_mut().zip(&self.addresses) {
            window.address = address.clone().unwrap_or_default();
        }
        session::add_restored(&paths::state_dir(), &hypr.instance, restored)
    }

    /// Arranges a tab's windows as they were.
    fn arrange(&self, hypr: &Hypr, key: u8) {
        // Everything is open; only where is off.
        if let Err(e) = arrange(hypr, &self.id, key, &self.saved, &self.addresses) {
            eprintln!("space: warning: {e:#}");
        }
    }

    /// Gives it back its name, its icon and its tabs' names. Again once more
    /// tabs are up, as the watcher forgets the names of tabs that aren't, and
    /// may have made the space's entry by now.
    fn describe(&self) -> Result<()> {
        let saved = &self.saved;
        let mut store = Store::lock(&paths::state_dir())?;
        let entry = store.entry(&self.id);
        for (tab, name) in &saved.tabs {
            entry.tabs.entry(*tab).or_insert_with(|| name.clone());
        }
        if !saved.home {
            entry.name = entry.name.take().or_else(|| saved.name.clone());
            entry.icon = entry.icon.take().or_else(|| saved.icon.clone());
            entry.folder = entry.folder.take().or_else(|| saved.folder.clone());
            entry.order = self.order;
        }
        store.save()?;
        drop(store);
        if let Some(last) = saved.last.filter(|_| !saved.home) {
            remember(&self.id, last);
        }
        Ok(())
    }
}

/// Opens a saved terminal, calling `opening` once it's on its way, and
/// returns its window once it's up. `token` tells its window from others.
fn reopen_terminal(
    hypr: &Hypr,
    config: &Config,
    id: &str,
    terminal: &session::Window,
    token: &str,
    opening: impl FnOnce() -> Result<()>,
) -> Result<Option<String>> {
    let workspace = session_workspace(id, terminal.tab);
    let events = hypr.events()?;
    hypr.dispatch(&hypr::exec_on(
        &restored_command(config, terminal, token),
        &format!("{} silent", session_selector(id, terminal.tab)),
    ))?;
    opening()?;
    // You may be opening windows of your own there by now.
    let ours = |address: &str| {
        hypr.clients().is_ok_and(|clients| {
            clients
                .iter()
                .find(|c| c.address == address)
                .and_then(|c| u32::try_from(c.pid).ok())
                .is_some_and(|pid| terminal::was_given(pid, shell::RESTORE, token))
        })
    };
    let window = wait_for_window(&events, &workspace, RESTORE_TIMEOUT, ours);
    match &window {
        Some(address) => {
            if let Err(e) = hypr.dispatch(&hypr::animate_again(address)) {
                eprintln!("space: warning: {e:#}");
            }
        }
        None => eprintln!("space: warning: no terminal yet on {workspace}"),
    }
    Ok(window)
}

/// Opens a saved browser window, as `reopen_terminal` does a terminal. A
/// browser that's running already opens it itself, from a process Hyprland
/// can't tell is ours, so a rule has the next such window open there.
fn open_browser(
    hypr: &Hypr,
    id: &str,
    window: &session::Window,
    browser: &Browser,
    command: &str,
) -> Result<Option<String>> {
    if browser::restores_itself(&browser.profile) && !browser::is_running() {
        eprintln!("space: warning: Chromium opens its last windows itself; leaving it to that");
        return Ok(None);
    }
    let workspace = session_workspace(id, window.tab);
    let rule = |enabled| {
        let on = format!("{} silent", session_selector(id, window.tab));
        hypr.eval(&hypr::open_class_on(browser.class_pattern(), &on, enabled))
    };
    let events = hypr.events()?;
    rule(true)?;
    let launched = hypr.dispatch(&hypr::exec(command));
    let opened = launched.map(|()| {
        let ours = |address: &str| {
            hypr.clients().is_ok_and(|clients| {
                clients
                    .iter()
                    .any(|c| c.address == address && browser.is_class(&c.class))
            })
        };
        wait_for_window(&events, &workspace, BROWSER_TIMEOUT, ours)
    });
    rule(false)?;
    let opened = opened?;
    if opened.is_none() {
        eprintln!("space: warning: no browser window yet on {workspace}");
    }
    Ok(opened)
}

/// How long a browser has to show its window: it may be starting.
const BROWSER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// The command for a saved terminal.
fn restored_command(config: &Config, terminal: &session::Window, token: &str) -> String {
    // A folder that's gone gives way to the nearest one still there, and one
    // on another machine to home.
    let dir = terminal
        .folder
        .as_ref()
        .and_then(Folder::local)
        .and_then(|dir| dir.ancestors().find(|dir| dir.is_dir()))
        .map_or_else(paths::home, Path::to_path_buf);
    let resume = terminal
        .conversation
        .as_ref()
        // Its conversations are by folder.
        .filter(|c| config.restore.agents && dir == c.folder)
        .filter(|c| c.command().is_some())
        .map(|c| format!("{} {}", c.agent, c.id));
    let mut env = vec![(shell::RESTORE, token)];
    let command = match &resume {
        Some(resume) => {
            env.push((shell::RESUME, resume));
            config.resuming_terminal()
        }
        None => config.terminal(),
    };
    terminal::command_in(&command, &dir, &env)
}

/// Arranges a restored tab's windows as they were. `addresses` are the
/// terminals' windows, in order, those that showed up.
fn arrange(
    hypr: &Hypr,
    id: &str,
    key: u8,
    saved: &session::Space,
    addresses: &[Option<String>],
) -> Result<()> {
    let default = session::Layout::default();
    let was = saved.layouts.get(&key).unwrap_or(&default);
    let name = session_workspace(id, key);
    let monitors = hypr.monitors().unwrap_or_default();
    let find = || -> Result<Option<hypr::Workspace>> {
        Ok(hypr.workspaces()?.into_iter().find(|ws| ws.name == name))
    };
    let Some(mut workspace) = find()? else {
        return Ok(());
    };
    // On the monitor whose tab it is, whichever its connector is now. `home`'s
    // are where Omarchy has them.
    let blocks = Blocks::load();
    if let Some(monitor) = blocks.monitor(key).or(was.monitor.as_deref())
        && !saved.home
        && workspace.monitor != monitor
        && monitors.iter().any(|m| m.name == monitor)
    {
        hypr.dispatch(&hypr::move_workspace(&session_selector(id, key), monitor))?;
        workspace = find()?.unwrap_or(workspace);
    }

    let window = |n: &usize| addresses.get(*n).cloned().flatten();
    let mut tab = layout::Tab {
        tiles: was
            .tiles
            .as_ref()
            .filter(|_| workspace.layout == "dwindle")
            .and_then(|tiles| tiles.filter_map(&window)),
        focused: was.focused.as_ref().and_then(window),
        ..Default::default()
    };
    let monitor = monitors.iter().find(|m| m.name == workspace.monitor);
    for (n, terminal) in saved.windows.iter().enumerate() {
        let Some(address) = window(&n).filter(|_| terminal.tab == key) else {
            continue;
        };
        if let Some(rect) = terminal.floating {
            tab.floating
                .insert(address.clone(), on_monitor(rect, monitor));
        }
        match terminal.fullscreen {
            0 => {}
            1 => drop(tab.fullscreen.insert(address, "maximized")),
            _ => drop(tab.fullscreen.insert(address, "fullscreen")),
        }
    }
    if tab.is_plain() {
        return Ok(());
    }
    hypr.eval(&tab.script())
}

/// A floating window's place on its monitor, which may be smaller than the
/// one it was on.
fn on_monitor(rect: Rect, monitor: Option<&hypr::Monitor>) -> Rect {
    let Some(monitor) = monitor else {
        return rect;
    };
    let (wide, high) = monitor.size();
    let (w, h) = (rect.w.min(wide), rect.h.min(high));
    Rect {
        x: monitor.x + rect.x.clamp(0, wide - w),
        y: monitor.y + rect.y.clamp(0, high - h),
        w,
        h,
    }
}

/// How long a restored terminal has to show up before the next one opens:
/// longer than usual, as everything is slow right after logging in.
const RESTORE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Opens a terminal in `dir` on `workspace` (a space's tab) while you stay
/// where you are, then goes there with `arrive` once its window is up, so you
/// never see the tab empty. Hyprland places it by an exec rule, which follows
/// the process it starts down to the terminal. The rule also skips its
/// pop-in, which would otherwise still be fading it in from nothing as you
/// arrive; once you're there it animates like any other window. The tab,
/// if reserved, is released once its window is up.
fn open_terminal_then(
    hypr: &Hypr,
    config: &Config,
    dir: &Path,
    workspace: &str,
    arrive: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let events = hypr.events().ok();
    let command = terminal::command_in(&config.terminal(), dir, &[]);
    let placed = hypr
        .dispatch(&hypr::exec_on(
            &command,
            &format!("name:{workspace} silent"),
        ))
        .is_ok();
    let window = match (placed, events) {
        (true, Some(events)) => wait_for_window(&events, workspace, OPEN_TIMEOUT, |_| true),
        (true, None) => None,
        (false, _) => {
            terminal::launch(&config.terminal(), dir)?;
            None
        }
    };
    if window.is_some() {
        reserve::release(&paths::reserved_dir(), workspace);
    }
    arrive()?;
    if let Some(address) = window
        && let Err(e) = hypr.dispatch(&hypr::animate_again(&address))
    {
        eprintln!("space: warning: {e:#}");
    }
    Ok(())
}

/// How long a new space or tab waits for its terminal before going there
/// anyway, where the terminal then shows up when it can.
const OPEN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Waits until a window that `ours` accepts by its address opens on
/// `workspace` (Hyprland's `openwindow` event: address, workspace, class,
/// title) and returns its address, or `None` once `timeout` passes.
fn wait_for_window(
    events: &std::sync::mpsc::Receiver<String>,
    workspace: &str,
    timeout: std::time::Duration,
    ours: impl Fn(&str) -> bool,
) -> Option<String> {
    let deadline = std::time::Instant::now() + timeout;
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        let event = events.recv_timeout(left).ok()?;
        if let Some(data) = event.strip_prefix("openwindow>>") {
            let mut fields = data.split(',');
            if let (Some(address), Some(on)) = (fields.next(), fields.next())
                && on == workspace
            {
                let address = format!("0x{}", address.trim_start_matches("0x"));
                if ours(&address) {
                    return Some(address);
                }
            }
        }
    }
    None
}

/// `space status <state>`: reports what an agent in a terminal window is doing
/// (`working`, `waiting`, `done`, or `idle`/`clear`), without needing
/// herdr-core. Finds the owning Hyprland window by walking the process tree
/// upward unless `--window`, `--pid`, or `SPACES_WINDOW` specifies one.
///
/// Outside Hyprland (or outside any Hyprland window when auto-detecting),
/// exits silently so agent hooks don't fail in headless or SSH sessions.
pub fn status(raw_word: &str, window: Option<String>, pid: Option<u32>) -> Result<()> {
    let word = status::parse_word(raw_word)?;
    let (address, owner_pid) = if let Some(window) = window {
        (status::normalize_address(&window)?, None)
    } else if pid.is_none()
        && let Some(env_window) = std::env::var_os("SPACES_WINDOW").filter(|s| !s.is_empty())
    {
        (
            status::normalize_address(&env_window.to_string_lossy())?,
            None,
        )
    } else {
        if pid.is_none()
            && std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
                .filter(|s| !s.is_empty())
                .is_none()
        {
            return Ok(());
        }
        let hypr = Hypr::from_env()?;
        let clients = hypr.clients()?;
        let start = pid.unwrap_or_else(std::process::id);
        let chain = terminal::ancestors(start);
        let matched = chain.iter().find_map(|&anc| {
            let mut hits: Vec<&hypr::Client> = clients
                .iter()
                .filter(|c| u32::try_from(c.pid).ok() == Some(anc))
                .collect();
            hits.sort_by_key(|c| c.opened());
            (!hits.is_empty()).then_some((anc, hits))
        });
        let Some((window_pid, hits)) = matched else {
            if let Some(pid) = pid {
                bail!("no Hyprland window for pid {pid}");
            }
            return Ok(());
        };
        let address = if hits.len() == 1 {
            hits[0].address.clone()
        } else if let Some(nth) = terminal::window_with(window_pid, hits.len(), &chain) {
            hits[nth].address.clone()
        } else {
            let active = hypr.active_window_address().ok().flatten();
            active
                .as_deref()
                .and_then(|active| hits.iter().find(|c| c.address == active))
                .unwrap_or(&hits[0])
                .address
                .clone()
        };
        let owner = if pid.is_none() {
            terminal::owner_pid(&chain, window_pid)
        } else {
            None
        };
        (status::normalize_address(&address)?, owner)
    };
    status::write_report(&paths::status_dir(), &address, word, owner_pid)
}

/// `space terminal`: a terminal in the folder you are in, so an empty tab of
/// a space opens on its folder rather than home.
pub fn open_terminal() -> Result<()> {
    let config = Config::load()?;
    let dir = match Hypr::from_env() {
        Ok(hypr) => here_folder(&hypr)?,
        Err(_) => paths::home(),
    };
    terminal::launch(&config.terminal(), &dir)
}

/// `space go [space]`: the space's last-focused workspace, or its first. With
/// no space named, a picker.
pub fn go(target: Option<String>) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (rows, _) = load_rows(Some(&hypr))?;
    let id = match target {
        Some(target) => resolve(&rows, &target)?,
        None => match pick(&rows)? {
            Some(id) => id,
            None => return Ok(()),
        },
    };
    switch(&hypr, &id)
}

/// `space next` / `space prev`: the space after or before the active one in
/// the sidebar's order (`home`, then the order they were made in), where you
/// left it. It wraps around at either end.
pub fn step(forward: bool) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let _turn = take_turn()?;
    let (rows, _) = load_rows(Some(&hypr))?;
    match neighbour(&rows, forward) {
        Some(id) => switch(&hypr, &id),
        None => Ok(()),
    }
}

/// Waits for any other step to the next or previous space or tab to finish,
/// until it's dropped. Each is from where you are, so two keypresses handled
/// at once would both go from the same place, and one be lost.
fn take_turn() -> Result<std::fs::File> {
    state::lock(&paths::state_dir(), ".step.lock")
}

/// The space next to the active one, or `None` when there's no other.
fn neighbour(rows: &[Row], forward: bool) -> Option<String> {
    let here = rows.iter().position(|r| r.active)?;
    let count = rows.len();
    let there = if forward {
        (here + 1) % count
    } else {
        (here + count - 1) % count
    };
    (there != here).then(|| rows[there].id.clone())
}

/// Leaves the active space, remembering where, for the target's last
/// workspace. Remembered first, so that whoever is told of the switch
/// (`watch.rs`) already finds what you left.
fn switch(hypr: &Hypr, target: &str) -> Result<()> {
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    if here.space() == target {
        return Ok(());
    }

    let last = Last::load(&paths::state_dir())?;
    let key = landing_tab(hypr, &last, &active, target)?;
    if let Some(left_at) = here.key() {
        remember(here.space(), left_at);
    }
    focus(hypr, target, key)
}

/// The tab you land on going to a space, on the monitor you are on: where you
/// left it, if that was there, else the one with the window you last used
/// there, else its first tab there. The other monitors follow
/// (`hypr/omarchy-spaces.lua`).
fn landing_tab(hypr: &Hypr, last: &Last, active: &hypr::Workspace, target: &str) -> Result<u8> {
    let workspaces = hypr.workspaces()?;
    let strip = Strip::new(&workspaces, target, active, &Blocks::load());
    // There, or where it was, unless another shows there now.
    if let Some(visit) = last.get(target)
        && (strip.slot(visit.key).is_some() || strip.at(name::slot_of(visit.key)) == visit.key)
    {
        return Ok(visit.key);
    }
    if target != HOME || last.get(target).is_some() {
        let tabs: Vec<(u8, &str)> = tabs_on(&workspaces, target, &active.monitor)
            .filter_map(|ws| Some((name::locate(ws.id, &ws.name).key()?, ws.name.as_str())))
            .collect();
        let clients = if tabs.len() > 1 {
            hypr.clients()?
        } else {
            Vec::new()
        };
        let used = |name: &str| {
            let there = clients.iter().filter(|c| c.workspace.name == name);
            there.map(|c| c.focus).filter(|focus| *focus >= 0).min()
        };
        let landing = tabs
            .iter()
            .min_by_key(|(key, name)| (used(name).unwrap_or(i64::MAX), *key));
        if let Some((key, _)) = landing {
            return Ok(*key);
        }
    }
    Ok(strip.at(1))
}

/// Which space `space window send` and `space tab send` send to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceTarget {
    /// What it's called, or its id; `home` for the plain workspaces.
    Named(String),
    Next,
    Prev,
}

impl SpaceTarget {
    pub fn of(raw: &str) -> Self {
        match raw {
            "next" => SpaceTarget::Next,
            "prev" => SpaceTarget::Prev,
            _ => SpaceTarget::Named(raw.to_string()),
        }
    }
}

/// `space window send <space|next|prev>`: the focused window to another space, on
/// the tab you'd land on going there, and you with it unless `silent`. Sent
/// silently, a tab it leaves empty gives way as closing its last window does.
pub fn send_window(target: SpaceTarget, silent: bool) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let _turn = matches!(target, SpaceTarget::Next | SpaceTarget::Prev)
        .then(take_turn)
        .transpose()?;
    if !hypr.has_active_window()? {
        return Ok(());
    }
    let (rows, _) = load_rows(Some(&hypr))?;
    let id = match target {
        SpaceTarget::Named(target) => resolve(&rows, &target)?,
        SpaceTarget::Next | SpaceTarget::Prev => {
            match neighbour(&rows, target == SpaceTarget::Next) {
                Some(id) => id,
                None => return Ok(()),
            }
        }
    };
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    if here.space() == id {
        return Ok(());
    }

    let last = Last::load(&paths::state_dir())?;
    let key = landing_tab(&hypr, &last, &active, &id)?;
    if id == HOME {
        let slot = name::slot_of(key);
        let omarchy = if silent {
            format!("o.workspaces.move_window({slot}, false)")
        } else {
            format!("o.workspaces.move_window({slot})")
        };
        hypr.dispatch(&omarchy)
            .or_else(|_| hypr.dispatch(&hypr::move_window(&key.to_string(), !silent)))?;
    } else {
        hypr.dispatch(&hypr::move_window(&selector(&id, key), !silent))?;
    }

    if silent {
        return emptied(&active.name);
    }
    // Where you left the space, unless that was its last window and it's
    // gone.
    if let Some(left_at) = here.key() {
        let left = here.space();
        let still_there = left == HOME
            || hypr.workspaces()?.iter().any(|ws| {
                ws.windows > 0
                    && matches!(name::locate(ws.id, &ws.name),
                        Location::Space { name, .. } if name == left)
            });
        if still_there {
            remember(left, left_at);
        } else {
            forget(left);
        }
    }
    Ok(())
}

/// `space rename [space] <name>`: gives a space, the active one by default,
/// a name that stays. An empty name has it called after its folder again.
pub fn rename(target: Option<String>, new_name: String) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (mut store, rows, _) = lock_rows(Some(&hypr))?;
    let id = match target {
        Some(target) => resolve(&rows, &target)?,
        None => rows
            .iter()
            .find(|r| r.active)
            .map(|r| r.id.clone())
            .unwrap_or_else(|| HOME.to_string()),
    };
    if id == HOME {
        bail!("'home' is the plain workspaces; it can't be renamed");
    }
    let new_name = spaces::clean_name(&new_name)?;

    store.entry(&id).name = new_name.clone();
    store.save()?;
    match new_name {
        Some(new_name) => println!("Named the space '{new_name}'"),
        None => println!("The space is called after its folder again"),
    }
    Ok(())
}

/// `space icon [space] <icon>`: gives a space, the active one by default, a
/// glyph or an emoji to show on its badge in place of its letters. An empty
/// icon has it show its letters again.
pub fn icon(target: Option<String>, new_icon: String) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (mut store, rows, _) = lock_rows(Some(&hypr))?;
    let id = match target {
        Some(target) => resolve(&rows, &target)?,
        None => rows
            .iter()
            .find(|r| r.active)
            .map(|r| r.id.clone())
            .unwrap_or_else(|| HOME.to_string()),
    };
    if id == HOME {
        bail!("'home' is the plain workspaces; it can't have an icon");
    }
    let new_icon = spaces::clean_icon(&new_icon)?;

    store.entry(&id).icon = new_icon.clone();
    store.save()?;
    match new_icon {
        Some(new_icon) => println!("Gave the space the icon {new_icon}"),
        None => println!("The space shows its letters again"),
    }
    Ok(())
}

/// `space reorder <space> <position>`: moves a space to `position` in the
/// sidebar, counting from 1 below `home`, which stays first. Past the end
/// puts it last.
pub fn reorder(target: String, position: usize) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (mut store, rows, _) = lock_rows(Some(&hypr))?;
    let id = resolve(&rows, &target)?;
    if id == HOME {
        bail!("'home' is the plain workspaces; it stays first");
    }
    if position == 0 {
        bail!("positions start at 1, the first space below home");
    }
    let order: Vec<String> = rows
        .into_iter()
        .map(|r| r.id)
        .filter(|r| r != HOME)
        .collect();
    let order = moved(order, id, position - 1);
    restamp(&mut store, &order);
    store.save()?;
    Ok(())
}

/// `order` with `item` taken out and put back at `index`, or last.
fn moved<T: PartialEq>(mut order: Vec<T>, item: T, index: usize) -> Vec<T> {
    order.retain(|other| *other != item);
    let index = index.min(order.len());
    order.insert(index, item);
    order
}

/// `space tab reorder <space>:<n> <position>`: moves a tab to `position` among
/// its space's tabs, counting from 1, past the end being last. The space
/// keeps the numbers it has tabs on: what's on them moves along, with the
/// names you gave them, and you stay on the tab you're on.
pub fn reorder_tab(tab: String, position: usize) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (mut store, rows, _) = lock_rows(Some(&hypr))?;
    if position == 0 {
        bail!("positions start at 1, the space's first tab");
    }
    let active = hypr.active_workspace()?;
    let Tab { id, key, slot } = tab_or_active(&hypr, &rows, Some(&tab), &active)?;
    // The tabs there are, as the bar shows them: its monitor's.
    let workspaces = hypr.workspaces()?;
    let on = workspaces
        .iter()
        .find(|ws| is_tab(&id, key, ws.id, &ws.name))
        .unwrap_or(&active);
    let strip = Strip::new(&workspaces, &id, on, &Blocks::load());
    let tabs = strip.keys();
    let order = moved(tabs.clone(), key, position - 1);
    // Where each tab that moves goes. Among the monitor's guests, they become
    // its own, on the slots they show as: put somewhere by hand, a tab stays.
    let moves: BTreeMap<u8, u8> = order
        .into_iter()
        .zip(tabs)
        .filter(|(from, to)| from != to)
        .map(|(from, to)| match strip.slot(to) {
            Some(slot) if slot > 0 && id != HOME => (from, strip.own(slot)),
            _ => (from, to),
        })
        .collect();
    let elsewhere = |ws: &&hypr::Workspace| ws.monitor != on.monitor;
    if let Some(taken) = workspaces
        .iter()
        .filter(elsewhere)
        .find(|ws| moves.values().any(|to| is_tab(&id, *to, ws.id, &ws.name)))
    {
        bail!("tab {} is on another monitor", taken.name);
    }
    let Some(&to) = moves.get(&key) else {
        return Ok(());
    };

    if id == HOME {
        reorder_home(&hypr, &active, key, &moves)?;
    } else {
        hypr.eval(&rename_tabs(&id, key, &moves))?;
    }

    if let Some(entry) = store.spaces.get_mut(&id) {
        entry.tabs = std::mem::take(&mut entry.tabs)
            .into_iter()
            .map(|(tab, name)| (*moves.get(&tab).unwrap_or(&tab), name))
            .collect();
        store.save()?;
    }
    drop(store);

    // Where you left the space is still the tab you left.
    if let Err(e) = Last::update(&paths::state_dir(), |last| last.renumber(&id, &moves)) {
        eprintln!("space: warning: {e:#}");
    }
    println!("Moved tab {slot} to {}", name::slot_of(to));
    Ok(())
}

/// The tabs that move in the order to move them, each into the place the one
/// before it left, once the tab `first` is out of the way.
fn in_turn(first: u8, moves: &BTreeMap<u8, u8>) -> Vec<(u8, u8)> {
    let mut waiting = moves.clone();
    waiting.remove(&first);
    let mut steps = Vec::new();
    while let Some((&from, &to)) = waiting.iter().find(|(_, to)| !waiting.contains_key(to)) {
        waiting.remove(&from);
        steps.push((from, to));
    }
    steps
}

/// Where a tab waits for the others to make room. All of it happens between
/// two frames, so nothing else sees it.
const ASIDE: &str = "name:omarchy-spaces-moving";

/// The Lua that renames a space's workspaces as `moves` says, `first` being
/// the tab the others make room for, which leaves their windows as they are.
fn rename_tabs(space: &str, first: u8, moves: &BTreeMap<u8, u8>) -> String {
    let tab = |key: u8| session_selector(space, key);
    let name = |key: u8| session_workspace(space, key);
    let aside = ASIDE.trim_start_matches("name:").to_string();
    let mut renames = vec![(tab(first), aside)];
    renames.extend(
        in_turn(first, moves)
            .into_iter()
            .map(|(from, to)| (tab(from), name(to))),
    );
    renames.push((ASIDE.to_string(), name(moves[&first])));
    renames
        .iter()
        .map(|(from, to)| format!("hl.dispatch({})\n", hypr::rename_workspace(from, to)))
        .collect()
}

/// Moves what's on `home`'s tabs as `moves` says, `first` being the tab the
/// others make room for. Their workspaces are Hyprland's numbered ones, which
/// keep their numbers, so each window moves, and the tiles are split again as
/// they were.
fn reorder_home(
    hypr: &Hypr,
    active: &hypr::Workspace,
    first: u8,
    moves: &BTreeMap<u8, u8>,
) -> Result<()> {
    let snapshot = snapshot(hypr)?;
    let tab = |key: u8| session_selector(HOME, key);
    let tabs: BTreeMap<u8, MovingTab> = moves
        .keys()
        .map(|key| {
            let on_tab = |w: &&SpaceWindow| w.space == HOME && w.tab == Some(*key);
            let windows: Vec<&SpaceWindow> = snapshot.space_windows.iter().filter(on_tab).collect();
            (*key, MovingTab::of(&snapshot, HOME, *key, &windows))
        })
        .collect();

    let mut script = String::from(
        "local animated = hl.get_config(\"animations.enabled\")\n\
         hl.config({ animations = { enabled = false } })\n\
         local ok, err = pcall(function()\n",
    );
    script.push_str(&tabs[&first].moves(ASIDE));
    for (from, to) in in_turn(first, moves) {
        script.push_str(&tabs[&from].moves(&tab(to)));
    }
    script.push_str(&tabs[&first].moves(&tab(moves[&first])));
    let here = u8::try_from(active.id)
        .ok()
        .filter(|_| matches!(name::locate(active.id, &active.name), Location::Home { .. }));
    if let Some(to) = here.and_then(|here| moves.get(&here)) {
        script.push_str(&format!(
            "hl.dispatch({})\n",
            hypr::focus_workspace(&tab(*to))
        ));
    }
    for moving in tabs.values().filter(|t| !t.layout.is_plain()) {
        script.push_str(&format!("do\n{}end\n", moving.layout.script()));
    }
    script.push_str(
        "end)\n\
         hl.config({ animations = { enabled = animated } })\n\
         if not ok then error(err) end\n",
    );
    hypr.eval(&script)
}

/// Puts the spaces in `order`, handing out the places they have again, made
/// to rise strictly so no two tie. New spaces, whose places are the time now,
/// still come after them all.
fn restamp(store: &mut Store, order: &[String]) {
    let mut places: Vec<u64> = order
        .iter()
        .map(|id| store.get(id).map_or(0, |e| e.order))
        .collect();
    places.sort_unstable();
    let mut last = 0;
    for (id, place) in order.iter().zip(places) {
        let place = place.max(last + 1);
        store.entry(id).order = place;
        last = place;
    }
}

/// A space's tab, and the number it shows.
struct Tab {
    id: String,
    key: u8,
    slot: u8,
}

/// The tab given as `<space>:<n>` (`s1:2`, `ren:2`), `n` being its key, or
/// with none, the one you are on.
fn tab_or_active(
    hypr: &Hypr,
    rows: &[Row],
    tab: Option<&str>,
    active: &hypr::Workspace,
) -> Result<Tab> {
    let here = name::locate(active.id, &active.name);
    let (space, key) = match tab {
        Some(tab) => tab
            .rsplit_once(':')
            .and_then(|(space, key)| Some((space, name::parse_key(key)?)))
            .with_context(|| format!("'{tab}' isn't a tab: give it as s1:2"))?,
        None => (here.space(), here.key().context("you aren't on a tab")?),
    };
    let id = resolve(rows, space)?;
    let row = rows.iter().find(|r| r.id == id).expect("resolved");
    let open = row.workspaces.as_deref().unwrap_or_default();
    if !open.contains(&key) {
        bail!("'{}' has no tab {key}", row.label);
    }
    let workspaces = hypr.workspaces()?;
    let on = workspaces
        .iter()
        .find(|ws| is_tab(&id, key, ws.id, &ws.name))
        .unwrap_or(active);
    let shown = Strip::new(&workspaces, &id, on, &Blocks::load()).slot(key);
    let slot = shown.filter(|slot| *slot > 0).unwrap_or(name::slot_of(key));
    Ok(Tab { id, key, slot })
}

/// `space tab rename [<space>:<n>] <name>`: gives a tab, the one you are on
/// by default, a name that stays, in place of its window's. An empty name has
/// it follow its window again, or, in `home`, show just its number.
pub fn rename_tab(tab: Option<String>, new_name: String) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (mut store, rows, _) = lock_rows(Some(&hypr))?;
    let active = hypr.active_workspace()?;
    let Tab { id, key, slot } = tab_or_active(&hypr, &rows, tab.as_deref(), &active)?;
    let new_name = spaces::clean_name(&new_name)?;

    let tabs = &mut store.entry(&id).tabs;
    match &new_name {
        Some(new_name) => tabs.insert(key, new_name.clone()),
        None => tabs.remove(&key),
    };
    store.save()?;
    match (new_name, id == HOME) {
        (Some(new_name), _) => println!("Named tab {slot} '{new_name}'"),
        (None, false) => println!("Tab {slot} is called after its window again"),
        (None, true) => println!("Tab {slot} shows just its number again"),
    }
    Ok(())
}

/// `space close [space]`: closes every window of a space, the active one by
/// default, which ends it. If you are in it, you go back to the space you
/// were in before, or `home`. Each app is asked, as `super+w` asks it, so it
/// can still stop to save.
pub fn close(target: Option<String>) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (rows, live) = load_rows(Some(&hypr))?;
    let live = live.expect("read from Hyprland");
    let id = match target {
        Some(target) => resolve(&rows, &target)?,
        None => live.active.clone().unwrap_or_else(|| HOME.to_string()),
    };
    if id == HOME {
        bail!("'home' is the plain workspaces; close its windows one by one");
    }
    let label = rows
        .iter()
        .find(|r| r.id == id)
        .map_or(id.clone(), |r| r.label.clone());

    let windows: Vec<String> = hypr
        .clients()?
        .into_iter()
        .filter(|c| {
            matches!(name::locate(c.workspace.id, &c.workspace.name),
                Location::Space { name, .. } if name == id)
        })
        .map(|c| c.address)
        .collect();

    if live.active.as_deref() == Some(id.as_str()) {
        let last = Last::load(&paths::state_dir())?;
        let spaces = live.spaces();
        let to = last
            .latest_except(&id, |s| s == HOME || spaces.contains(s))
            .unwrap_or(HOME)
            .to_string();
        switch(&hypr, &to)?;
    }
    for address in &windows {
        hypr.dispatch(&hypr::close_window(address))?;
    }
    forget(&id);
    match windows.len() {
        0 => println!("No windows open in '{label}'"),
        1 => println!("Closed 1 window in '{label}'"),
        n => println!("Closed {n} windows in '{label}'"),
    }
    Ok(())
}

/// `space tab close [<space>:<n>]`: closes every window on a tab, the one you
/// are on by default, which ends it. If you are on it, you go to another of
/// the space's tabs, as when its last window closes (`emptied`), and closing
/// its only tab ends the space. `home` doesn't end: you stay on its last tab.
pub fn close_tab(tab: Option<String>) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (rows, _) = load_rows(Some(&hypr))?;
    let active = hypr.active_workspace()?;
    let Tab { id, key, slot } = tab_or_active(&hypr, &rows, tab.as_deref(), &active)?;
    let on_tab = |ws_id, ws_name: &str| is_tab(&id, key, ws_id, ws_name);
    let windows: Vec<String> = hypr
        .clients()?
        .into_iter()
        .filter(|c| on_tab(c.workspace.id, &c.workspace.name))
        .map(|c| c.address)
        .collect();

    if on_tab(active.id, &active.name) {
        leave_tab(&hypr, &id, key, &active, true)?;
    }
    for address in &windows {
        hypr.dispatch(&hypr::close_window(address))?;
    }
    match windows.len() {
        0 => println!("No windows open on tab {slot}"),
        1 => println!("Closed 1 window on tab {slot}"),
        n => println!("Closed {n} windows on tab {slot}"),
    }
    Ok(())
}

/// Whether a workspace is a space's tab.
fn is_tab(space: &str, key: u8, ws_id: i64, ws_name: &str) -> bool {
    let location = name::locate(ws_id, ws_name);
    location.space() == space && location.key() == Some(key)
}

/// The tab to go to from a space's tab that is ending, on the workspace
/// `from`: the next one with windows on its monitor, else the one before.
fn tab_beside(
    workspaces: &[hypr::Workspace],
    id: &str,
    key: u8,
    from: &hypr::Workspace,
) -> Option<u8> {
    let full = |key: u8| {
        workspaces
            .iter()
            .any(|ws| ws.windows > 0 && is_tab(id, key, ws.id, &ws.name))
    };
    let tabs = Strip::new(workspaces, id, from, &Blocks::load()).keys();
    let at = tabs.iter().position(|tab| *tab == key)?;
    let after = tabs[at + 1..].iter().find(|tab| full(**tab));
    let before = tabs[..at].iter().rev().find(|tab| full(**tab));
    after.or(before).copied()
}

/// `space tab send [<space>:<n>] <space|next|prev>`: moves a tab, the one you
/// are on by default, to another space, after the tabs it has, and goes
/// there with it. Its windows stay arranged as they are and it keeps the name
/// you gave it. A space whose only tab it was ends.
pub fn send_tab(tab: Option<String>, to: SpaceTarget) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let _turn = matches!(to, SpaceTarget::Next | SpaceTarget::Prev)
        .then(take_turn)
        .transpose()?;
    let (rows, _) = load_rows(Some(&hypr))?;
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    let Tab { id, key, slot } = tab_or_active(&hypr, &rows, tab.as_deref(), &active)?;
    let to = match to {
        SpaceTarget::Named(target) => resolve(&rows, &target)?,
        SpaceTarget::Next | SpaceTarget::Prev => match neighbour(&rows, to == SpaceTarget::Next) {
            Some(to) => to,
            None => return Ok(()),
        },
    };
    if to == id {
        return Ok(());
    }
    let snapshot = snapshot(&hypr)?;
    let windows: Vec<&SpaceWindow> = snapshot
        .space_windows
        .iter()
        .filter(|w| w.space == id && w.tab == Some(key))
        .collect();
    if windows.is_empty() {
        bail!("nothing is open on tab {slot}");
    }
    let workspaces = hypr.workspaces()?;
    // It stays on the monitor it's on.
    let from = workspaces
        .iter()
        .find(|ws| is_tab(&id, key, ws.id, &ws.name))
        .unwrap_or(&active);
    let blocks = Blocks::load();
    let was_here = is_tab(&id, key, active.id, &active.name);
    let beside = tab_beside(&workspaces, &id, key, from);
    // Whether the space has anything else, on any monitor.
    let lives_on = snapshot
        .space_windows
        .iter()
        .any(|w| w.space == id && w.tab != Some(key));

    let label = rows.iter().find(|r| r.id == to).map_or("", |r| &r.label);
    let (to_key, reserved) = if to == HOME {
        let strip = Strip::new(&workspaces, HOME, from, &blocks);
        (key_after(&strip, open_in(&workspaces, HOME), HOME)?, None)
    } else {
        let (_, key) = reserve::reserve(&paths::reserved_dir(), |reserved| {
            let workspaces = hypr.workspaces()?;
            let strip = Strip::new(&workspaces, &to, from, &blocks);
            let opening = reserved.iter().filter(|(of, _)| *of == to);
            let taken = opening.map(|(_, key)| *key);
            let taken = open_in(&workspaces, &to).into_iter().chain(taken);
            Ok((to.clone(), key_after(&strip, taken, label)?))
        })?;
        (key, Some(name::workspace_name(&to, key)))
    };
    if from.monitor != active.monitor {
        hypr.dispatch(&hypr::focus_monitor(&from.monitor))?;
    }
    let moved = move_tab(&hypr, &snapshot, (&id, key), (&to, to_key), &windows);
    let went = moved
        .and_then(|()| hypr.dispatch(&hypr::focus_workspace(&session_selector(&to, to_key))))
        // Renamed, the tab you were on says nothing of being in another space.
        .and_then(|()| hypr.eval(hypr::FOLLOW));
    if let Some(reserved) = reserved {
        reserve::release(&paths::reserved_dir(), &reserved);
    }
    went?;

    let row = rows.iter().find(|r| r.id == id);
    if let Some(name) = row.and_then(|r| r.tabs.get(&key)) {
        let mut store = Store::lock(&paths::state_dir())?;
        if let Some(entry) = store.spaces.get_mut(&id) {
            entry.tabs.remove(&key);
        }
        store.entry(&to).tabs.insert(to_key, name.clone());
        store.save()?;
    }

    // Where you left the spaces you did: never on the tab that went.
    if let Some(next) = beside {
        let last = Last::load(&paths::state_dir())?;
        if was_here || last.get(&id).is_some_and(|visit| visit.key == key) {
            remember(&id, next);
        }
    } else if id != HOME && !lives_on {
        forget(&id);
    }
    let ended = here.space() == id && id != HOME && !lives_on;
    if let Some(left_at) = here.key()
        && here.space() != to
        && !was_here
        && !ended
    {
        remember(here.space(), left_at);
    }
    println!("Moved tab {slot} to '{label}'");
    Ok(())
}

/// The tabs of a space that have a workspace now, on any monitor.
fn open_in(workspaces: &[hypr::Workspace], space: &str) -> BTreeSet<u8> {
    workspaces
        .iter()
        .map(|ws| name::locate(ws.id, &ws.name))
        .filter(|location| location.space() == space)
        .filter_map(|location| location.key())
        .collect()
}

/// A new tab for a monitor, in the space called `label`.
fn key_after(strip: &Strip, taken: impl IntoIterator<Item = u8>, label: &str) -> Result<u8> {
    strip.new_tab(taken).with_context(|| {
        format!(
            "all {} tabs of '{label}' are open; close one first",
            name::SLOTS
        )
    })
}

/// Moves a tab's windows to another. Between
/// spaces its workspace is renamed, which leaves them just as they are.
/// Hyprland's numbered workspaces, `home`'s, can't become named ones or the
/// other way around, so then each window moves, and the tiles are split again
/// as they were, all between two frames.
fn move_tab(
    hypr: &Hypr,
    snapshot: &Snapshot,
    (from, key): (&str, u8),
    (to, to_key): (&str, u8),
    windows: &[&SpaceWindow],
) -> Result<()> {
    let selector = session_selector(to, to_key);
    if from != HOME && to != HOME {
        return hypr.dispatch(&hypr::rename_workspace(
            &session_selector(from, key),
            &session_workspace(to, to_key),
        ));
    }

    let moving = MovingTab::of(snapshot, from, key, windows);
    let mut script = moving.moves(&selector);
    // There first: the script comes back to where you were, which may be the
    // tab that just went.
    script.push_str(&format!(
        "hl.dispatch({})\n",
        hypr::focus_workspace(&selector)
    ));
    if !moving.layout.is_plain() {
        script.push_str(&moving.layout.script());
    }
    hypr.eval(&script)
}

/// A tab whose windows move one by one to another.
struct MovingTab {
    /// The windows to move: a group moves as one, by the one that shows.
    windows: Vec<String>,
    /// How its tiles are split, to split them again there.
    layout: layout::Tab,
}

impl MovingTab {
    fn of(snapshot: &Snapshot, space: &str, key: u8, windows: &[&SpaceWindow]) -> Self {
        let mut shown: Vec<&SpaceWindow> = windows.to_vec();
        shown.sort_by_key(|w| w.focus);
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        shown.retain(|w| {
            let first = !seen.contains(w.address.as_str());
            seen.extend(w.grouped.iter().map(String::as_str));
            first
        });
        let saved: Vec<session::Window> = shown
            .iter()
            .filter_map(|w| session_window(snapshot, w, &[]))
            .collect();
        let layout = session_layout(snapshot, space, key, &saved, &[]);
        let address = |n: &usize| saved.get(*n).map(|w| w.address.clone());
        Self {
            windows: shown.iter().map(|w| w.address.clone()).collect(),
            layout: layout::Tab {
                tiles: layout.tiles.as_ref().and_then(|t| t.filter_map(&address)),
                focused: layout.focused.as_ref().and_then(address),
                ..Default::default()
            },
        }
    }

    /// The Lua that moves its windows to a workspace.
    fn moves(&self, selector: &str) -> String {
        self.windows
            .iter()
            .map(|w| format!("hl.dispatch({})\n", hypr::move_window_to(w, selector)))
            .collect()
    }
}

/// Has the monitor showing a space's tab that is ending, on the workspace
/// `from`, go on to its next tab there with windows, else the one before.
/// With none left there, it stays, empty, as long as the space has windows on
/// another monitor. With none on any, the space ends and you go back to the
/// space you were in before it, or `home`, whose own last tab you stay on.
/// `focused` says whether that monitor is the one you are on.
fn leave_tab(hypr: &Hypr, id: &str, key: u8, from: &hypr::Workspace, focused: bool) -> Result<()> {
    let workspaces = hypr.workspaces()?;
    if let Some(next) = tab_beside(&workspaces, id, key, from) {
        if focused {
            focus(hypr, id, next)?;
            remember(id, next);
        } else if let Some(next) = workspaces
            .iter()
            .find(|ws| is_tab(id, next, ws.id, &ws.name))
        {
            hypr.eval(&hypr::show_workspace(&next.name))?;
        }
        return Ok(());
    }
    let elsewhere = |ws: &hypr::Workspace| {
        ws.windows > 0
            && !is_tab(id, key, ws.id, &ws.name)
            && name::locate(ws.id, &ws.name).space() == id
    };
    if id == HOME || workspaces.iter().any(elsewhere) {
        return Ok(());
    }

    let snapshot = snapshot(hypr)?;
    let others: BTreeSet<&str> = snapshot
        .space_windows
        .iter()
        .map(|w| w.space.as_str())
        .filter(|space| *space != id)
        .collect();
    let to = Last::load(&paths::state_dir())?
        .latest_except(id, |s| s == HOME || others.contains(s))
        .unwrap_or(HOME)
        .to_string();
    switch(hypr, &to)?;
    // The space is gone (switching noted where you left it): nothing to go
    // back to.
    forget(id);
    Ok(())
}

/// `space emptied <workspace>`, run by Hyprland (`hypr/omarchy-spaces.lua`) when
/// closing a window left a workspace that a monitor shows empty. An emptied
/// tab doesn't linger: see `leave_tab`. Nothing happens if things moved on
/// since: no monitor shows that workspace, or something opened on it.
pub fn emptied(workspace: &str) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let active = hypr.active_workspace()?;
    let workspaces = hypr.workspaces()?;
    let Some(emptied) = workspaces.iter().find(|ws| ws.name == workspace) else {
        return Ok(());
    };
    let focused = active.name == workspace;
    let shown = focused
        || hypr.monitors()?.iter().any(|monitor| {
            let shows = monitor.shows.as_ref();
            shows.is_some_and(|ws| ws.name == workspace)
        });
    if !shown || emptied.windows > 0 {
        return Ok(());
    }
    let Location::Space { name: id, key } = name::locate(emptied.id, &emptied.name) else {
        return Ok(());
    };
    leave_tab(&hypr, &id, key, emptied, focused)
}

/// What `space ls --json` prints.
#[derive(Serialize)]
struct List<'a> {
    version: u32,
    spaces: &'a [Row],
}

/// `space ls`: every space, the active one marked, with a window count.
pub fn ls(json: bool) -> Result<()> {
    let hypr = match Hypr::from_env() {
        Ok(hypr) => Some(hypr),
        Err(e) => {
            eprintln!("space: no window counts: {e:#}");
            None
        }
    };
    let rows = match load_rows(hypr.as_ref()) {
        Ok((rows, _)) => rows,
        Err(e) if hypr.is_some() => {
            eprintln!("space: no window counts: {e:#}");
            load_rows(None)?.0
        }
        Err(e) => return Err(e),
    };

    let mut out = std::io::stdout().lock();
    if json {
        let list = List {
            version: view::VERSION,
            spaces: &rows,
        };
        serde_json::to_writer_pretty(&mut out, &list)?;
        writeln!(out)?;
    } else {
        write!(out, "{}", format_rows(&rows))?;
    }
    Ok(())
}

fn format_rows(rows: &[Row]) -> String {
    let id_width = rows.iter().map(|r| r.id.len()).max().unwrap_or(0);
    let width = rows
        .iter()
        .map(|r| r.label.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for row in rows {
        let marker = if row.active { '*' } else { ' ' };
        let windows = match row.windows {
            Some(1) => "1 window".to_string(),
            Some(n) => format!("{n} windows"),
            None => "-".to_string(),
        };
        let folder = row.folder.as_ref().map(Folder::display).unwrap_or_default();
        let line = format!(
            "{marker} {:id_width$}  {:width$}  {windows:<11}  {folder}",
            row.id, row.label
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// The picker for `space go` with no space named: most recently left first,
/// the active space last, so Enter goes back to the previous space.
fn pick(rows: &[Row]) -> Result<Option<String>> {
    let config = Config::load()?;
    let mut order: Vec<&Row> = rows.iter().collect();
    order.sort_by(|a, b| {
        a.active
            .cmp(&b.active)
            .then(b.left_at.cmp(&a.left_at))
            .then(a.label.cmp(&b.label))
    });

    let lines = picker_lines(&order);
    let Some(picker) = config.picker() else {
        bail!(
            "no picker found: install vicinae or walker, or set `picker` in {}",
            paths::tilde(&paths::config_file())
        );
    };
    let Some(choice) = run_picker(&picker, &lines.concat())? else {
        return Ok(None);
    };
    Ok(lines
        .iter()
        .position(|line| line.trim() == choice)
        .map(|i| order[i].id.clone()))
}

fn picker_lines(rows: &[&Row]) -> Vec<String> {
    let width = rows
        .iter()
        .map(|r| r.label.chars().count())
        .max()
        .unwrap_or(0);
    rows.iter()
        .map(|row| {
            let mut details = Vec::new();
            match row.windows {
                Some(1) => details.push("1 window".to_string()),
                Some(n) => details.push(format!("{n} windows")),
                None => {}
            }
            if let Some(folder) = &row.folder {
                details.push(folder.display());
            }
            if row.active {
                details.push("current".into());
            }
            format!("{:width$}   {}\n", row.label, details.join(" · "))
        })
        .collect()
}

/// Focuses a space's tab. In `home`, that is Omarchy's own workspace on the
/// focused monitor, or the plain workspace outside Omarchy.
fn focus(hypr: &Hypr, space: &str, key: u8) -> Result<()> {
    if space == HOME {
        hypr.dispatch(&format!("o.workspaces.focus({})", name::slot_of(key)))
            .or_else(|_| hypr.dispatch(&hypr::focus_workspace(&key.to_string())))
    } else {
        hypr.dispatch(&hypr::focus_workspace(&selector(space, key)))
    }
}

fn selector(space: &str, key: u8) -> String {
    format!("name:{}", name::workspace_name(space, key))
}

/// Records the tab the active space is on. Failing to is only a warning: the
/// switch itself has happened.
fn remember(space: &str, key: u8) {
    if let Err(e) = Last::update(&paths::state_dir(), |last| last.set(space, key)) {
        eprintln!("space: warning: {e:#}");
    }
}

/// Forgets where a space was left, as it's gone.
fn forget(space: &str) {
    if let Err(e) = Last::update(&paths::state_dir(), |last| last.remove(space)) {
        eprintln!("space: warning: {e:#}");
    }
}

/// Runs a dmenu-style picker: lines in, the chosen line out. `None` when it
/// was dismissed.
fn run_picker(command: &str, input: &str) -> Result<Option<String>> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("can't run the picker `{command}`"))?;
    // A picker that exits without reading all of its input is fine.
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    let output = child.wait_with_output()?;
    let choice = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((output.status.success() && !choice.is_empty()).then_some(choice))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal(address: &str, tab: u8, pid: u32, cwd: &str) -> Terminal {
        Terminal {
            address: address.into(),
            tab,
            pid,
            shell: terminal::Shell {
                pid: None,
                folder: Folder::here(cwd),
                program: None,
                agent: None,
                conversation: None,
            },
        }
    }

    #[test]
    fn steps_through_the_open_tabs() {
        let step = step_tab;
        assert_eq!(step(true, &[1, 3, 4], Some(3)), Some(4));
        assert_eq!(step(true, &[1, 3, 4], Some(4)), Some(1), "wraps");
        assert_eq!(step(false, &[1, 3, 4], Some(1)), Some(4), "wraps");
        assert_eq!(step(false, &[1, 3, 4], Some(3)), Some(1));
        assert_eq!(step(true, &[1, 11, 3], Some(11)), Some(3), "as they show");
        assert_eq!(step(true, &[2], Some(2)), None, "only one tab");
        assert_eq!(step(true, &[2, 5], None), Some(2));
    }

    #[test]
    fn moves_tabs_into_the_places_others_leave() {
        // A rotation: 3 to the front.
        let moves = [(1, 2), (2, 3), (3, 1)].into();
        assert_eq!(in_turn(3, &moves), [(2, 3), (1, 2)]);
        // A guest to the front, of which nothing takes the place.
        let moves = [(1, 2), (2, 3), (11, 1)].into();
        assert_eq!(in_turn(11, &moves), [(2, 3), (1, 2)]);
        // A tab past a guest.
        let moves = [(1, 3), (2, 1), (11, 2)].into();
        assert_eq!(in_turn(1, &moves), [(2, 1), (11, 2)]);
    }

    #[test]
    fn a_space_follows_its_first_terminal() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("ren");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        let notes = tmp.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        let (repo_s, src_s, notes_s) = (
            repo.to_str().unwrap(),
            repo.join("src").to_str().unwrap().to_string(),
            notes.to_str().unwrap(),
        );

        let mut entry = spaces::Entry::default();
        let (label, folder) = describe("s1", &mut entry, &[]);
        assert_eq!(
            (label.as_str(), folder),
            ("s1", None),
            "never had a terminal"
        );

        // The oldest terminal names it; inside a checkout, the checkout.
        let terminals = [
            terminal("0xa", 1, 10, &src_s),
            terminal("0xb", 1, 20, notes_s),
        ];
        let (label, folder) = describe("s1", &mut entry, &terminals);
        assert_eq!(label, "ren");
        assert_eq!(folder, Some(Folder::here(repo_s)));
        assert_eq!(entry.anchor.as_deref(), Some("0xa"));

        // It keeps following that window even when it's no longer first.
        let terminals = [
            terminal("0xc", 1, 5, notes_s),
            terminal("0xa", 2, 10, notes_s),
        ];
        assert_eq!(describe("s1", &mut entry, &terminals).0, "notes");
        assert_eq!(entry.anchor.as_deref(), Some("0xa"));

        // When it closes, the next oldest; with none, the last folder seen.
        let terminals = [terminal("0xc", 1, 5, repo_s)];
        assert_eq!(describe("s1", &mut entry, &terminals).0, "ren");
        assert_eq!(describe("s1", &mut entry, &[]).0, "ren");
        assert_eq!(entry.anchor, None);

        // A name you give wins over the folder.
        entry.name = Some("Japanese".into());
        assert_eq!(describe("s1", &mut entry, &terminals).0, "Japanese");
    }

    fn row(id: &str, label: &str) -> Row {
        Row {
            id: id.into(),
            label: label.into(),
            named: false,
            icon: None,
            folder: None,
            active: false,
            windows: None,
            workspaces: None,
            last: 1,
            left_at: 0,
            terminals: Vec::new(),
            status: None,
            guests: BTreeMap::new(),
            tabs: BTreeMap::new(),
        }
    }

    #[test]
    fn resolves_ids_and_names() {
        let rows = [
            row("home", "home"),
            row("s1", "ren"),
            row("s2", "Mail"),
            row("s3", "mail"),
        ];
        assert_eq!(resolve(&rows, "s1").unwrap(), "s1");
        assert_eq!(resolve(&rows, "ren").unwrap(), "s1");
        assert_eq!(resolve(&rows, "home").unwrap(), "home");
        assert_eq!(resolve(&rows, "Mail").unwrap(), "s2", "exact case first");
        assert_eq!(resolve(&rows, "REN").unwrap(), "s1");
        let err = resolve(&rows, "MAIL").unwrap_err().to_string();
        assert!(
            err.contains("2 spaces are called 'MAIL'") && err.contains("s2, s3"),
            "{err}"
        );
        let err = resolve(&rows, "rne").unwrap_err().to_string();
        assert!(err.contains("Did you mean ren?"), "{err}");
    }

    #[test]
    fn reorders_spaces() {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let order = ids(&["s1", "s2", "s3"]);
        assert_eq!(
            moved(order.clone(), "s3".to_string(), 0),
            ids(&["s3", "s1", "s2"])
        );
        assert_eq!(
            moved(order.clone(), "s1".to_string(), 1),
            ids(&["s2", "s1", "s3"])
        );
        assert_eq!(
            moved(order.clone(), "s1".to_string(), 9),
            ids(&["s2", "s3", "s1"])
        );

        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load(dir.path()).unwrap();
        store.entry("s1").order = 10;
        store.entry("s2").order = 10;
        store.entry("s3").order = 30;
        restamp(&mut store, &ids(&["s3", "s1", "s2"]));
        let place = |id| store.get(id).unwrap().order;
        assert!(place("s3") < place("s1") && place("s1") < place("s2"));
        assert!(place("s2") <= 30, "{}", place("s2"));
    }

    #[test]
    fn runs_a_picker() {
        assert_eq!(
            run_picker("grep tax", "ren  x\ntax  y\n").unwrap(),
            Some("tax  y".into())
        );
        assert_eq!(run_picker("exit 1", "ren\n").unwrap(), None);
        assert_eq!(run_picker("true", "ren\n").unwrap(), None);
    }
}
