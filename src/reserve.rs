//! Workspaces whose terminal is on its way, kept for it in
//! `$XDG_RUNTIME_DIR/omarchy-spaces/reserved/`, a file each. Hyprland only has
//! a workspace once a window shows up there, so without this, a space or tab
//! opened before the last one's terminal is up would be given the same one.

use std::fs::{self, File};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::name::{self, Location};
use crate::state;

/// How long a workspace is kept when its terminal hasn't shown up: it still
/// can, after we stopped waiting.
const KEPT: Duration = Duration::from_secs(60);

/// Reserves the space and tab that `pick` chooses, given those reserved
/// already. One process picks at a time. `pick` has to ask Hyprland what's
/// open itself: what was released after the reserved ones were read is then
/// among it.
pub fn reserve(
    dir: &Path,
    pick: impl FnOnce(&[(String, u8)]) -> Result<(String, u8)>,
) -> Result<(String, u8)> {
    let _lock = state::lock(dir, ".lock")?;
    let (space, key) = pick(&reserved(dir)?)?;
    let workspace = name::workspace_name(&space, key);
    File::create(dir.join(&workspace)).with_context(|| format!("can't reserve {workspace}"))?;
    Ok((space, key))
}

/// What is reserved, clearing out what has been for too long.
pub fn reserved(dir: &Path) -> Result<Vec<(String, u8)>> {
    let mut reserved = Vec::new();
    for file in fs::read_dir(dir).with_context(|| format!("can't read {}", dir.display()))? {
        let file = file?;
        let workspace = file.file_name().to_string_lossy().into_owned();
        let Location::Space { name, key } = name::locate(0, &workspace) else {
            continue;
        };
        // One released meanwhile is gone.
        let Ok(made) = file.metadata().and_then(|meta| meta.modified()) else {
            continue;
        };
        if made.elapsed().is_ok_and(|age| age < KEPT) {
            reserved.push((name, key));
        } else {
            release(dir, &workspace);
        }
    }
    Ok(reserved)
}

/// Gives up a workspace, once Hyprland has it itself.
pub fn release(dir: &Path, workspace: &str) {
    // Failing to, it runs out by itself.
    let _ = fs::remove_file(dir.join(workspace));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_workspaces_for_a_while() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().join("reserved");
        let next = || {
            let (_, slot) = reserve(&dir, |reserved| {
                let free = (1..).find(|n| !reserved.contains(&("s1".into(), *n)));
                Ok(("s1".into(), free.unwrap()))
            })
            .unwrap();
            slot
        };
        assert_eq!(next(), 1);
        assert_eq!(next(), 2);

        release(&dir, "s1:1");
        assert_eq!(next(), 1);

        let long_ago = std::time::SystemTime::now() - KEPT;
        let file = File::options().write(true).open(dir.join("s1:2")).unwrap();
        file.set_modified(long_ago).unwrap();
        assert_eq!(next(), 2, "its terminal never came");
        assert_eq!(next(), 3);
    }
}
