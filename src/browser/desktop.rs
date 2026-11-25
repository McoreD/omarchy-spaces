//! Desktop entries: what starts an app, as the system has it.

use std::path::{Path, PathBuf};

use crate::paths;

/// The program the app's desktop entry runs, as `omarchy-launch-browser` reads
/// it: yours before the system's.
pub fn program(name: &str) -> Option<String> {
    data_dirs().iter().find_map(|dir| program_in(dir, name))
}

fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| paths::home().join(".local/share"), PathBuf::from);
    let system = std::env::var_os("XDG_DATA_DIRS")
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    std::iter::once(home)
        .chain(std::env::split_paths(&system))
        .collect()
}

fn program_in(dir: &Path, name: &str) -> Option<String> {
    let entry = dir.join(format!("applications/{name}.desktop"));
    let text = std::fs::read_to_string(entry).ok()?;
    // The first is the app's own, before its actions'.
    let exec = text.lines().find_map(|line| line.strip_prefix("Exec="))?;
    let program = exec.split_whitespace().next()?;
    Some(program.trim_matches('"').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_an_entry_runs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("applications")).unwrap();
        std::fs::write(
            dir.path().join("applications/web.desktop"),
            "[Desktop Entry]\nTryExec=no\nExec=/opt/web --flag %U\n\
             [Desktop Action new]\nExec=/opt/other\n",
        )
        .unwrap();
        assert_eq!(program_in(dir.path(), "web"), Some("/opt/web".into()));
        assert_eq!(program_in(dir.path(), "none"), None);
    }
}
