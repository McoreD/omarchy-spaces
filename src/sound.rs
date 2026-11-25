//! Sounds when an agent needs you, as Herdr plays them: one when an agent
//! starts waiting for you (blocked, or stopped by an error), one when it
//! finishes while you aren't looking at its window. `space watch --sounds`,
//! which the plugin runs, plays them.
//!
//! The sounds are Herdr's (`assets/sounds/`, Apache-2.0), built into the
//! binary and written out the first time they play, so players can open
//! them. `[sounds]` in the config can name others.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::{self, Config};
use crate::paths;
use crate::status::Status;

const DONE: &[u8] = include_bytes!("../assets/sounds/done.mp3");
const WAITING: &[u8] = include_bytes!("../assets/sounds/request.mp3");

/// Players that decode MP3 (and most else), tried in order. Never bare
/// `aplay`, which would play the MP3's bytes as noise.
const PLAYERS: &[(&str, &[&str])] = &[
    ("pw-play", &[]),
    ("paplay", &[]),
    ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
    ("mpv", &["--no-video", "--really-quiet"]),
];

/// Which sound a change of an agent's status word calls for, as Herdr
/// decides: starting to need you always does; finishing does only when
/// you're not `looking` at it.
pub fn alert(before: &str, now: &str, looking: bool) -> Option<Status> {
    let needs_you = |word: &str| matches!(word, "waiting" | "blocked" | "error");
    let busy = |word: &str| matches!(word, "working" | "waiting" | "blocked" | "error");
    if needs_you(now) && !needs_you(before) {
        Some(Status::Waiting)
    } else if busy(before) && !now.is_empty() && !busy(now) && !looking {
        Some(Status::Done)
    } else {
        None
    }
}

/// Plays the sound for `alert`, unless the config turns sounds off. It
/// plays on its own, so this returns at once.
pub fn play(config: &Config, alert: Status) {
    let sounds = &config.sounds;
    if !sounds.enabled {
        return;
    }
    let (chosen, name, bundled) = match alert {
        Status::Waiting => (sounds.waiting.as_deref(), "request.mp3", WAITING),
        Status::Done => (sounds.done.as_deref(), "done.mp3", DONE),
        Status::Working => return,
    };
    let file = match chosen {
        Some(file) => crate::paths::expand(file),
        None => match write_bundled(&paths::runtime_dir(), name, bundled) {
            Ok(file) => file,
            Err(e) => {
                eprintln!("space: warning: can't write the {name} sound: {e}");
                return;
            }
        },
    };
    let Some((player, args)) = PLAYERS.iter().find(|(p, _)| config::on_path(p)) else {
        eprintln!("space: warning: no sound player (pw-play, paplay, ffplay or mpv)");
        return;
    };
    match Command::new(player)
        .args(*args)
        .arg(Path::new(&file))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        // Collected once it ends, so it leaves no zombie.
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(e) => eprintln!("space: warning: can't play {}: {e}", file.display()),
    }
}

/// A built-in sound as a file in `dir`, written unless it's there already
/// (as these bytes: a new `space` may bring new sounds).
fn write_bundled(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let path = dir.join(name);
    if std::fs::read(&path).ok().as_deref() != Some(bytes) {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(".{name}.{}", std::process::id()));
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &path)?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_the_built_in_sounds_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_bundled(dir.path(), "done.mp3", DONE).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), DONE);
        assert!(
            DONE.starts_with(b"\xff\xfb") || DONE.starts_with(b"ID3"),
            "an MP3"
        );
        // A changed file is put right.
        std::fs::write(&path, b"old").unwrap();
        write_bundled(dir.path(), "done.mp3", DONE).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), DONE);
    }

    #[test]
    fn rings_as_herdr_does() {
        assert_eq!(
            alert("working", "waiting", true),
            Some(Status::Waiting),
            "even looking"
        );
        assert_eq!(
            alert("working", "blocked", true),
            Some(Status::Waiting),
            "even looking"
        );
        assert_eq!(alert("", "error", false), Some(Status::Waiting));
        assert_eq!(alert("blocked", "error", false), None, "still needs you");
        assert_eq!(alert("waiting", "blocked", false), None, "still needs you");
        assert_eq!(alert("working", "done", false), Some(Status::Done));
        assert_eq!(alert("waiting", "done", false), Some(Status::Done));
        assert_eq!(alert("blocked", "idle", false), Some(Status::Done));
        assert_eq!(alert("working", "done", true), None, "you saw it");
        assert_eq!(alert("idle", "done", false), None, "it wasn't working");
        assert_eq!(
            alert("working", "", false),
            None,
            "gone quiet, not finished"
        );
        assert_eq!(alert("done", "working", false), None);
    }
}
