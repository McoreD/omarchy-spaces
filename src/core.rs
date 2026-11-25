//! herdr-core (`hc`), the terminal runtime Spaces' terminals run in when
//! it's installed. Its terminals say what their programs are doing: agents
//! report it through OSC 7501, which core keeps as each terminal's status,
//! and core reads what runs in the foreground and its folder. Spaces asks
//! core over its socket, in JSON-RPC, one message a line.
//!
//! A terminal window runs `space shell` (`shell.rs`), which asks core for a
//! shell and attaches to it. Core labels the shell with the window's process,
//! so a window finds its terminal by its pid.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{OnceLock, mpsc};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

/// The label every Spaces terminal carries, `omarchy-spaces=1`, to ask core for
/// only those.
pub const LABEL: &str = "omarchy-spaces";
/// The label naming the window's process, the terminal app's, as `window_label`
/// makes it.
pub const WINDOW_LABEL: &str = "omarchy-spaces-window";
/// The last turn someone looked at, which Herdr's clients share (its
/// `docs/labels.md`), so a `done` seen stays seen when `watch` restarts, and
/// in Herdr too.
pub const SEEN_LABEL: &str = "herdr.seen";

/// One of core's terminals, as much of it as Spaces reads.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Terminal {
    pub address: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// The folder it started in.
    #[serde(default)]
    pub cwd: PathBuf,
    /// What runs in its foreground now, if anything.
    #[serde(default)]
    pub foreground: Option<Foreground>,
    /// What its program last reported through OSC 7501, if anything.
    #[serde(default)]
    pub status: Option<Report>,
    /// Goes up each time its program stops working.
    #[serde(default)]
    pub turn: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Foreground {
    #[serde(default)]
    pub pid: Option<u32>,
    /// Its name as the OS has it: `zsh`, `claude`, `nvim`.
    pub name: String,
    #[serde(default)]
    pub argv: Vec<String>,
    /// Its folder now.
    pub cwd: PathBuf,
}

/// An OSC 7501 report, in the program's own words.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Report {
    /// idle, working, done, blocked or error.
    pub state: String,
}

impl Terminal {
    /// The machine it runs on, as core calls it: `box` for `box/t7`. `None`
    /// for this one.
    pub fn machine(&self) -> Option<&str> {
        Some(self.address.split_once('/')?.0)
    }

    /// Whether it shows in the window run by `pid` here.
    fn shows_in(&self, pid: u32) -> bool {
        self.labels.get(WINDOW_LABEL) == Some(&window_label(pid))
    }

    /// The last turn someone looked at, from any client; `None` when none
    /// has said.
    pub fn seen_turn(&self) -> Option<u64> {
        self.labels.get(SEEN_LABEL)?.parse().ok()
    }
}

/// The window run by `pid`, as its terminals are labelled. A pid alone says
/// too little: core also lists the terminals of other machines added to it,
/// which may run Spaces too, and a terminal there can outlive this boot.
fn window_label(pid: u32) -> String {
    static BOOT: OnceLock<String> = OnceLock::new();
    let boot = BOOT.get_or_init(|| {
        std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .unwrap_or_default()
            .trim()
            .to_string()
    });
    format!("{boot}/{pid}")
}

/// Core's socket: `HC_SOCKET`, else `default.sock` in its runtime folder.
pub fn socket() -> PathBuf {
    if let Some(socket) = std::env::var_os("HC_SOCKET").filter(|s| !s.is_empty()) {
        return PathBuf::from(socket);
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", uid())));
    runtime.join("herdr-core/default.sock")
}

fn uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").map_or(0, |m| m.uid())
}

fn connect() -> Result<UnixStream> {
    let path = socket();
    let stream = UnixStream::connect(&path)
        .with_context(|| format!("herdr-core isn't running at {}", path.display()))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    Ok(stream)
}

/// Sends a request on `stream` and reads lines until its answer.
fn request(stream: &mut UnixStream, method: &str, params: Value) -> Result<Value> {
    let message = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    writeln!(stream, "{message}")?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            bail!("herdr-core closed the connection");
        }
        let reply: Value = serde_json::from_str(&line)
            .with_context(|| format!("unexpected reply from herdr-core: {}", line.trim()))?;
        if reply.get("id") != Some(&json!(1)) {
            continue;
        }
        if let Some(error) = reply.get("error") {
            let message = error.get("message").and_then(Value::as_str).unwrap_or("");
            bail!("herdr-core: {message}");
        }
        return Ok(reply.get("result").cloned().unwrap_or(Value::Null));
    }
}

/// Calls a method on its own connection.
pub fn call(method: &str, params: Value) -> Result<Value> {
    let mut stream = connect()?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    request(&mut stream, method, params)
}

