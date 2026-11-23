//! The commands. None of them keeps state about windows: each asks Hyprland
//! where things are, then dispatches. What Hyprland can't hold about a space,
//! its name and the terminal it follows, is in the store (`spaces.rs`).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::config::Config;
use crate::hypr::{self, Hypr};
use crate::name::{self, HOME, Location};
use crate::paths;
use crate::spaces::{self, Store};
use crate::state::{self, Last};
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

/// `space ws <n|new|next|prev>`: a workspace of the active space.
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

    focus(&hypr, here.space(), slot)?;
    remember(here.space(), slot);
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

/// A terminal window in a space, and the folder its shell is in.
#[derive(Debug, Clone)]
struct Terminal {
    address: String,
    slot: u8,
    pid: u32,
    cwd: PathBuf,
}

/// What Hyprland says about every space: the active one, and the windows,
/// workspaces and terminals each holds.
#[derive(Default)]
struct Live {
    active: Option<String>,
    windows: BTreeMap<String, u32>,
    workspaces: BTreeMap<String, BTreeSet<u8>>,
    terminals: BTreeMap<String, Vec<Terminal>>,
}

impl Live {
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

fn live(hypr: &Hypr) -> Result<Live> {
    let mut live = Live::default();
    let active = hypr.active_workspace()?;
    live.active = Some(name::locate(active.id, &active.name).space().to_string());
    for ws in hypr.workspaces()? {
        // Special workspaces (scratchpad and the like) belong to no space.
        if ws.name.starts_with("special:") {
            continue;
        }
        let location = name::locate(ws.id, &ws.name);
        let entry = live
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
        *live
            .windows
            .entry(location.space().to_string())
            .or_default() += 1;
        if let Location::Space { name: space, slot } = location
            && let Ok(pid) = u32::try_from(client.pid)
            && let Some(cwd) = terminal::shell_cwd(pid)
        {
            live.terminals.entry(space).or_default().push(Terminal {
                address: client.address,
                slot,
                pid,
                cwd,
            });
        }
    }
    // Oldest first, as far as Hyprland lets us tell: by tab, then process.
    for terminals in live.terminals.values_mut() {
        terminals.sort_by_key(|t| (t.slot, t.pid));
    }
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
        entry.folder = Some(terminal.cwd.clone());
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
fn rows(store: &mut Store, live: Option<&Live>, last: &Last) -> Vec<Row> {
    let ids: Vec<String> = match live {
        Some(live) => {
            let spaces = live.spaces();
            store.prune(&spaces);
            for id in &spaces {
                let entry = store.entry(id);
                if entry.created == 0 {
                    entry.created = state::now();
                }
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

    let row = |id: &str, label: String, named: bool, folder: Option<PathBuf>| {
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
        }
    };

    let mut out = vec![row(HOME, HOME.to_string(), false, None)];
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
        out.push(row(&id, label, named, folder));
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

    switch(&hypr, &id)?;
    *store.entry(&id) = spaces::Entry {
        folder: Some(dir.clone()),
        created: state::now(),
        ..Default::default()
    };
    store.save()?;
    terminal::launch(&config.terminal(), &dir)?;
    println!("Opened a space in {}", paths::tilde(&dir));
    Ok(())
}

/// `space terminal`: a terminal in the folder you are in, so a new tab of a
/// space opens on its folder rather than home.
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

/// Leaves the active space, remembering where, for the target's last
/// workspace.
fn switch(hypr: &Hypr, target: &str) -> Result<()> {
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    if here.space() == target {
        return Ok(());
    }

    let mut last = Last::load(&paths::state_dir())?;
    // Never visited (an older renSpace's, or made by hand): its first tab.
    let slot = match last.get(target) {
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
    };
    focus(hypr, target, slot)?;

    if let Some(left_at) = here.slot() {
        last.set(here.space(), left_at);
        save(&last);
    }
    Ok(())
}

/// `space rename [space] <name>`: gives a space, the active one by default,
/// a name that stays. An empty name has it called after its folder again.
pub fn rename(target: Option<String>, new_name: String) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let (rows, _) = load_rows(Some(&hypr))?;
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
    use std::path::Path;

    fn terminal(address: &str, slot: u8, pid: u32, cwd: &str) -> Terminal {
        Terminal {
            address: address.into(),
            slot,
            pid,
            cwd: cwd.into(),
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
