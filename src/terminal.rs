//! Terminals: the folder a terminal window's shell is in, which names its
//! space, what it runs, and opening a new terminal in a folder.
//!
//! A window's shell is herdr-core's when the window shows one of its
//! terminals (`core.rs`), and is found in `/proc` otherwise.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::{core, paths};

/// What the shell of a terminal window is doing: the folder it's in, and
/// the program it's running, unless it's at its prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    pub cwd: PathBuf,
    /// The command running in the terminal's foreground (`claude`, `nvim`),
    /// or `None` at the prompt.
    pub program: Option<String>,
    /// The herdr-core terminal it is, which knows what its agent says it's
    /// doing. `None` for a shell outside core.
    pub agent: Option<Agent>,
}

/// What a core terminal's program last reported through OSC 7501.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    /// The terminal's address in core: `t7`.
    pub address: String,
    /// The program's word: idle, working, done, blocked, error; empty when
    /// it reports nothing.
    pub word: String,
    /// Goes up each time it stops working, so a `done` seen can be told
    /// from the next.
    pub turn: u64,
    /// The last turn someone looked at, from its `herdr.seen` label; `None`
    /// when no client has said.
    pub seen_turn: Option<u64>,
}

impl Agent {
    /// Whether someone has looked at it since its turn ended, as its label
    /// says.
    pub fn seen(&self) -> bool {
        self.seen_turn.is_some_and(|seen| seen >= self.turn)
    }
}

impl Shell {
    /// What a tab shows for this terminal: what it's running, or else the
    /// folder it's in.
    pub fn label(&self) -> String {
        self.program
            .clone()
            .unwrap_or_else(|| folder_name(&self.cwd))
    }
}

/// The folder the shell of the terminal window run by `pid` is in, or `None`
/// when the window isn't a terminal.
pub fn shell_cwd(pid: u32) -> Option<PathBuf> {
    let core = core::list().ok().map(core::Terminals::new);
    shell_of(pid, core.as_ref()).map(|shell| shell.cwd)
}

/// The shell of the terminal window run by `pid`: core's terminal in it,
/// when `core` has one, else the window's own (`shell`).
pub fn shell_of(pid: u32, core: Option<&core::Terminals>) -> Option<Shell> {
    if let Some(terminal) = core.and_then(|core| core.in_window(pid).first().copied()) {
        return Some(from_core(terminal));
    }
    shell(pid)
}

/// A core terminal's shell: its foreground program's folder and name, a
/// shell counting as its prompt.
fn from_core(terminal: &core::Terminal) -> Shell {
    let shells = std::fs::read_to_string("/etc/shells").unwrap_or_default();
    let (cwd, program) = match &terminal.foreground {
        Some(fg) => {
            let shell = shells
                .lines()
                .filter_map(|line| line.trim().rsplit('/').next())
                .any(|name| !name.is_empty() && name == fg.name);
            let program = display_name(&fg.name, fg.argv.iter().map(String::as_str));
            (fg.cwd.clone(), program.filter(|_| !shell))
        }
        None => (terminal.cwd.clone(), None),
    };
    Shell {
        cwd,
        program,
        agent: Some(Agent {
            address: terminal.address.clone(),
            word: terminal
                .status
                .as_ref()
                .map(|s| s.state.clone())
                .unwrap_or_default(),
            turn: terminal.turn,
            seen_turn: terminal.seen_turn(),
        }),
    }
}

/// The window's own shell, run by the terminal app `pid`, or `None` when
/// the window isn't a terminal. As `omarchy-cmd-terminal-cwd` decides: a terminal's
/// shell is its newest child that leads its own session on a tty and is a
/// login shell (listed in `/etc/shells`). Other apps' children, such as
/// kitty's `kitten` helpers or a browser's, don't qualify.
///
/// For tests, `RENSPACE_FAKE_CWDS` names a folder of symlinks, one per pid,
/// pointing at that "terminal's" folder, and of `<pid>.program` files
/// holding what it runs.
pub fn shell(pid: u32) -> Option<Shell> {
    if let Some(fake) = std::env::var_os("RENSPACE_FAKE_CWDS") {
        let fake = Path::new(&fake);
        let cwd = std::fs::read_link(fake.join(pid.to_string())).ok()?;
        let program = std::fs::read_to_string(fake.join(format!("{pid}.program")))
            .ok()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        return Some(Shell {
            cwd,
            program,
            agent: None,
        });
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
    let (shell, stat) = children.into_iter().rev().find_map(|child| {
        let stat = Stat::read(child)?;
        is_shell(child, &stat, &shells).then_some((child, stat))
    })?;
    let cwd = std::fs::canonicalize(format!("/proc/{shell}/cwd")).ok()?;
    Some(Shell {
        cwd,
        program: foreground(&stat, &shells),
        agent: None,
    })
}

/// The fields of `/proc/<pid>/stat` that say how a process sits on its tty.
struct Stat {
    pgrp: i64,
    session: i64,
    tty: i64,
    /// The process group in the foreground of its tty.
    tpgid: i64,
}

impl Stat {
    fn read(pid: u32) -> Option<Self> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // The command name, in parentheses, can hold spaces; the fields after
        // it are state, ppid, pgrp, session, tty_nr, tpgid.
        let (_, rest) = stat.rsplit_once(") ")?;
        let fields: Vec<i64> = rest
            .split_whitespace()
            .skip(1)
            .take(5)
            .map(|f| f.parse().ok())
            .collect::<Option<_>>()?;
        let &[_ppid, pgrp, session, tty, tpgid] = fields.as_slice() else {
            return None;
        };
        Some(Self {
            pgrp,
            session,
            tty,
            tpgid,
        })
    }
}

