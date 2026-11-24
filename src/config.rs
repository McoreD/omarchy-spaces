//! `~/.config/omarchy-spaces/config.toml`, the global config.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::paths;

/// Pickers for `space go` with no name, used in this order when the config
/// names none: space names on stdin, the chosen line on stdout, run with
/// `sh -c`. Omarchy's walker needs its elephant service running, so vicinae,
/// whose server is usually up already, goes first when it is installed.
pub const DEFAULT_PICKERS: &[(&str, &str)] = &[
    (
        "vicinae",
        "vicinae dmenu --placeholder 'Switch space' --no-quick-look",
    ),
    ("walker", "walker --dmenu --placeholder 'Switch space'"),
];

/// Opens a terminal in `$SPACES_DIR` the way Omarchy's `super+enter`
/// does (`omarchy-launch-terminal`), so it is the terminal you chose in
/// Omarchy. Outside a uwsm session, without the app scope. It runs
/// `space shell`, a shell in herdr-core when it's installed (`shell.rs`).
const OMARCHY_TERMINAL: &str = "exec setsid uwsm-app -- xdg-terminal-exec --dir=\"$SPACES_DIR\"";
const PLAIN_TERMINAL: &str = "exec xdg-terminal-exec --dir=\"$SPACES_DIR\"";

/// This program, to run as `space shell`, by its path so it needn't be on
/// the terminal's PATH.
fn space_shell() -> String {
    let exe = std::env::current_exe()
        .map(|exe| exe.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "space".into());
    format!("'{}' shell", exe.replace('\'', "'\\''"))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// A dmenu-style command for `space go` with no name.
    pub picker: Option<String>,
    /// How to open a terminal: a command for `sh -c`, run in the folder with
    /// `SPACES_DIR` set to it.
    pub terminal: Option<String>,
    /// The sounds when an agent needs you (`sound.rs`).
    pub sounds: Option<Sounds>,
}

/// `[sounds]`: whether to play them, and files in place of the defaults.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sounds {
    pub enabled: bool,
    /// When an agent finishes while you aren't looking.
    pub done: Option<String>,
    /// When an agent starts waiting for you.
    pub waiting: Option<String>,
}

impl Default for Sounds {
    fn default() -> Self {
        Self {
            enabled: true,
            done: None,
            waiting: None,
        }
    }
}

impl Config {
    /// Reads the config file, or the defaults when there is none.
    pub fn load() -> Result<Self> {
        Self::load_from(&paths::config_file())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("can't read {}", paths::tilde(path)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("can't read {}", paths::tilde(path))),
        }
    }

    /// The configured terminal command, or Omarchy's way.
    pub fn terminal(&self) -> String {
        match &self.terminal {
            Some(terminal) => terminal.clone(),
            None if on_path("uwsm-app") => format!("{OMARCHY_TERMINAL} {}", space_shell()),
            None => format!("{PLAIN_TERMINAL} {}", space_shell()),
        }
    }

    /// The configured picker, or the first installed default.
    pub fn picker(&self) -> Option<String> {
        if let Some(picker) = &self.picker {
            return Some(picker.clone());
        }
        DEFAULT_PICKERS
            .iter()
            .find(|(program, _)| on_path(program))
            .map(|(_, command)| command.to_string())
    }
}

pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load_from(&dir.path().join("config.toml")).unwrap();
        assert_eq!(config.picker, None);
        assert!(config.terminal().contains("xdg-terminal-exec"));
        assert!(
            config.terminal().ends_with("' shell"),
            "{}",
            config.terminal()
        );
    }

    #[test]
    fn reads_partial_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "picker = \"vicinae dmenu\"\n").unwrap();
        let config = Config::load_from(&path).unwrap();
        assert_eq!(config.picker(), Some("vicinae dmenu".into()));
        assert!(config.terminal().contains("xdg-terminal-exec"));
    }

    #[test]
    fn reads_sounds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[sounds]\ndone = \"~/ding.oga\"\n").unwrap();
        let sounds = Config::load_from(&path).unwrap().sounds.unwrap();
        assert!(sounds.enabled);
        assert_eq!(sounds.done.as_deref(), Some("~/ding.oga"));
        fs::write(&path, "[sounds]\nenabled = false\n").unwrap();
        assert!(!Config::load_from(&path).unwrap().sounds.unwrap().enabled);
        fs::write(&path, "[sounds]\nbell = \"x\"\n").unwrap();
        assert!(Config::load_from(&path).is_err());
    }

    #[test]
    fn rejects_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "roots = [\"~/dev\"]\n").unwrap();
        assert!(Config::load_from(&path).is_err());
    }
}
