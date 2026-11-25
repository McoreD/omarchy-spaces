//! Space ids, and the `<id>:n` workspace names that tie windows to spaces.
//!
//! The workspace name is the data: a window belongs to the space whose id
//! prefixes its workspace, and the active space is read from the focused
//! workspace every time. What a space is called is separate (`spaces.rs`),
//! so it can follow a folder without renaming workspaces. Keep the format
//! strict, since anything else is treated as `home`.

/// The implicit space holding the plain numbered workspaces.
pub const HOME: &str = "home";

/// Tabs per space on each monitor, as Omarchy gives every monitor
/// workspaces 1-10.
pub const SLOTS: u8 = 10;

/// The highest a workspace is numbered: the last of the 25th monitor's.
const LAST_KEY: u8 = 250;

/// Whether `id` can be a space's id: `s1`, `s2`, ... Only what Spaces makes
/// itself, so that a workspace you or another program named `web:1` is left
/// alone.
pub fn is_id(id: &str) -> bool {
    id.strip_prefix('s').is_some_and(|number| {
        !number.starts_with('0') && !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
    })
}

/// A tab's slot, the number it shows, 1-10.
pub fn parse_slot(raw: &str) -> Option<u8> {
    if raw.starts_with(['0', '+']) {
        return None;
    }
    raw.parse::<u8>().ok().filter(|n| (1..=SLOTS).contains(n))
}

/// A tab's key, which tells it from its space's others: what its workspace is
/// numbered. Each monitor has a block of ten, as Omarchy gives it for its
/// workspaces, so the second one's tab 3 has key 13.
pub fn parse_key(raw: &str) -> Option<u8> {
    if raw.starts_with(['0', '+']) {
        return None;
    }
    raw.parse::<u8>()
        .ok()
        .filter(|n| (1..=LAST_KEY).contains(n))
}

/// The slot a key shows as: 3 for 13.
pub fn slot_of(key: u8) -> u8 {
    (key - 1) % SLOTS + 1
}

/// Which monitor's block a key is in, from 0.
pub fn block_of(key: u8) -> u8 {
    (key - 1) / SLOTS
}

/// The key of a slot in a monitor's block.
pub fn key_in(block: u8, slot: u8) -> u8 {
    block.min(LAST_KEY / SLOTS - 1) * SLOTS + slot
}

/// The Hyprland workspace name for a space's tab: `s1:3`.
pub fn workspace_name(space: &str, key: u8) -> String {
    format!("{space}:{key}")
}

/// A tab's id: `s1:3`, `home:3`. Commands take it and print it, as what it's
/// made of, the key, is for nobody else to work with.
pub fn tab_id(space: &str, key: u8) -> String {
    format!("{space}:{key}")
}

/// The space and key of a tab's id.
pub fn parse_tab_id(id: &str) -> Option<(&str, u8)> {
    let (space, key) = id.split_once(':')?;
    (space == HOME || is_id(space)).then_some((space, parse_key(key)?))
}

/// Where a workspace sits in Spaces' terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// A plain numbered workspace, its number the key, or anything else
    /// that's not a space's, without one.
    Home { key: Option<u8> },
    /// A space's workspace, `s1:n`.
    Space { name: String, key: u8 },
}

impl Location {
    pub fn space(&self) -> &str {
        match self {
            Location::Home { .. } => HOME,
            Location::Space { name, .. } => name,
        }
    }

    pub fn key(&self) -> Option<u8> {
        match self {
            Location::Home { key } => *key,
            Location::Space { key, .. } => Some(*key),
        }
    }

    pub fn slot(&self) -> Option<u8> {
        self.key().map(slot_of)
    }
}

/// Reads a Hyprland workspace's id and name.
pub fn locate(id: i64, name: &str) -> Location {
    if let Some((space, key)) = name.rsplit_once(':')
        && is_id(space)
        && let Some(key) = parse_key(key)
    {
        return Location::Space {
            name: space.to_string(),
            key,
        };
    }
    let key = u8::try_from(id).ok().filter(|id| *id > 0);
    Location::Home { key }
}

