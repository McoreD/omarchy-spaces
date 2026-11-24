//! Where Spaces keeps its files, and `~` in paths shown to the user.

use std::env;
use std::path::{Path, PathBuf};

pub fn home() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match env::var_os(var) {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home().join(fallback),
    }
}

/// `~/.config/omarchy-spaces/config.toml`.
pub fn config_file() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("omarchy-spaces/config.toml")
}

/// `~/.local/state/omarchy-spaces/`: safe to delete at any time, losing only names,
/// icons and the session to restore.
pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("omarchy-spaces")
}

/// `$XDG_RUNTIME_DIR/omarchy-spaces/`: per-session files (built-in sounds,
/// live agent statuses), gone with the session.
pub fn runtime_dir() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("omarchy-spaces")
}

/// `$XDG_RUNTIME_DIR/omarchy-spaces/status/`: one file per window that
/// reported an agent status through `space status`.
pub fn status_dir() -> PathBuf {
    runtime_dir().join("status")
}

/// A path as written in the config, `~/` meaning the home folder.
pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if path == "~" => home(),
        None => PathBuf::from(path),
    }
}

/// Shortens a path under the home directory to `~/...` for display.
pub fn tilde(path: &Path) -> String {
    let home = home();
    if home != Path::new("/")
        && let Ok(rest) = path.strip_prefix(&home)
    {
        if rest.as_os_str().is_empty() {
            return "~".into();
        }
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_home_in_config_paths() {
        assert_eq!(expand("~/a.oga"), home().join("a.oga"));
        assert_eq!(expand("~"), home());
        assert_eq!(expand("/s/a.oga"), PathBuf::from("/s/a.oga"));
        assert_eq!(expand("~x/a"), PathBuf::from("~x/a"));
    }

    #[test]
    fn shortens_home() {
        let home = home();
        assert_eq!(tilde(&home.join("Projects/ren")), "~/Projects/ren");
        assert_eq!(tilde(Path::new("/srv/x")), "/srv/x");
    }
}
