//! The commands. None of them keeps state about windows: each asks Hyprland
//! where things are, then dispatches.

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
use crate::spaces::{self, Registry};
use crate::state::Last;

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

/// Where `space new` puts the space.
pub enum NewDir {
    /// A folder, relative to the current one; the current one when `None`.
    Path(Option<PathBuf>),
    /// A folder of the space's name in the roots (`spaces::folder_in_roots`),
    /// for the sidebar, which has no current folder worth using.
    InRoots,
}

/// `space new [name] [dir]`: makes `dir` a space and goes to it.
pub fn new(space_name: Option<String>, dir: NewDir) -> Result<()> {
    let config = Config::load()?;
    let dir = match dir {
        NewDir::Path(dir) => dir.unwrap_or_default(),
        NewDir::InRoots => {
            let Some(space_name) = &space_name else {
                bail!("--in-roots needs a name: space new <name> --in-roots");
            };
            check_new_name(space_name)?;
            spaces::folder_in_roots(&config.root_dirs(), space_name).with_context(|| {
                format!(
                    "no roots to put it in: set `roots` in {}",
                    paths::tilde(&paths::config_file())
                )
            })?
        }
    };
    let cwd = std::env::current_dir().context("can't read the current folder")?;
    let mut dir = cwd.join(dir);
    if dir.exists() {
        dir = std::fs::canonicalize(&dir)?;
        if !dir.is_dir() {
            bail!("{} is not a folder", paths::tilde(&dir));
        }
        if let Some(existing) = spaces::read_space(&dir).ok().flatten() {
            bail!(
                "{} is already the space '{}'",
                paths::tilde(&dir),
                existing.name
            );
        }
    }

    let space_name = match space_name {
        Some(space_name) => space_name,
        None => {
            let derived = spaces::default_name(&dir);
            if derived.is_empty() {
                bail!(
                    "can't make a space name from the folder {}; give one: space new <name>",
                    paths::tilde(&dir)
                );
            }
            derived
        }
    };
    check_new_name(&space_name)?;
    let registry = Registry::load(&config, true)?;
    if let Some(existing) = registry.get(&space_name) {
        bail!(
            "the space '{space_name}' already exists in {}",
            paths::tilde(&existing.dir)
        );
    }

    // Only once everything checks out, so a typo leaves no stray folder.
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("can't create {}", paths::tilde(&dir)))?;
        dir = std::fs::canonicalize(&dir)?;
        println!("Created {}", paths::tilde(&dir));
    }
    let space = spaces::write_space(&dir, &space_name)?;
    Registry::remember(&space.dir)?;
    println!(
        "Created space '{}' in {}",
        space.name,
        paths::tilde(&space.dir)
    );

    // The space exists either way; not being able to switch to it (no
    // Hyprland, say over ssh) is worth saying but not failing for.
    if let Err(e) = Hypr::from_env().and_then(|hypr| switch(&hypr, &space.name)) {
        eprintln!("space: couldn't switch to '{}': {e:#}", space.name);
    }
    Ok(())
}

fn check_new_name(space_name: &str) -> Result<()> {
    name::validate(space_name)?;
    if space_name == HOME {
        bail!("'home' is reserved for the plain workspaces");
    }
    Ok(())
}

/// The error for a space nobody knows, suggesting a close name.
fn unknown_space<'a>(target: &str, known: impl IntoIterator<Item = &'a str>) -> anyhow::Error {
    let mut message = format!("no space called '{target}'");
    if let Some(close) = name::suggest(target, known) {
        message.push_str(&format!(". Did you mean {close}?"));
    }
    anyhow::anyhow!(message)
}

/// `space rename [old] <new>`: gives a space, the active one by default, a
/// new name. Its windows stay where they are: its workspaces are renamed
/// with it.
pub fn rename(old: Option<String>, new: String) -> Result<()> {
    check_new_name(&new)?;
    let hypr = Hypr::from_env();
    let old = match old {
        Some(old) => old,
        None => {
            let hypr = hypr.as_ref().map_err(|e| anyhow::anyhow!("{e:#}"))?;
            let active = hypr.active_workspace()?;
            name::locate(active.id, &active.name).space().to_string()
        }
    };
    if old == HOME {
        bail!("'home' is the plain workspaces; it can't be renamed");
    }
    if old == new {
        return Ok(());
    }

    let config = Config::load()?;
    let registry = Registry::load(&config, true)?;
    let live = match &hypr {
        Ok(hypr) => hypr_spaces(hypr)?,
        Err(_) => BTreeSet::new(),
    };
    if let Some(existing) = registry.get(&new) {
        bail!(
            "the space '{new}' already exists in {}",
            paths::tilde(&existing.dir)
        );
    }
    if live.contains(&new) {
        bail!("the space '{new}' already has windows open");
    }
    let space = registry.get(&old);
    if space.is_none() && !live.contains(&old) {
        let mut names: BTreeSet<&str> = registry.names().collect();
        names.extend(live.iter().map(String::as_str));
        return Err(unknown_space(&old, names));
    }

    // The folder first: it is the source of truth, and a space whose folder
    // is gone (only windows left) has none to change.
    if let Some(space) = space {
        spaces::set_name(&space.dir, &new)?;
    }
    if let Ok(hypr) = &hypr {
        for ws in hypr.workspaces()? {
            if let Location::Space { name, slot } = name::locate(ws.id, &ws.name)
                && name == old
            {
                hypr.dispatch(&hypr::rename_workspace(
                    &selector(&old, slot),
                    &name::workspace_name(&new, slot),
                ))?;
            }
        }
    }
    match Last::load(&paths::state_dir()) {
        Ok(mut last) => {
            last.rename(&old, &new);
            save(&last);
        }
        Err(e) => eprintln!("space: warning: {e:#}"),
    }
    println!("Renamed '{old}' to '{new}'");
    Ok(())
}

