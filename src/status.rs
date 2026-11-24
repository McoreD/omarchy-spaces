//! What the agent in a terminal is doing, as the bar and the sidebar show it:
//! either reported directly through `space status` (stored per window under
//! `$XDG_RUNTIME_DIR/omarchy-spaces/status/`) or read from herdr-core
//! (`core.rs`) through OSC 7501.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::state;

/// An agent's status, least to most in need of you. Idle is no status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Busy: leave it.
    Working,
    /// Finished while you were looking elsewhere.
    Done,
    /// Stopped until you answer it (a permission prompt, a question), or
    /// stopped by an error.
    Waiting,
}

/// A terminal's status from its agent's word. `done` holds until you've seen
/// it, which `seen` says.
pub fn from_report(word: &str, seen: bool) -> Option<Status> {
    match word {
        "working" => Some(Status::Working),
        "waiting" | "blocked" | "error" => Some(Status::Waiting),
        "done" if !seen => Some(Status::Done),
        _ => None,
    }
}

/// Normalizes a status word given to `space status`.
pub fn parse_word(raw: &str) -> Result<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "working" => Ok("working"),
        "waiting" => Ok("waiting"),
        "blocked" => Ok("blocked"),
        "error" => Ok("error"),
        "done" => Ok("done"),
        "idle" | "clear" => Ok("idle"),
        _ => bail!("unknown status '{raw}': use working, waiting, done or idle"),
    }
}

/// Normalizes a Hyprland window address to lowercase `0x<hex>`.
pub fn normalize_address(raw: &str) -> Result<String> {
    let hex = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
    if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("'{raw}' isn't a Hyprland window address");
    }
    Ok(format!("0x{}", hex.to_ascii_lowercase()))
}

/// What a native terminal window last reported through `space status`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub word: String,
    /// Goes up each time a turn finishes (`done`), so a `done` seen can be
    /// told from the next.
    #[serde(default)]
    pub turn: u64,
    /// The last turn someone looked at by focusing the window.
    #[serde(default)]
    pub seen_turn: Option<u64>,
    /// The agent process under the terminal's shell that reported this, if
    /// any. Once it exits, a lingering `working`/`waiting` status clears
    /// itself so a killed agent doesn't leave a stale dot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_pid: Option<u32>,
}

impl Report {
    pub fn is_busy(&self) -> bool {
        matches!(
            self.word.as_str(),
            "working" | "waiting" | "blocked" | "error"
        )
    }

    pub fn is_stale(&self) -> bool {
        self.is_busy()
            && self
                .owner_pid
                .is_some_and(|pid| !Path::new(&format!("/proc/{pid}")).exists())
    }
}

fn report_path(dir: &Path, address: &str) -> std::path::PathBuf {
    dir.join(format!("{address}.json"))
}

fn read_report(path: &Path) -> Option<Report> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Saves a native status report for `window`, or clears it on `"idle"`.
pub fn write_report(dir: &Path, window: &str, word: &str, owner_pid: Option<u32>) -> Result<()> {
    let address = normalize_address(window)?;
    let path = report_path(dir, &address);
    if word == "idle" {
        match fs::remove_file(&path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        }
    }
    let existing = read_report(&path).unwrap_or_default();
    let turn = if word == "done" {
        existing.turn + 1
    } else {
        existing.turn
    };
    let report = Report {
        word: word.to_string(),
        turn,
        seen_turn: existing.seen_turn,
        owner_pid,
    };
    state::write_atomic(&path, &serde_json::to_string(&report)?)
}

/// Notes that you've looked at `window` on its turn `turn`, so `done` stays
/// cleared across restarts of `space watch` and in `space ls`.
pub fn mark_seen(dir: &Path, window: &str, turn: u64) -> Result<()> {
    let address = normalize_address(window)?;
    let path = report_path(dir, &address);
    let Some(mut report) = read_report(&path) else {
        return Ok(());
    };
    if report.turn != turn || report.seen_turn == Some(turn) {
        return Ok(());
    }
    report.seen_turn = Some(turn);
    state::write_atomic(&path, &serde_json::to_string(&report)?)
}

