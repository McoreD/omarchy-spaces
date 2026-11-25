//! Spaces: what Spaces remembers about each one, in
//! `~/.local/state/omarchy-spaces/spaces.json`.
//!
//! A space is a set of Hyprland workspaces, `<id>:1` to `<id>:10`, and lives
//! as long as they do: when its last window closes and you leave it,
//! Hyprland drops its workspaces and the space is gone. Hyprland is the
//! source of truth for which spaces exist and their windows; this file only
//! adds what Hyprland can't hold. Entries for spaces Hyprland no longer has
//! are pruned whenever the live list is read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::folder::Folder;
use crate::state;

/// What Spaces remembers about one space.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The name you gave it. Without one, it is called after its folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The icon you picked for its badge, a glyph or an emoji, in place of
    /// the letters of its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// The folder of the terminal it follows, last time it was seen; where
    /// it started before that. New terminals in it open here.
    #[serde(flatten)]
    pub folder: Option<Folder>,
    /// The address of the terminal window it follows: its first one, kept
    /// while that window lives, so the name doesn't change with focus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    /// Its place in the sidebar, lowest first; 0 for none yet. A new space's
    /// is when it was made, in Unix milliseconds, so it comes last and keeps
    /// its place as names change. Moving a space (`space reorder`) hands the
    /// spaces' places out again, in the new order.
    #[serde(default, alias = "created", skip_serializing_if = "is_zero")]
    pub order: u64,
    /// The names you gave its tabs, by what their workspaces are numbered, as
    /// every monitor has its own tab 3 (3 and 13). A tab without one is called
    /// after its most recently focused window. A name lasts as long as its
    /// tab: when Hyprland drops the workspace, the name goes with it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tabs: BTreeMap<u8, String>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    spaces: BTreeMap<String, Entry>,
}

#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    pub spaces: BTreeMap<String, Entry>,
    /// What was read, to write only when something changed.
    read: BTreeMap<String, Entry>,
    /// Held to save it. A failure to lock only matters then.
    lock: Result<std::fs::File>,
}

impl Store {
    /// Reads it, to look at.
    pub fn load(state_dir: &Path) -> Result<Self> {
        Self::read(state_dir, Err(anyhow!("it was only read")))
    }

    /// Reads it, to change and save: other processes wait to do the same
    /// until it's dropped, so changes made at once are all kept.
    pub fn lock(state_dir: &Path) -> Result<Self> {
        Self::read(state_dir, state::lock(state_dir, ".spaces.lock"))
    }

    fn read(state_dir: &Path, mut lock: Result<std::fs::File>) -> Result<Self> {
        let path = state_dir.join("spaces.json");
        let text = state::read_optional(&path)?;
        let file = if text.trim().is_empty() {
            File::default()
        } else {
            state::parse(&text).unwrap_or_else(|e| {
                // One that's going to be saved over is put aside first. Not
                // locked, it may be another by now, and stays.
                if lock.is_ok()
                    && let Err(e) = state::put_aside(&path, &e)
                {
                    lock = Err(e);
                }
                File::default()
            })
        };
        Ok(Self {
            path,
            read: file.spaces.clone(),
            spaces: file.spaces,
            lock,
        })
    }

    /// What the file holds, without the changes not saved yet.
    pub fn saved(&self) -> &BTreeMap<String, Entry> {
        &self.read
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.spaces.get(id)
    }

    pub fn entry(&mut self, id: &str) -> &mut Entry {
        self.spaces.entry(id.to_string()).or_default()
    }

    /// Forgets spaces Hyprland no longer has. `home` is always there.
    pub fn prune(&mut self, live: &BTreeSet<String>) {
        self.spaces
            .retain(|id, _| id == crate::name::HOME || live.contains(id));
    }

    /// An id for a new space: `s1`, `s2`, ..., the first not in use.
    pub fn fresh_id(&self, live: &BTreeSet<String>) -> String {
        (1..)
            .map(|n| format!("s{n}"))
            .find(|id| !live.contains(id) && !self.spaces.contains_key(id))
            .expect("some id is free")
    }

    pub fn save(&mut self) -> Result<()> {
        if self.spaces == self.read {
            return Ok(());
        }
        if let Err(e) = &self.lock {
            bail!("can't save {}: {e:#}", self.path.display());
        }
        let text = serde_json::to_string_pretty(&File {
            version: state::VERSION,
            spaces: self.spaces.clone(),
        })?;
        state::write_atomic(&self.path, &(text + "\n"))
            .with_context(|| format!("can't save {}", self.path.display()))?;
        self.read = self.spaces.clone();
        Ok(())
    }
}

