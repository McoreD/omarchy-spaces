//! Browsers' windows, to open again after a restart. How each browser notes
//! what its windows hold, and is told to open it, is its adapter's alone to
//! know: a file here, named in `ADAPTERS`. Windows of a browser without one
//! aren't opened again. Which of Hyprland's windows is which, the titles tell,
//! and for windows that share one, the order they were opened in.
//!
//! Closed one by one, as Omarchy does before a restart, a browser itself would
//! only bring back the last.

mod chromium;
mod desktop;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

const ADAPTERS: &[Adapter] = &[chromium::ADAPTER];

/// How long a window must have gone without its browser noting it to be taken
/// for incognito. It notes them every few seconds.
const UNNOTED: Duration = Duration::from_secs(10);

pub(super) struct Adapter {
    /// Whether this adapter handles a saved browser with this `name`.
    pub owns: fn(&str) -> bool,
    /// The session files `pid` has open, if it belongs to this browser.
    pub files: fn(u32) -> Vec<SessionFile>,
    /// What one session file says the browser's windows hold.
    pub read: fn(&SessionFile) -> Vec<Noted>,
    /// Separates window kinds that share a process (e.g. regular windows vs.
    /// web apps), so matching only pairs windows within the same group.
    pub window_group: fn(&Window) -> u8,
    /// The site name of a web-app window class, if this adapter knows it.
    pub web_app_site: fn(&str) -> Option<&str>,
    /// Hyprland window-rule class regex for restoring a saved window.
    pub class_pattern: fn(&Browser) -> String,
    /// Whether a Hyprland window class matches a saved window.
    pub is_class: fn(&Browser, &str) -> bool,
    /// The shell command that opens a saved window again.
    pub command: fn(&Browser, &str) -> Option<String>,
    /// Whether opening its windows again is left to the browser.
    pub opens_itself: fn(&Browser) -> bool,
}

/// A session file a browser process has open.
pub(super) struct SessionFile {
    pub path: PathBuf,
    pub name: &'static str,
    pub profile: String,
    pub group: u8,
}

/// What a browser window holds, as saved in `session.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Browser {
    /// Which browser, by its desktop entry: `chromium`, `google-chrome`, …
    pub name: String,
    /// The window's class: `chromium`, or a web app's
    /// `chrome-app.hey.com__-Default`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub class: String,
    /// Its profile, if the browser uses one: `Default`, `Profile 1`, …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Whether it's a web app's, without tabs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub app: bool,
    /// Its tabs, in order.
    pub urls: Vec<String>,
    /// The tab that shows.
    #[serde(default)]
    pub tab: usize,
}

impl Browser {
    fn adapter(&self) -> Option<&'static Adapter> {
        ADAPTERS.iter().find(|adapter| (adapter.owns)(&self.name))
    }

    /// The class of its windows, for Hyprland's rules.
    pub fn class_pattern(&self) -> String {
        match self.adapter() {
            Some(adapter) => (adapter.class_pattern)(self),
            None if self.class.is_empty() => format!("^{}$", self.name),
            None => format!("^{}$", self.class.replace('.', "\\.")),
        }
    }

    /// Whether a window of this class can be it.
    pub fn is_class(&self, class: &str) -> bool {
        match self.adapter() {
            Some(adapter) => (adapter.is_class)(self, class),
            None if self.class.is_empty() => class == self.name,
            None => class == self.class,
        }
    }

    /// The command that opens it again, or `None` when there's nothing to
    /// open, or nothing to open it with.
    pub fn command(&self, launcher: &str) -> Option<String> {
        let adapter = self.adapter()?;
        (adapter.command)(self, launcher)
    }

    /// Whether opening it again is left to the browser.
    pub fn opens_itself(&self) -> bool {
        self.adapter()
            .is_some_and(|adapter| (adapter.opens_itself)(self))
    }
}

/// The site name of a web-app window's class, if any browser adapter knows it.
pub fn web_app_site(class: &str) -> Option<&str> {
    ADAPTERS.iter().find_map(|a| (a.web_app_site)(class))
}

/// A window of Hyprland's.
#[derive(Debug, Clone)]
pub struct Window {
    pub address: String,
    pub pid: u32,
    pub class: String,
    pub title: String,
    /// Goes up with each window opened.
    pub opened: u64,
}

/// A window as its browser notes it while running: what to save, plus its
/// runtime id and title for matching it to Hyprland's windows.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Noted {
    pub id: i32,
    pub browser: Browser,
    pub title: String,
}

/// What the browser windows hold, kept current.
#[derive(Default)]
pub struct Browsers {
    files: HashMap<PathBuf, (Option<SystemTime>, Vec<Noted>)>,
    by_address: HashMap<String, Browser>,
    /// Runtime window ids last matched to each Hyprland address.
    window_ids: HashMap<String, i32>,
    /// Windows a browser doesn't note though it notes all others, since when.
    unnoted: HashMap<String, Instant>,
}

