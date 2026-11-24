//! Space ids, and the `<id>:n` workspace names that tie windows to spaces.
//!
//! The workspace name is the data: a window belongs to the space whose id
//! prefixes its workspace, and the active space is read from the focused
//! workspace every time. What a space is called is separate (`spaces.rs`),
//! so it can follow a folder without renaming workspaces. Keep the format
//! strict, since anything else is treated as `home`.

/// The implicit space holding the plain numbered workspaces.
pub const HOME: &str = "home";

/// Workspaces per space, as Omarchy gives every monitor 1-10.
pub const SLOTS: u8 = 10;

/// Whether `id` can be a space's id: lowercase letters, digits and `-`,
/// starting with a letter or digit, so no `:`, which separates it from the
/// workspace number. Spaces makes `s1`, `s2`, ...; workspaces named by
/// hand, or by an older version (`ren:1`), count too.
pub fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A workspace slot, 1-10.
pub fn parse_slot(raw: &str) -> Option<u8> {
    if raw.starts_with('0') {
        return None;
    }
    raw.parse::<u8>().ok().filter(|n| (1..=SLOTS).contains(n))
}

/// The Hyprland workspace name for a space's slot: `s1:3`.
pub fn workspace_name(space: &str, slot: u8) -> String {
    format!("{space}:{slot}")
}

/// Where a workspace sits in Spaces' terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// A plain numbered workspace, or anything not in `name:n` form. The slot
    /// is known for numbered ones, counted within the monitor's block of ten
    /// as Omarchy numbers them (workspace 13 is slot 3 of the second monitor).
    Home { slot: Option<u8> },
    /// A space's workspace, `name:n`.
    Space { name: String, slot: u8 },
}

impl Location {
    pub fn space(&self) -> &str {
        match self {
            Location::Home { .. } => HOME,
            Location::Space { name, .. } => name,
        }
    }

    pub fn slot(&self) -> Option<u8> {
        match self {
            Location::Home { slot } => *slot,
            Location::Space { slot, .. } => Some(*slot),
        }
    }
}

/// Reads a Hyprland workspace's id and name.
pub fn locate(id: i64, name: &str) -> Location {
    if let Some((space, slot)) = name.rsplit_once(':')
        && is_id(space)
        && space != HOME
        && let Some(slot) = parse_slot(slot)
    {
        return Location::Space {
            name: space.to_string(),
            slot,
        };
    }
    let slot = (id > 0).then(|| ((id - 1) % i64::from(SLOTS) + 1) as u8);
    Location::Home { slot }
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
        for good in ["s1", "ren", "tax-2026", "a", "2026"] {
            assert!(is_id(good), "{good}");
        }
        for bad in [
            "",
            "Ren",
            "ren:1",
            "my project",
            "-ren",
            "ré",
            &"x".repeat(65),
        ] {
            assert!(!is_id(bad), "{bad}");
        }
    }

    #[test]
    fn parses_slots() {
        assert_eq!(parse_slot("1"), Some(1));
        assert_eq!(parse_slot("10"), Some(10));
        for bad in ["0", "11", "01", "-1", "x", ""] {
            assert_eq!(parse_slot(bad), None, "{bad}");
        }
    }

    #[test]
    fn locates_workspaces() {
        assert_eq!(
            locate(-1337, "ren:3"),
            Location::Space {
                name: "ren".into(),
                slot: 3
            }
        );
        assert_eq!(
            locate(-1338, "tax-2026:10"),
            Location::Space {
                name: "tax-2026".into(),
                slot: 10
            }
        );
        assert_eq!(locate(3, "3"), Location::Home { slot: Some(3) });
        // Omarchy's second monitor owns 11-20.
        assert_eq!(locate(13, "13"), Location::Home { slot: Some(3) });
        assert_eq!(locate(20, "20"), Location::Home { slot: Some(10) });
        // Anything not strictly `name:n` is home.
        for name in ["chat", "Ren:1", "ren:0", "ren:11", "home:1", "ren:", ":1"] {
            assert_eq!(locate(-5, name), Location::Home { slot: None }, "{name}");
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