/// `space close [name]`: closes every window of a space, the active one by
/// default, leaving it first if you are in it. Each app is asked, as
/// `super+w` asks it, so it can still stop to save.
pub fn close(target: Option<String>) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    let target = target.unwrap_or_else(|| here.space().to_string());
    if target == HOME {
        bail!("'home' is the plain workspaces; close its windows one by one");
    }

    let config = Config::load()?;
    let registry = Registry::load(&config, false)?;
    let live = hypr_spaces(&hypr)?;
    if registry.get(&target).is_none() && !live.contains(&target) {
        let registry = Registry::load(&config, true)?;
        if registry.get(&target).is_none() {
            let mut names: BTreeSet<&str> = registry.names().collect();
            names.extend(live.iter().map(String::as_str));
            return Err(unknown_space(&target, names));
        }
    }

    let windows: Vec<String> = hypr
        .clients()?
        .into_iter()
        .filter(|c| {
            matches!(name::locate(c.workspace.id, &c.workspace.name),
                Location::Space { name, .. } if name == target)
        })
        .map(|c| c.address)
        .collect();

    if here.space() == target {
        leave(&hypr, &target, &registry, &live)?;
    }
    for address in &windows {
        hypr.dispatch(&hypr::close_window(address))?;
    }
    // Closed, so nothing to go back to: next time it opens on its first tab,
    // and the sidebar no longer offers it as where you just were.
    if let Ok(mut last) = Last::load(&paths::state_dir()) {
        last.remove(&target);
        save(&last);
    }
    match windows.len() {
        0 => println!("No windows open in '{target}'"),
        1 => println!("Closed 1 window in '{target}'"),
        n => println!("Closed {n} windows in '{target}'"),
    }
    Ok(())
}

/// Goes from `space`, which you are in, to the space you were in before it,
/// or `home`.
fn leave(hypr: &Hypr, space: &str, registry: &Registry, live: &BTreeSet<String>) -> Result<()> {
    let last = Last::load(&paths::state_dir())?;
    let to = last
        .latest_except(space, |s| {
            s == HOME || registry.get(s).is_some() || live.contains(s)
        })
        .unwrap_or(HOME)
        .to_string();
    switch(hypr, &to)
}

/// `space rm <name>`: stops a folder being a space, by removing its
/// `.renspace.toml`. The folder itself stays. A space with windows open has
/// to be closed first.
pub fn remove(target: String) -> Result<()> {
    if target == HOME {
        bail!("'home' is the plain workspaces; it can't be removed");
    }
    let config = Config::load()?;
    let registry = Registry::load(&config, true)?;
    let hypr = Hypr::from_env().ok();
    let live = match &hypr {
        Some(hypr) => live(hypr).ok(),
        None => None,
    };
    let Some(space) = registry.get(&target) else {
        let mut names: BTreeSet<&str> = registry.names().collect();
        if let Some(live) = &live {
            names.extend(live.windows.keys().map(String::as_str));
        }
        names.remove(HOME);
        if live
            .as_ref()
            .is_some_and(|l| l.windows.contains_key(&target))
        {
            bail!(
                "the space '{target}' has no folder any more, only windows: close them with space close {target}"
            );
        }
        return Err(unknown_space(&target, names));
    };
    match live
        .as_ref()
        .map(|l| l.windows.get(&target).copied().unwrap_or(0))
    {
        Some(0) => {}
        Some(n) => bail!(
            "'{target}' still has {n} window{} open; close {} first: space close {target}",
            if n == 1 { "" } else { "s" },
            if n == 1 { "it" } else { "them" },
        ),
        None => {}
    }

    spaces::remove_space(&space.dir)?;
    // Standing on its empty tab: go back to where you were before.
    if let (Some(hypr), Some(live)) = (&hypr, &live)
        && live.active.as_deref() == Some(target.as_str())
    {
        let spaces: BTreeSet<String> = live.workspaces.keys().cloned().collect();
        if let Err(e) = leave(hypr, &target, &registry, &spaces) {
            eprintln!("space: couldn't leave '{target}': {e:#}");
        }
    }
    if let Ok(mut last) = Last::load(&paths::state_dir()) {
        last.remove(&target);
        save(&last);
    }
    // Reading the index prunes the folder from it.
    Registry::load(&config, false)?;
    println!(
        "Removed the space '{target}'. Its folder {} is untouched.",
        paths::tilde(&space.dir)
    );
    Ok(())
}

