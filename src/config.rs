//! `~/.config/omarchy-spaces/config.toml`, the global config.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

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
/// Omarchy. Outside a uwsm session, without the app scope. When herdr-core
/// is enabled (or `hc` is on `PATH`), it runs `space shell` (`shell.rs`);
/// otherwise it opens a plain terminal directly.
const OMARCHY_TERMINAL: &str = "exec setsid uwsm-app -- xdg-terminal-exec --dir=\"$SPACES_DIR\"";
const PLAIN_TERMINAL: &str = "exec xdg-terminal-exec --dir=\"$SPACES_DIR\"";

/// Runs an app as Omarchy does, in a scope of its own.
pub fn app_launcher() -> &'static str {
    if on_path("uwsm-app") {
        "exec setsid uwsm-app --"
    } else {
        "exec"
    }
}

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
    /// Whether terminals Spaces opens run in herdr-core (`space shell`).
    /// When unset, uses herdr-core only when its `hc` is on `PATH`.
    pub herdr_core: Option<bool>,
    /// What comes back after a restart (`space restore`).
    pub restore: Restore,
    /// The sounds when an agent needs you (`sound.rs`).
    pub sounds: Sounds,
}

/// `[restore]`: whether the plugin restores on start, and what comes back.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Restore {
    /// Whether the plugin opens your last spaces again after a restart.
    pub enabled: bool,
    /// Whether a restart picks up the agents' conversations again.
    pub agents: bool,
    /// Whether a restart opens Chromium's windows again.
    pub browser: bool,
}

impl Default for Restore {
    fn default() -> Self {
        Self {
            enabled: true,
            agents: true,
            browser: true,
        }
    }
}

/// `[sounds]`: whether to play them, and files in place of the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
                toml::from_str(&text).with_context(|| format!("can't read {}", paths::tilde(path)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("can't read {}", paths::tilde(path))),
        }
    }

    /// Whether default terminals run in herdr-core (`space shell`).
    pub fn use_herdr_core(&self) -> bool {
        self.herdr_core.unwrap_or_else(has_herdr_core)
    }

    /// The terminal command for picking up a conversation, which `space shell`
    /// does. A terminal of your own has to run that itself.
    pub fn resuming_terminal(&self) -> String {
        let terminal = self.terminal();
        if self.terminal.is_some() || self.use_herdr_core() {
            return terminal;
        }
        format!("{terminal} {}", space_shell())
    }

    /// The configured terminal command, or Omarchy's way.
    pub fn terminal(&self) -> String {
        if let Some(terminal) = &self.terminal {
            return terminal.clone();
        }
        let base = if on_path("uwsm-app") {
            OMARCHY_TERMINAL
        } else {
            PLAIN_TERMINAL
        };
        if self.use_herdr_core() {
            format!("{base} {}", space_shell())
        } else {
            base.to_string()
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
    find_on_path(program).is_some()
}

fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// Whether herdr-core's `hc` is on `PATH` (and not an unrelated binary of the
/// same name, such as `herbstclient`).
pub fn has_herdr_core() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| find_on_path("hc").is_some_and(|hc| is_herdr_core(&hc)))
}

fn is_herdr_core(binary: &Path) -> bool {
    Command::new(binary)
        .arg("--help")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|out| {
            out.status.success() && String::from_utf8_lossy(&out.stdout).contains("herdr-core")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load_from(&dir.path().join("config.toml")).unwrap();
        assert_eq!(config.picker, None);
        assert_eq!(config.restore, Restore::default());
        assert_eq!(config.sounds, Sounds::default());
        assert!(config.terminal().contains("xdg-terminal-exec"));
        assert_eq!(
            config.terminal().ends_with("' shell"),
            has_herdr_core(),
            "{}",
            config.terminal()
        );
    }

    #[test]
    fn herdr_core_setting_controls_shell_wrapper() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        fs::write(&path, "herdr_core = false\n").unwrap();
        let config = Config::load_from(&path).unwrap();
        assert!(!config.use_herdr_core());
        assert!(config.terminal().contains("xdg-terminal-exec"));
        assert!(
            !config.terminal().ends_with("' shell"),
            "{}",
            config.terminal()
        );

        fs::write(&path, "herdr_core = true\n").unwrap();
        let config = Config::load_from(&path).unwrap();
        assert!(config.use_herdr_core());
        assert!(
            config.terminal().ends_with("' shell"),
            "{}",
            config.terminal()
        );
    }

    #[test]
    fn distinguishes_herdr_core_from_another_hc() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = |name: &str, body: &str| {
            let path = dir.path().join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let other = script("other", "echo 'herbstclient usage'");
        let broken = script("broken", "exit 1");
        let core = script("core", "echo 'Settings: ~/.config/herdr-core/config.toml'");
        assert!(!is_herdr_core(&other));
        assert!(!is_herdr_core(&broken));
        assert!(is_herdr_core(&core));
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
    fn reads_restore() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[restore]\nbrowser = false\n").unwrap();
        let restore = Config::load_from(&path).unwrap().restore;
        assert!(restore.enabled);
        assert!(restore.agents);
        assert!(!restore.browser);

        fs::write(&path, "[restore]\nenabled = false\nagents = false\n").unwrap();
        let restore = Config::load_from(&path).unwrap().restore;
        assert!(!restore.enabled);
        assert!(!restore.agents);
        assert!(restore.browser);

        fs::write(&path, "[restore]\napps = true\n").unwrap();
        assert!(Config::load_from(&path).is_err());
    }

    #[test]
    fn reads_sounds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[sounds]\ndone = \"~/ding.oga\"\n").unwrap();
        let sounds = Config::load_from(&path).unwrap().sounds;
        assert!(sounds.enabled);
        assert_eq!(sounds.done.as_deref(), Some("~/ding.oga"));
        fs::write(&path, "[sounds]\nenabled = false\n").unwrap();
        assert!(!Config::load_from(&path).unwrap().sounds.enabled);
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
