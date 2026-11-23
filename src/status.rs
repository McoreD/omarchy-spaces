//! What the agent in a terminal is doing, as the bar and the sidebar show it,
//! from what its program reports to herdr-core (`core.rs`) through OSC 7501.

use serde::Serialize;

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

/// A terminal's status from its program's OSC 7501 word. `done` holds until
/// you've seen it, which `seen` says.
pub fn from_report(word: &str, seen: bool) -> Option<Status> {
    match word {
        "working" => Some(Status::Working),
        "blocked" | "error" => Some(Status::Waiting),
        "done" if !seen => Some(Status::Done),
        _ => None,
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
        assert_eq!(from_report("blocked", true), Some(Status::Waiting));
        assert_eq!(from_report("error", false), Some(Status::Waiting));
        assert_eq!(from_report("done", false), Some(Status::Done));
        assert_eq!(from_report("done", true), None, "seen");
        assert_eq!(from_report("idle", false), None);
        assert_eq!(from_report("thinking", false), None);
    }
}