/// `space go [name]`: the space's last-focused workspace, or its first. With
/// no name, a picker.
pub fn go(target: Option<String>) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let target = match target {
        Some(target) => target,
        None => match pick(&hypr)? {
            Some(target) => target,
            None => return Ok(()),
        },
    };

    if target != HOME {
        let config = Config::load()?;
        let live = hypr_spaces(&hypr)?;
        let known = |registry: &Registry| registry.get(&target).is_some() || live.contains(&target);
        let mut registry = Registry::load(&config, false)?;
        if !known(&registry) {
            // Maybe created by hand or by an agent since the index was written.
            registry = Registry::load(&config, true)?;
        }
        if !known(&registry) {
            let mut names: BTreeSet<&str> = registry.names().collect();
            names.extend(live.iter().map(String::as_str));
            names.insert(HOME);
            if name::suggest(&target, names.iter().copied()).is_some() {
                return Err(unknown_space(&target, names));
            }
            bail!("no space called '{target}'. Create it with: space new");
        }
    }

    switch(&hypr, &target)
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
    let slot = last.get(target).map(|visit| visit.slot).unwrap_or(1);
    focus(hypr, target, slot)?;

    if let Some(left_at) = here.slot() {
        last.set(here.space(), left_at);
        save(&last);
    }
    Ok(())
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

/// Spaces that have workspaces in Hyprland right now, whether or not their
/// folder is known. A space whose folder moved still holds its windows.
fn hypr_spaces(hypr: &Hypr) -> Result<BTreeSet<String>> {
    Ok(hypr
        .workspaces()?
        .iter()
        .filter_map(|ws| match name::locate(ws.id, &ws.name) {
            Location::Space { name, .. } => Some(name),
            Location::Home { .. } => None,
        })
        .collect())
}

/// One row of `space ls`.
#[derive(Debug, Serialize)]
pub struct Row {
    pub name: String,
    /// `None` for `home`, and for a space whose windows are still open but
    /// whose folder is no longer known.
    pub dir: Option<PathBuf>,
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

/// What Hyprland says about every space: the active one, and the windows and
/// workspaces each holds.
#[derive(Default)]
struct Live {
    active: Option<String>,
    windows: BTreeMap<String, u32>,
    workspaces: BTreeMap<String, BTreeSet<u8>>,
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
    }
    Ok(live)
}

/// Every space with what Hyprland knows about it: `home` first, then the rest
/// by name.
fn rows(registry: &Registry, live: Option<&Live>, last: &Last) -> Vec<Row> {
    let mut dirs: BTreeMap<String, Option<PathBuf>> = registry
        .spaces
        .iter()
        .map(|s| (s.name.clone(), Some(s.dir.clone())))
        .collect();
    if let Some(live) = live {
        for space in live.workspaces.keys().chain(live.windows.keys()) {
            if space != HOME {
                dirs.entry(space.clone()).or_insert(None);
            }
        }
    }

    std::iter::once((HOME.to_string(), None))
        .chain(dirs)
        .map(|(space, dir)| {
            let visit = last.get(&space);
            Row {
                active: live.is_some_and(|l| l.active.as_deref() == Some(space.as_str())),
                windows: live.map(|l| l.windows.get(&space).copied().unwrap_or(0)),
                workspaces: live.map(|l| {
                    l.workspaces
                        .get(&space)
                        .map(|slots| slots.iter().copied().collect())
                        .unwrap_or_default()
                }),
                last: visit.map(|v| v.slot).unwrap_or(1),
                left_at: visit.map(|v| v.at).unwrap_or(0),
                name: space,
                dir,
            }
        })
        .collect()
}

