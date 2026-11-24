//! Runs the `space` binary against a fake Hyprland that speaks the same
//! socket protocol, with its own home, config and state directories.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{Value, json};

#[derive(Default)]
struct Hyprland {
    /// Name of the focused workspace.
    active: String,
    /// Workspaces as (id, name).
    workspaces: Vec<(i64, String)>,
    /// Windows, by the name of the workspace each is on.
    windows: Vec<String>,
    /// Each window's address, alongside `windows`.
    addresses: Vec<u32>,
    next_address: u32,
    /// Whether Omarchy's `o.workspaces` Lua module is loaded.
    omarchy: bool,
    /// Whether `hl.dsp.exec_cmd` takes rules, as Hyprland 0.56 does.
    exec_rules: bool,
    /// The fake home, which commands it runs see as `HOME`.
    home: PathBuf,
    /// Whoever listens to Hyprland's events, as `space watch` does.
    listeners: Arc<Mutex<Vec<UnixStream>>>,
    /// Every dispatch received.
    dispatched: Vec<String>,
    next_named_id: i64,
}

impl Hyprland {
    fn ensure(&mut self, name: &str) -> i64 {
        if let Some((id, _)) = self.workspaces.iter().find(|(_, n)| n == name) {
            return *id;
        }
        let id = match name.parse::<i64>() {
            Ok(id) => id,
            Err(_) => {
                self.next_named_id -= 1;
                self.next_named_id
            }
        };
        self.workspaces.push((id, name.to_string()));
        id
    }

    fn workspace_json(&mut self, name: &str) -> Value {
        let id = self.ensure(name);
        let windows = self.windows.iter().filter(|w| *w == name).count();
        json!({ "id": id, "name": name, "monitor": "eDP-1", "windows": windows })
    }

    /// Hyprland destroys empty workspaces that aren't focused.
    fn tidy(&mut self) {
        let (active, windows) = (self.active.clone(), self.windows.clone());
        self.workspaces
            .retain(|(_, n)| *n == active || windows.contains(n));
    }

    fn handle(&mut self, request: &str) -> String {
        match request {
            "j/activeworkspace" => {
                let active = self.active.clone();
                self.workspace_json(&active).to_string()
            }
            "j/workspaces" => {
                let names: Vec<String> = self.workspaces.iter().map(|(_, n)| n.clone()).collect();
                Value::Array(names.iter().map(|n| self.workspace_json(n)).collect()).to_string()
            }
            "j/clients" => {
                let windows = self.windows.clone();
                let addresses = self.addresses.clone();
                Value::Array(
                    windows
                        .iter()
                        .zip(addresses)
                        .map(|(ws, address)| {
                            let id = self.ensure(ws);
                            json!({ "address": format!("0x{address:x}"), "pid": 1000 + address, "workspace": { "id": id, "name": ws } })
                        })
                        .collect(),
                )
                .to_string()
            }
            // The newest window on the focused workspace has focus.
            "j/activewindow" => match self.windows.iter().rposition(|w| *w == self.active) {
                Some(i) => {
                    let address = self.addresses[i];
                    json!({ "address": format!("0x{address:x}"), "pid": 1000 + address })
                        .to_string()
                }
                None => "{}".into(),
            },
            _ => match request.strip_prefix("dispatch ") {
                Some(expression) => {
                    self.dispatched.push(expression.to_string());
                    let reply = self.dispatch(expression);
                    self.tidy();
                    reply
                }
                None => "unknown request".into(),
            },
        }
    }

    fn dispatch(&mut self, expression: &str) -> String {
        let quoted = |e: &str| {
            let start = e.find("workspace = \"")? + "workspace = \"".len();
            let end = start + e[start..].find('"')?;
            Some(e[start..end].to_string())
        };
        let selector = |s: String| s.strip_prefix("name:").map(str::to_string).unwrap_or(s);
        let number = |e: &str| {
            let start = e.find('(')? + 1;
            e[start..]
                .split([',', ')'])
                .next()?
                .trim()
                .parse::<u8>()
                .ok()
        };

        let field = |e: &str, key: &str| {
            let start = e.find(&format!("{key} = \""))? + key.len() + 4;
            let end = start + e[start..].find('"')?;
            Some(e[start..end].to_string())
        };

        if expression.starts_with("hl.dsp.exec_cmd(") {
            if !self.exec_rules {
                return "error: exec_cmd: bad argument 2".into();
            }
            // Run it, as Hyprland does, and open its window where the rule
            // says, without going there.
            let start = "hl.dsp.exec_cmd(\"".len();
            let end = expression.find("\", { workspace").unwrap();
            let command = expression[start..end]
                .replace("\\\"", "\"")
                .replace("\\\\", "\\");
            let workspace = field(expression, "workspace").unwrap();
            let workspace = selector(workspace.trim_end_matches(" silent").to_string());
            std::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .env("HOME", &self.home)
                .status()
                .unwrap();
            self.ensure(&workspace);
            self.windows.push(workspace.clone());
            self.next_address += 1;
            let address = self.next_address;
            self.addresses.push(address);
            // Hyprland says so once the window maps, a moment later, as a
            // real terminal takes to start: by then whoever asked to be
            // told has been heard.
            let listeners = Arc::clone(&self.listeners);
            thread::spawn(move || {
                thread::sleep(std::time::Duration::from_millis(50));
                for stream in listeners.lock().unwrap().iter_mut() {
                    let _ = writeln!(stream, "openwindow>>{address:x},{workspace},kitty,zsh");
                }
            });
            return "ok".into();
        }
        if expression.starts_with("o.workspaces.") && !self.omarchy {
            return "error: attempt to index a nil value (global 'o')".into();
        }
        if expression.starts_with("hl.dsp.focus(") {
            let target = selector(quoted(expression).unwrap());
            self.ensure(&target);
            self.active = target;
        } else if expression.starts_with("hl.dsp.window.close(") {
            let address = field(expression, "window").unwrap();
            let address = address.strip_prefix("address:0x").unwrap();
            let address = u32::from_str_radix(address, 16).unwrap();
            let Some(index) = self.addresses.iter().position(|a| *a == address) else {
                return "error: no such window".into();
            };
            self.windows.remove(index);
            self.addresses.remove(index);
        } else if expression.starts_with("hl.dsp.window.set_prop(") {
            let address = field(expression, "window").unwrap();
            let address = address.strip_prefix("address:0x").unwrap();
            let address = u32::from_str_radix(address, 16).unwrap();
            if !self.addresses.contains(&address) {
                return "error: no such window".into();
            }
        } else if expression.starts_with("o.workspaces.focus(") {
            let target = number(expression).unwrap().to_string();
            self.ensure(&target);
            self.active = target;
        } else if expression.starts_with("hl.dsp.window.move(")
            || expression.starts_with("o.workspaces.move_window(")
        {
            let (target, follow) = if expression.starts_with("hl.") {
                (
                    selector(quoted(expression).unwrap()),
                    expression.contains("follow = true"),
                )
            } else {
                (
                    number(expression).unwrap().to_string(),
                    !expression.contains("false"),
                )
            };
            let Some(index) = self.windows.iter().position(|w| *w == self.active) else {
                return "error: no window".into();
            };
            self.ensure(&target);
            self.windows[index] = target.clone();
            if follow {
                self.active = target;
            }
        } else {
            return format!("error: unknown dispatch {expression}");
        }
        "ok".into()
    }
}

/// A fake herdr-core: Spaces' terminals, over the same JSON-RPC.
#[derive(Default)]
struct CoreState {
    terminals: Vec<Value>,
    subscribers: Vec<UnixStream>,
    requests: Vec<Value>,
    next: u32,
}

impl CoreState {
    fn broadcast(&mut self, terminal: &str, kind: &str, data: Value) {
        let line = json!({ "jsonrpc": "2.0", "method": "event", "params": { "seq": 1, "type": kind, "terminal": terminal, "data": data } });
        self.subscribers
            .retain_mut(|s| writeln!(s, "{line}").and_then(|()| s.flush()).is_ok());
    }

