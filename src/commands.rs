//! The five commands. None of them keeps state about windows: each asks
//! Hyprland where things are, then dispatches.

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

/// `space ws <n>`: workspace `n` of the active space.
pub fn ws(slot: u8) -> Result<()> {
    let hypr = Hypr::from_env()?;
    let active = hypr.active_workspace()?;
    let here = name::locate(active.id, &active.name);
    if here.slot() == Some(slot) {
        return Ok(());
    }

    focus(&hypr, here.space(), slot)?;
    remember(here.space(), slot);
    Ok(())
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

/// `space new [name] [dir]`: makes `dir` a space and goes to it.
pub fn new(space_name: Option<String>, dir: Option<PathBuf>) -> Result<()> {
    let config = Config::load()?;
    let cwd = std::env::current_dir().context("can't read the current folder")?;
    let mut dir = cwd.join(dir.unwrap_or_default());
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
            let mut message = format!("no space called '{target}'");
            if let Some(close) = name::suggest(&target, names) {
                message.push_str(&format!(". Did you mean {close}?"));
            } else {
                message.push_str(". Create it with: space new");
            }
            bail!(message);
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