/// `space ls`: every space, the active one marked, with a window count.
pub fn ls(json: bool) -> Result<()> {
    let config = Config::load()?;
    let registry = Registry::load(&config, true)?;
    for warning in &registry.warnings {
        eprintln!("space: warning: {warning}");
    }
    let live = match Hypr::from_env().and_then(|hypr| live(&hypr)) {
        Ok(live) => Some(live),
        Err(e) => {
            eprintln!("space: no window counts: {e:#}");
            None
        }
    };
    let last = Last::load(&paths::state_dir())?;
    let rows = rows(&registry, live.as_ref(), &last);

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
    let width = rows.iter().map(|r| r.name.len()).max().unwrap_or(0);
    let mut out = String::new();
    for row in rows {
        let marker = if row.active { '*' } else { ' ' };
        let windows = match row.windows {
            Some(1) => "1 window".to_string(),
            Some(n) => format!("{n} windows"),
            None => "-".to_string(),
        };
        let dir = match (&row.dir, row.name.as_str()) {
            (Some(dir), _) => paths::tilde(dir),
            (None, HOME) => String::new(),
            (None, _) => "(folder not found)".to_string(),
        };
        let line = format!("{marker} {:width$}  {windows:<11}  {dir}", row.name);
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// The picker for `space go` with no name: most recently left first, the
/// active space last, so Enter goes back to the previous space.
fn pick(hypr: &Hypr) -> Result<Option<String>> {
    let config = Config::load()?;
    let registry = Registry::load(&config, true)?;
    let live = live(hypr)?;
    let last = Last::load(&paths::state_dir())?;
    let mut rows = rows(&registry, Some(&live), &last);
    rows.sort_by(|a, b| {
        a.active
            .cmp(&b.active)
            .then(b.left_at.cmp(&a.left_at))
            .then(a.name.cmp(&b.name))
    });

    let input = picker_lines(&rows);
    let Some(picker) = config.picker() else {
        bail!(
            "no picker found: install vicinae or walker, or set `picker` in {}",
            paths::tilde(&paths::config_file())
        );
    };
    let Some(choice) = run_picker(&picker, &input)? else {
        return Ok(None);
    };
    Ok(choice.split_whitespace().next().map(str::to_string))
}

fn picker_lines(rows: &[Row]) -> String {
    let width = rows.iter().map(|r| r.name.len()).max().unwrap_or(0);
    rows.iter()
        .map(|row| {
            let mut details = Vec::new();
            match row.windows {
                Some(1) => details.push("1 window".to_string()),
                Some(n) => details.push(format!("{n} windows")),
                None => {}
            }
            if let Some(dir) = &row.dir {
                details.push(paths::tilde(dir));
            }
            if row.active {
                details.push("current".into());
            }
            format!("{:width$}   {}\n", row.name, details.join(" · "))
        })
        .collect()
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
    use crate::spaces::Space;

    fn registry() -> Registry {
        Registry {
            spaces: vec![
                Space {
                    name: "ren".into(),
                    dir: "/p/ren".into(),
                },
                Space {
                    name: "tax".into(),
                    dir: "/p/tax".into(),
                },
            ],
            warnings: vec![],
        }
    }

    fn live_ren() -> Live {
        let mut live = Live {
            active: Some("ren".into()),
            ..Default::default()
        };
        live.windows.insert("ren".into(), 3);
        live.windows.insert(HOME.into(), 1);
        live.windows.insert("old".into(), 2);
        live.workspaces.insert("ren".into(), [1, 2].into());
        live.workspaces.insert(HOME.into(), [1].into());
        live.workspaces.insert("old".into(), [4].into());
        live
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
    fn rows_put_home_first_and_include_orphans() {
        let last = Last::default();
        let live = live_ren();
        let rows = rows(&registry(), Some(&live), &last);
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["home", "old", "ren", "tax"]);
        assert!(rows[2].active);
        assert_eq!(rows[2].windows, Some(3));
        assert_eq!(rows[2].workspaces.as_deref(), Some(&[1, 2][..]));
        assert_eq!(rows[3].windows, Some(0));
        assert_eq!(rows[1].dir, None);
    }

    #[test]
    fn formats_rows() {
        let live = live_ren();
        let text = format_rows(&rows(&registry(), Some(&live), &Last::default()));
        assert_eq!(
            text,
            "  home  1 window\n  \
             old   2 windows    (folder not found)\n\
             * ren   3 windows    /p/ren\n  \
             tax   0 windows    /p/tax\n"
        );

        let text = format_rows(&rows(&registry(), None, &Last::default()));
        assert!(text.contains("  ren   -            /p/ren\n"), "{text}");
    }

    #[test]
    fn picker_lines_start_with_the_name() {
        let live = live_ren();
        let lines = picker_lines(&rows(&registry(), Some(&live), &Last::default()));
        for line in lines.lines() {
            let first = line.split_whitespace().next().unwrap();
            assert!(["home", "old", "ren", "tax"].contains(&first), "{line}");
        }
        assert!(
            lines.contains("ren    3 windows · /p/ren · current"),
            "{lines}"
        );
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