    fn handle(&mut self, request: &Value, stream: &UnixStream) -> Value {
        self.requests.push(request.clone());
        let params = &request["params"];
        let result = match request["method"].as_str().unwrap_or("") {
            "terminal.list" => {
                json!({ "terminals": self.terminals, "hidden_finished": 0, "machines": [] })
            }
            "events.subscribe" => {
                self.subscribers.push(stream.try_clone().unwrap());
                json!({ "seq": 0, "terminals": self.terminals })
            }
            "terminal.run" => {
                self.next += 1;
                let record = json!({ "address": format!("t{}", self.next), "labels": params["labels"], "cwd": params["cwd"], "turn": 0, "state": "live" });
                self.terminals.push(record.clone());
                record
            }
            "terminal.kill" => {
                let address = params["terminal"].as_str().unwrap_or("").to_string();
                self.terminals.retain(|t| t["address"] != address.as_str());
                self.broadcast(&address, "finished", json!({ "reason": "killed" }));
                json!({})
            }
            _ => {
                return json!({ "jsonrpc": "2.0", "id": request["id"], "error": { "code": -32601, "message": "no such method" } });
            }
        };
        json!({ "jsonrpc": "2.0", "id": request["id"], "result": result })
    }
}

struct Core {
    state: Arc<Mutex<CoreState>>,
    socket: PathBuf,
}