fn is_shell(pid: u32, stat: &Stat, shells: &str) -> bool {
    stat.session == i64::from(pid) && stat.tty != 0 && is_shell_binary(pid, shells)
}

/// Whether `pid` runs one of the shells `/etc/shells` lists.
fn is_shell_binary(pid: u32, shells: &str) -> bool {
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

/// What runs in the foreground of the shell's tty, when it isn't the shell
/// at its prompt: the leader of the foreground process group, which is the
/// command you ran, or the first of a pipeline. Another shell started from
/// this one (`bash`, `nix develop`) counts as a prompt too.
fn foreground(shell: &Stat, shells: &str) -> Option<String> {
    if shell.tpgid <= 0 || shell.tpgid == shell.pgrp {
        return None;
    }
    let leader = u32::try_from(shell.tpgid).ok()?;
    if is_shell_binary(leader, shells) {
        return None;
    }
    program_name(leader)
}

/// What to call the program `pid` runs: its command name, as `ps` shows it,
/// read in full from its arguments when the kernel cut it short, and up to
/// any `: ` (tmux calls its client `tmux: client`).
fn program_name(pid: u32) -> Option<String> {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let args: Vec<String> = cmdline
        .split(|&b| b == 0)
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect();
    display_name(comm.trim_end_matches('\n'), args.iter().map(String::as_str))
}

/// What to call a program from its command name (`comm`) and arguments.
fn display_name<'a>(comm: &str, args: impl Iterator<Item = &'a str>) -> Option<String> {
    // The kernel keeps 15 bytes of it.
    let full = if comm.len() >= 15 {
        args.map(|arg| arg.rsplit('/').next().unwrap_or_default())
            .find(|base| base.starts_with(comm))
            .map(String::from)
    } else {
        None
    };
    let name = full.as_deref().unwrap_or(comm);
    let name = name
        .split_once(": ")
        .map_or(name, |(before, _)| before)
        .trim();
    (!name.is_empty()).then(|| name.to_string())
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
    fn seen_once_someone_looked_at_its_last_turn() {
        let agent = |turn, seen_turn| Agent {
            address: "t1".into(),
            word: "done".into(),
            turn,
            seen_turn,
        };
        assert!(agent(3, Some(3)).seen());
        assert!(agent(3, Some(4)).seen());
        assert!(!agent(3, Some(2)).seen(), "a turn ended since");
        assert!(!agent(3, None).seen(), "no client has said");
    }

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
        assert_eq!(shell(std::process::id()), None);
        assert_eq!(shell(u32::MAX), None);
    }

    #[test]
    fn names_programs_by_their_command() {
        assert_eq!(
            program_name(std::process::id())
                .as_deref()
                .map(str::is_empty),
            Some(false)
        );
        let child = Command::new("sleep").arg("5").spawn().unwrap();
        assert_eq!(program_name(child.id()).as_deref(), Some("sleep"));
        let mut child = child;
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(program_name(u32::MAX), None);
    }

    #[test]
    fn labels_a_terminal_by_its_program_else_its_folder() {
        let at = |cwd: &Path, program: Option<&str>| Shell {
            cwd: cwd.to_path_buf(),
            program: program.map(String::from),
            agent: None,
        };
        assert_eq!(at(Path::new("/p/ren"), Some("claude")).label(), "claude");
        assert_eq!(at(Path::new("/p/ren/src"), None).label(), "src");
        assert_eq!(at(&paths::home(), None).label(), "~");
    }
}