/// Every live native status report in `dir`, keyed by Hyprland window address.
pub fn load_reports(dir: &Path) -> HashMap<String, Report> {
    let Ok(entries) = fs::read_dir(dir) else {
        return HashMap::new();
    };
    let mut reports = HashMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let Some(address) = name.strip_suffix(".json") else {
            continue;
        };
        let Some(report) = read_report(&path) else {
            continue;
        };
        if report.is_stale() {
            continue;
        }
        reports.insert(address.to_string(), report);
    }
    reports
}

/// Drops status files whose windows have closed or whose busy owner exited.
pub fn prune_reports(dir: &Path, live_windows: &HashSet<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let Some(address) = name.strip_suffix(".json") else {
            continue;
        };
        let remove = !live_windows.contains(address)
            || read_report(&path).is_none_or(|report| report.is_stale());
        if remove {
            let _ = fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_what_needs_you_highest() {
        assert!(Status::Waiting > Status::Done && Status::Done > Status::Working);
    }

    #[test]
    fn reads_the_programs_own_words() {
        assert_eq!(from_report("working", false), Some(Status::Working));
        assert_eq!(from_report("waiting", false), Some(Status::Waiting));
        assert_eq!(from_report("blocked", true), Some(Status::Waiting));
        assert_eq!(from_report("error", false), Some(Status::Waiting));
        assert_eq!(from_report("done", false), Some(Status::Done));
        assert_eq!(from_report("done", true), None, "seen");
        assert_eq!(from_report("idle", false), None);
        assert_eq!(from_report("thinking", false), None);
    }

    #[test]
    fn parses_status_words_and_addresses() {
        assert_eq!(parse_word("Working").unwrap(), "working");
        assert_eq!(parse_word("waiting").unwrap(), "waiting");
        assert_eq!(parse_word("blocked").unwrap(), "blocked");
        assert_eq!(parse_word("error").unwrap(), "error");
        assert_eq!(parse_word("done").unwrap(), "done");
        assert_eq!(parse_word("clear").unwrap(), "idle");
        assert_eq!(parse_word("idle").unwrap(), "idle");
        assert!(parse_word("nope").is_err());

        assert_eq!(normalize_address("55A4").unwrap(), "0x55a4");
        assert_eq!(normalize_address("0x55A4").unwrap(), "0x55a4");
        assert!(normalize_address("").is_err());
        assert!(normalize_address("../etc").is_err());
    }

    #[test]
    fn round_trips_reports_turns_and_pruning() {
        let dir = tempfile::tempdir().unwrap();
        let status_dir = dir.path().join("status");

        write_report(&status_dir, "0x1", "working", None).unwrap();
        let reports = load_reports(&status_dir);
        assert_eq!(reports["0x1"].word, "working");
        assert_eq!(reports["0x1"].turn, 0);

        write_report(&status_dir, "0x1", "done", None).unwrap();
        let reports = load_reports(&status_dir);
        assert_eq!(reports["0x1"].word, "done");
        assert_eq!(reports["0x1"].turn, 1);
        assert_eq!(reports["0x1"].seen_turn, None);

        mark_seen(&status_dir, "0x1", 1).unwrap();
        let reports = load_reports(&status_dir);
        assert_eq!(reports["0x1"].seen_turn, Some(1));

        // Next turn bumps turn past seen_turn.
        write_report(&status_dir, "0x1", "working", None).unwrap();
        write_report(&status_dir, "0x1", "done", None).unwrap();
        let reports = load_reports(&status_dir);
        assert_eq!(reports["0x1"].turn, 2);
        assert_eq!(reports["0x1"].seen_turn, Some(1));

        // A busy report whose owner pid no longer exists is stale, while done stays.
        write_report(&status_dir, "0x2", "working", Some(u32::MAX)).unwrap();
        write_report(&status_dir, "0x3", "done", Some(u32::MAX)).unwrap();
        let reports = load_reports(&status_dir);
        assert!(!reports.contains_key("0x2"));
        assert!(reports.contains_key("0x3"));

        // Idle clears the file.
        write_report(&status_dir, "0x3", "idle", None).unwrap();
        assert!(!load_reports(&status_dir).contains_key("0x3"));

        // Pruning drops closed windows and stale reports.
        prune_reports(&status_dir, &HashSet::new());
        assert!(load_reports(&status_dir).is_empty());
    }
}
