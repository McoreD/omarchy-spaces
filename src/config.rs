//! `~/.config/omarchy-spaces/config.toml`, the global config.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde::de::IgnoredAny;

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
/// Omarchy.
const XDG_TERMINAL: &str = "xdg-terminal-exec --dir=\"$SPACES_DIR\"";

/// Ghostty opens all its windows from one process, which Hyprland then can't
/// place, nor we tell apart. So ours are a process each, ending with its
/// window.
const GHOSTTY: &str = "ghostty --gtk-single-instance=false --quit-after-last-window-closed=true \
     --working-directory=\"$SPACES_DIR\"";
const GHOSTTY_ENTRY: &str = "com.mitchellh.ghostty.desktop";

/// The command for Omarchy's terminal, running `program` in place of your
/// shell.
fn omarchy_terminal(ghostty: bool, program: Option<&str>) -> String {
    let launcher = app_launcher();
    let (terminal, run) = match ghostty {
        true => (GHOSTTY, " -e "),
        false => (XDG_TERMINAL, " "),
    };
    match program {
        Some(program) => format!("{launcher} {terminal}{run}{program}"),
        None => format!("{launcher} {terminal}"),
    }
}

/// Whether the terminal you chose in Omarchy is Ghostty: the first in the list
/// that `xdg-terminal-exec` picks from, read here as asking it would hold up
/// each terminal.
fn prefers_ghostty(config_dir: &Path) -> bool {
    ["hyprland-xdg-terminals.list", "xdg-terminals.list"]
        .iter()
        .filter_map(|name| fs::read_to_string(config_dir.join(name)).ok())
        .find_map(|list| {
            let first = list
                .lines()
                .map(str::trim)
                .find(|line| line.ends_with(".desktop") && !line.starts_with(['#', '-', '/']))?;
            Some(first.trim_start_matches('+') == GHOSTTY_ENTRY)
        })
        .unwrap_or(false)
}

/// Runs an app as Omarchy does, in a scope of its own. Outside a uwsm session,
/// without it.
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

/// Settings that aren't ours: misspelled, or an older or newer Spaces'. Each
/// table takes its own, to warn of them; the rest of the file still counts.
type Unknown = BTreeMap<String, IgnoredAny>;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    /// A dmenu-style command for `space go` with no name.
    pub picker: Option<String>,
    /// The terminals Spaces opens.
    pub terminal: Terminal,
    /// What comes back after a restart (`space restore`).
    pub restore: Restore,
    /// The sounds when an agent needs you (`sound.rs`).
    pub sounds: Sounds,
    #[serde(flatten)]
    unknown: Unknown,
}

/// `[terminal]`: what opens a terminal, and what runs in it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Terminal {
    /// How to open a terminal: a command for `sh -c`, run in the folder with
    /// `SPACES_DIR` set to it.
    pub command: Option<String>,
    /// Whether terminals Spaces opens run in herdr-core (`space shell`).
    /// When unset, uses herdr-core only when its `hc` is on `PATH`.
    pub herdr_core: Option<bool>,
    #[serde(flatten)]
    unknown: Unknown,
}

/// `[restore]`: whether the plugin restores on start, and what comes back.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Restore {
    /// Whether the plugin opens your last spaces again after a restart.
    pub enabled: bool,
    /// Whether a restart picks up the agents' conversations again.
    pub agents: bool,
    /// Whether a restart opens Chromium's windows again.
    pub browser: bool,
    #[serde(flatten)]
    unknown: Unknown,
}

impl Default for Restore {
    fn default() -> Self {
        Self {
            enabled: true,
            agents: true,
            browser: true,
            unknown: Unknown::new(),
        }
    }
}

/// `[sounds]`: whether to play them, and files in place of the defaults.
#[derive(Debug, Clone, PartialEq, Deserialize)]
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

