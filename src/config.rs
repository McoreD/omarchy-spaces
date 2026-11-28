//! `~/.config/omarchy-spaces/config.toml`, the global config: what Spaces
//! itself does. What Omarchy has a setting for, such as which terminal to
//! open, is Omarchy's to say (`launch.rs`).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde::de::IgnoredAny;

use crate::paths;

/// Settings that aren't ours: misspelled, or an older or newer Spaces'. Each
/// table takes its own, to warn of them; the rest of the file still counts.
type Unknown = BTreeMap<String, IgnoredAny>;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    /// What comes back after a restart (`space restore`).
    pub restore: Restore,
    /// The sounds when an agent needs you (`sound.rs`).
    pub sounds: Sounds,
    /// What a new space opens (`newspace.rs`).
    pub new_space: NewSpace,
    /// How a space's tabs are numbered and shown (`monitors.rs`).
    pub tabs: Tabs,
    #[serde(flatten)]
    unknown: Unknown,
}

/// How the tabs of a space are numbered and shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TabStyle {
    /// As a browser's: numbered 1, 2, 3, ... again as one ends.
    #[default]
    Browser,
    /// As Omarchy's workspaces: each keeps its number, and 1-5 always show.
    Omarchy,
}

impl TabStyle {
    pub fn word(self) -> &'static str {
        match self {
            TabStyle::Browser => "browser",
            TabStyle::Omarchy => "omarchy",
        }
    }

    fn of(word: &str) -> Option<Self> {
        match word {
            "browser" => Some(TabStyle::Browser),
            "omarchy" => Some(TabStyle::Omarchy),
            _ => None,
        }
    }
}

/// `[tabs]`: how they're numbered and shown.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Tabs {
    /// `browser` or `omarchy`. Anything else is read as `browser`, with a
    /// warning.
    pub style: String,
    #[serde(flatten)]
    unknown: Unknown,
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            style: TabStyle::Browser.word().into(),
            unknown: Unknown::new(),
        }
    }
}

/// `[new_space]`: what `space new` opens, all on the new space's first tab.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct NewSpace {
    /// `terminal`, `files`, or an Omarchy agent's id, side by side in this
    /// order; none for an empty space. Whether each is one Spaces knows is
    /// for `newspace.rs` to say, when the space opens.
    pub open: Vec<String>,
    #[serde(flatten)]
    unknown: Unknown,
}

impl Default for NewSpace {
    fn default() -> Self {
        Self {
            open: vec!["terminal".into()],
            unknown: Unknown::new(),
        }
    }
}

/// `[restore]`: whether the plugin restores on start, and what comes back.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Restore {
    /// Whether the plugin opens your last spaces again after a restart. `space
    /// restore` does either way.
    pub auto: bool,
    /// Whether a restart picks up the agents' conversations again.
    pub agents: bool,
    /// Whether a restart opens the browser's windows again, and so whether
    /// what they hold is saved.
    pub browsers: bool,
    #[serde(flatten)]
    unknown: Unknown,
}

impl Default for Restore {
    fn default() -> Self {
        Self {
            auto: true,
            agents: true,
            browsers: true,
            unknown: Unknown::new(),
        }
    }
}

/// `[sounds]`: whether to play them, and files in place of the defaults.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Sounds {
    pub enabled: bool,
    /// When an agent finishes while you aren't looking.
    pub done: Option<String>,
    /// When an agent starts waiting for you.
    pub waiting: Option<String>,
    #[serde(flatten)]
    unknown: Unknown,
}

impl Config {
    /// Reads the config file, or the defaults when there is none.
    pub fn load() -> Result<Self> {
        Self::load_from(&paths::config_file())
    }

    /// Reads the config file, warning and falling back to defaults when it
    /// can't be read.
    pub fn load_or_default() -> Self {
        Self::load().unwrap_or_else(|e| {
            eprintln!("space: warning: {e:#}");
            Self::default()
        })
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => {
                let config: Self = toml::from_str(&text)
                    .with_context(|| format!("can't read {}", paths::tilde(path)))?;
                let mut config = config;
                if TabStyle::of(&config.tabs.style).is_none() {
                    eprintln!(
                        "space: warning: '{}' isn't a value for tabs.style, in {}: using browser",
                        config.tabs.style,
                        paths::tilde(path)
                    );
                    config.tabs.style = TabStyle::Browser.word().into();
                }
                for setting in config.unknown() {
                    eprintln!(
                        "space: warning: there's no setting '{setting}', in {}",
                        paths::tilde(path)
                    );
                }
                Ok(config)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("can't read {}", paths::tilde(path))),
        }
    }

    /// The style of the tabs. A style that isn't one reads as `browser`.
    pub fn tab_style(&self) -> TabStyle {
        TabStyle::of(&self.tabs.style).unwrap_or_default()
    }

    /// The settings that aren't ours, as `sounds.bell`.
    fn unknown(&self) -> Vec<String> {
        [
            ("", &self.unknown),
            ("restore.", &self.restore.unknown),
            ("sounds.", &self.sounds.unknown),
            ("new_space.", &self.new_space.unknown),
            ("tabs.", &self.tabs.unknown),
        ]
        .into_iter()
        .flat_map(|(table, unknown)| unknown.keys().map(move |key| format!("{table}{key}")))
        .collect()
    }
}

