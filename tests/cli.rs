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
                Value::Array(
                    windows
                        .iter()
                        .enumerate()
                        .map(|(i, ws)| {
                            let id = self.ensure(ws);
                            json!({ "address": format!("0x{i}"), "workspace": { "id": id, "name": ws } })
                        })
                        .collect(),
                )
                .to_string()
            }
            "j/activewindow" => {
                if self.windows.contains(&self.active) {
                    json!({ "address": "0x0" }).to_string()
                } else {
                    "{}".into()
                }
            }
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

        if expression.starts_with("o.workspaces.") && !self.omarchy {
            return "error: attempt to index a nil value (global 'o')".into();
        }
        if expression.starts_with("hl.dsp.focus(") {
            let target = selector(quoted(expression).unwrap());
            self.ensure(&target);
            self.active = target;
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
        Self { root, hypr }
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
            .env("HYPRLAND_INSTANCE_SIGNATURE", "test");
        command
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

    fn add_window(&self, workspace: &str) {
        let mut hypr = self.hypr.lock().unwrap();
        hypr.ensure(workspace);
        hypr.windows.push(workspace.into());
    }

    fn windows(&self) -> Vec<String> {
        self.hypr.lock().unwrap().windows.clone()
    }

    fn dispatched(&self) -> Vec<String> {
        std::mem::take(&mut self.hypr.lock().unwrap().dispatched)
    }

    fn write_config(&self, text: &str) {
        let dir = self.home().join(".config/renspace");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("config.toml"), text).unwrap();
    }

    fn make_space(&self, dir: &Path, toml: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(".renspace.toml"), toml).unwrap();
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
fn new_creates_the_space_and_goes_to_it() {
    let env = Env::new();
    let dir = env.home().join("Projects/Ren");
    fs::create_dir_all(&dir).unwrap();

    let out = env.command(&["new"]).current_dir(&dir).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read_to_string(dir.join(".renspace.toml")).unwrap(),
        "name = \"ren\"\n"
    );
    assert_eq!(env.active(), "ren:1");
    let index = fs::read_to_string(env.state().join("index")).unwrap();
    assert!(index.contains("Projects/Ren"), "{index}");

    // Same folder again, or the same name elsewhere, fails.
    let out = env.command(&["new"]).current_dir(&dir).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("is already the space 'ren'"));
    let err = env.fails(&["new", "ren", "Projects/other"]);
    assert!(err.contains("already exists"), "{err}");
    assert!(
        !env.home().join("Projects/other").exists(),
        "no stray folder"
    );
}

#[test]
fn new_with_name_and_dir_outside_roots() {
    let env = Env::new();
    let out = env.ok(&["new", "tax-2026", "elsewhere/tax"]);
    assert!(out.contains("Created space 'tax-2026'"), "{out}");
    assert!(env.home().join("elsewhere/tax/.renspace.toml").exists());
    assert_eq!(env.active(), "tax-2026:1");

    // Outside the roots, but the index remembers it.
    env.set_active("1");
    env.ok(&["go", "tax-2026"]);
    assert_eq!(env.active(), "tax-2026:1");
}

#[test]
fn new_rejects_bad_names() {
    let env = Env::new();
    for bad in ["Ren", "ren:1", "home"] {
        env.fails(&["new", bad]);
    }
    assert!(!env.home().join(".renspace.toml").exists());
}

#[test]
fn new_without_hyprland_still_creates_the_space() {
    let env = Env::new();
    let out = env
        .command(&["new", "offline"])
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("couldn't switch"));
    assert!(env.home().join(".renspace.toml").exists());
}

#[test]
fn go_returns_to_where_you_left_each_space() {
    let env = Env::new();
    env.make_space(&env.home().join("Projects/ren"), "");
    env.make_space(&env.home().join("Projects/trip"), "name = \"trip\"\n");
    env.set_active("2");

    env.ok(&["go", "ren"]);
    assert_eq!(env.active(), "ren:1");
    env.ok(&["ws", "3"]);
    env.ok(&["go", "trip"]);
    assert_eq!(env.active(), "trip:1");
    env.ok(&["go", "ren"]);
    assert_eq!(env.active(), "ren:3");
    env.ok(&["go", "home"]);
    assert_eq!(env.active(), "2");
    env.ok(&["go", "trip"]);
    assert_eq!(env.active(), "trip:1");

    // Going to the space you're in does nothing.
    env.dispatched();
    env.ok(&["go", "trip"]);
    assert!(env.dispatched().is_empty());
}

