//! Browsers' windows, to open again after a restart. How a browser notes what
//! its windows hold, and is told to open it, is its adapter's alone to know
//! (`chromium.rs`); windows of one without aren't opened again. Which of
//! Hyprland's windows is which, the titles tell, and for windows that share
//! one, the order they were opened in.
//!
//! Closed one by one, as Omarchy does before a restart, a browser itself would
//! only bring back the last.

mod chromium;
mod desktop;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

/// How long a window must have gone without its browser noting it to be taken
/// for incognito. It notes them every few seconds.
const UNNOTED: Duration = Duration::from_secs(10);

/// What a browser window holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Browser {
    /// Which browser, by its desktop entry: `chromium`. Sessions from before
    /// it was kept are Chromium's.
    #[serde(default = "chromium_entry")]
    pub name: String,
    /// The window's class: `chromium`, or a web app's
    /// `chrome-app.hey.com__-Default`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub class: String,
    /// The browser's number for it, while it runs.
    pub window: i32,
    /// Its profile's folder: `Default`.
    pub profile: String,
    /// Whether it's a web app's, without tabs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub app: bool,
    /// Its tabs, in order.
    pub urls: Vec<String>,
    /// The tab that shows.
    #[serde(default)]
    pub tab: usize,
}

fn chromium_entry() -> String {
    "chromium".into()
}

impl Browser {
    /// The class of its windows, for Hyprland's rules. A web app's is after
    /// the page it's opened on, which may be another by now.
    pub fn class_pattern(&self) -> String {
        match web_app(&self.class) {
            Some((family, _)) => format!("^{family}-(.+__.*|[a-p]{{32}})-.+$"),
            None if self.app => "^chrome-.+$".into(),
            None if self.class.is_empty() => format!("^{}$", self.name),
            None => format!("^{}$", self.class.replace('.', "\\.")),
        }
    }

    /// Whether a window of this class can be it.
    pub fn is_class(&self, class: &str) -> bool {
        match (web_app(&self.class), web_app(class)) {
            (Some((family, _)), Some((other, _))) => family == other,
            (None, Some(_)) => self.app,
            (Some(_), None) => false,
            (None, None) if self.class.is_empty() => !self.app && class == self.name,
            (None, None) => class == self.class,
        }
    }

    /// The command that opens it again, or `None` when there's nothing to
    /// open, or nothing to open it with.
    pub fn command(&self, launcher: &str) -> Option<String> {
        chromium::command(self, launcher)
    }

    /// Whether opening it again is left to the browser.
    pub fn opens_itself(&self) -> bool {
        chromium::opens_them_itself(self)
    }
}

/// What a web app's windows' class is made of: its browser's family, and the
/// page it's of or, for one that's installed, its id. `None` for other
/// windows' classes.
pub fn web_app(class: &str) -> Option<(&str, &str)> {
    let (family, rest) = class.split_once('-')?;
    let is_id = |id: &str| id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b));
    let (app, _profile) = rest
        .split_once("__")
        .or_else(|| rest.split_once('-').filter(|(id, _)| is_id(id)))?;
    let is_family = !family.is_empty() && family.bytes().all(|b| b.is_ascii_lowercase());
    (is_family && !app.is_empty()).then_some((family, app))
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

impl Window {
    fn is_app(&self) -> bool {
        web_app(&self.class).is_some()
    }
}

/// A window as its browser notes it, and the title of the tab that shows.
#[derive(Debug, Clone, PartialEq)]
struct Noted {
    browser: Browser,
    title: String,
}

/// What the browser windows hold, kept current.
#[derive(Default)]
pub struct Browsers {
    files: HashMap<PathBuf, (Option<SystemTime>, Vec<Noted>)>,
    by_address: HashMap<String, Browser>,
    /// Windows a browser doesn't note though it notes all others, since when.
    unnoted: HashMap<String, Instant>,
}

