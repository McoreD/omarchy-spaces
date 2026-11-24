//! Spaces: what renSpace remembers about each one, in
//! `~/.local/state/renspace/spaces.json`.
//!
//! A space is a set of Hyprland workspaces, `<id>:1` to `<id>:10`, and lives
//! as long as they do: when its last window closes and you leave it,
//! Hyprland drops its workspaces and the space is gone. Hyprland is the
//! source of truth for which spaces exist and their windows; this file only
//! adds what Hyprland can't hold. Entries for spaces Hyprland no longer has
//! are pruned whenever the live list is read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::state;

/// What renSpace remembers about one space.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
    /// The address of the terminal window it follows: its first one, kept
    /// while that window lives, so the name doesn't change with focus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    /// When it was made, in Unix milliseconds: the sidebar lists spaces in
    /// the order you made them, so they keep their places as names change.
    /// Moving a space (`space reorder`) hands these times out again, in the
    /// new order.
    #[serde(default)]
    pub created: u64,
    /// The names you gave its tabs, by slot. A tab without one is called
    /// after its most recently focused window. A name lasts as long as its
    /// tab: when Hyprland drops the workspace, the name goes with it.
    /// `home`'s are by workspace id instead, as every monitor has its own
    /// slot 3 (workspaces 3 and 13).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tabs: BTreeMap<u8, String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    spaces: BTreeMap<String, Entry>,
}

#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    pub spaces: BTreeMap<String, Entry>,
    /// What was read, to write only when something changed.
    read: BTreeMap<String, Entry>,
}

impl Store {
    pub fn load(state_dir: &Path) -> Result<Self> {
        let path = state_dir.join("spaces.json");
        let text = state::read_optional(&path)?;
        let file: File = if text.trim().is_empty() {
            File::default()
        } else {
            // A broken file loses only names and folders; say so, start over.
            serde_json::from_str(&text).unwrap_or_else(|e| {
                eprintln!("space: warning: ignoring {}: {e}", path.display());
                File::default()
            })
        };
        Ok(Self {
            path,
            read: file.spaces.clone(),
            spaces: file.spaces,
        })
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
        let text = serde_json::to_string_pretty(&File {
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
        anyhow::bail!("a name has to fit on one line");
    }
    if name.chars().count() > 64 {
        anyhow::bail!("'{name}' is longer than 64 characters");
    }
    Ok((!name.is_empty()).then(|| name.to_string()))
}

/// An icon you give a space: a Nerd Font glyph or an emoji, which can take
/// several characters (`👩‍💻` is three), trimmed. Empty clears it.
pub fn clean_icon(raw: &str) -> Result<Option<String>> {
    let icon = raw.trim();
    if icon.chars().any(|c| c.is_control() || c.is_whitespace()) {
        anyhow::bail!("an icon is one glyph or emoji, without spaces");
    }
    if icon.chars().count() > 16 {
        anyhow::bail!("'{icon}' is too long for an icon: use one glyph or emoji");
    }
    Ok((!icon.is_empty()).then(|| icon.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_prunes_and_hands_out_ids() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::load(dir.path()).unwrap();
        assert!(store.spaces.is_empty());
        store.entry("s1").name = Some("Japanese".into());
        store.entry("s1").icon = Some("🗾".into());
        store.entry("s2").folder = Some("/p/mail".into());
        store.entry("s2").tabs.insert(3, "server".into());
        store.entry("home").tabs.insert(13, "mail".into());
        store.save().unwrap();

        let mut store = Store::load(dir.path()).unwrap();
        assert_eq!(store.get("s1").unwrap().name.as_deref(), Some("Japanese"));
        assert_eq!(store.get("s1").unwrap().icon.as_deref(), Some("🗾"));
        assert_eq!(
            store.get("s2").unwrap().folder,
            Some(PathBuf::from("/p/mail"))
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
    fn a_broken_file_starts_over() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("spaces.json"), "{ nope").unwrap();
        assert!(Store::load(dir.path()).unwrap().spaces.is_empty());
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
