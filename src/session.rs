//! The spaces that were up, saved so `space restore` can open them again
//! after a restart. `space watch` writes `session.json`; unlike
//! `spaces.json`, it isn't pruned the moment Hyprland drops a space.
//!
//! Logging out closes every window, which looks just like closing every
//! space, so a change is saved only once it has lasted `SETTLE`: by then a
//! session that was ending has ended. A session saved under another Hyprland
//! is moved aside to `session-previous.json` before this one's is saved, and
//! stays there until it's restored.

use std::collections::BTreeMap;
use std::fs::{File, TryLockError};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::state;

/// How long the spaces must stay as they are before they're saved.
const SETTLE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// The Hyprland it was saved under (`HYPRLAND_INSTANCE_SIGNATURE`).
    pub instance: String,
    /// In the sidebar's order.
    pub spaces: Vec<Space>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Space {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// The names you gave its tabs, by slot.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tabs: BTreeMap<u8, String>,
    /// Oldest first.
    pub terminals: Vec<Terminal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Terminal {
    pub slot: u8,
    pub folder: PathBuf,
}

fn current_path(state_dir: &Path) -> PathBuf {
    state_dir.join("session.json")
}

fn previous_path(state_dir: &Path) -> PathBuf {
    state_dir.join("session-previous.json")
}

fn read(path: &Path) -> Result<Option<Session>> {
    let text = state::read_optional(path)?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&text)
        .map(Some)
        .with_context(|| format!("can't understand {}", path.display()))
}

fn write(path: &Path, session: &Session) -> Result<()> {
    let text = serde_json::to_string_pretty(session)?;
    state::write_atomic(path, &(text + "\n"))
}

fn lock_file(state_dir: &Path, name: &str) -> Result<File> {
    std::fs::create_dir_all(state_dir)
        .with_context(|| format!("can't create {}", state_dir.display()))?;
    let path = state_dir.join(name);
    File::create(&path).with_context(|| format!("can't open {}", path.display()))
}

/// Held while restoring, until it's dropped. `None` when another `space
/// restore` holds it.
pub fn lock_restore(state_dir: &Path) -> Result<Option<File>> {
    let file = lock_file(state_dir, ".restore.lock")?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e).context("can't lock the restore"),
    }
}

/// Moves a session saved under another Hyprland aside, for `restore`, after
/// any spaces an earlier restore didn't get to. `watch` and `restore` both
/// do this as the plugin starts, so one waits for the other.
pub fn set_aside(state_dir: &Path, instance: &str) -> Result<()> {
    let lock = lock_file(state_dir, ".session.lock")?;
    lock.lock().context("can't lock the session")?;
    let path = current_path(state_dir);
    if let Some(mut saved) = read(&path)?
        && saved.instance != instance
        && !saved.spaces.is_empty()
    {
        if let Some(left) = previous(state_dir)? {
            saved.spaces.splice(0..0, left.spaces);
        }
        write(&previous_path(state_dir), &saved)?;
        std::fs::remove_file(&path).with_context(|| format!("can't remove {}", path.display()))?;
    }
    Ok(())
}

/// The session to restore, if there is one.
pub fn previous(state_dir: &Path) -> Result<Option<Session>> {
    Ok(read(&previous_path(state_dir))?.filter(|s| !s.spaces.is_empty()))
}

/// Saves what is left to restore, so a restore that stops halfway can go on
/// without opening the same spaces twice.
pub fn set_previous(state_dir: &Path, session: &Session) -> Result<()> {
    let path = previous_path(state_dir);
    if !session.spaces.is_empty() {
        return write(&path, session);
    }
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != ErrorKind::NotFound => {
            Err(e).with_context(|| format!("can't remove {}", path.display()))
        }
        _ => Ok(()),
    }
}

/// Saves the session as it changes, once each change has settled.
pub struct Saver {
    path: PathBuf,
    saved: Option<Session>,
    pending: Option<(Session, Instant)>,
}

impl Saver {
    pub fn new(state_dir: &Path, instance: &str) -> Result<Self> {
        set_aside(state_dir, instance)?;
        let path = current_path(state_dir);
        Ok(Self {
            saved: read(&path)?,
            path,
            pending: None,
        })
    }

    pub fn update(&mut self, session: Session, now: Instant) -> Result<()> {
        if self.saved.as_ref() == Some(&session) {
            self.pending = None;
            return Ok(());
        }
        match &self.pending {
            Some((pending, since)) if *pending == session => {
                if now.duration_since(*since) < SETTLE {
                    return Ok(());
                }
                write(&self.path, &session)?;
                self.saved = Some(session);
                self.pending = None;
            }
            _ => self.pending = Some((session, now)),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(instance: &str, folders: &[&str]) -> Session {
        Session {
            instance: instance.into(),
            spaces: folders
                .iter()
                .map(|folder| Space {
                    terminals: vec![Terminal {
                        slot: 1,
                        folder: folder.into(),
                    }],
                    ..Default::default()
                })
                .collect(),
        }
    }

    #[test]
    fn saves_only_what_lasts() {
        let dir = tempfile::tempdir().unwrap();
        let path = current_path(dir.path());
        let start = Instant::now();
        let mut saver = Saver::new(dir.path(), "a").unwrap();
        let both = session("a", &["/x", "/y"]);
        saver.update(both.clone(), start).unwrap();
        assert_eq!(read(&path).unwrap(), None);
        saver.update(both.clone(), start + SETTLE).unwrap();
        assert_eq!(read(&path).unwrap(), Some(both.clone()));

        // Logging out: they go one by one, then the watch does.
        let secs = |n| start + SETTLE + Duration::from_secs(n);
        saver.update(session("a", &["/y"]), secs(1)).unwrap();
        saver.update(session("a", &[]), secs(9)).unwrap();
        saver.update(session("a", &[]), secs(18)).unwrap();
        assert_eq!(read(&path).unwrap(), Some(both));

        // Closed on purpose, and still closed.
        saver.update(session("a", &[]), secs(19)).unwrap();
        assert_eq!(read(&path).unwrap(), Some(session("a", &[])));
    }

    #[test]
    fn restores_one_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let held = lock_restore(dir.path()).unwrap();
        assert!(held.is_some());
        assert!(lock_restore(dir.path()).unwrap().is_none());
        drop(held);
        assert!(lock_restore(dir.path()).unwrap().is_some());
    }

    #[test]
    fn sets_another_hyprlands_session_aside_until_restored() {
        let dir = tempfile::tempdir().unwrap();
        let path = current_path(dir.path());
        write(&path, &session("a", &["/x"])).unwrap();
        set_aside(dir.path(), "a").unwrap();
        assert_eq!(previous(dir.path()).unwrap(), None);
        set_aside(dir.path(), "b").unwrap();
        assert_eq!(previous(dir.path()).unwrap(), Some(session("a", &["/x"])));

        // A session that never had a space doesn't take its place.
        write(&path, &session("b", &[])).unwrap();
        set_aside(dir.path(), "c").unwrap();
        assert_eq!(previous(dir.path()).unwrap().unwrap().instance, "a");

        // One that ended before everything was restored keeps the rest.
        write(&path, &session("c", &["/y"])).unwrap();
        set_aside(dir.path(), "d").unwrap();
        assert_eq!(
            previous(dir.path()).unwrap(),
            Some(session("c", &["/x", "/y"]))
        );

        set_previous(dir.path(), &session("a", &[])).unwrap();
        assert_eq!(previous(dir.path()).unwrap(), None);
    }
}