fn space_labels() -> Value {
    json!({ LABEL: "1" })
}

/// Spaces' live terminals, as a subscription starts with them: from memory.
/// `terminal.list` asks each machine added to core, waiting seconds for one
/// that doesn't answer and then leaving its terminals out.
pub fn list() -> Result<Vec<Terminal>> {
    let result = call("events.subscribe", json!({ "labels": space_labels() }))?;
    Ok(serde_json::from_value(result["terminals"].clone())?)
}

/// Starts a shell, or `argv` instead, for the window run by `window`, in
/// `cwd`, with `env` on top of core's own, `cols` by `rows` until a viewer resizes it. It goes
/// once it exits. Returns its address.
pub fn run_shell(
    argv: &[String],
    cwd: &std::path::Path,
    env: &BTreeMap<String, String>,
    window: u32,
    size: Option<(u16, u16)>,
) -> Result<String> {
    let mut params = json!({
        "cwd": cwd,
        "env": env,
        "labels": { LABEL: "1", WINDOW_LABEL: window_label(window), SEEN_LABEL: "0" },
        "no_parent": true,
        "rm": true,
    });
    if !argv.is_empty() {
        params["argv"] = json!(argv);
    }
    if let Some((cols, rows)) = size {
        params["size"] = json!({ "cols": cols, "rows": rows });
    }
    let result = call("terminal.run", params)?;
    result["address"]
        .as_str()
        .map(String::from)
        .context("herdr-core gave the new terminal no address")
}

/// Notes that you've looked at `address` on its turn `turn`, for every
/// client to read.
pub fn mark_seen(address: &str, turn: u64) -> Result<()> {
    call(
        "terminal.label",
        json!({ "terminal": address, "labels": { SEEN_LABEL: turn.to_string() } }),
    )?;
    Ok(())
}

/// What a subscription sends.
#[derive(Debug)]
pub enum Update {
    /// The terminals as they are now, replacing any before.
    Snapshot(Vec<Terminal>),
    /// A change to apply (`Terminals::apply`).
    Event(Value),
    /// The connection ended.
    Gone,
}

/// Follows Spaces' terminals on its own connection, sending `wrap` of
/// each update to `send`, until the connection or the receiver ends.
pub fn subscribe<T: Send + 'static>(send: mpsc::Sender<T>, wrap: fn(Update) -> T) -> Result<()> {
    let mut stream = connect()?;
    let result = request(
        &mut stream,
        "events.subscribe",
        json!({ "labels": space_labels() }),
    )?;
    let terminals: Vec<Terminal> = serde_json::from_value(result["terminals"].clone())?;
    send.send(wrap(Update::Snapshot(terminals)))
        .map_err(|_| anyhow::anyhow!("nobody follows herdr-core's terminals"))?;
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let update = match message.get("method").and_then(Value::as_str) {
                Some("event") => Update::Event(message["params"].clone()),
                Some("missed") => {
                    match serde_json::from_value(message["params"]["terminals"].clone()) {
                        Ok(terminals) => Update::Snapshot(terminals),
                        Err(_) => continue,
                    }
                }
                _ => continue,
            };
            if send.send(wrap(update)).is_err() {
                return;
            }
        }
        let _ = send.send(wrap(Update::Gone));
    });
    Ok(())
}

/// Spaces' terminals, kept current from a subscription's updates.
#[derive(Debug, Default)]
pub struct Terminals {
    by_address: HashMap<String, Terminal>,
}

impl Terminals {
    pub fn new(terminals: Vec<Terminal>) -> Self {
        Self {
            by_address: terminals
                .into_iter()
                .map(|t| (t.address.clone(), t))
                .collect(),
        }
    }

    /// The terminals showing in the window run by `pid`, oldest first: one,
    /// unless the app draws several in one window (tabs or splits).
    pub fn in_window(&self, pid: u32) -> Vec<&Terminal> {
        let mut found: Vec<&Terminal> = self
            .by_address
            .values()
            .filter(|t| t.shows_in(pid))
            .collect();
        found.sort_by(|a, b| a.address.cmp(&b.address));
        found
    }

    pub fn get(&self, address: &str) -> Option<&Terminal> {
        self.by_address.get(address)
    }

