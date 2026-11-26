//! Codex CLI. It writes each conversation to
//! `<CODEX_HOME>/sessions/…/rollout-<time>-<uuid>.jsonl`, held open while it's
//! going on: its first line says which conversation it is and the folder it's
//! in, and turns starting and ending are lines of their own, so this needs no
//! hooks. It doesn't write down that it's waiting for you, so it never shows
//! as waiting.
//!
//! Its terminal program needn't be the one holding the file: it can leave the
//! work to a server, `codex app-server`, which every Codex of an account
//! shares. Then what's in the folder the program is in is its conversation.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use serde::Deserialize;

use super::{Adapter, Conversation, Noted, Process, is_uuid};
use crate::paths;

pub const ADAPTER: Adapter = Adapter {
    name: "codex",
    noted,
    // One you leave isn't a failure; one that's gone is, and a new one takes
    // its place.
    resume: |id| is_uuid(id).then(|| format!("codex resume {id} || codex")),
};

fn noted(process: &Process) -> Option<Noted> {
    let program = program(process.pid)?;
    // Omarchy runs it with a folder of its own for each account beyond the
    // first, so ours doesn't tell.
    let home = process
        .var("CODEX_HOME")
        .map_or_else(|| paths::home().join(".codex"), PathBuf::from);
    let sessions = home.join("sessions");
    let open = [Some(program), server(&home)]
        .into_iter()
        .flatten()
        .flat_map(|pid| open_rollouts(pid, &sessions));
    let noted = std::fs::canonicalize(format!("/proc/{program}/cwd"))
        .ok()
        .and_then(|folder| pick(open, &folder));
    // Codex all the same, before there's anything in its conversation.
    Some(noted.unwrap_or(Noted {
        conversation: None,
        word: "",
    }))
}

