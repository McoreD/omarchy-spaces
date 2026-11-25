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
use crate::hypr::{self, Hypr};
use crate::layout::{self, Rect, Tree};
use crate::name::{self, HOME, Location};
use crate::paths;
use crate::reserve;
use crate::session::{self, Session};
use crate::shell;
use crate::spaces::{self, Store};
use crate::state::{self, Last};
use crate::status::{self, Status};
use crate::terminal;

/// Where `space ws` goes. A space's workspaces are its tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Slot(u8),
    /// A new tab, after the last one.
    New,
    Next,
    Prev,
}

/// `space ws <n|new|next|prev>`: a workspace of the active space. A space's
/// new tab opens with a terminal in the space's folder, as a new space does,
/// so it never shows empty; `home`'s is Omarchy's next workspace, empty.
pub fn ws(target: Target) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    let space = here.space();
    if target == Target::New && space != HOME {
        return new_tab(&hypr, &active, &here);
    }
    let slot = match target {
        Target::Slot(slot) => slot,
        _ => {
            let open = open_slots(&hypr.workspaces()?, &active, &here);
            match pick_slot(target, &open, here.slot())? {
                Some(slot) => slot,
                None => return Ok(()),
            }
        }
    };
    if here.slot() == Some(slot) {
        return Ok(());
    }

    focus(&hypr, space, slot)?;
    remember(space, slot);
    Ok(())
}

/// A new tab in the active space, reserved until its terminal is up.
fn new_tab(hypr: &Hypr, active: &hypr::Workspace, here: &Location) -> Result<()> {
    let config = Config::load()?;
    let space = here.space();
    let dir = space_folder(hypr, space)?;
    let (_, slot) = reserve::reserve(&paths::reserved_dir(), |reserved| {
        let mut open = open_slots(&hypr.workspaces()?, active, here);
        let opening = reserved.iter().filter(|(of, _)| of == space);
        open.extend(opening.map(|(_, slot)| *slot));
        open.sort_unstable();
        open.dedup();
        let slot = pick_slot(Target::New, &open, here.slot())?;
        Ok((space.to_string(), slot.expect("a new tab has a slot")))
    })?;
    let workspace = name::workspace_name(space, slot);
    open_terminal_then(hypr, &config, &dir, &workspace, || focus(hypr, space, slot))?;
    remember(space, slot);
    Ok(())
}

/// The slots of the active space that have a workspace now, in order, the
/// focused one included. In `home`, those of the focused monitor's block of
/// ten, as Omarchy numbers them.
fn open_slots(
    workspaces: &[hypr::Workspace],
    active: &hypr::Workspace,
    here: &Location,
) -> Vec<u8> {
    let block = |id: i64| (id - 1).div_euclid(i64::from(name::SLOTS));
    let mut open: BTreeSet<u8> = workspaces
        .iter()
        .filter_map(|ws| {
            let location = name::locate(ws.id, &ws.name);
            let same_space = match (&location, here) {
                (Location::Space { name: a, .. }, Location::Space { name: b, .. }) => a == b,
                (Location::Home { .. }, Location::Home { .. }) => {
                    ws.id > 0 && block(ws.id) == block(active.id.max(1))
                }
                _ => false,
            };
            if same_space { location.slot() } else { None }
        })
        .collect();
    open.extend(here.slot());
    open.into_iter().collect()
}

/// The slot `new`, `next` or `prev` leads to from `current`, given the open
/// ones; `None` when there's nowhere else to go.
fn pick_slot(target: Target, open: &[u8], current: Option<u8>) -> Result<Option<u8>> {
    let position = current.and_then(|c| open.iter().position(|&s| s == c));
    Ok(match target {
        Target::Slot(slot) => Some(slot),
        Target::New => {
            let after_last = open.last().map_or(1, |&last| last + 1);
            let slot = if after_last <= name::SLOTS {
                Some(after_last)
            } else {
                (1..=name::SLOTS).find(|s| !open.contains(s))
            };
            match slot {
                Some(slot) => Some(slot),
                None => bail!("all {} tabs are open; close one first", name::SLOTS),
            }
        }
        Target::Next | Target::Prev if open.len() < 2 && position.is_some() => None,
        Target::Next => match position {
            Some(i) => Some(open[(i + 1) % open.len()]),
            None => open.first().copied(),
        },
        Target::Prev => match position {
            Some(i) => Some(open[(i + open.len() - 1) % open.len()]),
            None => open.last().copied(),
        },
    })
}

