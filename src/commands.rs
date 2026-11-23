//! The commands. None of them keeps state about windows: each asks Hyprland
//! where things are, then dispatches. What Hyprland can't hold about a space,
//! its name and the terminal it follows, is in the store (`spaces.rs`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::config::Config;
use crate::core;
use crate::hypr::{self, Hypr};
use crate::name::{self, HOME, Location};
use crate::paths;
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

    let space = here.space();
    if target == Target::New && space != HOME {
        let config = Config::load()?;
        let dir = space_folder(&hypr, space)?;
        let workspace = name::workspace_name(space, slot);
        open_terminal_then(&hypr, &config, &dir, &workspace, || {
            focus(&hypr, space, slot)
        })?;
    } else {
        focus(&hypr, space, slot)?;
    }
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
}

/// What Hyprland says about every space, before any terminal's folder is
/// read: the active one, and the windows and workspaces each holds.
#[derive(Debug, Default)]
pub(crate) struct Snapshot {
    pub active: Option<String>,
    windows: BTreeMap<String, u32>,
    workspaces: BTreeMap<String, BTreeSet<u8>>,
    /// The windows on spaces' workspaces, `home`'s included.
    pub space_windows: Vec<SpaceWindow>,
}

/// The snapshot, which of its windows are terminals and where, and what
/// the agents in them are doing.
#[derive(Default)]
pub(crate) struct Live {
    active: Option<String>,
    windows: BTreeMap<String, u32>,
    workspaces: BTreeMap<String, BTreeSet<u8>>,
    terminals: BTreeMap<String, Vec<Terminal>>,
    /// Every window's address, by space.
    addresses: BTreeMap<String, Vec<String>>,
    /// Agents' statuses, by window address.
    pub statuses: HashMap<String, Status>,
}

impl Live {
    /// Each terminal's status from what its agent reported, `seen` saying
    /// whether you've seen a core terminal's turn end.
    pub fn set_statuses(&mut self, seen: impl Fn(&terminal::Agent) -> bool) {
        self.statuses = self
            .terminals
            .values()
            .flatten()
            .filter_map(|t| {
                let agent = t.shell.agent.as_ref()?;
                let status = status::from_report(&agent.word, seen(agent))?;
                Some((t.address.clone(), status))
            })
            .collect();
    }

    /// The core terminals of the windows on screen, by window address.
    pub fn agents(&self) -> impl Iterator<Item = (&str, &terminal::Agent)> {
        self.terminals
            .values()
            .flatten()
            .filter_map(|t| Some((t.address.as_str(), t.shell.agent.as_ref()?)))
    }

    /// The spaces Hyprland has, `home` aside.
    fn spaces(&self) -> BTreeSet<String> {
        self.workspaces
            .keys()
            .chain(self.windows.keys())
            .filter(|space| *space != HOME)
            .cloned()
            .collect()
    }
}

pub(crate) fn snapshot(hypr: &Hypr) -> Result<Snapshot> {
    let mut snapshot = Snapshot::default();
    let active = hypr.active_workspace()?;
    snapshot.active = Some(name::locate(active.id, &active.name).space().to_string());
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
    }
    for client in hypr.clients()? {
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
            });
        }
    }
    Ok(snapshot)
}

