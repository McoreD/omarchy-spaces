//! Files under `~/.local/state/omarchy-spaces/`. The project folders and
//! Hyprland are the sources of truth: deleting this directory loses only
//! names, icons, where each space was last left and the session to restore.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, ensure};
use serde::de::DeserializeOwned;

use crate::name;

/// The version of `spaces.json` and of sessions. Goes up when one changes in a
/// way that an older `space` would read wrong; a new field that it can do
/// without isn't one.
pub const VERSION: u32 = 1;

/// Replaces a file in one step, so a reader never sees half of it. Writing
/// back what was read takes a lock too (`lock`), so two `space` processes
/// racing on a keypress don't undo each other's change.
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

/// Reads the JSON of a file that says its version. One that doesn't is from
/// before they did, which is the first.
pub fn parse<T: DeserializeOwned>(text: &str) -> Result<T> {
    let value: serde_json::Value = serde_json::from_str(text)?;
    let version = value.get("version").map_or(Some(1), |v| v.as_u64());
    let version = version.context("its version isn't a number")?;
    ensure!(
        version <= u64::from(VERSION),
        "it's version {version}, made by a newer space than this one, which reads {VERSION}"
    );
    Ok(serde_json::from_value(value)?)
}

/// Puts a file that can't be understood aside, so that nothing is saved over
/// it, and says so. Hold its lock.
pub fn put_aside(path: &Path, why: &anyhow::Error) -> Result<()> {
    let aside = path.with_extension(format!("broken-{}", now()));
    fs::rename(path, &aside).with_context(|| format!("can't put {} aside", path.display()))?;
    eprintln!(
        "space: warning: can't understand {} ({why:#}), so it's now {}",
        path.display(),
        aside.display()
    );
    Ok(())
}

/// Opens a file to lock, making it if needed.
pub fn lock_file(dir: &Path, name: &str) -> Result<File> {
    fs::create_dir_all(dir).with_context(|| format!("can't create {}", dir.display()))?;
    let path = dir.join(name);
    File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .with_context(|| format!("can't open {}", path.display()))
}

/// Waits for a lock, held until it's dropped.
pub fn lock(dir: &Path, name: &str) -> Result<File> {
    let file = lock_file(dir, name)?;
    file.lock()
        .with_context(|| format!("can't lock {}", dir.join(name).display()))?;
    Ok(file)
}

/// When a space was last left, and on which workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visit {
    pub key: u8,
    /// Unix milliseconds, for ordering the picker by most recent.
    pub at: u64,
}

/// `last`: each space's last-focused workspace, one `name<TAB>key<TAB>time`
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
            let (Some(space), Some(key)) = (fields.next(), fields.next()) else {
                continue;
            };
            let Some(key) = name::parse_key(key) else {
                continue;
            };
            let at = fields.next().and_then(|t| t.parse().ok()).unwrap_or(0);
            visits.insert(space.to_string(), Visit { key, at });
        }
        Ok(Self { path, visits })
    }

    pub fn get(&self, space: &str) -> Option<Visit> {
        self.visits.get(space).copied()
    }

    /// Records a visit as the latest, even if the clock says otherwise, so two
    /// switches within a millisecond still keep their order.
    pub fn set(&mut self, space: &str, key: u8) {
        let latest = self.visits.values().map(|v| v.at).max().unwrap_or(0);
        let at = now().max(latest + 1);
        self.visits.insert(space.to_string(), Visit { key, at });
    }

    /// Follows a space's tabs to their new slots, when they were left.
    pub fn renumber(&mut self, space: &str, moves: &BTreeMap<u8, u8>) {
        if let Some(visit) = self.visits.get_mut(space)
            && let Some(key) = moves.get(&visit.key)
        {
            visit.key = *key;
        }
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

    /// Changes what's saved, so that changes made at once are all kept.
    pub fn update(state_dir: &Path, change: impl FnOnce(&mut Self)) -> Result<()> {
        let _lock = lock(state_dir, ".last.lock")?;
        let mut last = Self::load(state_dir)?;
        change(&mut last);
        last.save()
    }

    fn save(&self) -> Result<()> {
        let mut out = String::new();
        for (space, visit) in &self.visits {
            out.push_str(&format!("{space}\t{}\t{}\n", visit.key, visit.at));
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
            "ren\t3\t100\nbroken\nhome\t13\t5\ns9\t251\t5\ntax\t2\n",
        )
        .unwrap();

        let mut last = Last::load(dir.path()).unwrap();
        assert_eq!(last.get("ren"), Some(Visit { key: 3, at: 100 }));
        assert_eq!(last.get("home"), Some(Visit { key: 13, at: 5 }));
        assert_eq!(last.get("s9"), None, "key 251 is out of range");
        assert_eq!(last.get("tax"), Some(Visit { key: 2, at: 0 }));

        last.set("ren", 5);
        last.set("tax", 1);
        last.save().unwrap();

        let last = Last::load(dir.path()).unwrap();
        assert_eq!(last.get("ren").unwrap().key, 5);
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

        last.renumber("c", &[(3, 1), (1, 2)].into());
        assert_eq!(last.get("c"), Some(Visit { key: 1, at: 200 }));
        last.renumber("a", &[(3, 1)].into());
        assert_eq!(last.get("a").unwrap().key, 1, "its tab didn't move");
    }

    #[test]
    fn reads_the_versions_it_knows() {
        type Read = BTreeMap<String, u32>;
        assert_eq!(parse::<Read>(r#"{"a":2}"#).unwrap()["a"], 2);
        assert_eq!(parse::<Read>(r#"{"version":1,"a":2}"#).unwrap()["a"], 2);
        let newer = parse::<Read>(r#"{"version":2,"a":2}"#).unwrap_err();
        assert!(newer.to_string().contains("newer space"), "{newer}");
        assert!(parse::<Read>(r#"{"version":"1"}"#).is_err());
        assert!(parse::<Read>("{ half").is_err());
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
