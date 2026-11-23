//! Where renSpace keeps its files, and `~` in paths shown to the user.

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

/// `~/.config/renspace/config.toml`.
pub fn config_file() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("renspace/config.toml")
}

/// `~/.local/state/renspace/`: disposable cache, safe to delete at any time.
pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("renspace")
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
    fn shortens_home() {
        let home = home();
        assert_eq!(tilde(&home.join("Projects/ren")), "~/Projects/ren");
        assert_eq!(tilde(Path::new("/srv/x")), "/srv/x");
    }
}