impl Browsers {
    /// Those known before this started, by address.
    pub fn new(known: HashMap<String, Browser>) -> Self {
        Self {
            files: HashMap::new(),
            by_address: known,
            window_ids: HashMap::new(),
            unnoted: HashMap::new(),
        }
    }

    pub fn get(&self, address: &str) -> Option<&Browser> {
        self.by_address.get(address)
    }

    /// What they hold is none of ours.
    pub fn forget(&mut self) {
        *self = Self::default();
    }

    /// Reads what changed. A window that can't be told now, as while it
    /// closes, keeps what it held.
    pub fn refresh(&mut self, windows: &[Window], now: Instant) {
        self.by_address
            .retain(|address, _| windows.iter().any(|w| w.address == *address));
        self.window_ids
            .retain(|address, _| windows.iter().any(|w| w.address == *address));
        self.unnoted
            .retain(|address, _| windows.iter().any(|w| w.address == *address));
        let pids: HashSet<u32> = windows.iter().map(|w| w.pid).collect();
        let mut open = HashSet::new();
        for pid in pids {
            for adapter in ADAPTERS {
                let files = (adapter.files)(pid);
                if files.is_empty() {
                    continue;
                }
                let groups: BTreeSet<u8> = files.iter().map(|file| file.group).collect();
                for group in groups {
                    let mut noted: Vec<Noted> = Vec::new();
                    for file in files.iter().filter(|file| file.group == group) {
                        open.insert(file.path.clone());
                        noted.extend(self.read(adapter, file).iter().cloned());
                    }
                    let windows: Vec<&Window> = windows
                        .iter()
                        .filter(|w| w.pid == pid && (adapter.window_group)(w) == group)
                        .collect();
                    let incognito: HashSet<String> = self
                        .unnoted
                        .iter()
                        .filter(|(_, since)| now.duration_since(**since) >= UNNOTED)
                        .map(|(address, _)| address.clone())
                        .collect();
                    let matched = match_up(&windows, &noted, &self.window_ids, &incognito);
                    let all = !noted.is_empty() && matched.len() == noted.len();
                    for window in windows {
                        let address = &window.address;
                        if matched.contains_key(address) || self.by_address.contains_key(address) {
                            self.unnoted.remove(address);
                        } else if all {
                            self.unnoted.entry(address.clone()).or_insert(now);
                        } else if !incognito.contains(address) {
                            self.unnoted.remove(address);
                        }
                    }
                    for (address, (id, browser)) in matched {
                        self.window_ids.insert(address.clone(), id);
                        self.by_address.insert(address, browser);
                    }
                }
            }
        }
        self.files.retain(|path, _| open.contains(path));
    }

    fn read(&mut self, adapter: &Adapter, file: &SessionFile) -> &[Noted] {
        let at = std::fs::metadata(&file.path)
            .and_then(|m| m.modified())
            .ok();
        let entry = self.files.entry(file.path.clone()).or_default();
        if entry.0 != at || at.is_none() {
            *entry = (at, (adapter.read)(file));
        }
        &entry.1
    }
}

/// What a browser puts after the titles of its windows: ` - Chromium`. It's
/// the same after each, so the least of what follows a page's title there.
fn suffix(windows: &[&Window], noted: &[Noted]) -> String {
    windows
        .iter()
        .flat_map(|w| {
            noted
                .iter()
                .filter(|n| !n.title.is_empty())
                .filter_map(|n| w.title.strip_prefix(&n.title))
        })
        .filter(|rest| rest.starts_with(" - "))
        .min_by_key(|rest| rest.len())
        .unwrap_or_default()
        .to_string()
}

