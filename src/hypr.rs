//! A client for Hyprland's IPC socket, the same one `hyprctl` talks to.
//!
//! Talking to the socket directly instead of shelling out to `hyprctl` keeps
//! a keypress to about a millisecond. Hyprland 0.56 is configured in Lua, and
//! its `dispatch` request takes a Lua expression, which it wraps in
//! `hl.dispatch(...)`.

use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;

#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub windows: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Client {
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub pid: i64,
    pub workspace: WorkspaceRef,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceRef {
    pub id: i64,
    pub name: String,
}

pub struct Hypr {
    socket: PathBuf,
}

impl Hypr {
    /// The running Hyprland instance, from `HYPRLAND_INSTANCE_SIGNATURE`.
    pub fn from_env() -> Result<Self> {
        let signature = env::var("HYPRLAND_INSTANCE_SIGNATURE")
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow!("Hyprland isn't running (HYPRLAND_INSTANCE_SIGNATURE is unset)")
            })?;
        let runtime = env::var_os("XDG_RUNTIME_DIR")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", current_uid())));
        Ok(Self {
            socket: runtime.join("hypr").join(signature).join(".socket.sock"),
        })
    }

    /// Hyprland's events (`openwindow>>...`), one line each, read on a
    /// thread of their own. The channel closes when Hyprland does.
    pub fn events(&self) -> Result<mpsc::Receiver<String>> {
        let path = self.socket.with_file_name(".socket2.sock");
        let stream = UnixStream::connect(&path)
            .with_context(|| format!("can't reach Hyprland's events at {}", path.display()))?;
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(receive)
    }

    /// Sends one request and returns Hyprland's reply.
    pub fn request(&self, request: &str) -> Result<String> {
        let mut stream = UnixStream::connect(&self.socket)
            .with_context(|| format!("can't reach Hyprland at {}", self.socket.display()))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(request.as_bytes())?;
        let mut reply = String::new();
        stream
            .read_to_string(&mut reply)
            .with_context(|| format!("no reply from Hyprland to '{request}'"))?;
        Ok(reply)
    }

    fn query<T: DeserializeOwned>(&self, what: &str) -> Result<T> {
        let reply = self.request(&format!("j/{what}"))?;
        serde_json::from_str(&reply).with_context(|| {
            format!(
                "unexpected reply from Hyprland to '{what}': {}",
                reply.trim()
            )
        })
    }

    pub fn active_workspace(&self) -> Result<Workspace> {
        self.query("activeworkspace")
    }

    pub fn workspaces(&self) -> Result<Vec<Workspace>> {
        self.query("workspaces")
    }

    pub fn clients(&self) -> Result<Vec<Client>> {
        self.query("clients")
    }

    /// Whether any window has focus. Hyprland answers `{}` when none does.
    pub fn has_active_window(&self) -> Result<bool> {
        let window: serde_json::Value = self.query("activewindow")?;
        Ok(window.get("address").is_some())
    }

    /// The address of the focused window, if a window has focus.
    pub fn active_window_address(&self) -> Result<Option<String>> {
        let window: serde_json::Value = self.query("activewindow")?;
        Ok(window
            .get("address")
            .and_then(serde_json::Value::as_str)
            .map(String::from))
    }

    /// The process of the focused window, if a window has focus.
    pub fn active_window_pid(&self) -> Result<Option<u32>> {
        let window: serde_json::Value = self.query("activewindow")?;
        Ok(window
            .get("pid")
            .and_then(serde_json::Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok())
            .filter(|&pid| pid > 0))
    }

    /// Runs a Lua dispatch expression, such as
    /// `hl.dsp.focus({ workspace = "name:ren:1" })`.
    pub fn dispatch(&self, expression: &str) -> Result<()> {
        let reply = self.request(&format!("dispatch {expression}"))?;
        let reply = reply.trim();
        if reply == "ok" {
            Ok(())
        } else {
            bail!("Hyprland refused `{expression}`: {reply}")
        }
    }
}

/// A Lua string literal.
pub fn lua_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Focus a workspace by Hyprland selector (`name:ren:1`, `3`), creating it if
/// it doesn't exist.
pub fn focus_workspace(selector: &str) -> String {
    format!("hl.dsp.focus({{ workspace = {} }})", lua_string(selector))
}

/// Move the focused window to a workspace by selector.
pub fn move_window(selector: &str, follow: bool) -> String {
    format!(
        "hl.dsp.window.move({{ workspace = {}, follow = {follow} }})",
        lua_string(selector)
    )
}

/// Run a shell command, its first window placed on `workspace`, as
/// `name:s1:1 silent`: there, without going there, and drawn whole at once,
/// without its open animation (`animate_again` brings animations back).
pub fn exec_on(command: &str, workspace: &str) -> String {
    format!(
        "hl.dsp.exec_cmd({}, {{ workspace = {}, no_anim = true }})",
        lua_string(command),
        lua_string(workspace)
    )
}

/// Lets a window placed by `exec_on` animate again, as others do. A value
/// set this way outranks the exec rule. (Lua's `false` reads as missing to
/// Hyprland, so it's the string.)
pub fn animate_again(address: &str) -> String {
    format!(
        "hl.dsp.window.set_prop({{ prop = \"no_anim\", value = \"false\", window = {} }})",
        lua_string(&format!("address:{address}"))
    )
}

/// Ask a window to close, by its address, as `super+w` does: the app can
/// still ask about unsaved work.
pub fn close_window(address: &str) -> String {
    format!(
        "hl.dsp.window.close({{ window = {} }})",
        lua_string(&format!("address:{address}"))
    )
}

fn current_uid() -> u32 {
    // Only reached when XDG_RUNTIME_DIR is unset; read the uid from /proc
    // rather than pulling in libc for one call.
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("Uid:"))
                .and_then(|ids| ids.split_whitespace().next()?.parse().ok())
        })
        .unwrap_or(1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_dispatch_expressions() {
        assert_eq!(
            focus_workspace("name:ren:1"),
            r#"hl.dsp.focus({ workspace = "name:ren:1" })"#
        );
        assert_eq!(
            move_window("name:ren:2", false),
            r#"hl.dsp.window.move({ workspace = "name:ren:2", follow = false })"#
        );
        assert_eq!(lua_string(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(
            exec_on("kitty", "name:s1:1 silent"),
            r#"hl.dsp.exec_cmd("kitty", { workspace = "name:s1:1 silent", no_anim = true })"#
        );
        assert_eq!(
            animate_again("0x55a4"),
            r#"hl.dsp.window.set_prop({ prop = "no_anim", value = "false", window = "address:0x55a4" })"#
        );
        assert_eq!(
            close_window("0x55a4"),
            r#"hl.dsp.window.close({ window = "address:0x55a4" })"#
        );
    }
}