/// Makes the live view from a snapshot, with `shell` saying which windows
/// are terminals, which folder each one's shell is in and what it runs.
pub(crate) fn live_from(
    snapshot: &Snapshot,
    mut shell: impl FnMut(&SpaceWindow) -> Option<terminal::Shell>,
) -> Live {
    let mut terminals: BTreeMap<String, Vec<Terminal>> = BTreeMap::new();
    let mut addresses: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for window in &snapshot.space_windows {
        addresses
            .entry(window.space.clone())
            .or_default()
            .push(window.address.clone());
        if let Some(shell) = shell(window) {
            terminals
                .entry(window.space.clone())
                .or_default()
                .push(Terminal {
                    address: window.address.clone(),
                    slot: window.slot,
                    pid: window.pid,
                    shell,
                });
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
        terminals,
        addresses,
        statuses: HashMap::new(),
    }
}

fn live(hypr: &Hypr) -> Result<Live> {
    let core = core::list().ok().map(core::Terminals::new);
    let mut live = live_from(&snapshot(hypr)?, |window| {
        terminal::shell_of(window.pid, core.as_ref())
    });
    // Nothing here knows what you've seen: a `done` shows until `watch`,
    // which does, says otherwise.
    live.set_statuses(|_| false);
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
            let mut ids: Vec<String> = spaces.into_iter().collect();
            ids.sort_by_key(|id| (store.get(id).map_or(0, |e| e.created), id.clone()));
            ids
        }
        None => {
            let mut ids: Vec<String> = store.spaces.keys().cloned().collect();
            ids.sort_by_key(|id| (store.get(id).map_or(0, |e| e.created), id.clone()));
            ids
        }
    };

    let row = |id: &str, label: String, named: bool, folder: Option<PathBuf>, tabs| {
        let visit = last.get(id);
        Row {
            id: id.to_string(),
            label,
            named,
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

    let mut out = vec![row(HOME, HOME.to_string(), false, None, BTreeMap::new())];
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
        let tabs = entry.tabs.clone();
        out.push(row(&id, label, named, folder, tabs));
    }
    out
}

/// Reads the spaces and saves what changed about them. A failure to save is
/// only a warning: the list itself is right.
fn load_rows(hypr: Option<&Hypr>) -> Result<(Vec<Row>, Option<Live>)> {
    let live = match hypr {
        Some(hypr) => Some(live(hypr)?),
        None => None,
    };
    let mut store = Store::load(&paths::state_dir())?;
    let last = Last::load(&paths::state_dir())?;
    let rows = rows(&mut store, live.as_ref(), &last);
    if let Err(e) = store.save() {
        eprintln!("space: warning: {e:#}");
    }
    Ok((rows, live))
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
    let (rows, _) = load_rows(Some(hypr))?;
    let store = Store::load(&paths::state_dir())?;
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

    let live = live(&hypr)?;
    let spaces = live.spaces();
    let mut store = Store::load(&paths::state_dir())?;
    store.prune(&spaces);
    let id = store.fresh_id(&spaces);
    // A space by this id before may have left a visit behind.
    if let Ok(mut last) = Last::load(&paths::state_dir()) {
        last.remove(&id);
        save(&last);
    }

    let workspace = name::workspace_name(&id, 1);
    open_terminal_then(&hypr, &config, &dir, &workspace, || switch(&hypr, &id))?;

    // The watcher may have seen the space by now, and made its entry.
    let mut store = Store::load(&paths::state_dir())?;
    let entry = store.entry(&id);
    entry.folder.get_or_insert(dir.clone());
    if entry.created == 0 {
        entry.created = state::now();
    }
    store.save()?;
    println!("Opened a space in {}", paths::tilde(&dir));
    Ok(())
}

/// Opens a terminal in `dir` on `workspace` (a space's tab) while you stay
/// where you are, then goes there with `arrive` once its window is up, so you
/// never see the tab empty. Hyprland places it by an exec rule, which follows
/// the process it starts down to the terminal. The rule also skips its
/// pop-in, which would otherwise still be fading it in from nothing as you
/// arrive; once you're there it animates like any other window.
fn open_terminal_then(
    hypr: &Hypr,
    config: &Config,
    dir: &Path,
    workspace: &str,
    arrive: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let events = hypr.events().ok();
    let command = terminal::command_in(&config.terminal(), dir);
    let placed = hypr
        .dispatch(&hypr::exec_on(
            &command,
            &format!("name:{workspace} silent"),
        ))
        .is_ok();
    let window = match (placed, events) {
        (true, Some(events)) => wait_for_window(&events, workspace, OPEN_TIMEOUT),
        (true, None) => None,
        (false, _) => {
            terminal::launch(&config.terminal(), dir)?;
            None
        }
    };
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

/// Waits until a window opens on `workspace` (Hyprland's `openwindow` event:
/// address, workspace, class, title) and returns its address, or `None`
/// once `timeout` passes.
fn wait_for_window(
    events: &std::sync::mpsc::Receiver<String>,
    workspace: &str,
    timeout: std::time::Duration,
) -> Option<String> {
    let deadline = std::time::Instant::now() + timeout;
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        let event = events.recv_timeout(left).ok()?;
        if let Some(data) = event.strip_prefix("openwindow>>") {
            let mut fields = data.split(',');
            if let (Some(address), Some(on)) = (fields.next(), fields.next())
                && on == workspace
            {
                return Some(format!("0x{}", address.trim_start_matches("0x")));
            }
        }
    }
    None
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

    let mut last = Last::load(&paths::state_dir())?;
    let slot = landing_slot(hypr, &last, target)?;
    focus(hypr, target, slot)?;

    if let Some(left_at) = here.slot() {
        last.set(here.space(), left_at);
        save(&last);
    }
    Ok(())
}