/// The closest known name to a mistyped one, if any is close enough to be
/// what was meant.
pub fn suggest<'a>(wanted: &str, known: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let mut best: Option<(usize, &str)> = None;
    for candidate in known {
        let edits = levenshtein(wanted, candidate);
        // A prefix is a likely match however long the rest is, but the name
        // needing fewest edits still wins.
        let prefix = candidate.starts_with(wanted) || wanted.starts_with(candidate);
        let limit = (wanted.len().max(candidate.len()) / 3).max(2);
        if (prefix || edits <= limit) && best.is_none_or(|(b, _)| edits < b) {
            best = Some((edits, candidate));
        }
    }
    best.map(|(_, name)| name)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if ca == *cb {
                diagonal
            } else {
                1 + diagonal.min(above).min(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_ids() {
        for good in ["s1", "s9", "s10", "s123"] {
            assert!(is_id(good), "{good}");
        }
        for bad in [
            "", "s", "s0", "s01", "S1", "1", "s1a", "s-1", "ss1", "ren", "home", "s1:1", "s١",
        ] {
            assert!(!is_id(bad), "{bad}");
        }
    }

    #[test]
    fn parses_slots() {
        assert_eq!(parse_slot("1"), Some(1));
        assert_eq!(parse_slot("10"), Some(10));
        for bad in ["0", "11", "01", "-1", "+1", "x", ""] {
            assert_eq!(parse_slot(bad), None, "{bad}");
        }
    }

    #[test]
    fn keys_are_slots_in_a_monitors_block() {
        assert_eq!(parse_key("13"), Some(13));
        for bad in ["0", "251", "013", "+13", "x", ""] {
            assert_eq!(parse_key(bad), None, "{bad}");
        }
        for (key, block, slot) in [
            (1, 0, 1),
            (10, 0, 10),
            (11, 1, 1),
            (20, 1, 10),
            (250, 24, 10),
        ] {
            assert_eq!((block_of(key), slot_of(key)), (block, slot), "{key}");
            assert_eq!(key_in(block, slot), key);
        }
    }

    #[test]
    fn locates_workspaces() {
        assert_eq!(
            locate(-1337, "s1:3"),
            Location::Space {
                name: "s1".into(),
                key: 3
            }
        );
        assert_eq!(
            locate(-1338, "s12:10"),
            Location::Space {
                name: "s12".into(),
                key: 10
            }
        );
        // The second monitor's are 11-20, as Omarchy has it.
        let second = locate(-1339, "s1:13");
        assert_eq!((second.key(), second.slot()), (Some(13), Some(3)));
        assert_eq!(locate(3, "3"), Location::Home { key: Some(3) });
        assert_eq!(locate(13, "13").slot(), Some(3));
        assert_eq!(locate(20, "20").slot(), Some(10));
        // Anything not strictly `s1:n` is home.
        for name in [
            "chat", "ren:1", "S1:1", "s1:0", "s1:251", "home:1", "s1:", ":1",
        ] {
            assert_eq!(locate(-5, name), Location::Home { key: None }, "{name}");
        }
    }

    #[test]
    fn reads_tab_ids() {
        for (space, key) in [("s1", 12), (HOME, 3)] {
            assert_eq!(parse_tab_id(&tab_id(space, key)), Some((space, key)));
        }
        for bad in ["s1", "s1:", "s1:0", "s1:251", "ren:1", "work:2:2", "3"] {
            assert_eq!(parse_tab_id(bad), None, "{bad}");
        }
    }

    #[test]
    fn suggests_close_names() {
        let known = ["ren", "renspace", "tax-2026", "home"];
        assert_eq!(suggest("rn", known), Some("ren"));
        assert_eq!(suggest("rensapce", known), Some("renspace"));
        assert_eq!(suggest("tax", known), Some("tax-2026"));
        assert_eq!(suggest("hmoe", known), Some("home"));
        assert_eq!(suggest("zzzzzz", known), None);
        // Of two prefixes, the closer one.
        assert_eq!(suggest("renspace2", known), Some("renspace"));
    }

    #[test]
    fn levenshtein_distance() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("same", "same"), 0);
    }
}