impl Core {
    fn start(env: &Env) -> Self {
        let socket = env.root.path().join("core.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let state = Arc::new(Mutex::new(CoreState::default()));
        let served = Arc::clone(&state);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let served = Arc::clone(&served);
                thread::spawn(move || {
                    use std::io::BufRead;
                    let reader = std::io::BufReader::new(stream.try_clone().unwrap());
                    let mut writer = stream.try_clone().unwrap();
                    for line in reader.lines() {
                        let Ok(line) = line else { break };
                        let Ok(request) = serde_json::from_str::<Value>(&line) else {
                            break;
                        };
                        let reply = served.lock().unwrap().handle(&request, &stream);
                        if writeln!(writer, "{reply}").is_err() {
                            break;
                        }
                    }
                });
            }
        });
        Self { state, socket }
    }

    /// A terminal showing in the window at `address`, whose process is
    /// `1000 + address` as the fake Hyprland has it, at its prompt in `dir`.
    fn add(&self, window: &str, dir: &Path) -> String {
        let pid = 1000 + u32::from_str_radix(window.trim_start_matches("0x"), 16).unwrap();
        let mut state = self.state.lock().unwrap();
        state.next += 1;
        let address = format!("t{}", state.next);
        let record = json!({
            "address": address,
            "labels": { "omarchy-spaces": "1", "omarchy-spaces-window": pid.to_string() },
            "cwd": dir.display().to_string(),
            "foreground": { "pid": 1, "name": "zsh", "argv": ["-zsh"], "cwd": dir.display().to_string() },
            "turn": 0,
            "state": "live",
        });
        state.terminals.push(record.clone());
        state.broadcast(&address, "created", record);
        address
    }

    /// Something changes in a terminal, as core's events say.
    fn event(&self, terminal: &str, kind: &str, data: Value) {
        let mut state = self.state.lock().unwrap();
        if let Some(record) = state
            .terminals
            .iter_mut()
            .find(|t| t["address"] == terminal)
        {
            match kind {
                "status" => record["status"] = data["status"].clone(),
                "foreground" => record["foreground"] = data.clone(),
                "turn" => record["turn"] = data["turn"].clone(),
                _ => {}
            }
        }
        state.broadcast(terminal, kind, data);
    }

    fn requests(&self, method: &str) -> Vec<Value> {
        let state = self.state.lock().unwrap();
        state
            .requests
            .iter()
            .filter(|r| r["method"] == method)
            .cloned()
            .collect()
    }

    /// Core goes away.
    fn stop(&self) {
        let _ = fs::remove_file(&self.socket);
        for stream in self.state.lock().unwrap().subscribers.drain(..) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

struct Env {
    root: tempfile::TempDir,
    hypr: Arc<Mutex<Hyprland>>,
    /// Whoever listens to Hyprland's events, as `space watch` does.
    listeners: Arc<Mutex<Vec<UnixStream>>>,
}

impl Env {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let socket_dir = root.path().join("run/hypr/test");
        fs::create_dir_all(&socket_dir).unwrap();
        fs::create_dir_all(root.path().join("home/Projects")).unwrap();
        let listener = UnixListener::bind(socket_dir.join(".socket.sock")).unwrap();

        let listeners = Arc::new(Mutex::new(Vec::new()));
        let hypr = Arc::new(Mutex::new(Hyprland {
            active: "1".into(),
            omarchy: true,
            exec_rules: true,
            home: root.path().join("home"),
            listeners: Arc::clone(&listeners),
            ..Default::default()
        }));
        hypr.lock().unwrap().ensure("1");
        let served = Arc::clone(&hypr);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                // Like Hyprland, read one request without waiting for EOF.
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let reply = served.lock().unwrap().handle(&request);
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        let events = UnixListener::bind(socket_dir.join(".socket2.sock")).unwrap();
        let joined = Arc::clone(&listeners);
        thread::spawn(move || {
            for stream in events.incoming() {
                let Ok(stream) = stream else { break };
                joined.lock().unwrap().push(stream);
            }
        });
        fs::create_dir_all(root.path().join("cwds")).unwrap();
        let env = Self {
            root,
            hypr,
            listeners,
        };
        env.write_config("");
        env
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn state(&self) -> PathBuf {
        self.home().join(".local/state/omarchy-spaces")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_space"));
        command
            .args(args)
            .current_dir(self.home())
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap())
            .env("HOME", self.home())
            .env("XDG_RUNTIME_DIR", self.root.path().join("run"))
            .env("HYPRLAND_INSTANCE_SIGNATURE", "test")
            .env("SPACES_FAKE_CWDS", self.cwds())
            .env("HC_SOCKET", self.root.path().join("core.sock"));
        command
    }

    /// Sends a Hyprland event (`openwindow>>...`) to whoever listens.
    fn event(&self, line: &str) {
        for stream in self.listeners.lock().unwrap().iter_mut() {
            let _ = writeln!(stream, "{line}");
        }
    }

    /// Hyprland goes away: its event socket closes.
    fn end_events(&self) {
        for stream in self.listeners.lock().unwrap().drain(..) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }

    /// Where each fake terminal's folder is: a symlink per pid.
    fn cwds(&self) -> PathBuf {
        self.root.path().join("cwds")
    }

    /// Opens a terminal window whose shell is in `dir`; returns its address.
    fn add_terminal(&self, workspace: &str, dir: &Path) -> String {
        let address = self.add_window(workspace);
        self.cd(&address, dir);
        address
    }

    /// The terminal at `address` changes folder.
    fn cd(&self, address: &str, dir: &Path) {
        let pid = 1000 + u32::from_str_radix(address.trim_start_matches("0x"), 16).unwrap();
        let link = self.cwds().join(pid.to_string());
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(dir, link).unwrap();
    }

    /// The terminal at `address` starts running `program`, or goes back to
    /// its prompt with `None`.
    fn run_program(&self, address: &str, program: Option<&str>) {
        let pid = 1000 + u32::from_str_radix(address.trim_start_matches("0x"), 16).unwrap();
        let file = self.cwds().join(format!("{pid}.program"));
        match program {
            Some(program) => fs::write(file, program).unwrap(),
            None => {
                let _ = fs::remove_file(file);
            }
        }
    }

    /// The address of the newest window, such as the terminal `space new`
    /// opened.
    fn newest_window(&self) -> String {
        format!(
            "0x{:x}",
            self.hypr.lock().unwrap().addresses.last().unwrap()
        )
    }

    fn close_window(&self, address: &str) {
        let mut hypr = self.hypr.lock().unwrap();
        let address = u32::from_str_radix(address.trim_start_matches("0x"), 16).unwrap();
        let i = hypr.addresses.iter().position(|a| *a == address).unwrap();
        hypr.windows.remove(i);
        hypr.addresses.remove(i);
        hypr.tidy();
    }

    /// The folders the first `count` terminals were opened in, waiting a
    /// moment for them: the terminal command runs on its own.
    fn launched(&self, count: usize) -> Vec<String> {
        let file = self.home().join("launched");
        let mut lines = Vec::new();
        for _ in 0..100 {
            let text = fs::read_to_string(&file).unwrap_or_default();
            lines = text.lines().map(str::to_string).collect();
            if lines.len() >= count && text.ends_with('\n') {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(20));
        }
        lines
    }

    /// A folder under the fake home, made if needed.
    fn dir(&self, path: &str) -> PathBuf {
        let dir = self.home().join(path);
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(dir).unwrap()
    }

    fn ls_json(&self) -> Vec<Value> {
        match serde_json::from_str(&self.ok(&["ls", "--json"])).unwrap() {
            Value::Array(rows) => rows,
            other => panic!("{other}"),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    #[track_caller]
    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "space {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    #[track_caller]
    fn fails(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(!out.status.success(), "space {args:?} should have failed");
        String::from_utf8(out.stderr).unwrap()
    }

    fn active(&self) -> String {
        self.hypr.lock().unwrap().active.clone()
    }

    fn set_active(&self, name: &str) {
        let mut hypr = self.hypr.lock().unwrap();
        hypr.ensure(name);
        hypr.active = name.into();
    }

    fn add_window(&self, workspace: &str) -> String {
        let mut hypr = self.hypr.lock().unwrap();
        hypr.ensure(workspace);
        hypr.windows.push(workspace.into());
        hypr.next_address += 1;
        let address = hypr.next_address;
        hypr.addresses.push(address);
        format!("0x{address:x}")
    }

    fn windows(&self) -> Vec<String> {
        self.hypr.lock().unwrap().windows.clone()
    }

    fn dispatched(&self) -> Vec<String> {
        std::mem::take(&mut self.hypr.lock().unwrap().dispatched)
    }

    /// Writes the config, with a "terminal" that notes the folder it was
    /// opened in.
    fn write_config(&self, text: &str) {
        let dir = self.home().join(".config/omarchy-spaces");
        fs::create_dir_all(&dir).unwrap();
        let terminal = "terminal = 'pwd >> \"$HOME/launched\"'\n";
        fs::write(dir.join("config.toml"), format!("{terminal}{text}")).unwrap();
    }
}

#[test]
fn ws_in_home_is_omarchys_workspace() {
    let env = Env::new();
    env.ok(&["ws", "3"]);
    assert_eq!(env.dispatched(), ["o.workspaces.focus(3)"]);
    assert_eq!(env.active(), "3");

    // Already there: nothing to do.
    env.ok(&["ws", "3"]);
    assert!(env.dispatched().is_empty());
}

#[test]
fn ws_in_home_without_omarchy_is_the_plain_workspace() {
    let env = Env::new();
    env.hypr.lock().unwrap().omarchy = false;
    env.ok(&["ws", "4"]);
    assert_eq!(
        env.dispatched(),
        [
            "o.workspaces.focus(4)",
            r#"hl.dsp.focus({ workspace = "4" })"#
        ]
    );
    assert_eq!(env.active(), "4");
}

#[test]
fn ws_stays_inside_the_space() {
    let env = Env::new();
    env.set_active("ren:1");
    env.ok(&["ws", "2"]);
    assert_eq!(
        env.dispatched(),
        [r#"hl.dsp.focus({ workspace = "name:ren:2" })"#]
    );
    assert_eq!(env.active(), "ren:2");
    env.ok(&["ws", "10"]);
    assert_eq!(env.active(), "ren:10");

    let last = fs::read_to_string(env.state().join("last")).unwrap();
    assert!(last.starts_with("ren\t10\t"), "{last}");
}

#[test]
fn ws_rejects_bad_numbers() {
    let env = Env::new();
    for bad in ["0", "11", "x"] {
        let err = env.fails(&["ws", bad]);
        assert!(err.contains("isn't a workspace"), "{err}");
    }
}

#[test]
fn move_takes_the_window_within_the_space() {
    let env = Env::new();
    env.set_active("ren:1");
    env.add_window("ren:1");
    env.add_window("ren:1");

    env.ok(&["move", "2"]);
    assert_eq!(
        env.dispatched(),
        [r#"hl.dsp.window.move({ workspace = "name:ren:2", follow = true })"#]
    );
    assert_eq!(env.active(), "ren:2");

    env.set_active("ren:1");
    env.ok(&["move", "3", "--silent"]);
    assert_eq!(
        env.dispatched(),
        [r#"hl.dsp.window.move({ workspace = "name:ren:3", follow = false })"#]
    );
    assert_eq!(env.active(), "ren:1");
    assert_eq!(env.windows(), ["ren:2", "ren:3"]);
}

#[test]
fn move_in_home_uses_omarchy_and_ignores_empty_workspaces() {
    let env = Env::new();
    env.ok(&["move", "2"]);
    assert!(env.dispatched().is_empty(), "no window to move");

    env.add_window("1");
    env.ok(&["move", "2", "--silent"]);
    assert_eq!(env.dispatched(), ["o.workspaces.move_window(2, false)"]);
    assert_eq!(env.windows(), ["2"]);
    assert_eq!(env.active(), "1");
}

#[test]
fn send_takes_the_window_to_another_space_where_you_left_it() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_terminal("s2:1", &env.dir("mail"));
    env.add_window("s2:3");
    env.ok(&["go", "mail"]);
    env.ok(&["ws", "3"]);
    env.ok(&["go", "ren"]);
    env.add_window("s1:1");
    env.dispatched();

    env.ok(&["send", "mail"]);
    assert_eq!(
        env.dispatched(),
        [r#"hl.dsp.window.move({ workspace = "name:s2:3", follow = true })"#]
    );
    assert_eq!(env.active(), "s2:3", "the tab you left mail on");
    let mut windows = env.windows();
    windows.sort();
    assert_eq!(windows, ["s1:1", "s2:1", "s2:3", "s2:3"]);
    env.ok(&["go", "ren"]);
    assert_eq!(env.active(), "s1:1", "ren remembers where you left it");

    // Already there, or no window: nothing to do.
    env.ok(&["send", "s1"]);
    env.set_active("s1:5");
    env.ok(&["send", "mail"]);
    assert!(env.dispatched().iter().all(|d| !d.contains("window.move")));
}

#[test]
fn send_next_and_prev_and_home() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_terminal("s2:2", &env.dir("mail"));
    env.add_window("1");
    env.set_active("1");

    env.ok(&["send", "next"]);
    assert_eq!(env.active(), "s1:1");
    env.ok(&["send", "prev"]);
    assert_eq!(
        env.dispatched().last().unwrap(),
        "o.workspaces.move_window(1)"
    );
    assert_eq!(env.active(), "1");
    env.ok(&["send", "prev", "--silent"]);
    assert_eq!(env.active(), "1");
    let mut windows = env.windows();
    windows.sort();
    assert_eq!(windows, ["s1:1", "s2:2", "s2:2"]);
}

#[test]
fn sending_a_spaces_last_window_away_ends_it() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_terminal("s2:1", &env.dir("mail"));
    env.add_window("s2:2");
    env.ok(&["go", "ren"]);
    env.ok(&["go", "mail"]);
    env.ok(&["ws", "2"]);

    // Silently: the emptied tab gives way to the next one with windows.
    env.ok(&["send", "ren", "--silent"]);
    assert_eq!(env.active(), "s2:1");

    // Following: mail is gone, and so is where you left it.
    env.ok(&["send", "ren"]);
    assert_eq!(env.active(), "s1:1");
    let labels: Vec<String> = env
        .ls_json()
        .iter()
        .map(|r| r["label"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(labels, ["home", "ren"]);
    let visits = fs::read_to_string(env.state().join("last")).unwrap();
    assert!(!visits.contains("s2\t"), "{visits}");
}

#[test]
fn ws_new_next_and_prev_move_between_a_spaces_tabs() {
    let env = Env::new();
    env.add_window("ren:1");
    env.add_window("ren:3");
    env.add_window("other:2");
    env.set_active("ren:1");

    env.ok(&["ws", "next"]);
    assert_eq!(env.active(), "ren:3");
    env.ok(&["ws", "next"]);
    assert_eq!(env.active(), "ren:1", "wraps, skipping other spaces");
    env.ok(&["ws", "prev"]);
    assert_eq!(env.active(), "ren:3");
    env.ok(&["ws", "new"]);
    assert_eq!(env.active(), "ren:4", "after the last tab");
    env.ok(&["ws", "1"]);
    env.ok(&["ws", "new"]);
    assert_eq!(env.active(), "ren:5", "the new tab kept its terminal");
}

#[test]
fn ws_new_opens_a_tab_with_a_terminal_in_the_spaces_folder() {
    let env = Env::new();
    let ren = env.dir("Projects/ren");
    fs::create_dir_all(ren.join(".git")).unwrap();
    let src = env.dir("Projects/ren/src");
    env.add_terminal("s1:1", &src);
    env.set_active("s1:1");
    env.ok(&["ls"]);
    env.dispatched();

    env.ok(&["ws", "new"]);
    assert_eq!(env.active(), "s1:2");
    assert_eq!(env.windows().last().unwrap(), "s1:2");
    // The space's checkout, not the subfolder its terminal is in.
    assert_eq!(env.launched(1), [ren.display().to_string()]);
    // As with a new space: the terminal opens there first, then you go.
    let dispatched = env.dispatched();
    assert!(
        dispatched[0].ends_with(r#"{ workspace = "name:s1:2 silent", no_anim = true })"#),
        "{dispatched:?}"
    );
    assert_eq!(
        dispatched[1],
        r#"hl.dsp.focus({ workspace = "name:s1:2" })"#
    );
    let window = env.newest_window();
    assert_eq!(
        dispatched[2],
        format!(
            r#"hl.dsp.window.set_prop({{ prop = "no_anim", value = "false", window = "address:{window}" }})"#
        )
    );
}

#[test]
fn ws_new_in_home_uses_omarchys_next_free_workspace() {
    let env = Env::new();
    env.add_window("1");
    env.add_window("2");
    env.ok(&["ws", "new"]);
    assert_eq!(env.dispatched(), ["o.workspaces.focus(3)"]);

    // A single tab has no next.
    let env = Env::new();
    env.ok(&["ws", "next"]);
    assert!(env.dispatched().is_empty());
}

#[test]
fn new_opens_a_space_with_a_terminal_in_the_focused_terminals_folder() {
    let env = Env::new();
    let ren = env.dir("Projects/ren");
    fs::create_dir_all(ren.join(".git")).unwrap();
    let src = env.dir("Projects/ren/src");
    env.add_terminal("1", &src);

    let out = env.ok(&["new"]);
    assert_eq!(out, "Opened a space in ~/Projects/ren/src\n");
    assert_eq!(env.active(), "s1:1");
    assert_eq!(env.launched(1), [src.display().to_string()]);
    // The terminal opened there first, without its pop-in, which would still
    // be fading it in from nothing as you arrived; only then did you go
    // there, so the space never shows empty. Then it animates as usual.
    let dispatched = env.dispatched();
    assert!(
        dispatched[0].starts_with("hl.dsp.exec_cmd("),
        "{dispatched:?}"
    );
    assert!(
        dispatched[0].ends_with(r#"{ workspace = "name:s1:1 silent", no_anim = true })"#),
        "{dispatched:?}"
    );
    let window = env.newest_window();
    assert_eq!(
        dispatched[2],
        format!(
            r#"hl.dsp.window.set_prop({{ prop = "no_anim", value = "false", window = "address:{window}" }})"#
        )
    );
    assert_eq!(
        dispatched[1],
        r#"hl.dsp.focus({ workspace = "name:s1:1" })"#
    );
    assert_eq!(env.windows().last().unwrap(), "s1:1");

    // Until its terminal shows up, it goes by the folder it opened in, as
    // it will after: by that folder's checkout.
    let rows = env.ls_json();
    assert_eq!(rows[1]["id"], "s1");
    assert_eq!(rows[1]["label"], "ren");
    // Its terminal's shell starts.
    env.cd(&env.newest_window(), &src);
    let rows = env.ls_json();
    assert_eq!(rows[1]["label"], "ren");
    assert_eq!(rows[1]["named"], false);
    assert_eq!(rows[1]["folder"], ren.display().to_string());
    assert_eq!(rows[1]["active"], true);
}

#[test]
fn the_first_space_opens_at_home() {
    let env = Env::new();
    env.ok(&["new"]);
    assert_eq!(env.launched(1), [env.dir("").display().to_string()]);
    assert_eq!(env.ls_json()[1]["label"], "~");
}

#[test]
fn new_from_a_window_that_isnt_a_terminal_uses_the_spaces_folder() {
    let env = Env::new();
    let mail = env.dir("mail");
    env.ok(&["new", "mail"]);
    env.cd(&env.newest_window(), &mail);
    env.add_window("s1:1");
    env.ok(&["ls"]);

    env.ok(&["new"]);
    assert_eq!(env.active(), "s2:1");
    assert_eq!(
        env.launched(2),
        [mail.display().to_string(), mail.display().to_string()]
    );

    let err = env.fails(&["new", "nowhere"]);
    assert!(err.contains("no folder ~/nowhere"), "{err}");
}

#[test]
fn a_space_follows_its_first_terminal_from_folder_to_folder() {
    let env = Env::new();
    let (notes, japanese) = (env.dir("notes"), env.dir("japanese"));
    env.set_active("s1:1");
    let first = env.add_terminal("s1:1", &notes);
    assert_eq!(env.ls_json()[1]["label"], "notes");

    env.cd(&first, &japanese);
    assert_eq!(env.ls_json()[1]["label"], "japanese");

    // A second terminal, wherever it is, doesn't change it.
    let second = env.add_terminal("s1:2", &notes);
    assert_eq!(env.ls_json()[1]["label"], "japanese");

    // Once the first closes, the next one names it.
    env.close_window(&first);
    assert_eq!(env.ls_json()[1]["label"], "notes");
    env.close_window(&second);
    assert_eq!(env.ls_json()[1]["label"], "notes", "the last folder it saw");
}

#[test]
fn rename_gives_a_name_that_stays_until_cleared() {
    let env = Env::new();
    let notes = env.dir("notes");
    env.set_active("s1:1");
    let terminal = env.add_terminal("s1:1", &notes);

    assert_eq!(
        env.ok(&["rename", "Japanese"]),
        "Named the space 'Japanese'\n"
    );
    env.cd(&terminal, &env.dir("elsewhere"));
    let rows = env.ls_json();
    assert_eq!(rows[1]["label"], "Japanese");
    assert_eq!(rows[1]["named"], true);

    // By what it's called, from elsewhere; then back to its folder's name.
    env.set_active("1");
    env.ok(&["rename", "japanese", "  "]);
    assert_eq!(env.ls_json()[1]["label"], "elsewhere");

    let err = env.fails(&["rename", "home", "x"]);
    assert!(err.contains("can't be renamed"), "{err}");
    let err = env.fails(&["rename", "x"]);
    assert!(
        err.contains("can't be renamed"),
        "in home, nothing to rename: {err}"
    );
    let err = env.fails(&["rename", "s1", "a\nb"]);
    assert!(err.contains("one line"), "{err}");
}

#[test]
fn icon_shows_on_the_badge_until_cleared() {
    let env = Env::new();
    let notes = env.dir("notes");
    env.set_active("s1:1");
    env.add_terminal("s1:1", &notes);
    assert_eq!(env.ls_json()[1]["icon"], Value::Null);

    assert_eq!(env.ok(&["icon", "🚀"]), "Gave the space the icon 🚀\n");
    let rows = env.ls_json();
    assert_eq!(rows[1]["icon"], "🚀");
    assert_eq!(rows[1]["label"], "notes", "the name stays its folder's");

    // By what it's called, from elsewhere; then back to its letters.
    env.set_active("1");
    env.ok(&["icon", "notes", "\u{f0415}"]);
    assert_eq!(env.ls_json()[1]["icon"], "\u{f0415}");
    assert_eq!(
        env.ok(&["icon", "s1", ""]),
        "The space shows its letters again\n"
    );
    assert_eq!(env.ls_json()[1]["icon"], Value::Null);

    let err = env.fails(&["icon", "home", "🏠"]);
    assert!(err.contains("can't have an icon"), "{err}");
    let err = env.fails(&["icon", "s1", "a b"]);
    assert!(err.contains("without spaces"), "{err}");
}

#[test]
fn rename_names_a_tab_until_cleared_or_closed() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    let second = env.add_window("s1:2");
    env.set_active("s1:1");
    let tabs = |env: &Env| env.ls_json()[1]["tabs"].clone();

    assert_eq!(
        env.ok(&["rename", "s1:2", "server"]),
        "Named tab 2 'server'\n"
    );
    assert_eq!(tabs(&env), serde_json::json!({ "2": "server" }));
    env.ok(&["rename", "ren:1", "editor"]);
    assert_eq!(
        tabs(&env),
        serde_json::json!({ "1": "editor", "2": "server" }),
        "by what the space is called"
    );
    assert_eq!(
        env.ok(&["rename", "s1:1", ""]),
        "Tab 1 is called after its window again\n"
    );
    assert_eq!(tabs(&env), serde_json::json!({ "2": "server" }));

    // The name goes with its tab, so a new tab there starts without it.
    env.close_window(&second);
    assert_eq!(tabs(&env), serde_json::json!({}));

    let err = env.fails(&["rename", "s1:4", "x"]);
    assert!(err.contains("'ren' has no tab 4"), "{err}");
}

#[test]
fn rename_names_home_tabs_by_workspace() {
    let env = Env::new();
    let mail = env.add_window("3");
    let other = env.add_window("13");
    env.set_active("3");
    let tabs = |env: &Env| env.ls_json()[0]["tabs"].clone();

    // From home, tab 3 is the focused monitor's: workspace 3, not 13.
    assert_eq!(
        env.ok(&["rename", "home:3", "mail"]),
        "Named tab 3 'mail'\n"
    );
    assert_eq!(tabs(&env), serde_json::json!({ "3": "mail" }));

    // From a space, the tab 3 meant is ambiguous with two monitors.
    env.add_window("s1:1");
    env.set_active("s1:1");
    let err = env.fails(&["rename", "home:3", "x"]);
    assert!(err.contains("more than one monitor"), "{err}");
    env.close_window(&other);
    env.ok(&["rename", "home:3", "inbox"]);
    assert_eq!(tabs(&env), serde_json::json!({ "3": "inbox" }));

    assert_eq!(
        env.ok(&["rename", "home:3", ""]),
        "Tab 3 shows just its number again\n"
    );
    assert_eq!(tabs(&env), serde_json::json!({}));
    let err = env.fails(&["rename", "home:5", "x"]);
    assert!(err.contains("'home' has no tab 5"), "{err}");

    // The name goes with its workspace, once Hyprland drops it.
    env.ok(&["rename", "home:3", "mail"]);
    env.close_window(&mail);
    assert_eq!(tabs(&env), serde_json::json!({}));
}

#[test]
fn go_finds_spaces_by_id_or_by_what_they_are_called() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_terminal("s2:3", &env.dir("mail"));

    env.ok(&["go", "mail"]);
    assert_eq!(env.active(), "s2:3");
    env.ok(&["go", "s1"]);
    assert_eq!(env.active(), "s1:1");
    env.ok(&["go", "Mail"]);
    assert_eq!(env.active(), "s2:3", "ignoring case");
    env.ok(&["go", "home"]);
    assert_eq!(env.active(), "1");

    let err = env.fails(&["go", "rne"]);
    assert!(
        err.contains("no space called 'rne'. Did you mean ren?"),
        "{err}"
    );

    // Two spaces in the same folder are told apart by id.
    env.add_terminal("s3:1", &env.dir("ren"));
    let err = env.fails(&["go", "ren"]);
    assert!(
        err.contains("2 spaces are called 'ren'; pick one by id: s1, s3"),
        "{err}"
    );
}

#[test]
fn next_and_prev_move_down_and_up_the_spaces_wrapping_around() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_terminal("s2:3", &env.dir("mail"));
    env.set_active("1");

    env.ok(&["next"]);
    assert_eq!(env.active(), "s1:1");
    env.ok(&["next"]);
    assert_eq!(env.active(), "s2:3", "its only tab");
    env.ok(&["next"]);
    assert_eq!(env.active(), "1", "wraps to home");
    env.ok(&["prev"]);
    assert_eq!(env.active(), "s2:3", "wraps to the last");
    env.ok(&["prev"]);
    assert_eq!(env.active(), "s1:1");
}

#[test]
fn next_with_only_home_stays_put() {
    let env = Env::new();
    env.set_active("2");
    env.ok(&["next"]);
    env.ok(&["prev"]);
    assert_eq!(env.active(), "2");
}

#[test]
fn spaces_end_with_their_windows_and_keep_the_order_they_were_made_in() {
    let env = Env::new();
    env.ok(&["new"]);
    let first = env.newest_window();
    env.cd(&first, &env.dir("zebra"));
    env.ok(&["new"]);
    env.cd(&env.newest_window(), &env.dir("apple"));
    let labels = |env: &Env| -> Vec<String> {
        env.ls_json()
            .iter()
            .map(|r| r["label"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(labels(&env), ["home", "zebra", "apple"]);

    env.close_window(&first);
    assert_eq!(labels(&env), ["home", "apple"]);
    let store = fs::read_to_string(env.state().join("spaces.json")).unwrap();
    assert!(!store.contains("\"s1\""), "{store}");

    // Its id is free again.
    env.ok(&["new"]);
    assert_eq!(env.active(), "s1:1");
}

#[test]
fn close_closes_the_spaces_windows_and_leaves_it() {
    let env = Env::new();
    env.add_window("1");
    env.add_terminal("s1:1", &env.dir("tax"));
    env.add_terminal("s2:1", &env.dir("ren"));
    env.add_window("s2:2");
    env.ok(&["go", "tax"]);
    env.ok(&["go", "ren"]);
    env.ok(&["ws", "2"]);

    assert_eq!(env.ok(&["close"]), "Closed 2 windows in 'ren'\n");
    assert_eq!(env.windows(), ["1", "s1:1"]);
    assert_eq!(env.active(), "s1:1", "back to the space before");

    assert_eq!(env.ok(&["close", "tax"]), "Closed 1 window in 'tax'\n");
    assert_eq!(env.windows(), ["1"]);
    assert_eq!(env.active(), "1", "then home");

    let err = env.fails(&["close", "home"]);
    assert!(err.contains("one by one"), "{err}");
    let err = env.fails(&["close", "nope"]);
    assert!(err.contains("no space called 'nope'"), "{err}");
}

#[test]
fn terminal_opens_in_the_focused_terminals_folder_or_the_spaces() {
    let env = Env::new();
    let (ren, docs) = (env.dir("ren"), env.dir("ren/docs"));
    env.add_terminal("s1:1", &ren);
    env.set_active("s1:1");
    env.ok(&["ls"]);

    // An empty tab of the space: its folder.
    env.set_active("s1:2");
    env.ok(&["terminal"]);
    // A tab whose terminal is elsewhere: that terminal's folder.
    env.add_terminal("s1:2", &docs);
    env.ok(&["terminal"]);
    // Home with nothing focused: home.
    env.ok(&["go", "home"]);
    env.ok(&["terminal"]);
    assert_eq!(
        env.launched(3),
        [
            ren.display().to_string(),
            docs.display().to_string(),
            env.dir("").display().to_string()
        ]
    );
}

#[test]
fn ls_lists_spaces_with_counts() {
    let env = Env::new();
    let ren = env.dir("Projects/ren");
    env.add_window("1");
    env.add_terminal("s1:1", &ren);
    env.add_window("s1:2");
    env.add_window("special:scratchpad");
    env.set_active("s1:2");
    // A workspace an older version named after its space is a space too.
    env.add_window("tax:1");

    assert_eq!(
        env.ok(&["ls"]),
        "  home  home  1 window\n\
         * s1    ren   2 windows    ~/Projects/ren\n  \
         tax   tax   1 window\n"
    );
    let rows = env.ls_json();
    assert_eq!(rows[1]["workspaces"], json!([1, 2]));
    assert_eq!(rows[0]["folder"], Value::Null);
    assert_eq!(rows[2]["left_at"], 0, "never left");
}

#[test]
fn ls_lists_each_spaces_terminals_by_what_they_run() {
    let env = Env::new();
    let ren = env.dir("Projects/ren");
    let notes = env.add_terminal("1", &env.dir(""));
    env.add_window("2");
    let agent = env.add_terminal("s1:1", &ren);
    let shell = env.add_terminal("s1:2", &ren.join("src"));
    env.run_program(&agent, Some("claude"));

    let rows = env.ls_json();
    assert_eq!(
        rows[0]["terminals"],
        json!([{ "address": notes, "slot": 1, "folder": env.dir("").display().to_string(), "program": null, "label": "~", "status": null }])
    );
    assert_eq!(
        rows[1]["terminals"],
        json!([
            { "address": agent, "slot": 1, "folder": ren.display().to_string(), "program": "claude", "label": "claude", "status": null },
            { "address": shell, "slot": 2, "folder": ren.join("src").display().to_string(), "program": null, "label": "src", "status": null },
        ])
    );
    // The space still goes by its first terminal's checkout.
    assert_eq!(rows[1]["label"], "ren");

    env.run_program(&agent, None);
    assert_eq!(env.ls_json()[1]["terminals"][0]["label"], "ren");
}

#[test]
fn a_terminal_in_core_is_named_and_followed_through_core() {
    let env = Env::new();
    let core = Core::start(&env);
    let ren = env.dir("ren");
    fs::create_dir_all(ren.join(".git")).unwrap();
    // A window showing a core terminal, whose own shell Spaces can't see,
    // and a plain one.
    let window = env.add_window("s1:1");
    let t = core.add(&window, &ren);
    let plain = env.add_terminal("s1:2", &env.dir("notes"));
    env.set_active("1");

    let rows = env.ls_json();
    assert_eq!(rows[1]["label"], "ren");
    assert_eq!(rows[1]["terminals"][0]["address"], window);
    assert_eq!(rows[1]["terminals"][0]["label"], "ren");
    assert_eq!(rows[1]["terminals"][1]["address"], plain);
    assert_eq!(rows[1]["status"], Value::Null);

    // Its shell starts an agent in a folder further in.
    core.event(&t, "foreground", json!({ "pid": 9, "name": "claude", "argv": ["claude"], "cwd": ren.join("src").display().to_string() }));
    core.event(&t, "status", json!({ "status": { "state": "working", "app": "claude-code" }, "records": {}, "reported": true }));
    let rows = env.ls_json();
    assert_eq!(rows[1]["terminals"][0]["label"], "claude");
    assert_eq!(
        rows[1]["terminals"][0]["folder"],
        ren.join("src").display().to_string()
    );
    assert_eq!(rows[1]["label"], "ren", "still its checkout");
    assert_eq!(rows[1]["terminals"][0]["status"], "working");
    assert_eq!(rows[1]["status"], "working");

    for (word, shown) in [
        ("blocked", json!("waiting")),
        ("error", json!("waiting")),
        ("done", json!("done")),
        ("idle", Value::Null),
    ] {
        core.event(
            &t,
            "status",
            json!({ "status": { "state": word }, "records": {}, "reported": true }),
        );
        assert_eq!(env.ls_json()[1]["status"], shown, "{word}");
    }

    // A shell at its prompt is named after its folder.
    core.event(
        &t,
        "foreground",
        json!({ "pid": 8, "name": "zsh", "argv": ["-zsh"], "cwd": ren.display().to_string() }),
    );
    assert_eq!(env.ls_json()[1]["terminals"][0]["label"], "ren");
}

#[test]
fn watch_shows_what_core_reports_and_clears_done_once_seen() {
    use std::time::Duration;
    let env = Env::new();
    let core = Core::start(&env);
    let ren = env.dir("ren");
    let agent = env.add_window("s1:1");
    let t = core.add(&agent, &ren);
    let other = env.add_window("s1:2");
    let u = core.add(&other, &ren);
    env.add_window("1");
    env.set_active("1");

    let watch = Watch::start(&env);
    let status = |rows: &[Value]| rows[1]["status"].clone();
    assert_eq!(
        status(&watch.next_rows(Duration::from_secs(3))),
        Value::Null
    );
    watch.joined(&env);

    // Core says so as it happens.
    core.event(
        &t,
        "status",
        json!({ "status": { "state": "working" }, "records": {}, "reported": true }),
    );
    assert_eq!(status(&watch.next_rows(Duration::from_secs(1))), "working");
    core.event(&t, "turn", json!({ "turn": 1, "state": "done" }));
    core.event(
        &t,
        "status",
        json!({ "status": { "state": "done" }, "records": {}, "reported": true }),
    );
    core.event(&u, "turn", json!({ "turn": 1, "state": "done" }));
    core.event(
        &u,
        "status",
        json!({ "status": { "state": "done" }, "records": {}, "reported": true }),
    );
    assert_eq!(status(&watch.next_rows(Duration::from_secs(1))), "done");

    // You look at the agent's window: its turn is seen, the other's isn't.
    env.event(&format!(
        "activewindowv2>>{}",
        agent.trim_start_matches("0x")
    ));
    let rows = watch.next_rows(Duration::from_secs(1));
    assert_eq!(rows[1]["terminals"][0]["status"], Value::Null);
    assert_eq!(rows[1]["terminals"][1]["status"], "done");

    // Its next turn ends while you're elsewhere: done again.
    env.event("activewindowv2>>");
    core.event(
        &t,
        "status",
        json!({ "status": { "state": "working" }, "records": {}, "reported": true }),
    );
    core.event(&t, "turn", json!({ "turn": 2, "state": "done" }));
    core.event(
        &t,
        "status",
        json!({ "status": { "state": "done" }, "records": {}, "reported": true }),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let rows = watch.next_rows(Duration::from_secs(2));
        if rows[1]["terminals"][0]["status"] == "done" {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "never done again");
    }

    // Core goes: so do the statuses.
    core.stop();
    let rows = watch.next_rows(Duration::from_secs(2));
    assert_eq!(status(&rows), Value::Null);
}

#[test]
fn watch_rings_when_an_agent_needs_you_or_finishes_unseen() {
    use std::time::Duration;
    let env = Env::new();
    let core = Core::start(&env);
    // A player that notes what it was asked to play.
    let bin = env.root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let played = env.root.path().join("played");
    fs::write(
        bin.join("pw-play"),
        format!("#!/bin/sh\necho \"$1\" >> '{}'\n", played.display()),
    )
    .unwrap();
    fs::set_permissions(
        bin.join("pw-play"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    // Its own sound for done; Herdr's, built in, for waiting.
    env.write_config("[sounds]\ndone = \"/s/done.oga\"\n");
    let heard = || {
        thread::sleep(Duration::from_millis(300));
        fs::read_to_string(&played)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect::<Vec<_>>()
    };

    let ren = env.dir("ren");
    let agent = env.add_window("s1:1");
    let t = core.add(&agent, &ren);
    // Waiting for you since before the watch started: no sound for that.
    core.event(
        &t,
        "status",
        json!({ "status": { "state": "blocked" }, "records": {}, "reported": true }),
    );
    env.add_window("1");
    env.set_active("1");

    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let mut command = env.command(&["watch", "--sounds"]);
    command.env("PATH", path);
    let watch = Watch::run(command);
    // Once it has core's terminals, it shows the agent waiting.
    while watch.next_rows(Duration::from_secs(3))[1]["status"] != "waiting" {}
    watch.joined(&env);
    assert!(heard().is_empty());

    let status = |state: &str| {
        core.event(
            &t,
            "status",
            json!({ "status": { "state": state }, "records": {}, "reported": true }),
        );
        let _ = watch.next_rows(Duration::from_secs(1));
    };
    status("working");
    status("blocked");
    let request = env.root.path().join("run/omarchy-spaces/request.mp3");
    assert_eq!(heard(), [request.display().to_string()]);
    assert_eq!(
        fs::read(&request).unwrap(),
        fs::read("assets/sounds/request.mp3").unwrap()
    );
    status("working");
    status("done");
    assert_eq!(heard().len(), 2);
    assert_eq!(heard()[1], "/s/done.oga");

    // Finishing in the window you're looking at: nothing to tell you.
    env.event(&format!(
        "activewindowv2>>{}",
        agent.trim_start_matches("0x")
    ));
    status("working");
    status("done");
    assert_eq!(heard().len(), 2);
}

#[test]
fn shell_runs_in_core_and_ends_with_its_window() {
    // It attaches with the real `hc`.
    let on_path = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .any(|dir| dir.join("hc").is_file());
    if !on_path {
        eprintln!("skipped: hc isn't installed");
        return;
    }
    let env = Env::new();
    let core = Core::start(&env);
    let ren = env.dir("ren");
    let out = env
        .command(&["shell"])
        .current_dir(&ren)
        .env("TERM", "xterm-kitty")
        .env("SSH_AUTH_SOCK", "/run/ssh")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);

    let runs = core.requests("terminal.run");
    assert_eq!(runs.len(), 1, "{stderr}");
    let run = &runs[0]["params"];
    assert_eq!(run["cwd"], ren.display().to_string());
    assert_eq!(run["labels"]["omarchy-spaces"], "1");
    assert_eq!(
        run["labels"]["omarchy-spaces-window"],
        std::process::id().to_string(),
        "the window's process: its parent"
    );
    assert_eq!(run["rm"], true);
    assert_eq!(run["env"]["SSH_AUTH_SOCK"], "/run/ssh");
    assert_eq!(
        run["env"]["TERM"],
        Value::Null,
        "core's emulator names itself"
    );
    // Attaching here fails, as if the window closed at once: the shell ends.
    let kills = core.requests("terminal.kill");
    assert_eq!(kills.len(), 1, "{stderr}");
    assert_eq!(kills[0]["params"]["terminal"], "t1");
}

#[test]
fn shell_without_core_runs_your_own() {
    let env = Env::new();
    // Without `hc` on PATH: runs your shell quietly, no stderr noise.
    let out = env
        .command(&["shell"])
        .env("PATH", "/usr/bin:/bin")
        .env("SHELL", "/usr/bin/true")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success());
    let has_hc_in_usr = Path::new("/usr/bin/hc").is_file() || Path::new("/bin/hc").is_file();
    if !has_hc_in_usr {
        assert!(
            out.stderr.is_empty(),
            "unexpected stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // With `hc` on PATH but core unreachable: warns and falls back to own shell.
    let bin = env.root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(bin.join("hc"), "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(
        bin.join("hc"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let out = env
        .command(&["shell"])
        .env("PATH", bin)
        .env("SHELL", "/usr/bin/true")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("plain shell"));
}

#[test]
fn status_reports_agent_state_without_herdr_core() {
    let env = Env::new();
    let app = env.dir("app");
    let window = env.add_terminal("s1:1", &app);
    env.set_active("1");

    assert_eq!(env.ls_json()[1]["status"], Value::Null);

    for (arg, shown) in [
        ("working", json!("working")),
        ("waiting", json!("waiting")),
        ("blocked", json!("waiting")),
        ("error", json!("waiting")),
        ("done", json!("done")),
        ("idle", Value::Null),
    ] {
        env.ok(&["status", arg, "--window", &window]);
        let rows = env.ls_json();
        assert_eq!(rows[1]["status"], shown, "{arg}");
        assert_eq!(rows[1]["terminals"][0]["status"], shown, "{arg}");
    }

    // `SPACES_WINDOW` works without passing `--window`, and `clear` clears it.
    let out = env
        .command(&["status", "working"])
        .env("SPACES_WINDOW", &window)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(env.ls_json()[1]["status"], "working");

    let out = env
        .command(&["status", "clear"])
        .env("SPACES_WINDOW", &window)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(env.ls_json()[1]["status"], Value::Null);

    // Outside Hyprland, auto-detecting hooks exit cleanly without failing.
    let out = env
        .command(&["status", "working"])
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
        .output()
        .unwrap();
    assert!(out.status.success());

    // Unknown status words still fail clearly.
    let err = env.fails(&["status", "bogus", "--window", &window]);
    assert!(err.contains("unknown status"), "{err}");
}

#[test]
fn watch_shows_native_status_clears_done_on_focus_and_prunes() {
    use std::time::Duration;
    let env = Env::new();
    let app = env.dir("app");
    let agent = env.add_terminal("s1:1", &app);
    let other = env.add_terminal("s1:2", &app);
    env.add_window("1");
    env.set_active("1");

    let watch = Watch::start(&env);
    let status = |rows: &[Value]| rows[1]["status"].clone();
    assert_eq!(
        status(&watch.next_rows(Duration::from_secs(3))),
        Value::Null
    );
    watch.joined(&env);

    env.ok(&["status", "working", "--window", &agent]);
    assert_eq!(status(&watch.next_rows(Duration::from_secs(1))), "working");

    env.ok(&["status", "done", "--window", &agent]);
    env.ok(&["status", "done", "--window", &other]);
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let rows = watch.next_rows(Duration::from_secs(2));
        if rows[1]["terminals"][0]["status"] == "done"
            && rows[1]["terminals"][1]["status"] == "done"
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "never both done");
    }

    // Focusing the first agent clears its `done`, persists it for `ls`, and
    // leaves the second agent's `done` alone.
    env.event(&format!(
        "activewindowv2>>{}",
        agent.trim_start_matches("0x")
    ));
    let rows = watch.next_rows(Duration::from_secs(1));
    assert_eq!(rows[1]["terminals"][0]["status"], Value::Null);
    assert_eq!(rows[1]["terminals"][1]["status"], "done");
    assert_eq!(env.ls_json()[1]["terminals"][0]["status"], Value::Null);

    // Its next turn ends while you're elsewhere: `done` again.
    env.event("activewindowv2>>");
    env.ok(&["status", "working", "--window", &agent]);
    assert_eq!(
        watch.next_rows(Duration::from_secs(1))[1]["terminals"][0]["status"],
        "working"
    );
    env.ok(&["status", "done", "--window", &agent]);
    assert_eq!(
        watch.next_rows(Duration::from_secs(1))[1]["terminals"][0]["status"],
        "done"
    );

    // Closing a window prunes its status file.
    let status_file = env
        .root
        .path()
        .join(format!("run/omarchy-spaces/status/{other}.json"));
    assert!(status_file.exists());
    env.close_window(&other);
    env.event(&format!("closewindow>>{}", other.trim_start_matches("0x")));
    let _ = watch.next_rows(Duration::from_secs(1));
    assert!(!status_file.exists());
}

#[test]
fn watch_rings_for_native_status_without_herdr_core() {
    use std::time::Duration;
    let env = Env::new();
    let bin = env.root.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let played = env.root.path().join("played");
    fs::write(
        bin.join("pw-play"),
        format!("#!/bin/sh\necho \"$1\" >> '{}'\n", played.display()),
    )
    .unwrap();
    fs::set_permissions(
        bin.join("pw-play"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    env.write_config("[sounds]\ndone = \"/s/done.oga\"\n");
    let heard = || {
        thread::sleep(Duration::from_millis(300));
        fs::read_to_string(&played)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect::<Vec<_>>()
    };

    let app = env.dir("app");
    let agent = env.add_terminal("s1:1", &app);
    // Waiting since before `watch` started: no sound for that.
    env.ok(&["status", "waiting", "--window", &agent]);
    env.add_window("1");
    env.set_active("1");

    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let mut command = env.command(&["watch", "--sounds"]);
    command.env("PATH", path);
    let watch = Watch::run(command);
    assert_eq!(
        watch.next_rows(Duration::from_secs(3))[1]["status"],
        "waiting"
    );
    watch.joined(&env);
    assert!(heard().is_empty());

    let set = |state: &str| {
        env.ok(&["status", state, "--window", &agent]);
        let _ = watch.next_rows(Duration::from_secs(1));
    };
    set("working");
    set("waiting");
    let request = env.root.path().join("run/omarchy-spaces/request.mp3");
    assert_eq!(heard(), [request.display().to_string()]);

    set("working");
    set("done");
    assert_eq!(heard().len(), 2);
    assert_eq!(heard()[1], "/s/done.oga");

    // Finishing while you're looking at the window: silent.
    env.event(&format!(
        "activewindowv2>>{}",
        agent.trim_start_matches("0x")
    ));
    let _ = watch.next_rows(Duration::from_secs(1));
    set("working");
    set("done");
    assert_eq!(heard().len(), 2);

    // One-shot `done` while unfocused (e.g. Codex notify or `cmd && space status done`): rings.
    env.event("activewindowv2>>");
    env.ok(&["status", "done", "--window", &agent]);
    let _ = watch.next_rows(Duration::from_secs(1));
    assert_eq!(heard().len(), 3);
    assert_eq!(heard()[2], "/s/done.oga");

    // Clearing a working agent goes quiet without ringing `done`.
    set("working");
    set("clear");
    assert_eq!(heard().len(), 3);
}

#[test]
fn ls_without_hyprland_lists_what_it_remembers() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.ok(&["ls"]);
    let out = env
        .command(&["ls"])
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "  home  home  -\n  s1    ren   -            ~/ren\n"
    );
}

#[test]
fn go_without_a_space_opens_the_picker() {
    let env = Env::new();
    env.add_terminal("s1:1", &env.dir("ren"));
    env.add_terminal("s2:1", &env.dir("trip"));

    // The picker sees the most recently left space first.
    env.write_config("picker = \"cat > $HOME/picked; head -1 $HOME/picked\"\n");
    env.ok(&["go", "trip"]);
    env.ok(&["go", "ren"]);
    env.ok(&["go"]);
    let shown = fs::read_to_string(env.home().join("picked")).unwrap();
    let order: Vec<_> = shown
        .lines()
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(order, ["trip", "home", "ren"], "{shown}");
    assert_eq!(env.active(), "s2:1");

    // Dismissing the picker does nothing.
    env.write_config("picker = \"exit 130\"\n");
    env.ok(&["go"]);
    assert_eq!(env.active(), "s2:1");
}

/// A running `space watch`, its lines read on a thread.
struct Watch {
    child: std::process::Child,
    lines: std::sync::mpsc::Receiver<String>,
}

impl Watch {
    fn start(env: &Env) -> Self {
        Self::run(env.command(&["watch"]))
    }

    fn run(mut command: Command) -> Self {
        let mut child = command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = std::sync::mpsc::channel();
        thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Self { child, lines }
    }

    /// The labels in the next list it prints within `within`.
    #[track_caller]
    fn next(&self, within: std::time::Duration) -> Vec<String> {
        self.next_rows(within)
            .iter()
            .map(|r| r["label"].as_str().unwrap().to_string())
            .collect()
    }

    /// The next list it prints within `within`.
    #[track_caller]
    fn next_rows(&self, within: std::time::Duration) -> Vec<Value> {
        let line = self
            .lines
            .recv_timeout(within)
            .expect("space watch printed nothing new in time");
        serde_json::from_str(&line).unwrap()
    }

    /// Waits for its listener to join, so events reach it.
    fn joined(&self, env: &Env) {
        for _ in 0..100 {
            if !env.listeners.lock().unwrap().is_empty() {
                return;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("space watch never listened to events");
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn watch_prints_the_spaces_again_as_they_change() {
    use std::time::Duration;
    let env = Env::new();
    let (notes, japanese, mail, inbox) = (
        env.dir("notes"),
        env.dir("japanese"),
        env.dir("mail"),
        env.dir("inbox"),
    );
    let watch = Watch::start(&env);
    assert_eq!(watch.next(Duration::from_secs(2)), ["home"]);
    watch.joined(&env);

    // A window opens: Hyprland says so.
    let first = env.add_terminal("s1:1", &notes);
    env.event("openwindow>>1,s1:1,kitty,zsh");
    assert_eq!(watch.next(Duration::from_secs(1)), ["home", "notes"]);

    // In the space you're in, a `cd` shows within a fraction of a second,
    // though nothing says so.
    env.set_active("s1:1");
    env.event("workspacev2>>-1,s1:1");
    let _ = watch.next(Duration::from_secs(1));
    env.cd(&first, &japanese);
    assert_eq!(watch.next(Duration::from_millis(800)), ["home", "japanese"]);

    // Elsewhere, a terminal's new title is enough, well before the slow
    // round would read it.
    let other = env.add_terminal("s2:1", &mail);
    env.event("openwindow>>2,s2:1,kitty,zsh");
    assert_eq!(
        watch.next(Duration::from_secs(1)),
        ["home", "japanese", "mail"]
    );
    env.cd(&other, &inbox);
    env.event("windowtitlev2>>2,~/inbox");
    assert_eq!(
        watch.next(Duration::from_millis(1500)),
        ["home", "japanese", "inbox"]
    );

    // A name given by another `space` command.
    env.ok(&["rename", "s2", "Inbox"]);
    assert_eq!(
        watch.next(Duration::from_secs(1)),
        ["home", "japanese", "Inbox"]
    );

    // Nothing changed: nothing printed.
    assert!(
        watch
            .lines
            .recv_timeout(Duration::from_millis(500))
            .is_err()
    );
}

#[test]
fn watch_ends_with_hyprland() {
    let env = Env::new();
    let mut watch = Watch::start(&env);
    let _ = watch.next(std::time::Duration::from_secs(2));
    watch.joined(&env);
    env.end_events();
    for _ in 0..100 {
        if let Some(status) = watch.child.try_wait().unwrap() {
            assert!(
                !status.success(),
                "it ends with an error, to be started again"
            );
            return;
        }
        thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("space watch kept running without Hyprland");
}

#[test]
fn new_without_exec_rules_goes_there_and_then_opens_the_terminal() {
    let env = Env::new();
    env.hypr.lock().unwrap().exec_rules = false;
    env.ok(&["new"]);
    assert_eq!(env.active(), "s1:1");
    assert_eq!(env.launched(1), [env.dir("").display().to_string()]);
}

#[test]
fn an_emptied_tab_gives_way_to_the_next_one_then_the_one_before() {
    let env = Env::new();
    env.add_window("s1:1");
    env.add_window("s1:3");
    let closing = env.add_window("s1:2");
    env.set_active("s1:2");
    env.close_window(&closing);

    env.ok(&["emptied", "s1:2"]);
    assert_eq!(env.active(), "s1:3", "the next tab with windows");

    let closing = env.add_window("s1:4");
    env.set_active("s1:4");
    env.close_window(&closing);
    env.ok(&["emptied", "s1:4"]);
    assert_eq!(env.active(), "s1:3", "none after it: the one before");
}

#[test]
fn closing_a_spaces_last_window_ends_it() {
    let env = Env::new();
    env.add_window("1");
    env.add_terminal("s2:1", &env.dir("mail"));
    let last = env.add_terminal("s1:2", &env.dir("notes"));
    env.ok(&["go", "mail"]);
    env.ok(&["go", "notes"]);
    assert_eq!(env.active(), "s1:2");

    env.close_window(&last);
    env.ok(&["emptied", "s1:2"]);
    assert_eq!(env.active(), "s2:1", "back to the space you were in before");
    let labels: Vec<String> = env
        .ls_json()
        .iter()
        .map(|r| r["label"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(labels, ["home", "mail"]);
    let visits = fs::read_to_string(env.state().join("last")).unwrap();
    assert!(!visits.contains("s1\t"), "{visits}");

    // The last space of all: home.
    let only = env.newest_window();
    assert_eq!(env.windows().last().unwrap(), "s2:1");
    env.close_window(&only);
    env.ok(&["emptied", "s2:1"]);
    assert_eq!(env.active(), "1");
}

#[test]
fn emptied_does_nothing_once_things_have_moved_on() {
    let env = Env::new();
    env.add_window("s1:1");
    env.set_active("s1:2");
    // You already left it, or it isn't empty, or it's one of home's.
    env.ok(&["emptied", "s1:3"]);
    env.ok(&["emptied", "s1:1"]);
    env.set_active("s1:1");
    env.ok(&["emptied", "s1:1"]);
    env.set_active("2");
    env.ok(&["emptied", "2"]);
    assert!(env.dispatched().is_empty());
    assert_eq!(env.active(), "2");
}