/// Of the conversations open, the one in `folder`, read. When there are
/// several, it's the one written to last.
fn pick(open: impl IntoIterator<Item = PathBuf>, folder: &Path) -> Option<Noted> {
    open.into_iter()
        .filter_map(|path| {
            let (meta, word) = read(&path)?;
            let here = std::fs::canonicalize(&meta.cwd).unwrap_or_else(|_| meta.cwd.clone());
            (here == folder).then(|| (modified(&path), meta, word))
        })
        .max_by_key(|(modified, ..)| *modified)
        .map(|(_, meta, word)| Noted {
            conversation: Some(Conversation {
                agent: ADAPTER.name.into(),
                id: meta.id,
                folder: meta.cwd,
            }),
            word,
        })
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// The Codex program `pid` runs, if it's Codex's: itself, or, when it's the
/// `node` script npm installs it as, the program that script starts.
fn program(pid: u32) -> Option<u32> {
    if is_codex(pid) {
        return Some(pid);
    }
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let mut args = cmdline.split(|&b| b == 0);
    let node = args.next().and_then(file_name).is_some_and(|n| n == "node");
    let script = args.next().and_then(file_name);
    if !node || !script.is_some_and(|s| s == "codex" || s == "codex.js") {
        return None;
    }
    children(pid).into_iter().find(|&child| is_codex(child))
}

fn file_name(arg: &[u8]) -> Option<&str> {
    std::str::from_utf8(arg).ok()?.rsplit('/').next()
}

fn children(pid: u32) -> Vec<u32> {
    std::fs::read_dir(format!("/proc/{pid}/task"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|task| std::fs::read_to_string(task.path().join("children")).ok())
        .flat_map(|list| {
            list.split_whitespace()
                .filter_map(|child| child.parse().ok())
                .collect::<Vec<u32>>()
        })
        .collect()
}

/// Whether `pid` runs Codex's own program, and not as its server.
fn is_codex(pid: u32) -> bool {
    let named = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .is_ok_and(|comm| comm.trim_end() == "codex")
        || std::fs::canonicalize(format!("/proc/{pid}/exe"))
            .is_ok_and(|exe| exe.file_name().is_some_and(|name| name == "codex"));
    named
        && !std::fs::read(format!("/proc/{pid}/cmdline"))
            .unwrap_or_default()
            .split(|&b| b == 0)
            .any(|arg| arg == b"app-server")
}

/// The pid of the server the account in `home` runs, while it does.
fn server(home: &Path) -> Option<u32> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Pid {
        pid: u32,
        #[serde(default)]
        process_identity: Option<Identity>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Identity {
        start_ticks: u64,
    }
    let text = std::fs::read_to_string(home.join("app-server-daemon/daemon.pid")).ok()?;
    let noted: Pid = serde_json::from_str(&text).ok()?;
    let process = Process::read(noted.pid)?;
    // Not another process that has its pid since.
    let same = noted
        .process_identity
        .is_none_or(|id| process.started == id.start_ticks.to_string());
    same.then_some(noted.pid)
}

/// The conversations in `sessions` that `pid` has open.
fn open_rollouts(pid: u32, sessions: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .filter(|path| is_rollout(path, sessions))
        .collect()
}

fn is_rollout(path: &Path, sessions: &Path) -> bool {
    path.starts_with(sessions)
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
}

/// What a conversation's first line says.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Meta {
    id: String,
    cwd: PathBuf,
}

/// What's been read of a conversation so far.
#[derive(Default)]
struct Read {
    /// How far, to the end of its last whole line.
    upto: u64,
    /// `None` when it's not one to show: a subagent's, or `codex exec`'s.
    meta: Option<Meta>,
    word: &'static str,
}

fn cache() -> &'static Mutex<HashMap<PathBuf, Read>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Read>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A conversation you started, and what it's doing. It only grows, so each
/// time this reads just what has been added.
fn read(path: &Path) -> Option<(Meta, &'static str)> {
    let mut cache = cache().lock().ok()?;
    let len = std::fs::metadata(path).ok()?.len();
    if cache.len() >= 64 && !cache.contains_key(path) {
        cache.clear();
    }
    let read = cache.entry(path.to_path_buf()).or_default();
    if len < read.upto {
        *read = Read::default();
    }
    if len > read.upto {
        go_on(path, read);
    }
    Some((read.meta.clone()?, read.word))
}

fn go_on(path: &Path, read: &mut Read) {
    let Ok(mut file) = File::open(path) else {
        return;
    };
    if file.seek(SeekFrom::Start(read.upto)).is_err() {
        return;
    }
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            // Half written: it's read again once it's whole.
            Ok(n) if n > 0 && line.ends_with(b"\n") => {
                if read.upto == 0 {
                    read.meta = meta(&line);
                } else if let Some(word) = word(&line) {
                    read.word = word;
                }
                read.upto += n as u64;
            }
            _ => return,
        }
    }
}

/// A conversation's first line: which it is, and where.
fn meta(line: &[u8]) -> Option<Meta> {
    #[derive(Deserialize)]
    struct Line {
        r#type: String,
        payload: Payload,
    }
    #[derive(Deserialize)]
    struct Payload {
        id: String,
        cwd: PathBuf,
        /// `cli` or `vscode` for one you started; a subagent's is an object.
        source: serde_json::Value,
    }
    let line: Line = serde_json::from_slice(line).ok()?;
    let yours = matches!(line.payload.source.as_str(), Some("cli" | "vscode"));
    (line.r#type == "session_meta" && yours && is_uuid(&line.payload.id)).then_some(Meta {
        id: line.payload.id,
        cwd: line.payload.cwd,
    })
}