/// `space move <n>`: the focused window to workspace `n` of the active space,
/// following it unless `silent`.
pub fn move_window(slot: u8, silent: bool) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    if here.slot() == Some(slot) || !hypr.has_active_window()? {
        return Ok(());
    }

    match &here {
        Location::Space { name, .. } => {
            hypr.dispatch(&hypr::move_window(&selector(name, slot), !silent))?
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
        remember(here.space(), slot);
    }
    Ok(())
}

// ---------------------------------------------------------------- spaces

/// A terminal window in a space, the folder its shell is in and what it
/// runs.
#[derive(Debug, Clone)]
pub(crate) struct Terminal {
    address: String,
    slot: u8,
    pid: u32,
    shell: terminal::Shell,
}

/// A window on one of a space's workspaces, `home`'s included, which may be
/// a terminal.
#[derive(Debug, Clone)]
pub(crate) struct SpaceWindow {
    pub address: String,
    pub pid: u32,
    pub space: String,
    /// The tab it's on; 0 for a `home` workspace that has no number.
    pub slot: u8,
    /// The tab as a session has it: a slot, or in `home`, a workspace's id.
    tab: Option<u8>,
    rect: Rect,
    floating: bool,
    fullscreen: u8,
    grouped: Vec<String>,
    /// How long ago it had focus.
    focus: i64,
}

/// What a session saves about a tab, besides its windows.
#[derive(Debug, Default)]
struct TabPlace {
    monitor: String,
    /// Its monitor's top left corner.
    corner: (i32, i32),
    dwindle: bool,
}

/// What Hyprland says about every space, before any terminal's folder is
/// read: the active one, and the windows and workspaces each holds.
#[derive(Debug, Default)]
pub(crate) struct Snapshot {
    pub active: Option<String>,
    windows: BTreeMap<String, u32>,
    workspaces: BTreeMap<String, BTreeSet<u8>>,
    /// `home`'s workspaces by id, as its slots repeat on every monitor.
    home: BTreeSet<i64>,
    /// The windows on spaces' workspaces, `home`'s included.
    pub space_windows: Vec<SpaceWindow>,
    /// Every window, spaces' or not, for the browser's to be told apart.
    pub browser_windows: Vec<browser::Window>,
    /// The tab you're on, as a session has it.
    active_tab: Option<u8>,
    /// Where the tabs are, by space and tab as a session has it.
    places: BTreeMap<(String, u8), TabPlace>,
    /// How far a window keeps from its tile's edge.
    border: i32,
    /// What grouped windows give up at the top for the group's bar.
    group_bar: i32,
}

/// The tab a workspace is, as a session has it.
fn session_tab(id: i64, location: &Location) -> Option<u8> {
    match location {
        Location::Space { slot, .. } => Some(*slot),
        Location::Home { .. } => u8::try_from(id).ok().filter(|id| *id > 0),
    }
}