/// Which of Hyprland's windows hold what: by a title only one of each has,
/// else as before, else by the order they were opened in, when that leaves
/// as many of each, the `incognito` ones aside.
fn match_up(
    windows: &[&Window],
    noted: &[Noted],
    before: &HashMap<String, i32>,
    incognito: &HashSet<String>,
) -> HashMap<String, (i32, Browser)> {
    let suffix = suffix(windows, noted);
    let mut windows: Vec<&Window> = windows.to_vec();
    let mut noted: Vec<&Noted> = noted.iter().collect();
    let mut matched = HashMap::new();

    let titled = |w: &Window, n: &Noted| {
        let title = w.title.strip_suffix(&suffix).unwrap_or(&w.title);
        !n.title.is_empty() && (title == n.title || w.title == n.title)
    };
    let by_title: Vec<(String, i32)> = windows
        .iter()
        .filter_map(|w| {
            let mut same = noted.iter().filter(|n| titled(w, n));
            let one = same.next()?;
            let alone = same.next().is_none()
                && windows.iter().filter(|other| titled(other, one)).count() == 1;
            alone.then(|| (w.address.clone(), one.id))
        })
        .collect();
    let mut take =
        |pairs: Vec<(String, i32)>, windows: &mut Vec<&Window>, noted: &mut Vec<&Noted>| {
            for (address, id) in pairs {
                if let Some(i) = noted.iter().position(|n| n.id == id) {
                    let mut browser = noted.remove(i).browser.clone();
                    if let Some(i) = windows.iter().position(|w| w.address == address) {
                        browser.class = windows.remove(i).class.clone();
                    }
                    matched.insert(address, (id, browser));
                }
            }
        };
    take(by_title, &mut windows, &mut noted);

    let as_before = windows
        .iter()
        .filter_map(|w| Some((w.address.clone(), *before.get(&w.address)?)))
        .collect();
    take(as_before, &mut windows, &mut noted);

    windows.retain(|w| !incognito.contains(&w.address));
    if windows.len() == noted.len() {
        windows.sort_by_key(|w| w.opened);
        noted.sort_by_key(|n| n.id);
        let in_order = windows
            .iter()
            .zip(&noted)
            .map(|(w, n)| (w.address.clone(), n.id))
            .collect();
        take(in_order, &mut windows, &mut noted);
    }
    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn browser(urls: &[&str], tab: usize) -> Browser {
        Browser {
            name: "chromium".into(),
            class: String::new(),
            profile: Some("Default".into()),
            app: false,
            urls: urls.iter().map(|u| u.to_string()).collect(),
            tab,
        }
    }

    #[test]
    fn tells_windows_by_title_then_by_before_then_by_order() {
        let window = |address: &str, title: &str, opened| Window {
            address: address.into(),
            pid: 1,
            class: "chromium".into(),
            title: title.into(),
            opened,
        };
        let noted = |id, title: &str| Noted {
            id,
            browser: browser(&["https://x/"], 0),
            title: title.into(),
        };
        let pair = |windows: &[&Window], all: &[Noted], before: &HashMap<String, i32>| {
            let mut found: Vec<(String, i32)> = match_up(windows, all, before, &HashSet::new())
                .into_iter()
                .map(|(address, (id, _))| (address, id))
                .collect();
            found.sort();
            found
        };
        let (a, b, c) = (
            window("a", "Mail - Chromium", 3),
            window("b", "New Tab - Chromium", 1),
            window("c", "New Tab - Chromium", 2),
        );
        let all = [noted(1, "Mail"), noted(2, "New Tab"), noted(3, "New Tab")];
        let none = HashMap::new();
        assert_eq!(
            pair(&[&a, &b, &c], &all, &none),
            [("a".into(), 1), ("b".into(), 2), ("c".into(), 3)]
        );
        let before = HashMap::from([("c".to_string(), 2)]);
        assert_eq!(
            pair(&[&a, &b, &c], &all, &before),
            [("a".into(), 1), ("b".into(), 3), ("c".into(), 2)]
        );
        // One of them is incognito: not noted, so not told apart.
        assert_eq!(pair(&[&a, &b, &c], &all[..2], &none), [("a".into(), 1)]);
        // Once it's known to be, it is.
        let found = match_up(
            &[&a, &b, &c],
            &all[..2],
            &none,
            &HashSet::from(["b".into()]),
        );
        assert_eq!(found["c"].0, 2);
    }

    #[test]
    fn tells_what_follows_the_titles_itself() {
        let window = |address: &str, class: &str, title: &str| Window {
            address: address.into(),
            pid: 1,
            class: class.into(),
            title: title.into(),
            opened: 0,
        };
        let noted = |id, title: &str| Noted {
            id,
            browser: browser(&["https://x/"], 0),
            title: title.into(),
        };
        // One page's title starts as another's does.
        let a = window("a", "google-chrome", "Mail - Inbox - Google Chrome");
        let b = window("b", "google-chrome", "Mail - Google Chrome");
        let all = [noted(1, "Mail"), noted(2, "Mail - Inbox")];
        let found = match_up(&[&a, &b], &all, &HashMap::new(), &HashSet::new());
        assert_eq!((found["a"].0, found["b"].0), (2, 1));
        assert_eq!(found["a"].1.class, "google-chrome");
    }

    #[test]
    fn opens_where_windows_of_its_class_do() {
        let mut b = browser(&["https://x/"], 0);
        assert_eq!(b.class_pattern(), "^chromium$");
        assert!(b.is_class("chromium") && !b.is_class("google-chrome"));
        b.class = "google-chrome".into();
        assert_eq!(b.class_pattern(), "^google-chrome$");
        assert!(b.is_class("google-chrome") && !b.is_class("chromium"));
        assert!(!b.is_class("chrome-x.com__-Default"));
        b.app = true;
        b.class = "brave-x.com__-Default".into();
        assert_eq!(b.class_pattern(), "^brave-(.+__.*|[a-p]{32})-.+$");
        assert!(b.is_class("brave-y.com__z-Default"));
        assert!(!b.is_class("chrome-y.com__z-Default") && !b.is_class("brave-browser"));
        b.class = String::new();
        assert!(b.is_class("chrome-y.com__z-Default"));
    }
}
