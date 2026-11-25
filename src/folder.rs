//! Folders, which may be on another machine: a terminal window is always
//! here, but what runs in it needn't be (`core.rs`). Only a folder on this
//! machine is a path to look at or open something in (`Folder::local`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, Serializer};

use crate::paths;

/// A folder, on this machine or another. Wherever it's saved or printed,
/// its path is `folder`, with `machine` beside it when it's on another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    #[serde(rename = "folder")]
    path: PathBuf,
    /// What herdr-core here calls the machine. `None` for this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    machine: Option<String>,
}

impl Folder {
    /// A folder on this machine.
    pub fn here(path: impl Into<PathBuf>) -> Self {
        Self::on(None, path)
    }

    /// A folder on `machine`, or on this one.
    pub fn on(machine: Option<&str>, path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            machine: machine.map(String::from),
        }
    }

    /// Its path, when it's on this machine.
    pub fn local(&self) -> Option<&Path> {
        self.machine.is_none().then_some(self.path.as_path())
    }

    /// The checkout it's in (a worktree's own folder), else itself, as
    /// herdr names its spaces: `cd src` inside a repo keeps the repo's name.
    /// On another machine, itself: there's no looking for a checkout there.
    pub fn checkout(&self) -> Self {
        let Some(dir) = self.local() else {
            return self.clone();
        };
        let home = paths::home();
        // Stop at home: a dotfiles repo there would name every space `~`.
        dir.ancestors()
            .take_while(|candidate| *candidate != home)
            .find(|candidate| candidate.join(".git").exists())
            .map_or_else(|| self.clone(), Self::here)
    }

    /// What a space in it is called: its last part, `~` for the home folder.
    pub fn name(&self) -> String {
        if self.local() == Some(&paths::home()) {
            return "~".into();
        }
        self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    /// The whole of it, to show: `~/p/ren`, or `box:/srv/ren`.
    pub fn display(&self) -> String {
        match &self.machine {
            Some(machine) => format!("{machine}:{}", self.path.display()),
            None => paths::tilde(&self.path),
        }
    }
}

/// Prints a folder there may not be, its `folder` then being `null`.
pub fn or_null<S: Serializer>(folder: &Option<Folder>, to: S) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct Null {
        folder: Option<()>,
    }
    match folder {
        Some(folder) => folder.serialize(to),
        None => Null { folder: None }.serialize(to),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn is_called_after_its_checkout() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("ren");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src/deep")).unwrap();
        let checkout = |dir: &Path| Folder::here(dir).checkout();
        assert_eq!(checkout(&repo.join("src/deep")), Folder::here(&repo));
        // A worktree has a `.git` file of its own.
        let worktree = repo.join("worktrees/fix");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(worktree.join(".git"), "gitdir: ../../.git/worktrees/fix\n").unwrap();
        assert_eq!(checkout(&worktree), Folder::here(&worktree));
        let plain = tmp.path().join("notes");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(checkout(&plain), Folder::here(&plain));

        assert_eq!(Folder::here(&repo).name(), "ren");
        assert_eq!(Folder::here(paths::home()).name(), "~");
        assert_eq!(Folder::here("/").name(), "/");
    }

    #[test]
    fn on_another_machine_isnt_looked_for_here() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let there = Folder::on(Some("box"), tmp.path().join("src"));
        assert_eq!(there.local(), None);
        assert_eq!(there.checkout(), there);
        assert_eq!(there.name(), "src");
        assert_eq!(Folder::on(Some("box"), "/srv/x").display(), "box:/srv/x");

        // The home folder here is only a folder there.
        let home = paths::home();
        assert_ne!(Folder::on(Some("box"), home).name(), "~");
    }

    #[test]
    fn is_saved_with_its_machine_beside_it() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Saved {
            #[serde(flatten)]
            folder: Option<Folder>,
            more: u8,
        }
        for (folder, saved) in [
            (None, json!({ "more": 1 })),
            (
                Some(Folder::here("/p")),
                json!({ "folder": "/p", "more": 1 }),
            ),
            (
                Some(Folder::on(Some("box"), "/p")),
                json!({ "folder": "/p", "machine": "box", "more": 1 }),
            ),
        ] {
            let value = Saved { folder, more: 1 };
            assert_eq!(serde_json::to_value(&value).unwrap(), saved);
            assert_eq!(serde_json::from_value::<Saved>(saved).unwrap(), value);
        }

        #[derive(Serialize)]
        struct Printed {
            #[serde(flatten, serialize_with = "or_null")]
            folder: Option<Folder>,
        }
        let print = |folder| serde_json::to_value(Printed { folder }).unwrap();
        assert_eq!(print(None), json!({ "folder": null }));
        assert_eq!(print(Some(Folder::here("/p"))), json!({ "folder": "/p" }));
    }
}
