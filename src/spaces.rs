//! Spaces: project folders holding a `.renspace.toml`.
//!
//! The folder is the source of truth. The index under the state directory
//! only remembers where spaces were found, so `space go` needn't scan the
//! roots on every switch, and spaces created outside the roots are still
//! known. Entries for folders that moved or lost their `.renspace.toml` are
//! pruned whenever the index is read.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::config::Config;
use crate::name;
use crate::paths;
use crate::state;

pub const SPACE_FILE: &str = ".renspace.toml";

/// Folders never worth descending into when scanning for spaces.
const SKIP: &[&str] = &["node_modules", "target", "vendor", "dist", "build"];

#[derive(Debug, Default, Deserialize)]
struct SpaceFile {
    /// Defaults to the folder name. Other keys (apps, browser profile,
    /// layout) are for later and ignored for now.
    name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Space {
    pub name: String,
    pub dir: PathBuf,
}

/// Reads the space defined in `dir`, or `None` when the folder has no
/// `.renspace.toml`.
pub fn read_space(dir: &Path) -> Result<Option<Space>> {
    let file = dir.join(SPACE_FILE);
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("can't read {}", paths::tilde(&file))),
    };
    let parsed: SpaceFile =
        toml::from_str(&text).with_context(|| format!("can't read {}", paths::tilde(&file)))?;
    let name = match parsed.name {
        Some(name) => name,
        None => default_name(dir),
    };
    name::validate(&name).with_context(|| format!("in {}", paths::tilde(&file)))?;
    if name == name::HOME {
        bail!(
            "in {}: 'home' is reserved for the plain workspaces",
            paths::tilde(&file)
        );
    }
    Ok(Some(Space {
        name,
        dir: dir.to_path_buf(),
    }))
}

/// The name a folder's space gets when none is given: its folder name, made
/// valid.
pub fn default_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| name::slugify(&n.to_string_lossy()))
        .unwrap_or_default()
}

/// Writes `.renspace.toml` in `dir`.
pub fn write_space(dir: &Path, space_name: &str) -> Result<Space> {
    let file = dir.join(SPACE_FILE);
    let contents = format!("name = {}\n", toml::Value::String(space_name.to_string()));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file)
        .and_then(|mut f| std::io::Write::write_all(&mut f, contents.as_bytes()))
        .with_context(|| format!("can't write {}", paths::tilde(&file)))?;
    Ok(Space {
        name: space_name.to_string(),
        dir: dir.to_path_buf(),
    })
}

/// Every folder at most `depth` levels below `root` (the root included) that
/// holds a `.renspace.toml`. Hidden folders, build output and symlinks are
/// skipped.
pub fn scan(root: &Path, depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((dir, level)) = stack.pop() {
        if dir.join(SPACE_FILE).is_file() {
            found.push(dir.clone());
        }
        if level == depth {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if !kind.is_dir() {
                continue;
            }
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if file_name.starts_with('.') || SKIP.contains(&file_name.as_ref()) {
                continue;
            }
            stack.push((entry.path(), level + 1));
        }
    }
    found.sort();
    found
}

/// The spaces renSpace knows about.
#[derive(Debug, Default)]
pub struct Registry {
    /// Sorted by name.
    pub spaces: Vec<Space>,
    /// Problems worth telling the user about, such as two folders claiming the
    /// same name or a `.renspace.toml` that doesn't parse.
    pub warnings: Vec<String>,
}

impl Registry {
    /// Reads the index, scanning the configured roots too when `rescan` is
    /// set, and writes back the index without stale entries.
    pub fn load(config: &Config, rescan: bool) -> Result<Self> {
        let roots = if rescan {
            config.root_dirs()
        } else {
            Vec::new()
        };
        Self::load_with(&index_file(), &roots, config.scan_depth)
    }

    pub fn load_with(index: &Path, roots: &[PathBuf], depth: usize) -> Result<Self> {
        let before = state::read_optional(index)?;
        let mut dirs: BTreeSet<PathBuf> = before
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(PathBuf::from)
            .collect();
        for root in roots {
            for dir in scan(root, depth) {
                dirs.insert(fs::canonicalize(&dir).unwrap_or(dir));
            }
        }

        let mut registry = Registry::default();
        let mut kept = Vec::new();
        for dir in dirs {
            match read_space(&dir) {
                Ok(Some(space)) => {
                    kept.push(dir);
                    registry.spaces.push(space);
                }
                // Moved or deleted: prune it.
                Ok(None) => {}
                // Still there but broken: keep it, so fixing the file is enough.
                Err(e) => {
                    kept.push(dir);
                    registry.warnings.push(format!("{e:#}"));
                }
            }
        }

        registry
            .spaces
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.dir.cmp(&b.dir)));
        let mut unique: Vec<Space> = Vec::with_capacity(registry.spaces.len());
        for space in registry.spaces.drain(..) {
            match unique.last() {
                Some(first) if first.name == space.name => registry.warnings.push(format!(
                    "both {} and {} define the space '{}'; using the first. Give one a different `name` in its {SPACE_FILE}.",
                    paths::tilde(&first.dir),
                    paths::tilde(&space.dir),
                    space.name
                )),
                _ => unique.push(space),
            }
        }
        registry.spaces = unique;

        let after: String = kept.iter().map(|d| format!("{}\n", d.display())).collect();
        if after != before {
            state::write_atomic(index, &after)?;
        }
        Ok(registry)
    }

    pub fn get(&self, space: &str) -> Option<&Space> {
        self.spaces.iter().find(|s| s.name == space)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.spaces.iter().map(|s| s.name.as_str())
    }

    /// Adds a space created outside the roots to the index.
    pub fn remember(dir: &Path) -> Result<()> {
        let index = index_file();
        let mut text = state::read_optional(&index)?;
        let line = dir.display().to_string();
        if !text.lines().any(|l| l == line) {
            text.push_str(&line);
            text.push('\n');
            state::write_atomic(&index, &text)?;
        }
        Ok(())
    }
}