    /// Applies an event, as core's API docs say: returns whether anything
    /// Spaces shows changed.
    pub fn apply(&mut self, event: &Value) -> bool {
        let Some(address) = event.get("terminal").and_then(Value::as_str) else {
            return false;
        };
        let data = &event["data"];
        let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
        let record = || serde_json::from_value::<Terminal>(data.clone()).ok();
        match kind {
            "created" | "restarted" => match record() {
                Some(t) => {
                    self.by_address.insert(address.to_string(), t);
                    true
                }
                None => false,
            },
            "finished" | "removed" => self.by_address.remove(address).is_some(),
            "labels" => {
                if let Some(record) = data
                    .get("record")
                    .and_then(|r| serde_json::from_value::<Terminal>(r.clone()).ok())
                {
                    self.by_address.insert(address.to_string(), record);
                    return true;
                }
                let Some(t) = self.by_address.get_mut(address) else {
                    return false;
                };
                t.labels = serde_json::from_value(data["labels"].clone()).unwrap_or_default();
                if t.labels.get(LABEL).map(String::as_str) != Some("1") {
                    self.by_address.remove(address);
                }
                true
            }
            _ => {
                let Some(t) = self.by_address.get_mut(address) else {
                    return false;
                };
                match kind {
                    "status" => {
                        t.status = serde_json::from_value(data["status"].clone())
                            .ok()
                            .flatten();
                    }
                    "turn" => t.turn = data["turn"].as_u64().unwrap_or(t.turn),
                    "foreground" => {
                        t.foreground = serde_json::from_value(data.clone()).ok().flatten();
                    }
                    "lost" => t.foreground = None,
                    _ => return false,
                }
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal(address: &str, window: u32) -> Terminal {
        Terminal {
            address: address.into(),
            labels: [
                (LABEL.to_string(), "1".to_string()),
                (WINDOW_LABEL.to_string(), window_label(window)),
            ]
            .into(),
            cwd: "/p/ren".into(),
            ..Terminal::default()
        }
    }

    #[test]
    fn finds_a_windows_terminals_by_its_process() {
        let terminals = Terminals::new(vec![
            terminal("t2", 50),
            terminal("t1", 50),
            terminal("t3", 60),
        ]);
        let found: Vec<&str> = terminals
            .in_window(50)
            .iter()
            .map(|t| t.address.as_str())
            .collect();
        assert_eq!(found, ["t1", "t2"]);
        assert!(terminals.in_window(70).is_empty());
    }

    #[test]
    fn leaves_another_machines_windows_alone() {
        let mut theirs = terminal("box/t1", 50);
        theirs
            .labels
            .insert(WINDOW_LABEL.into(), "another-boot/50".into());
        let mut older = terminal("t2", 50);
        older.labels.insert(WINDOW_LABEL.into(), "50".into());
        let terminals = Terminals::new(vec![theirs, older, terminal("box/t3", 50)]);
        let found: Vec<&str> = terminals
            .in_window(50)
            .iter()
            .map(|t| t.address.as_str())
            .collect();
        assert_eq!(found, ["box/t3"], "one running there, in a window here");
    }

    #[test]
    fn applies_events_as_core_documents_them() {
        let mut terminals = Terminals::new(vec![terminal("t1", 50)]);
        let event = |kind: &str, data: Value| json!({ "seq": 1, "type": kind, "terminal": "t1", "data": data });

        assert!(terminals.apply(&event(
            "foreground",
            json!({ "pid": 9, "name": "claude", "argv": ["claude"], "cwd": "/p/ren/src" })
        )));
        assert_eq!(
            terminals
                .get("t1")
                .unwrap()
                .foreground
                .as_ref()
                .unwrap()
                .name,
            "claude"
        );
        assert!(terminals.apply(&event("status", json!({ "status": { "state": "working", "app": "claude-code" }, "records": {}, "reported": true }))));
        assert_eq!(
            terminals.get("t1").unwrap().status.as_ref().unwrap().state,
            "working"
        );
        assert!(terminals.apply(&event("turn", json!({ "turn": 3, "state": "done" }))));
        assert_eq!(terminals.get("t1").unwrap().turn, 3);
        assert!(terminals.apply(&event(
            "status",
            json!({ "status": null, "records": {}, "reported": true })
        )));
        assert_eq!(terminals.get("t1").unwrap().status, None);
        assert!(!terminals.apply(&event("title", json!({ "title": "✳ Claude Code" }))));
        assert!(terminals.apply(&event("foreground", Value::Null)));
        assert_eq!(terminals.get("t1").unwrap().foreground, None);

        // Labelled away from Spaces: out of view.
        assert!(terminals.apply(&event("labels", json!({ "labels": {} }))));
        assert!(terminals.get("t1").is_none());
        let created = serde_json::to_value(json!({ "address": "t1", "labels": { LABEL: "1", WINDOW_LABEL: window_label(50) }, "cwd": "/p", "turn": 0 })).unwrap();
        assert!(terminals.apply(&event("created", created)));
        assert_eq!(terminals.in_window(50).len(), 1);
        assert!(terminals.apply(&event("finished", json!({ "reason": "exited" }))));
        assert!(terminals.get("t1").is_none());
        assert!(!terminals.apply(&json!({ "type": "status" })));
    }
}
