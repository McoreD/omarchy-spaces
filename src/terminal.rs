//! Terminals: the folder a terminal window's shell is in, which names its
//! space, and opening a new terminal in a folder.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::paths;

/// The folder the shell of the terminal window run by `pid` is in, or `None`
/// when the window isn't a terminal. As `omarchy-cmd-terminal-cwd` decides:
/// a terminal's shell is its newest child that leads its own session on a
/// tty and is a login shell (listed in `/etc/shells`). Other apps' children,
/// such as kitty's `kitten` helpers or a browser's, don't qualify.
///
/// For tests, `RENSPACE_FAKE_CWDS` names a folder of symlinks, one per pid,
/// pointing at that "terminal's" folder.
pub fn shell_cwd(pid: u32) -> Option<PathBuf> {
    if let Some(fake) = std::env::var_os("RENSPACE_FAKE_CWDS") {
        return std::fs::read_link(Path::new(&fake).join(pid.to_string())).ok();
    }
    let shells = std::fs::read_to_string("/etc/shells").unwrap_or_default();
    let mut children: Vec<u32> = std::fs::read_dir(format!("/proc/{pid}/task"))
        .ok()?
        .flatten()
        .filter_map(|task| std::fs::read_to_string(task.path().join("children")).ok())
        .flat_map(|list| {
            list.split_whitespace()
                .filter_map(|c| c.parse().ok())
                .collect::<Vec<u32>>()
        })
        .collect();
    children.sort_unstable();
    children
        .into_iter()
        .rev()
        .find(|&child| is_shell(child, &shells))
        .and_then(|child| std::fs::canonicalize(format!("/proc/{child}/cwd")).ok())
}

fn is_shell(pid: u32, shells: &str) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    // The command name, in parentheses, can hold spaces; the fields after
    // it are state, ppid, pgrp, session, tty_nr.
    let Some((_, rest)) = stat.rsplit_once(") ") else {
        return false;
    };
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let (Some(session), Some(tty)) = (fields.get(3), fields.get(4)) else {
        return false;
    };
    if session.parse::<u32>().ok() != Some(pid) || *tty == "0" {
        return false;
    }
    let Ok(exe) = std::fs::canonicalize(format!("/proc/{pid}/exe")) else {
        return false;
    };
    shells.lines().any(|line| {
        let line = line.trim();
        !line.starts_with('#')
            && !line.is_empty()
            && std::fs::canonicalize(line).is_ok_and(|shell| shell == exe)
    })
}

/// The checkout `dir` is in (a worktree's own folder), else `dir` itself, as
/// herdr names its spaces: `cd src` inside a repo keeps the repo's name.
pub fn checkout(dir: &Path) -> PathBuf {
    let home = paths::home();
    for candidate in dir.ancestors() {
        // Stop at home: a dotfiles repo there would name every space `~`.
        if candidate == home {
            break;
        }
        if candidate.join(".git").exists() {
            return candidate.to_path_buf();
        }
    }
    dir.to_path_buf()
}

/// A folder as a space's name: its last part, `~` for the home folder.
pub fn folder_name(dir: &Path) -> String {
    if dir == paths::home() {
        return "~".into();
    }
    dir.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string())
}

/// The terminal `command` as one shell command that runs it in `dir` with
/// `RENSPACE_DIR` set, for Hyprland to run (`launch` sets them itself).
pub fn command_in(command: &str, dir: &Path) -> String {
    let dir = shell_quote(&dir.to_string_lossy());
    format!("cd -- {dir} && export RENSPACE_DIR={dir} && {command}")
}

/// `value` in single quotes for `sh`.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Opens a terminal in `dir` with `command` (the `terminal` setting), run by
/// `sh -c` in that folder with `RENSPACE_DIR` set to it. It runs on its own,
/// outliving `space`.
pub fn launch(command: &str, dir: &Path) -> Result<()> {
    Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .env("RENSPACE_DIR", dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("can't open a terminal with `{command}`"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_folders_after_their_checkout() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("ren");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src/deep")).unwrap();
        assert_eq!(checkout(&repo.join("src/deep")), repo);
        // A worktree has a `.git` file of its own.
        let worktree = repo.join("worktrees/fix");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(worktree.join(".git"), "gitdir: ../../.git/worktrees/fix\n").unwrap();
        assert_eq!(checkout(&worktree), worktree);
        let plain = tmp.path().join("notes");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(checkout(&plain), plain);

        assert_eq!(folder_name(&repo), "ren");
        assert_eq!(folder_name(&paths::home()), "~");
        assert_eq!(folder_name(Path::new("/")), "/");
    }

    #[test]
    fn runs_the_terminal_command_in_a_folder() {
        assert_eq!(
            command_in("exec kitty", Path::new("/p/it's here")),
            r#"cd -- '/p/it'\''s here' && export RENSPACE_DIR='/p/it'\''s here' && exec kitty"#
        );
        let tmp = tempfile::tempdir().unwrap();
        let odd = tmp.path().join("a 'b' $c");
        std::fs::create_dir_all(&odd).unwrap();
        let out = Command::new("sh")
            .arg("-c")
            .arg(command_in("pwd; echo \"$RENSPACE_DIR\"", &odd))
            .output()
            .unwrap();
        let odd = odd.display().to_string();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            format!("{odd}\n{odd}\n")
        );
    }

    #[test]
    fn finds_no_shell_for_a_non_terminal() {
        // This test process's children aren't session-leading shells.
        assert_eq!(shell_cwd(std::process::id()), None);
        assert_eq!(shell_cwd(u32::MAX), None);
    }
}