/// When a file last changed, and its size; `None` for no file.
type Stamp = Option<(SystemTime, u64)>;

/// What the config file last said of the tabs, and when it said it, so that
/// it's read again only after a change: the tabs ask on every look at them,
/// many a second in `space watch`.
static TAB_STYLE: Mutex<Option<(Stamp, TabStyle)>> = Mutex::new(None);

/// The style of the tabs: the one thing that `space`, the plugin and the Lua
/// in Hyprland all go by, so that they can't disagree. It's what the config
/// says now, read again when the file changes, and every `space` that reads
/// it writes it to a file of its own in the state folder (`paths::tab_style`),
/// which the Lua reads. A file that can't be read as TOML, as it may be
/// halfway through an edit, leaves the style as it last was, as does one with
/// something else wrong in it: only `[tabs] style` counts. No file or no
/// setting is `browser`.
pub fn tab_style() -> TabStyle {
    let Ok(mut cached) = TAB_STYLE.lock() else {
        return TabStyle::default();
    };
    read_tab_style(&paths::config_file(), &paths::tab_style_file(), &mut cached)
}

fn read_tab_style(config: &Path, saved: &Path, cached: &mut Option<(Stamp, TabStyle)>) -> TabStyle {
    let stamp = fs::metadata(config)
        .ok()
        .and_then(|meta| Some((meta.modified().ok()?, meta.len())));
    if let Some((was, style)) = *cached
        && was == stamp
    {
        return style;
    }
    // As it last was: here, else in whatever last ran.
    let saved_style = fs::read_to_string(saved)
        .ok()
        .and_then(|text| TabStyle::of(text.trim()));
    let last = cached.map(|(_, style)| style).or(saved_style);
    let style = match fs::read_to_string(config) {
        Ok(text) => match text.parse::<toml::Table>() {
            Ok(table) => style_in(&table, config),
            Err(e) => {
                eprintln!(
                    "space: warning: can't read {}: {e}; the tabs stay {}",
                    paths::tilde(config),
                    last.unwrap_or_default().word()
                );
                last.unwrap_or_default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => TabStyle::default(),
        Err(_) => last.unwrap_or_default(),
    };
    if saved_style != Some(style)
        && let Err(e) = crate::state::write_atomic(saved, &format!("{}\n", style.word()))
    {
        eprintln!("space: warning: {e:#}");
    }
    *cached = Some((stamp, style));
    style
}

/// `[tabs] style` of a config, `browser` if it isn't there or isn't a style.
fn style_in(table: &toml::Table, config: &Path) -> TabStyle {
    let Some(style) = table.get("tabs").and_then(|tabs| tabs.get("style")) else {
        return TabStyle::default();
    };
    style.as_str().and_then(TabStyle::of).unwrap_or_else(|| {
        eprintln!(
            "space: warning: tabs.style isn't browser or omarchy, in {}: using browser",
            paths::tilde(config)
        );
        TabStyle::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Result<Config> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, text).unwrap();
        Config::load_from(&path)
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load_from(&dir.path().join("config.toml")).unwrap();
        let restore = config.restore;
        assert!(restore.auto && restore.agents && restore.browsers);
        assert!(!config.sounds.enabled, "no new noises unless you ask");
    }

    #[test]
    fn reads_restore() {
        let restore = read("[restore]\nbrowsers = false\n").unwrap().restore;
        assert!(restore.auto && restore.agents && !restore.browsers);

        let restore = read("[restore]\nauto = false\nagents = false\n")
            .unwrap()
            .restore;
        assert!(!restore.auto && !restore.agents && restore.browsers);
    }

    #[test]
    fn reads_sounds() {
        let sounds = read("[sounds]\nenabled = true\ndone = \"~/ding.oga\"\n")
            .unwrap()
            .sounds;
        assert!(sounds.enabled);
        assert_eq!(sounds.done.as_deref(), Some("~/ding.oga"));
        assert_eq!(sounds.waiting, None);
    }

    #[test]
    fn reads_what_a_new_space_opens() {
        assert_eq!(read("").unwrap().new_space.open, ["terminal"]);
        let open = |text: &str| read(text).unwrap().new_space.open;
        assert_eq!(
            open("[new_space]\nopen = [\"files\", \"claude\"]\n"),
            ["files", "claude"]
        );
        assert!(open("[new_space]\nopen = []\n").is_empty(), "none");
        assert!(read("[new_space]\nopen = \"files\"\n").is_err());
        let config = read("[new_space]\nopen = [\"files\"]\nshape = 1\n").unwrap();
        assert_eq!(config.unknown(), ["new_space.shape"]);
    }

    #[test]
    fn reads_the_tab_style() {
        assert_eq!(read("").unwrap().tab_style(), TabStyle::Browser);
        let style = |text: &str| read(text).unwrap().tab_style();
        assert_eq!(style("[tabs]\nstyle = \"omarchy\"\n"), TabStyle::Omarchy);
        assert_eq!(style("[tabs]\nstyle = \"browser\"\n"), TabStyle::Browser);
        // A word that isn't one: browser, and the file still counts.
        let config = read("[tabs]\nstyle = \"chrome\"\n[restore]\nauto = false\n").unwrap();
        assert_eq!(config.tab_style(), TabStyle::Browser);
        assert_eq!(config.tabs.style, "browser");
        assert!(!config.restore.auto);
        assert!(read("[tabs]\nstyle = 3\n").is_err());
        let config = read("[tabs]\nshape = 1\n").unwrap();
        assert_eq!(config.unknown(), ["tabs.shape"]);
    }

    #[test]
    fn the_style_of_the_tabs_has_one_source() {
        let dir = tempfile::tempdir().unwrap();
        let (config, saved) = (
            dir.path().join("config.toml"),
            dir.path().join("s/tab-style"),
        );
        let style =
            |cached: &mut Option<(Stamp, TabStyle)>| read_tab_style(&config, &saved, cached);
        let said = || fs::read_to_string(&saved).ok();
        let mut cached = None;
        // No config: a browser's.
        assert_eq!(style(&mut cached), TabStyle::Browser);
        assert_eq!(said().as_deref(), Some("browser\n"));

        // Another setting that's wrong doesn't matter, only the style.
        fs::write(
            &config,
            "[sounds]\nenabled = \"yes\"\n[tabs]\nstyle = \"omarchy\"\n",
        )
        .unwrap();
        assert_eq!(style(&mut cached), TabStyle::Omarchy);
        assert_eq!(said().as_deref(), Some("omarchy\n"));

        // Halfway through an edit: as it was, and what it wrote stays.
        fs::write(&config, "[tabs\nstyle = \"browser\"\nand then some\n").unwrap();
        assert_eq!(style(&mut cached), TabStyle::Omarchy);
        assert_eq!(said().as_deref(), Some("omarchy\n"));
        // A process of its own that has seen nothing before goes by the file.
        assert_eq!(style(&mut None), TabStyle::Omarchy);
        // Mended.
        fs::write(&config, "[tabs]\nstyle = \"browser\"\nand = 1\n").unwrap();
        assert_eq!(style(&mut cached), TabStyle::Browser);
        assert_eq!(said().as_deref(), Some("browser\n"));

        // The forms the Lua once scanned wrongly are read as TOML.
        for (text, want) in [
            ("tabs = { style = \"omarchy\" }\n", TabStyle::Omarchy),
            ("tabs.style = \"omarchy\"\n", TabStyle::Omarchy),
            (
                "[[x]]\n[tabs]\nstyle = 'omarchy' # yes\n",
                TabStyle::Omarchy,
            ),
            ("[other]\nstyle = \"omarchy\"\n", TabStyle::Browser),
            ("[tabs]\nstyle = \"chrome\"\n", TabStyle::Browser),
            ("[tabs]\nstyle = 3\n", TabStyle::Browser),
        ] {
            fs::write(&config, text).unwrap();
            assert_eq!(style(&mut None), want, "{text}");
            assert_eq!(
                said().as_deref(),
                Some(format!("{}\n", want.word()).as_str())
            );
        }
        // Broken with nothing saved: a browser's.
        fs::remove_file(&saved).unwrap();
        fs::write(&config, "[tabs\n").unwrap();
        assert_eq!(style(&mut None), TabStyle::Browser);
    }

    #[test]
    fn unknown_settings_leave_the_others_be() {
        let config = read(
            "roots = [\"~/dev\"]\n\
             [restore]\nbrowsers = false\napps = true\n\
             [sounds]\nenabled = true\nbell = { a = 1.5 }\n\
             [icons]\nsize = 2\n",
        )
        .unwrap();
        assert!(!config.restore.browsers && config.restore.auto);
        assert!(config.sounds.enabled);
        assert_eq!(
            config.unknown(),
            ["icons", "roots", "restore.apps", "sounds.bell"]
        );

        // A value of the wrong kind is still a mistake.
        assert!(read("[sounds]\nenabled = \"no\"\n").is_err());
    }
}