/// What a line says it's doing now, if it does.
fn word(line: &[u8]) -> Option<&'static str> {
    const TURNS: [&[u8]; 3] = [b"task_started", b"task_complete", b"turn_aborted"];
    // Most lines are what was said and done, long, and about something else.
    if !TURNS
        .iter()
        .any(|turn| line.windows(turn.len()).any(|w| w == *turn))
    {
        return None;
    }
    #[derive(Deserialize)]
    struct Line {
        r#type: String,
        payload: Payload,
    }
    #[derive(Deserialize)]
    struct Payload {
        r#type: String,
    }
    let line: Line = serde_json::from_slice(line).ok()?;
    if line.r#type != "event_msg" {
        return None;
    }
    match line.payload.r#type.as_str() {
        "task_started" => Some("working"),
        "task_complete" | "turn_aborted" => Some("idle"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const ID: &str = "01a21736-ab36-7fc2-8683-8febb465c9c5";

    fn first(id: &str, cwd: &Path, source: &str) -> String {
        format!(
            r#"{{"timestamp":"t","type":"session_meta","payload":{{"id":"{id}","cwd":"{}","originator":"codex-tui","source":{source},"base_instructions":{{"text":"task_started"}}}}}}"#,
            cwd.display()
        )
    }

    fn event(kind: &str) -> String {
        format!(r#"{{"timestamp":"t","type":"event_msg","payload":{{"type":"{kind}"}}}}"#)
    }

    fn append(path: &Path, text: &str) {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn makes_a_command_only_from_an_id() {
        assert_eq!(
            (ADAPTER.resume)(ID),
            Some(format!("codex resume {ID} || codex"))
        );
        assert_eq!((ADAPTER.resume)("x; rm -rf ~"), None);
    }

    #[test]
    fn reads_what_it_is_doing() {
        assert_eq!(word(event("task_started").as_bytes()), Some("working"));
        assert_eq!(word(event("task_complete").as_bytes()), Some("idle"));
        assert_eq!(word(event("turn_aborted").as_bytes()), Some("idle"));
        assert_eq!(word(event("token_count").as_bytes()), None);
        let said = r#"{"type":"response_item","payload":{"type":"message","text":"task_started"}}"#;
        assert_eq!(word(said.as_bytes()), None, "only said");
    }

    #[test]
    fn reads_only_conversations_you_started() {
        let cwd = Path::new("/p");
        let meta = |source: &str| super::meta(first(ID, cwd, source).as_bytes());
        let want = Some(Meta {
            id: ID.into(),
            cwd: cwd.into(),
        });
        assert_eq!(meta(r#""cli""#), want);
        assert_eq!(meta(r#""vscode""#), want);
        assert_eq!(meta(r#""exec""#), None);
        assert_eq!(meta(r#"{"subagent":{"other":"guardian"}}"#), None);
        assert_eq!(super::meta(first("x", cwd, r#""cli""#).as_bytes()), None);
        assert_eq!(super::meta(event("task_started").as_bytes()), None);
    }

    #[test]
    fn follows_a_conversation_as_it_grows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-a.jsonl");
        append(&path, &format!("{}\n", first(ID, dir.path(), r#""cli""#)));
        assert_eq!(read(&path).unwrap().1, "");
        append(
            &path,
            &format!("{}\n{{\"type\":\"event", event("task_started")),
        );
        assert_eq!(read(&path).unwrap().1, "working");
        // The half-written line is read once it's whole.
        append(&path, "_msg\",\"payload\":{\"type\":\"task_complete\"}}\n");
        assert_eq!(read(&path).unwrap().1, "idle");
        assert_eq!(read(&path).unwrap().0.id, ID);
    }

    #[test]
    fn picks_the_conversation_in_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (here, there) = (dir.path().join("here"), dir.path().join("there"));
        std::fs::create_dir(&here).unwrap();
        std::fs::create_dir(&there).unwrap();
        let other = "01a21737-f7fa-73c1-a1b4-d4ba7171394f";
        let a = dir.path().join("rollout-a.jsonl");
        let b = dir.path().join("rollout-b.jsonl");
        append(&a, &format!("{}\n", first(ID, &here, r#""cli""#)));
        append(&b, &format!("{}\n", first(other, &there, r#""cli""#)));
        append(&b, &format!("{}\n", event("task_started")));
        let open = || [a.clone(), b.clone()];
        let noted = pick(open(), &there).unwrap();
        assert_eq!(noted.word, "working");
        assert_eq!(noted.conversation.unwrap().id, other);
        assert_eq!(pick(open(), &here).unwrap().conversation.unwrap().id, ID);
        assert_eq!(pick(open(), dir.path()), None);
    }

    #[test]
    fn knows_its_files() {
        let sessions = Path::new("/h/.codex/sessions");
        assert!(is_rollout(
            &sessions.join(format!("2026/11/26/rollout-2026-11-26T09-36-28-{ID}.jsonl")),
            sessions
        ));
        assert!(!is_rollout(&sessions.join("2026/x.jsonl"), sessions));
        assert!(!is_rollout(Path::new("/tmp/rollout-a.jsonl"), sessions));
    }
}