/// A name you give a space: any text, trimmed, on one line. Empty clears it.
pub fn clean_name(raw: &str) -> Result<Option<String>> {
    let name = raw.trim();
    if name.chars().any(char::is_control) {
        bail!("a name has to fit on one line");
    }
    if name.chars().count() > 64 {
        bail!("'{name}' is longer than 64 characters");
    }
    Ok((!name.is_empty()).then(|| name.to_string()))
}

/// An icon you give a space: a Nerd Font glyph or an emoji, which can take
/// several characters (`👩‍💻` is three), trimmed. Empty clears it.
pub fn clean_icon(raw: &str) -> Result<Option<String>> {
    let icon = raw.trim();
    if icon.chars().any(|c| c.is_control() || c.is_whitespace()) {
        bail!("an icon is one glyph or emoji, without spaces");
    }
    if icon.chars().count() > 16 {
        bail!("'{icon}' is too long for an icon: use one glyph or emoji");
    }
    Ok((!icon.is_empty()).then(|| icon.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_prunes_and_hands_out_ids() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::lock(dir.path()).unwrap();
        assert!(store.spaces.is_empty());
        store.entry("s1").name = Some("Japanese".into());
        store.entry("s1").icon = Some("🗾".into());
        store.entry("s2").folder = Some(Folder::here("/p/mail"));
        store.entry("s2").tabs.insert(3, "server".into());
        store.entry("home").tabs.insert(13, "mail".into());
        store.save().unwrap();
        drop(store);

        let mut store = Store::load(dir.path()).unwrap();
        assert_eq!(store.get("s1").unwrap().name.as_deref(), Some("Japanese"));
        assert_eq!(store.get("s1").unwrap().icon.as_deref(), Some("🗾"));
        assert_eq!(
            store.get("s2").unwrap().folder,
            Some(Folder::here("/p/mail"))
        );
        assert_eq!(store.get("s2").unwrap().tabs[&3], "server");

        let live: BTreeSet<String> = ["s2".to_string(), "s4".to_string()].into();
        store.prune(&live);
        assert_eq!(store.spaces.keys().collect::<Vec<_>>(), ["home", "s2"]);
        assert_eq!(store.fresh_id(&live), "s1");
        let live: BTreeSet<String> = ["s1".to_string()].into();
        assert_eq!(store.fresh_id(&live), "s3", "s2 is still remembered");
    }

    #[test]
    fn a_file_it_cant_read_is_put_aside() {
        for text in ["{ nope", r#"{ "version": 99, "spaces": {} }"#] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("spaces.json");
            std::fs::write(&path, text).unwrap();
            assert!(Store::load(dir.path()).unwrap().spaces.is_empty());
            assert!(path.exists(), "only looked at");

            let mut store = Store::lock(dir.path()).unwrap();
            assert!(store.spaces.is_empty());
            store.entry("s1").name = Some("Mail".into());
            store.save().unwrap();
            let kept = std::fs::read_dir(dir.path()).unwrap().flatten();
            let aside = kept
                .map(|file| file.path())
                .find(|path| path.to_string_lossy().contains("spaces.broken-"));
            assert_eq!(std::fs::read_to_string(aside.unwrap()).unwrap(), text);
            let store = Store::load(dir.path()).unwrap();
            assert_eq!(store.get("s1").unwrap().name.as_deref(), Some("Mail"));
        }
    }

    #[test]
    fn says_its_version_and_reads_files_from_before() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spaces.json");
        std::fs::write(&path, r#"{ "spaces": { "s1": { "created": 5 } } }"#).unwrap();
        let mut store = Store::lock(dir.path()).unwrap();
        assert_eq!(store.get("s1").unwrap().order, 5);
        store.entry("s1").order = 6;
        store.save().unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved,
            serde_json::json!({ "version": 1, "spaces": { "s1": { "order": 6 } } })
        );
    }

    #[test]
    fn cleans_names() {
        assert_eq!(clean_name("  Japanese ").unwrap(), Some("Japanese".into()));
        assert_eq!(clean_name("  ").unwrap(), None);
        assert!(clean_name("a\nb").is_err());
        assert!(clean_name(&"x".repeat(65)).is_err());
        assert_eq!(clean_name("日本語").unwrap(), Some("日本語".into()));
    }

    #[test]
    fn cleans_icons() {
        assert_eq!(clean_icon(" 🚀 ").unwrap(), Some("🚀".into()));
        assert_eq!(clean_icon("👩‍💻").unwrap(), Some("👩‍💻".into()));
        assert_eq!(clean_icon("\u{f0415}").unwrap(), Some("\u{f0415}".into()));
        assert_eq!(clean_icon("").unwrap(), None);
        assert!(clean_icon("a b").is_err());
        assert!(clean_icon("a\nb").is_err());
        assert!(clean_icon(&"x".repeat(17)).is_err());
    }
}
