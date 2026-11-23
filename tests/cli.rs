//! Runs the `space` binary against a fake Hyprland that speaks the same
//! socket protocol, with its own home, config and state directories.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
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

struct Env {
    root: tempfile::TempDir,
    hypr: Arc<Mutex<Hyprland>>,
}

impl Env {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let socket_dir = root.path().join("run/hypr/test");
        fs::create_dir_all(&socket_dir).unwrap();
        fs::create_dir_all(root.path().join("home/Projects")).unwrap();
        let listener = UnixListener::bind(socket_dir.join(".socket.sock")).unwrap();

        let hypr = Arc::new(Mutex::new(Hyprland {
            active: "1".into(),
            omarchy: true,
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
        fs::create_dir_all(root.path().join("cwds")).unwrap();
        let env = Self { root, hypr };
        env.write_config("");
        env
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn state(&self) -> PathBuf {
        self.home().join(".local/state/renspace")
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
            .env("RENSPACE_FAKE_CWDS", self.cwds());
        command
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
        let dir = self.home().join(".config/renspace");
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

    // The empty new tab goes away when left, as Hyprland does.
    env.ok(&["ws", "1"]);
    env.ok(&["ws", "new"]);
    assert_eq!(env.active(), "ren:4");
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

    // Until its terminal shows up, it goes by the folder it opened in, as
    // it will after: by that folder's checkout.
    let rows = env.ls_json();
    assert_eq!(rows[1]["id"], "s1");
    assert_eq!(rows[1]["label"], "ren");
    env.add_terminal("s1:1", &src);
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
    env.add_terminal("s1:1", &mail);
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
fn spaces_end_with_their_windows_and_keep_the_order_they_were_made_in() {
    let env = Env::new();
    env.ok(&["new"]);
    let first = env.add_terminal("s1:1", &env.dir("zebra"));
    env.ok(&["new"]);
    env.add_terminal("s2:1", &env.dir("apple"));
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

    // A new, empty tab of the space: its folder.
    env.ok(&["ws", "new"]);
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
    // A workspace an older renSpace named after its space is a space too.
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
