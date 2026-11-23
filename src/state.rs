//! Files under `~/.local/state/renspace/`. All of it is disposable cache: the
//! project folders and Hyprland are the sources of truth, and deleting this
//! directory loses nothing but where each space was last left.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::name;

/// Replaces a file in one step, so a reader never sees half of it and two
/// `space` processes racing on a keypress leave one whole version.
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let dir = path.parent().expect("state files live in a directory");
    fs::create_dir_all(dir).with_context(|| format!("can't create {}", dir.display()))?;
    let tmp = dir.join(format!(
        ".{}.{}",
        path.file_name().unwrap().to_string_lossy(),
        std::process::id()
    ));
    fs::write(&tmp, contents).with_context(|| format!("can't write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("can't write {}", path.display()))
}

/// Reads a state file, empty when it doesn't exist yet.
pub fn read_optional(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e).with_context(|| format!("can't read {}", path.display())),
    }
}

/// When a space was last left, and on which workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visit {
    pub slot: u8,
    /// Unix milliseconds, for ordering the picker by most recent.
    pub at: u64,
}

/// `last`: each space's last-focused workspace, one `name<TAB>slot<TAB>time`
/// line per space, so `space go` returns you to where you were.
#[derive(Debug, Default)]
pub struct Last {
    path: PathBuf,
    visits: BTreeMap<String, Visit>,
}

impl Last {
    pub fn load(state_dir: &Path) -> Result<Self> {
        let path = state_dir.join("last");
        let mut visits = BTreeMap::new();
        for line in read_optional(&path)?.lines() {
            let mut fields = line.split('\t');
            let (Some(space), Some(slot)) = (fields.next(), fields.next()) else {
                continue;
            };
            let Some(slot) = name::parse_slot(slot) else {
                continue;
            };
            let at = fields.next().and_then(|t| t.parse().ok()).unwrap_or(0);
            visits.insert(space.to_string(), Visit { slot, at });
        }
        Ok(Self { path, visits })
    }

    pub fn get(&self, space: &str) -> Option<Visit> {
        self.visits.get(space).copied()
    }

    /// Records a visit as the latest, even if the clock says otherwise, so two
    /// switches within a millisecond still keep their order.
    pub fn set(&mut self, space: &str, slot: u8) {
        let latest = self.visits.values().map(|v| v.at).max().unwrap_or(0);
        let at = now().max(latest + 1);
        self.visits.insert(space.to_string(), Visit { slot, at });
    }

    /// Forgets a space, as if it was never visited.
    pub fn remove(&mut self, space: &str) {
        self.visits.remove(space);
    }

    /// The most recently left space other than `except`, among those `known`
    /// accepts.
    pub fn latest_except(&self, except: &str, known: impl Fn(&str) -> bool) -> Option<&str> {
        self.visits
            .iter()
            .filter(|(space, _)| *space != except && known(space))
            .max_by_key(|(_, visit)| visit.at)
            .map(|(space, _)| space.as_str())
    }

    pub fn save(&self) -> Result<()> {
        let mut out = String::new();
        for (space, visit) in &self.visits {
            out.push_str(&format!("{space}\t{}\t{}\n", visit.slot, visit.at));
        }
        write_atomic(&self.path, &out)
    }
}

/// Unix milliseconds.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_round_trips_and_skips_bad_lines() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("last"),
            "ren\t3\t100\nbroken\nhome\t11\t5\ntax\t2\n",
        )
        .unwrap();

        let mut last = Last::load(dir.path()).unwrap();
        assert_eq!(last.get("ren"), Some(Visit { slot: 3, at: 100 }));
        assert_eq!(last.get("home"), None, "slot 11 is out of range");
        assert_eq!(last.get("tax"), Some(Visit { slot: 2, at: 0 }));

        last.set("ren", 5);
        last.set("tax", 1);
        last.save().unwrap();

        let last = Last::load(dir.path()).unwrap();
        assert_eq!(last.get("ren").unwrap().slot, 5);
        assert!(last.get("ren").unwrap().at > 100);
        assert!(last.get("tax").unwrap().at > last.get("ren").unwrap().at);
    }

    #[test]
    fn last_removes_and_finds_the_latest() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("last"), "a\t1\t100\nb\t2\t300\nc\t3\t200\n").unwrap();
        let mut last = Last::load(dir.path()).unwrap();
        assert_eq!(last.latest_except("x", |_| true), Some("b"));
        assert_eq!(last.latest_except("b", |_| true), Some("c"));
        assert_eq!(last.latest_except("b", |s| s != "c"), Some("a"));

        last.remove("b");
        assert_eq!(last.latest_except("x", |_| true), Some("c"));
    }

    #[test]
    fn missing_state_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let last = Last::load(&dir.path().join("nowhere")).unwrap();
        assert_eq!(last.get("ren"), None);
    }

    #[test]
    fn write_atomic_creates_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/index");
        write_atomic(&path, "x\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "x\n");
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }
}
