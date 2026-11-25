//! What agents note about themselves: the conversation they're in, to pick up
//! again after a restart, and what they're doing. Claude Code notes both for
//! each of its processes, in `~/.claude/sessions/<pid>.json`, so this needs no
//! hooks.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::paths;

/// A conversation to go on with. What runs is made from its id, which is
/// checked, so a saved session can't hold a command of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    /// Which agent: `claude`.
    pub agent: String,
    pub id: String,
    /// The folder it was started in, where it has to be picked up.
    pub folder: PathBuf,
}

impl Conversation {
    /// The command that picks it up, or `None` for one that can't be.
    pub fn command(&self) -> Option<String> {
        resume_command(&self.agent, &self.id)
    }
}

/// `claude <id>` as `SPACES_RESUME` hands it to `space shell`.
pub fn parse_resume(value: &str) -> Option<String> {
    let (agent, id) = value.split_once(' ')?;
    resume_command(agent, id)
}

fn resume_command(agent: &str, id: &str) -> Option<String> {
    (agent == "claude" && is_uuid(id)).then(|| format!("claude --resume {id}"))
}

fn is_uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// What an agent's process notes about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Noted {
    /// Which process, to tell it from another that gets its pid.
    pub process: String,
    pub conversation: Option<Conversation>,
    /// What it's doing: working, waiting or idle; empty when it doesn't say.
    pub word: &'static str,
}

/// What Claude Code notes about one of its processes.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeProcess {
    pid: u32,
    session_id: String,
    cwd: PathBuf,
    /// When the process started, as the kernel counts it.
    #[serde(default)]
    proc_start: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    status: Option<String>,
    /// What it's waiting for, when it is.
    #[serde(default)]
    waiting_for: Option<String>,
}

impl ClaudeProcess {
    /// Its status, in our words.
    fn word(&self) -> &'static str {
        match (self.status.as_deref(), self.waiting_for.as_deref()) {
            // You opened it yourself, as with `/config`.
            (Some("waiting"), Some("dialog open")) => "",
            (Some("waiting"), _) => "waiting",
            (Some("busy"), _) => "working",
            // `shell` is a turn that has ended with a command left running.
            (Some("idle" | "shell"), _) => "idle",
            _ => "",
        }
    }
}

/// What the process `pid` notes about itself, if it's an agent we can tell.
pub fn noted(pid: u32) -> Option<Noted> {
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| paths::home().join(".claude"));
    claude(&dir, pid, started(pid)?.as_str())
}

fn claude(dir: &Path, pid: u32, started: &str) -> Option<Noted> {
    let process = read_claude(&dir.join(format!("sessions/{pid}.json")))?;
    // Left behind by another process that had the same pid.
    if process.pid != pid
        || process
            .proc_start
            .as_deref()
            .is_some_and(|start| start != started)
    {
        return None;
    }
    if process
        .kind
        .as_deref()
        .is_some_and(|kind| kind != "interactive")
    {
        return None;
    }
    let word = process.word();
    Some(Noted {
        process: format!("{pid}/{started}"),
        conversation: is_uuid(&process.session_id).then(|| Conversation {
            agent: "claude".into(),
            id: process.session_id,
            folder: process.cwd,
        }),
        word,
    })
}

/// Claude writes it in place, so it can be caught half written: then again, a
/// moment later.
fn read_claude(path: &Path) -> Option<ClaudeProcess> {
    for _ in 0..3 {
        let text = std::fs::read_to_string(path).ok()?;
        if let Ok(process) = serde_json::from_str(&text) {
            return Some(process);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    None
}

/// When a process started, as `/proc/<pid>/stat` counts it.
fn started(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, rest) = stat.rsplit_once(") ")?;
    rest.split_whitespace().nth(19).map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "be6fb713-2beb-4233-86c2-266ee407ec56";

    #[test]
    fn makes_a_command_only_from_an_id() {
        assert_eq!(
            parse_resume(&format!("claude {ID}")),
            Some(format!("claude --resume {ID}"))
        );
        assert_eq!(parse_resume("claude x; rm -rf ~"), None);
        assert_eq!(parse_resume(&format!("sh {ID}")), None);
        assert_eq!(parse_resume(ID), None);
    }

    #[test]
    fn reads_what_claude_notes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sessions")).unwrap();
        let write = |text: String| {
            std::fs::write(dir.path().join("sessions/7.json"), text).unwrap();
        };
        let note = |kind: &str| {
            format!(
                r#"{{"pid":7,"sessionId":"{ID}","cwd":"/p","procStart":"42","kind":"{kind}","new":1}}"#
            )
        };
        write(note("interactive"));
        assert_eq!(
            claude(dir.path(), 7, "42"),
            Some(Noted {
                process: "7/42".into(),
                conversation: Some(Conversation {
                    agent: "claude".into(),
                    id: ID.into(),
                    folder: "/p".into(),
                }),
                word: "",
            })
        );
        assert_eq!(claude(dir.path(), 7, "43"), None, "another process's");
        assert_eq!(claude(dir.path(), 8, "42"), None);
        write(note("background"));
        assert_eq!(claude(dir.path(), 7, "42"), None);
        write("{".into());
        assert_eq!(claude(dir.path(), 7, "42"), None);
    }

    #[test]
    fn reads_what_claude_is_doing() {
        let word = |status: &str, waiting_for: &str| {
            let note = format!(
                r#"{{"pid":7,"sessionId":"{ID}","cwd":"/p","status":"{status}","waitingFor":"{waiting_for}"}}"#
            );
            serde_json::from_str::<ClaudeProcess>(&note).unwrap().word()
        };
        assert_eq!(word("busy", ""), "working");
        assert_eq!(word("waiting", "permission prompt"), "waiting");
        assert_eq!(word("waiting", "input needed"), "waiting");
        assert_eq!(word("waiting", "dialog open"), "", "you opened it");
        assert_eq!(word("idle", ""), "idle");
        assert_eq!(word("shell", ""), "idle");
        assert_eq!(word("dreaming", ""), "");
    }

    #[test]
    fn knows_when_a_process_started() {
        let now = started(std::process::id()).unwrap();
        assert!(now.parse::<u64>().unwrap() > 0);
        assert_eq!(started(u32::MAX), None);
    }
}
