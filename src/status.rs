//! What the agent in a terminal is doing, as the bar and the sidebar show it,
//! from what it notes about itself (`agent/`).

use std::collections::HashMap;

use serde::Serialize;

use crate::terminal::Agent;

/// An agent's status, least to most in need of you. Idle is no status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Busy: leave it.
    Working,
    /// Finished while you were looking elsewhere.
    Done,
    /// Stopped until you answer it (a permission prompt, a question).
    Waiting,
}

/// A terminal's status from its agent's word. `done` holds until you've seen
/// it, which `seen` says.
pub fn from_report(word: &str, seen: bool) -> Option<Status> {
    match word {
        "working" => Some(Status::Working),
        "waiting" => Some(Status::Waiting),
        "done" if !seen => Some(Status::Done),
        _ => None,
    }
}

/// What agents were doing when last read. They only say they're
/// idle, so whether one has finished, or not started, is in what it did before.
#[derive(Debug, Default)]
pub struct Turns {
    /// Each one's word, and the turns it has finished, by its key.
    last: HashMap<String, (String, u64)>,
}

impl Turns {
    /// Brings `agents`, as just read, up to date.
    pub fn follow<'a>(&mut self, agents: impl Iterator<Item = &'a mut Agent>) {
        let mut now = HashMap::new();
        for agent in agents {
            let key = agent.process.clone();
            let (before, mut turn) = self.last.get(&key).cloned().unwrap_or_default();
            match (agent.word.as_str(), before.as_str()) {
                ("idle", "working" | "waiting") => {
                    turn += 1;
                    agent.word = "done".into();
                }
                ("idle", "done") | ("", _) => agent.word = before,
                _ => {}
            }
            agent.turn = turn;
            now.insert(key, (agent.word.clone(), turn));
        }
        self.last = now;
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
        assert_eq!(from_report("done", false), Some(Status::Done));
        assert_eq!(from_report("done", true), None, "seen");
        assert_eq!(from_report("idle", false), None);
        assert_eq!(from_report("thinking", false), None);
    }

    #[test]
    fn an_agent_is_done_once_it_stops_working() {
        let mut turns = Turns::default();
        let mut follow = |word: &str| {
            let mut agent = Agent::new("7/42", word);
            turns.follow([&mut agent].into_iter());
            (agent.word, agent.turn)
        };
        let is = |word: &str, turn| (word.to_string(), turn);
        assert_eq!(follow("idle"), is("idle", 0), "it hasn't started");
        assert_eq!(follow("working"), is("working", 0));
        assert_eq!(follow("waiting"), is("waiting", 0));
        assert_eq!(follow(""), is("waiting", 0), "nothing new");
        assert_eq!(follow("working"), is("working", 0), "you answered");
        assert_eq!(follow("idle"), is("done", 1));
        assert_eq!(follow("idle"), is("done", 1));
        assert_eq!(follow(""), is("done", 1));
        assert_eq!(follow("working"), is("working", 1));
        assert_eq!(follow("idle"), is("done", 2));

        // It ends, and so does what it did.
        turns.follow(std::iter::empty());
        let mut agent = Agent::new("7/42", "idle");
        turns.follow([&mut agent].into_iter());
        assert_eq!((agent.word.as_str(), agent.turn), ("idle", 0));
    }
}