impl Default for Sounds {
    fn default() -> Self {
        Self {
            enabled: true,
            done: None,
            waiting: None,
            unknown: Unknown::new(),
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
                let config: Self = toml::from_str(&text)
                    .with_context(|| format!("can't read {}", paths::tilde(path)))?;
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

    /// The settings that aren't ours, as `sounds.bell`.
    fn unknown(&self) -> Vec<String> {
        [
            ("", &self.unknown),
            ("terminal.", &self.terminal.unknown),
            ("restore.", &self.restore.unknown),
            ("sounds.", &self.sounds.unknown),
        ]
        .into_iter()
        .flat_map(|(table, unknown)| unknown.keys().map(move |key| format!("{table}{key}")))
        .collect()
    }

    /// Whether default terminals run in herdr-core (`space shell`).
    pub fn use_herdr_core(&self) -> bool {
        self.terminal.herdr_core.unwrap_or_else(has_herdr_core)
    }

    /// The terminal command for picking up a conversation, which `space shell`
    /// does. A terminal of your own has to run that itself.
    pub fn resuming_terminal(&self) -> String {
        self.terminal_running(true)
    }

    /// The configured terminal command, or Omarchy's way. When herdr-core is
    /// enabled (or `hc` is on `PATH`), that runs `space shell` (`shell.rs`).
    pub fn terminal(&self) -> String {
        self.terminal_running(self.use_herdr_core())
    }

    fn terminal_running(&self, space: bool) -> String {
        if let Some(command) = &self.terminal.command {
            return command.clone();
        }
        let ghostty = prefers_ghostty(&paths::config_dir()) && on_path("ghostty");
        let shell = space.then(space_shell);
        omarchy_terminal(ghostty, shell.as_deref())
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
        assert!(config.terminal().contains("$SPACES_DIR"));
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

        fs::write(&path, "[terminal]\nherdr_core = false\n").unwrap();
        let config = Config::load_from(&path).unwrap();
        assert!(!config.use_herdr_core());
        assert!(config.terminal().contains("$SPACES_DIR"));
        assert!(
            !config.terminal().ends_with("' shell"),
            "{}",
            config.terminal()
        );

        fs::write(&path, "[terminal]\nherdr_core = true\n").unwrap();
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
        assert!(config.terminal().contains("$SPACES_DIR"));
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
    }

    #[test]
    fn opens_ghostty_a_process_a_window() {
        let plain = omarchy_terminal(false, None);
        assert!(
            plain.ends_with(r#"exec setsid uwsm-app -- xdg-terminal-exec --dir="$SPACES_DIR""#)
                || plain == r#"exec xdg-terminal-exec --dir="$SPACES_DIR""#
        );
        let ghostty = omarchy_terminal(true, Some("'/b/space' shell"));
        assert!(
            ghostty.contains(" --gtk-single-instance=false "),
            "{ghostty}"
        );
        assert!(
            ghostty.ends_with(r#" --working-directory="$SPACES_DIR" -e '/b/space' shell"#),
            "{ghostty}"
        );

        let dir = tempfile::tempdir().unwrap();
        let list = |text: &str| {
            fs::write(dir.path().join("xdg-terminals.list"), text).unwrap();
            prefers_ghostty(dir.path())
        };
        assert!(!prefers_ghostty(&dir.path().join("nowhere")));
        assert!(list(
            "# Preferred\ncom.mitchellh.ghostty.desktop\nkitty.desktop\n"
        ));
        assert!(!list("kitty.desktop\ncom.mitchellh.ghostty.desktop\n"));
        assert!(!list("# com.mitchellh.ghostty.desktop\n"));
        assert!(list(
            "/execarg_default:x.desktop:-e\n-kitty.desktop\n+com.mitchellh.ghostty.desktop\n"
        ));
    }

    #[test]
    fn reads_the_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[terminal]\ncommand = \"exec foot\"\n").unwrap();
        assert_eq!(Config::load_from(&path).unwrap().terminal(), "exec foot");
    }

    #[test]
    fn unknown_settings_leave_the_others_be() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "roots = [\"~/dev\"]\n\
             [terminal]\nfont = 9\n\
             [restore]\nbrowser = false\napps = true\n\
             [sounds]\nenabled = false\nbell = { a = 1.5 }\n\
             [icons]\nsize = 2\n",
        )
        .unwrap();
        let config = Config::load_from(&path).unwrap();
        assert!(!config.restore.browser && config.restore.enabled);
        assert!(!config.sounds.enabled);
        assert_eq!(
            config.unknown(),
            [
                "icons",
                "roots",
                "terminal.font",
                "restore.apps",
                "sounds.bell"
            ]
        );

        // A value of the wrong kind is still a mistake.
        fs::write(&path, "[sounds]\nenabled = \"no\"\n").unwrap();
        assert!(Config::load_from(&path).is_err());
    }
}
