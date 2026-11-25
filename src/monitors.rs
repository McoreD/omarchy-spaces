//! Which block of ten tabs is each monitor's. Omarchy gives every monitor
//! workspaces of its own, the first 1-10, the next one 11-20, and keeps which
//! is whose in `~/.local/state/omarchy/monitor-workspaces`, for the bar to
//! read: `block<TAB>connector<TAB>identity`, a line per monitor ever seen, the
//! connector empty while it's unplugged. A space's tabs follow the same.

use std::path::Path;

use crate::hypr::Workspace;
use crate::name;
use crate::paths;
use crate::state;

/// The blocks of the monitors plugged in now, by connector.
#[derive(Debug, Default)]
pub struct Blocks(Vec<(String, u8)>);

impl Blocks {
    /// None known when Omarchy hasn't said.
    pub fn load() -> Self {
        Self::read(&paths::omarchy_monitors())
    }

    fn read(path: &Path) -> Self {
        let text = state::read_optional(path).unwrap_or_default();
        Self(
            text.lines()
                .filter_map(|line| {
                    let mut fields = line.split('\t');
                    let block = fields.next()?.parse().ok()?;
                    let connector = fields.next().filter(|c| !c.is_empty())?;
                    Some((connector.to_string(), block))
                })
                .collect(),
        )
    }

    /// The block of the monitor a workspace is on, else the one it's in
    /// itself.
    pub fn of(&self, workspace: &Workspace) -> u8 {
        let known = self.0.iter().find(|(name, _)| *name == workspace.monitor);
        match known {
            Some((_, block)) => *block,
            None => name::locate(workspace.id, &workspace.name)
                .key()
                .map_or(0, name::block_of),
        }
    }

    /// The monitor whose tab a key is, if it's plugged in.
    pub fn monitor(&self, key: u8) -> Option<&str> {
        let block = name::block_of(key);
        let known = self.0.iter().find(|(_, of)| *of == block);
        known.map(|(name, _)| name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(name: &str, monitor: &str) -> Workspace {
        Workspace {
            id: -1337,
            name: name.into(),
            windows: 0,
            monitor: monitor.into(),
            layout: String::new(),
        }
    }

    #[test]
    fn reads_omarchys_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("monitor-workspaces");
        std::fs::write(
            &path,
            "0\teDP-1\tBOE\n1\tDP-7\tStudioDisplay\n2\t\tFALLBACK\nbroken\n",
        )
        .unwrap();
        let blocks = Blocks::read(&path);
        assert_eq!(blocks.of(&workspace("s1:12", "DP-7")), 1);
        // A tab left behind by an unplugged monitor is on this one.
        assert_eq!(blocks.of(&workspace("s1:21", "eDP-1")), 0);
        assert_eq!(blocks.of(&workspace("s1:21", "HDMI-1")), 2);
        assert_eq!(blocks.monitor(12), Some("DP-7"));
        assert_eq!(blocks.monitor(21), None);
    }

    #[test]
    fn without_them_a_workspace_is_in_its_own() {
        let blocks = Blocks::read(Path::new("/nonexistent"));
        assert_eq!(blocks.of(&workspace("s1:12", "DP-7")), 1);
        assert_eq!(blocks.of(&workspace("chat", "DP-7")), 0);
    }
}