/// The snapshot, which of its windows are terminals and where, and what
/// the agents in them are doing.
#[derive(Default)]
pub(crate) struct Live {
    active: Option<String>,
    windows: BTreeMap<String, u32>,
    workspaces: BTreeMap<String, BTreeSet<u8>>,
    /// `home`'s workspaces by id.
    home: BTreeSet<i64>,
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
    let mut snapshot = Snapshot::default();
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    snapshot.active = Some(here.space().to_string());
    snapshot.active_tab = session_tab(active.id, &here);
    snapshot.border = hypr.option("general:border_size").unwrap_or(0);
    let monitors = hypr.monitors().unwrap_or_default();
    for ws in hypr.workspaces()? {
        // Special workspaces (scratchpad and the like) belong to no space.
        if ws.name.starts_with("special:") {
            continue;
        }
        let location = name::locate(ws.id, &ws.name);
        let entry = snapshot
            .workspaces
            .entry(location.space().to_string())
            .or_default();
        if let Some(slot) = location.slot() {
            entry.insert(slot);
        }
        if location.space() == HOME && ws.id > 0 {
            snapshot.home.insert(ws.id);
        }
        if let Some(tab) = session_tab(ws.id, &location) {
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
    let clients = hypr.clients()?;
    if clients.iter().any(|c| c.grouped.len() > 1) {
        snapshot.group_bar = group_bar(hypr);
    }
    for client in clients {
        if let Ok(pid) = u32::try_from(client.pid) {
            snapshot.browser_windows.push(browser::Window {
                address: client.address.clone(),
                pid,
                class: client.class,
                title: client.title,
                opened: u64::from_str_radix(&client.stable_id, 16).unwrap_or(0),
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
        if let Ok(pid) = u32::try_from(client.pid) {
            snapshot.space_windows.push(SpaceWindow {
                address: client.address,
                pid,
                space: location.space().to_string(),
                slot: location.slot().unwrap_or(0),
                tab: session_tab(client.workspace.id, &location),
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
                    slot: window.slot,
                    pid: window.pid,
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
        list.sort_by_key(|t| (t.slot, t.pid));
    }
    Live {
        active: snapshot.active.clone(),
        windows: snapshot.windows.clone(),
        workspaces: snapshot.workspaces.clone(),
        home: snapshot.home.clone(),
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
        |window| terminal::shell_of(window.pid, core.as_ref()),
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
    pub folder: Option<PathBuf>,
    pub active: bool,
    /// `None` when Hyprland can't be reached.
    pub windows: Option<u32>,
    /// The space's workspaces that exist right now.
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
    /// The names you gave its tabs, by slot.
    pub tabs: BTreeMap<u8, String>,
}

/// A terminal window, as the bar names its tab.
#[derive(Debug, Serialize)]
pub struct TerminalRow {
    /// Hyprland's address for the window: `0x55a4...`.
    pub address: String,
    pub slot: u8,
    /// The folder its shell is in.
    pub folder: PathBuf,
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
            slot: t.slot,
            folder: t.shell.cwd.clone(),
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
) -> (String, Option<PathBuf>) {
    let followed = entry
        .anchor
        .as_ref()
        .and_then(|anchor| terminals.iter().find(|t| &t.address == anchor))
        .or_else(|| terminals.first());
    if let Some(terminal) = followed {
        entry.anchor = Some(terminal.address.clone());
        entry.folder = Some(terminal.shell.cwd.clone());
    } else {
        entry.anchor = None;
    }
    let folder = entry.folder.as_deref().map(terminal::checkout);
    let label = match (&entry.name, &folder) {
        (Some(name), _) => name.clone(),
        (None, Some(folder)) => terminal::folder_name(folder),
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
                if entry.created == 0 {
                    entry.created = state::now();
                }
                // A tab's name goes with its tab.
                entry
                    .tabs
                    .retain(|slot, _| open.is_some_and(|open| open.contains(slot)));
            }
            if let Some(home) = store.spaces.get_mut(HOME) {
                home.tabs
                    .retain(|id, _| live.home.contains(&i64::from(*id)));
            }
            let mut ids: Vec<String> = spaces.into_iter().collect();
            ids.sort_by_key(|id| (store.get(id).map_or(0, |e| e.created), id.clone()));
            ids
        }
        None => {
            let mut ids: Vec<String> = store
                .spaces
                .keys()
                .filter(|id| *id != HOME)
                .cloned()
                .collect();
            ids.sort_by_key(|id| (store.get(id).map_or(0, |e| e.created), id.clone()));
            ids
        }
    };

    let row = |id: &str,
               label: String,
               named: bool,
               icon: Option<String>,
               folder: Option<PathBuf>,
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
            last: visit.map(|v| v.slot).unwrap_or(1),
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
/// else home.
fn here_folder(hypr: &Hypr) -> Result<PathBuf> {
    if let Some(pid) = hypr.active_window_pid()?
        && let Some(cwd) = terminal::shell_cwd(pid)
    {
        return Ok(cwd);
    }
    let (store, rows, _) = lock_rows(Some(hypr))?;
    Ok(rows
        .iter()
        .find(|r| r.active)
        .and_then(|r| store.get(&r.id))
        .and_then(|entry| entry.folder.clone())
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(paths::home))
}

/// The folder a space's new tab opens in: the space's own, its checkout's
/// when it's in one, else the folder you are in.
fn space_folder(hypr: &Hypr, space: &str) -> Result<PathBuf> {
    let (rows, _) = load_rows(Some(hypr))?;
    match rows
        .into_iter()
        .find(|r| r.id == space)
        .and_then(|r| r.folder)
        .filter(|dir| dir.is_dir())
    {
        Some(dir) => Ok(dir),
        None => here_folder(hypr),
    }
}

/// An id for a new space whose first terminal opens on tab `slot`, reserved
/// until that terminal is up.
fn fresh_space(hypr: &Hypr, slot: u8) -> Result<String> {
    let (id, _) = reserve::reserve(&paths::reserved_dir(), |reserved| {
        let mut taken = snapshot(hypr)?.spaces();
        let mut store = Store::load(&paths::state_dir())?;
        store.prune(&taken);
        taken.extend(reserved.iter().map(|(space, _)| space.clone()));
        Ok((store.fresh_id(&taken), slot))
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

    let id = fresh_space(&hypr, 1)?;
    let workspace = name::workspace_name(&id, 1);
    open_terminal_then(&hypr, &config, &dir, &workspace, || switch(&hypr, &id))?;

    // The watcher may have seen the space by now, and made its entry.
    let mut store = Store::lock(&paths::state_dir())?;
    let entry = store.entry(&id);
    entry.folder.get_or_insert(dir.clone());
    if entry.created == 0 {
        entry.created = state::now();
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
                    Some(session::Window {
                        folder: conversation
                            .as_ref()
                            .map_or_else(|| t.shell.cwd.clone(), |c| c.folder.clone()),
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
            let tabs: BTreeSet<u8> = windows.iter().map(|w| w.slot).collect();
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
                snapshot.active_tab
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
    let slot = window.tab?;
    let corner = snapshot
        .places
        .get(&(window.space.clone(), slot))
        .map_or((0, 0), |place| place.corner);
    Some(session::Window {
        slot,
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
pub fn restore(file: Option<PathBuf>) -> Result<()> {
    // A mistake in the config shouldn't cost you your spaces.
    let config = Config::load().unwrap_or_else(|e| {
        eprintln!("space: warning: {e:#}");
        Config::default()
    });
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
            created: now + spaces.len() as u64,
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
            for slot in space.slots() {
                if !space.has_browser(Some(slot)) {
                    space.arrange(&hypr, slot);
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
    let mut browser_works = config.restore_browser.unwrap_or(true);
    for space in spaces.iter_mut().filter(|s| s.has_browser(None)) {
        space.open_browsers(&hypr, &mut opened, &mut browser_works);
        space.note_restored(&hypr)?;
        for slot in space.slots() {
            if space.has_browser(Some(slot)) {
                space.arrange(&hypr, slot);
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
    created: u64,
}

impl Restoring {
    /// Whether any of its windows, or of those on a tab, is a browser's.
    fn has_browser(&self, slot: Option<u8>) -> bool {
        self.saved
            .windows
            .iter()
            .any(|w| w.browser.is_some() && slot.is_none_or(|slot| w.slot == slot))
    }

    fn slots(&self) -> BTreeSet<u8> {
        self.saved.windows.iter().map(|w| w.slot).collect()
    }

    /// The tab you were on.
    fn back(&self) -> Option<String> {
        let first = self.saved.windows.first()?.slot;
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
            Some(first) if !self.saved.home => fresh_space(hypr, first.slot)?,
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
            let workspace = session_workspace(&self.id, self.saved.windows[n].slot);
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
    fn arrange(&self, hypr: &Hypr, slot: u8) {
        // Everything is open; only where is off.
        if let Err(e) = arrange(hypr, &self.id, slot, &self.saved, &self.addresses) {
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
            entry.created = self.created;
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
    let workspace = session_workspace(id, terminal.slot);
    let events = hypr.events()?;
    hypr.dispatch(&hypr::exec_on(
        &restored_command(config, terminal, token),
        &format!("{} silent", session_selector(id, terminal.slot)),
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
    let workspace = session_workspace(id, window.slot);
    let rule = |enabled| {
        let on = format!("{} silent", session_selector(id, window.slot));
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
    // A folder that's gone gives way to the nearest one still there.
    let dir = terminal
        .folder
        .ancestors()
        .find(|dir| dir.is_dir())
        .map_or_else(paths::home, Path::to_path_buf);
    let resume = terminal
        .conversation
        .as_ref()
        // Its conversations are by folder.
        .filter(|c| config.resume_agents.unwrap_or(true) && dir == c.folder)
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
    slot: u8,
    saved: &session::Space,
    addresses: &[Option<String>],
) -> Result<()> {
    let default = session::Layout::default();
    let was = saved.layouts.get(&slot).unwrap_or(&default);
    let name = session_workspace(id, slot);
    let monitors = hypr.monitors().unwrap_or_default();
    let find = || -> Result<Option<hypr::Workspace>> {
        Ok(hypr.workspaces()?.into_iter().find(|ws| ws.name == name))
    };
    let Some(mut workspace) = find()? else {
        return Ok(());
    };
    // `home`'s are where Omarchy has them.
    if let Some(monitor) = &was.monitor
        && !saved.home
        && workspace.monitor != *monitor
        && monitors.iter().any(|m| m.name == *monitor)
    {
        hypr.dispatch(&hypr::move_workspace(&session_selector(id, slot), monitor))?;
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
        let Some(address) = window(&n).filter(|_| terminal.slot == slot) else {
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
            let hits: Vec<&hypr::Client> = clients
                .iter()
                .filter(|c| u32::try_from(c.pid).ok() == Some(anc))
                .collect();
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
    let (rows, _) = load_rows(Some(&hypr))?;
    match neighbour(&rows, forward) {
        Some(id) => switch(&hypr, &id),
        None => Ok(()),
    }
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
/// workspace.
fn switch(hypr: &Hypr, target: &str) -> Result<()> {
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    if here.space() == target {
        return Ok(());
    }

    let last = Last::load(&paths::state_dir())?;
    let slot = landing_slot(hypr, &last, target)?;
    focus(hypr, target, slot)?;

    if let Some(left_at) = here.slot() {
        remember(here.space(), left_at);
    }
    Ok(())
}

/// The tab you land on going to a space: where you left it, or, never
/// visited (an older version's, or made by hand), its first tab.
fn landing_slot(hypr: &Hypr, last: &Last, target: &str) -> Result<u8> {
    Ok(match last.get(target) {
        Some(visit) => visit.slot,
        None if target == HOME => 1,
        None => hypr
            .workspaces()?
            .iter()
            .filter_map(|ws| match name::locate(ws.id, &ws.name) {
                Location::Space { name, slot } if name == target => Some(slot),
                _ => None,
            })
            .min()
            .unwrap_or(1),
    })
}

/// Which space `space send` sends the window to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceTarget {
    /// What it's called, or its id; `home` for the plain workspaces.
    Named(String),
    Next,
    Prev,
}

/// `space send <space|next|prev>`: the focused window to another space, on
/// the tab you'd land on going there, and you with it unless `silent`. Sent
/// silently, a tab it leaves empty gives way as closing its last window does.
pub fn send(target: SpaceTarget, silent: bool) -> Result<()> {
    let hypr = Hypr::from_env()?;
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
    let slot = landing_slot(&hypr, &last, &id)?;
    if id == HOME {
        let omarchy = if silent {
            format!("o.workspaces.move_window({slot}, false)")
        } else {
            format!("o.workspaces.move_window({slot})")
        };
        hypr.dispatch(&omarchy)
            .or_else(|_| hypr.dispatch(&hypr::move_window(&slot.to_string(), !silent)))?;
    } else {
        hypr.dispatch(&hypr::move_window(&selector(&id, slot), !silent))?;
    }

    if silent {
        return emptied(&active.name);
    }
    // Where you left the space, unless that was its last window and it's
    // gone.
    if let Some(left_at) = here.slot() {
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
    if let Some((space, slot)) = target.as_deref().and_then(|t| tab_target(&rows, t)) {
        return rename_tab(&hypr, store, &rows, space, slot, &new_name);
    }
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
    let order = moved(order, &id, position - 1);
    restamp(&mut store, &order);
    store.save()?;
    Ok(())
}

/// `order` with `id` taken out and put back at `index`, or last.
fn moved(mut order: Vec<String>, id: &str, index: usize) -> Vec<String> {
    order.retain(|other| other != id);
    let index = index.min(order.len());
    order.insert(index, id.to_string());
    order
}

/// Gives the spaces `created` times in `order`, from the times they have:
/// the same times, handed out in the new order, made to rise strictly so no
/// two tie. New spaces, stamped now, still come after them all.
fn restamp(store: &mut Store, order: &[String]) {
    let mut times: Vec<u64> = order
        .iter()
        .map(|id| store.get(id).map_or(0, |e| e.created))
        .collect();
    times.sort_unstable();
    let mut last = 0;
    for (id, time) in order.iter().zip(times) {
        let time = time.max(last + 1);
        store.entry(id).created = time;
        last = time;
    }
}

/// A tab, `<space>:<n>` (`s1:2`, `ren:2`), unless the whole of `target` is
/// what a space is called.
fn tab_target<'a>(rows: &[Row], target: &'a str) -> Option<(&'a str, u8)> {
    let (space, slot) = target.rsplit_once(':')?;
    if resolve(rows, target).is_ok() {
        return None;
    }
    Some((space, name::parse_slot(slot)?))
}

/// `space rename <space>:<n> <name>`: gives a tab a name that stays, in
/// place of its window's. An empty name has it follow its window again, or,
/// in `home`, show just its number.
fn rename_tab(
    hypr: &Hypr,
    mut store: Store,
    rows: &[Row],
    space: &str,
    slot: u8,
    new_name: &str,
) -> Result<()> {
    let id = resolve(rows, space)?;
    // `home`'s names are by workspace id, as each monitor has its own tab n.
    let key = if id == HOME {
        home_workspace(hypr, slot)?
    } else {
        let row = rows.iter().find(|r| r.id == id).expect("resolved");
        let open = row.workspaces.as_deref().unwrap_or_default();
        if !open.contains(&slot) {
            bail!("'{}' has no tab {slot}", row.label);
        }
        slot
    };
    let new_name = spaces::clean_name(new_name)?;

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

/// The id of `home`'s tab `slot`: on the focused monitor when you are in
/// `home`, as the bar shows it, else the only open one with that number.
fn home_workspace(hypr: &Hypr, slot: u8) -> Result<u8> {
    let in_block =
        |id: i64, of: i64| (id - 1) / i64::from(name::SLOTS) == (of - 1) / i64::from(name::SLOTS);
    let active = hypr.active_workspace()?;
    let here =
        matches!(name::locate(active.id, &active.name), Location::Home { .. }) && active.id > 0;
    let open: Vec<i64> = hypr
        .workspaces()?
        .into_iter()
        .filter(|ws| {
            ws.id > 0 && name::locate(ws.id, &ws.name) == (Location::Home { slot: Some(slot) })
        })
        .map(|ws| ws.id)
        .collect();
    let found = if here {
        open.iter().copied().find(|id| in_block(*id, active.id))
    } else {
        match open.as_slice() {
            [only] => Some(*only),
            [] => None,
            _ => bail!("'home' has a tab {slot} on more than one monitor; name it from there"),
        }
    };
    let Some(found) = found else {
        bail!("'home' has no tab {slot}");
    };
    u8::try_from(found).with_context(|| format!("workspace {found} can't be named"))
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

/// `space emptied <workspace>`, run by Hyprland (`hypr/omarchy-spaces.lua`) when
/// closing a window left the workspace you are on empty. An emptied tab
/// doesn't linger: you go to the space's next tab with windows, else the one
/// before; with none left, the space ends and you go back to the space you
/// were in before it, or `home`. Nothing happens if things moved on since: you left that
/// workspace, or something opened on it.
pub fn emptied(workspace: &str) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let active = hypr.active_workspace()?;
    if active.name != workspace {
        return Ok(());
    }
    let Location::Space { name: id, slot } = name::locate(active.id, &active.name) else {
        return Ok(());
    };
    let workspaces = hypr.workspaces()?;
    let mut tabs: BTreeMap<u8, u32> = BTreeMap::new();
    for ws in &workspaces {
        if let Location::Space { name, slot } = name::locate(ws.id, &ws.name)
            && name == id
        {
            tabs.insert(slot, ws.windows);
        }
    }
    if tabs.get(&slot).copied().unwrap_or(0) > 0 {
        return Ok(());
    }

    let has_windows = |(_, windows): &(&u8, &u32)| **windows > 0;
    let after = tabs.range(slot + 1..).find(has_windows).map(|(s, _)| *s);
    let before = tabs.range(..slot).rev().find(has_windows).map(|(s, _)| *s);
    if let Some(next) = after.or(before) {
        focus(&hypr, &id, next)?;
        remember(&id, next);
        return Ok(());
    }

    let snapshot = snapshot(&hypr)?;
    let others: BTreeSet<&str> = snapshot
        .space_windows
        .iter()
        .map(|w| w.space.as_str())
        .filter(|space| *space != id)
        .collect();
    let to = Last::load(&paths::state_dir())?
        .latest_except(&id, |s| s == HOME || others.contains(s))
        .unwrap_or(HOME)
        .to_string();
    switch(&hypr, &to)?;
    // The space is gone (switching noted where you left it): nothing to go
    // back to.
    forget(&id);
    Ok(())
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
        serde_json::to_writer_pretty(&mut out, &rows)?;
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
        let folder = row.folder.as_deref().map(paths::tilde).unwrap_or_default();
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
                details.push(paths::tilde(folder));
            }
            if row.active {
                details.push("current".into());
            }
            format!("{:width$}   {}\n", row.label, details.join(" · "))
        })
        .collect()
}

/// Focuses a slot of a space. In `home`, that is Omarchy's own per-monitor
/// workspace `n`, or plain workspace `n` outside Omarchy.
fn focus(hypr: &Hypr, space: &str, slot: u8) -> Result<()> {
    if space == HOME {
        hypr.dispatch(&format!("o.workspaces.focus({slot})"))
            .or_else(|_| hypr.dispatch(&hypr::focus_workspace(&slot.to_string())))
    } else {
        hypr.dispatch(&hypr::focus_workspace(&selector(space, slot)))
    }
}

fn selector(space: &str, slot: u8) -> String {
    format!("name:{}", name::workspace_name(space, slot))
}

/// Records the workspace the active space is on. Failing to is only a
/// warning: the switch itself has happened.
fn remember(space: &str, slot: u8) {
    if let Err(e) = Last::update(&paths::state_dir(), |last| last.set(space, slot)) {
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

    fn terminal(address: &str, slot: u8, pid: u32, cwd: &str) -> Terminal {
        Terminal {
            address: address.into(),
            slot,
            pid,
            shell: terminal::Shell {
                cwd: cwd.into(),
                program: None,
                agent: None,
                conversation: None,
            },
        }
    }

    #[test]
    fn picks_new_next_and_prev_tabs() {
        use Target::*;
        let pick = |t, open: &[u8], cur| pick_slot(t, open, cur).unwrap();
        assert_eq!(pick(New, &[1], Some(1)), Some(2));
        assert_eq!(pick(New, &[1, 2, 5], Some(2)), Some(6));
        assert_eq!(
            pick(New, &[2, 10], Some(10)),
            Some(1),
            "a gap once 10 is taken"
        );
        assert!(pick_slot(New, &(1..=10).collect::<Vec<_>>(), Some(1)).is_err());
        assert_eq!(pick(Next, &[1, 3, 4], Some(3)), Some(4));
        assert_eq!(pick(Next, &[1, 3, 4], Some(4)), Some(1), "wraps");
        assert_eq!(pick(Prev, &[1, 3, 4], Some(1)), Some(4), "wraps");
        assert_eq!(pick(Prev, &[1, 3, 4], Some(3)), Some(1));
        assert_eq!(pick(Next, &[2], Some(2)), None, "only one tab");
        assert_eq!(pick(Next, &[2, 5], None), Some(2));
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
        assert_eq!(folder.as_deref(), Some(Path::new(repo_s)));
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
        assert_eq!(moved(order.clone(), "s3", 0), ids(&["s3", "s1", "s2"]));
        assert_eq!(moved(order.clone(), "s1", 1), ids(&["s2", "s1", "s3"]));
        assert_eq!(moved(order.clone(), "s1", 9), ids(&["s2", "s3", "s1"]));

        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load(dir.path()).unwrap();
        store.entry("s1").created = 10;
        store.entry("s2").created = 10;
        store.entry("s3").created = 30;
        restamp(&mut store, &ids(&["s3", "s1", "s2"]));
        let created = |id| store.get(id).unwrap().created;
        assert!(created("s3") < created("s1") && created("s1") < created("s2"));
        assert!(created("s2") <= 30, "{}", created("s2"));
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