/// The tab you land on going to a space: where you left it, or, never
/// visited (an older renSpace's, or made by hand), its first tab.
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

    let mut last = Last::load(&paths::state_dir())?;
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
            last.set(left, left_at);
        } else {
            last.remove(left);
        }
        save(&last);
    }
    Ok(())
}

/// `space rename [space] <name>`: gives a space, the active one by default,
/// a name that stays. An empty name has it called after its folder again.
pub fn rename(target: Option<String>, new_name: String) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (rows, _) = load_rows(Some(&hypr))?;
    if let Some((space, slot)) = target.as_deref().and_then(|t| tab_target(&rows, t)) {
        return rename_tab(&rows, space, slot, &new_name);
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

    let mut store = Store::load(&paths::state_dir())?;
    store.entry(&id).name = new_name.clone();
    store.save()?;
    match new_name {
        Some(new_name) => println!("Named the space '{new_name}'"),
        None => println!("The space is called after its folder again"),
    }
    Ok(())
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
/// place of its window's. An empty name has it follow its window again.
fn rename_tab(rows: &[Row], space: &str, slot: u8, new_name: &str) -> Result<()> {
    let id = resolve(rows, space)?;
    if id == HOME {
        bail!("'home' is the plain workspaces; its tabs can't be named");
    }
    let row = rows.iter().find(|r| r.id == id).expect("resolved");
    let open = row.workspaces.as_deref().unwrap_or_default();
    if !open.contains(&slot) {
        bail!("'{}' has no tab {slot}", row.label);
    }
    let new_name = spaces::clean_name(new_name)?;

    let mut store = Store::load(&paths::state_dir())?;
    let tabs = &mut store.entry(&id).tabs;
    match &new_name {
        Some(new_name) => tabs.insert(slot, new_name.clone()),
        None => tabs.remove(&slot),
    };
    store.save()?;
    match new_name {
        Some(new_name) => println!("Named tab {slot} '{new_name}'"),
        None => println!("Tab {slot} is called after its window again"),
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
    if let Ok(mut last) = Last::load(&paths::state_dir()) {
        last.remove(&id);
        save(&last);
    }
    match windows.len() {
        0 => println!("No windows open in '{label}'"),
        1 => println!("Closed 1 window in '{label}'"),
        n => println!("Closed {n} windows in '{label}'"),
    }
    Ok(())
}

/// `space emptied <workspace>`, run by Hyprland (`hypr/renspace.lua`) when
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
    if let Ok(mut last) = Last::load(&paths::state_dir()) {
        last.remove(&id);
        save(&last);
    }
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
    match Last::load(&paths::state_dir()) {
        Ok(mut last) => {
            last.set(space, slot);
            save(&last);
        }
        Err(e) => eprintln!("space: warning: {e:#}"),
    }
}

fn save(last: &Last) {
    if let Err(e) = last.save() {
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
    fn runs_a_picker() {
        assert_eq!(
            run_picker("grep tax", "ren  x\ntax  y\n").unwrap(),
            Some("tax  y".into())
        );
        assert_eq!(run_picker("exit 1", "ren\n").unwrap(), None);
        assert_eq!(run_picker("true", "ren\n").unwrap(), None);
    }
}