#[test]
fn go_suggests_instead_of_creating() {
    let env = Env::new();
    env.make_space(&env.home().join("Projects/renspace"), "");
    let err = env.fails(&["go", "rensapce"]);
    assert!(
        err.contains("no space called 'rensapce'. Did you mean renspace?"),
        "{err}"
    );
    let err = env.fails(&["go", "qqqqqq"]);
    assert!(err.contains("Create it with: space new"), "{err}");
    assert_eq!(env.active(), "1");
}

#[test]
fn go_finds_spaces_added_by_hand_since_the_last_scan() {
    let env = Env::new();
    env.write_config("roots = [\"~/Projects\"]\nscan_depth = 2\n");
    env.ok(&["ls"]);
    env.make_space(&env.home().join("Projects/talks/rustconf"), "");
    env.ok(&["go", "rustconf"]);
    assert_eq!(env.active(), "rustconf:1");
}

#[test]
fn go_reaches_a_space_whose_folder_is_gone_while_it_has_windows() {
    let env = Env::new();
    env.add_window("old:2");
    env.ok(&["go", "old"]);
    // Never visited, so its first workspace.
    assert_eq!(env.active(), "old:1");
}

#[test]
fn ls_lists_spaces_with_counts() {
    let env = Env::new();
    env.make_space(&env.home().join("Projects/ren"), "");
    env.make_space(&env.home().join("Spaces/tax"), "");
    env.add_window("1");
    env.add_window("ren:1");
    env.add_window("ren:2");
    env.add_window("special:scratchpad");
    env.set_active("ren:2");

    let out = env.ok(&["ls"]);
    assert_eq!(
        out,
        "  home  1 window\n\
         * ren   2 windows    ~/Projects/ren\n  \
         tax   0 windows    ~/Spaces/tax\n"
    );

    let json: Value = serde_json::from_str(&env.ok(&["ls", "--json"])).unwrap();
    assert_eq!(json[0]["name"], "home");
    assert_eq!(json[0]["dir"], Value::Null);
    assert_eq!(json[1]["name"], "ren");
    assert_eq!(json[1]["active"], true);
    assert_eq!(json[1]["windows"], 2);
    assert_eq!(json[1]["workspaces"], json!([1, 2]));
    assert!(json[1]["dir"].as_str().unwrap().ends_with("/Projects/ren"));
    assert_eq!(json[2]["left_at"], 0, "never left");
}

#[test]
fn ls_without_hyprland_still_lists() {
    let env = Env::new();
    env.make_space(&env.home().join("Projects/ren"), "");
    let out = env
        .command(&["ls"])
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "  home  -\n  ren   -            ~/Projects/ren\n"
    );
}

#[test]
fn ls_warns_about_broken_and_duplicate_spaces() {
    let env = Env::new();
    env.make_space(&env.home().join("Projects/a"), "name = \"same\"\n");
    env.make_space(&env.home().join("Projects/b"), "name = \"same\"\n");
    env.make_space(&env.home().join("Projects/c"), "name = \"Bad:Name\"\n");
    let out = env.run(&["ls"]);
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("define the space 'same'"), "{err}");
    assert!(err.contains("can't contain"), "{err}");
}

#[test]
fn go_without_a_name_opens_the_picker() {
    let env = Env::new();
    env.make_space(&env.home().join("Projects/ren"), "");
    env.make_space(&env.home().join("Projects/trip"), "");

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
    assert_eq!(env.active(), "trip:1");

    // Dismissing the picker does nothing.
    env.write_config("picker = \"exit 130\"\n");
    env.ok(&["go"]);
    assert_eq!(env.active(), "trip:1");
}