impl Browsers {
    /// Those known before this started, by address.
    pub fn new(known: HashMap<String, Browser>) -> Self {
        Self {
            files: HashMap::new(),
            by_address: known,
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
        self.unnoted
            .retain(|address, _| windows.iter().any(|w| w.address == *address));
        let pids: HashSet<u32> = windows.iter().map(|w| w.pid).collect();
        let mut open = HashSet::new();
        for pid in pids {
            let files = chromium::session_files(pid);
            for app in [false, true] {
                let mut noted: Vec<Noted> = Vec::new();
                for file in files.iter().filter(|file| file.app == app) {
                    open.insert(file.path.clone());
                    noted.extend(self.read(file).iter().cloned());
                }
                let windows: Vec<&Window> = windows
                    .iter()
                    .filter(|w| w.pid == pid && w.is_app() == app)
                    .collect();
                let incognito: HashSet<String> = self
                    .unnoted
                    .iter()
                    .filter(|(_, since)| now.duration_since(**since) >= UNNOTED)
                    .map(|(address, _)| address.clone())
                    .collect();
                let matched = match_up(&windows, &noted, &self.by_address, &incognito);
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
                self.by_address.extend(matched);
            }
        }
        self.files.retain(|path, _| open.contains(path));
    }

    fn read(&mut self, file: &chromium::SessionFile) -> &[Noted] {
        let at = std::fs::metadata(&file.path)
            .and_then(|m| m.modified())
            .ok();
        let entry = self.files.entry(file.path.clone()).or_default();
        if entry.0 != at || at.is_none() {
            *entry = (at, chromium::read(file));
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
    before: &HashMap<String, Browser>,
    incognito: &HashSet<String>,
) -> HashMap<String, Browser> {
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
            alone.then(|| (w.address.clone(), one.browser.window))
        })
        .collect();
    let mut take =
        |pairs: Vec<(String, i32)>, windows: &mut Vec<&Window>, noted: &mut Vec<&Noted>| {
            for (address, id) in pairs {
                if let Some(i) = noted.iter().position(|n| n.browser.window == id) {
                    let mut browser = noted.remove(i).browser.clone();
                    if let Some(i) = windows.iter().position(|w| w.address == address) {
                        browser.class = windows.remove(i).class.clone();
                    }
                    matched.insert(address, browser);
                }
            }
        };
    take(by_title, &mut windows, &mut noted);

    let as_before = windows
        .iter()
        .filter_map(|w| Some((w.address.clone(), before.get(&w.address)?.window)))
        .collect();
    take(as_before, &mut windows, &mut noted);

    windows.retain(|w| !incognito.contains(&w.address));
    if windows.len() == noted.len() {
        windows.sort_by_key(|w| w.opened);
        noted.sort_by_key(|n| n.browser.window);
        let in_order = windows
            .iter()
            .zip(&noted)
            .map(|(w, n)| (w.address.clone(), n.browser.window))
            .collect();
        take(in_order, &mut windows, &mut noted);
    }
    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn browser(window: i32, urls: &[&str], tab: usize) -> Browser {
        Browser {
            name: "chromium".into(),
            class: String::new(),
            window,
            profile: "Default".into(),
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
            browser: browser(id, &["https://x/"], 0),
            title: title.into(),
        };
        let pair = |windows: &[&Window], all: &[Noted], before: &HashMap<String, Browser>| {
            let mut found: Vec<(String, i32)> = match_up(windows, all, before, &HashSet::new())
                .into_iter()
                .map(|(address, b)| (address, b.window))
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
        let before = HashMap::from([("c".to_string(), browser(2, &[], 0))]);
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
        assert_eq!(found["c"].window, 2);
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
            browser: browser(id, &["https://x/"], 0),
            title: title.into(),
        };
        // One page's title starts as another's does.
        let a = window("a", "google-chrome", "Mail - Inbox - Google Chrome");
        let b = window("b", "google-chrome", "Mail - Google Chrome");
        let all = [noted(1, "Mail"), noted(2, "Mail - Inbox")];
        let found = match_up(&[&a, &b], &all, &HashMap::new(), &HashSet::new());
        assert_eq!((found["a"].window, found["b"].window), (2, 1));
        assert_eq!(found["a"].class, "google-chrome");
    }

    #[test]
    fn tells_web_apps_windows_from_others() {
        let id = "cinhimbnkkaeohfgghhklpknlkffjgod";
        assert_eq!(
            web_app("chrome-app.hey.com__-Default"),
            Some(("chrome", "app.hey.com"))
        );
        assert_eq!(
            web_app("brave-x.com__a_b-Profile 1"),
            Some(("brave", "x.com"))
        );
        assert_eq!(
            web_app(&format!("msedge-{id}-Default")),
            Some(("msedge", id))
        );
        for other in [
            "chromium",
            "google-chrome",
            "brave-browser",
            "org.my-app.x__y",
            "",
        ] {
            assert_eq!(web_app(other), None, "{other}");
        }
    }

    #[test]
    fn opens_where_windows_of_its_class_do() {
        let mut b = browser(1, &["https://x/"], 0);
        assert_eq!(b.class_pattern(), "^chromium$", "before classes were kept");
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

    #[test]
    fn reads_sessions_from_before_other_browsers() {
        let b: Browser = serde_json::from_str(
            r#"{ "window": 1, "profile": "Default", "urls": ["https://x/"] }"#,
        )
        .unwrap();
        assert_eq!(b, browser(1, &["https://x/"], 0));
    }
}