fn index_file() -> PathBuf {
    paths::state_dir().join("index")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_space(dir: &Path, toml: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(SPACE_FILE), toml).unwrap();
    }

    #[test]
    fn reads_names_from_file_or_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let named = tmp.path().join("Whatever");
        make_space(&named, "name = \"ren\"\n");
        assert_eq!(read_space(&named).unwrap().unwrap().name, "ren");

        let unnamed = tmp.path().join("My Trip");
        make_space(&unnamed, "# no name\n");
        assert_eq!(read_space(&unnamed).unwrap().unwrap().name, "my-trip");

        let extra = tmp.path().join("extra");
        make_space(&extra, "name = \"extra\"\n[apps]\nbrowser = true\n");
        assert_eq!(read_space(&extra).unwrap().unwrap().name, "extra");

        assert!(read_space(&tmp.path().join("nothing")).unwrap().is_none());
    }

    #[test]
    fn rejects_bad_names() {
        let tmp = tempfile::tempdir().unwrap();
        let bad = tmp.path().join("bad");
        make_space(&bad, "name = \"Ren:1\"\n");
        assert!(read_space(&bad).is_err());
        let home = tmp.path().join("home");
        make_space(&home, "");
        assert!(read_space(&home).is_err());
    }

    #[test]
    fn writes_space_file_once() {
        let tmp = tempfile::tempdir().unwrap();
        let space = write_space(tmp.path(), "ren").unwrap();
        assert_eq!(space.name, "ren");
        assert_eq!(
            fs::read_to_string(tmp.path().join(SPACE_FILE)).unwrap(),
            "name = \"ren\"\n"
        );
        assert!(write_space(tmp.path(), "ren").is_err());
    }

    #[test]
    fn scans_to_depth_skipping_noise() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        make_space(&root.join("a"), "");
        make_space(&root.join("a/nested/deeper"), "");
        make_space(&root.join("b/c/d/too-deep"), "");
        make_space(&root.join(".hidden/x"), "");
        make_space(&root.join("node_modules/pkg"), "");
        std::os::unix::fs::symlink(root.join("a"), root.join("link")).unwrap();

        let found = scan(root, 3);
        assert_eq!(found, [root.join("a"), root.join("a/nested/deeper")]);
        assert_eq!(scan(root, 1), [root.join("a")]);
    }

    #[test]
    fn registry_merges_prunes_and_warns() {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let projects = root.join("projects");
        make_space(&projects.join("ren"), "");
        make_space(&projects.join("other"), "name = \"ren\"\n");
        make_space(&projects.join("broken"), "name = 3\n");
        let outside = root.join("elsewhere/tax");
        make_space(&outside, "");
        let gone = root.join("gone");

        let index = root.join("state/index");
        state::write_atomic(
            &index,
            &format!("{}\n{}\n", outside.display(), gone.display()),
        )
        .unwrap();

        let registry = Registry::load_with(&index, std::slice::from_ref(&projects), 3).unwrap();
        let names: Vec<_> = registry.names().collect();
        assert_eq!(names, ["ren", "tax"]);
        // Sorted by path within a name, so `other` comes before `ren`.
        assert_eq!(registry.get("ren").unwrap().dir, projects.join("other"));
        assert_eq!(registry.warnings.len(), 2, "{:?}", registry.warnings);

        let index_text = fs::read_to_string(&index).unwrap();
        assert!(!index_text.contains("gone"), "stale entry pruned");
        assert!(index_text.contains("broken"), "broken entry kept");
        assert!(index_text.contains("elsewhere/tax"), "outside entry kept");

        // Without a rescan the index alone is enough.
        let registry = Registry::load_with(&index, &[], 3).unwrap();
        assert_eq!(registry.names().collect::<Vec<_>>(), ["ren", "tax"]);
    }
}
