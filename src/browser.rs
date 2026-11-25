//! Chromium's windows, to open again after a restart. Chromium notes what its
//! windows hold as they change, in files it keeps open in its profile
//! (`Sessions/Session_*`, and `Apps_*` for web apps). Incognito windows aren't
//! in them. Which of Hyprland's windows is which, the titles tell, and for
//! windows that share one, the order they were opened in.
//!
//! Closed one by one, as Omarchy does before a restart, Chromium itself would
//! only bring back the last.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::paths;
use crate::terminal::shell_quote;

/// How long a window must have gone without Chromium noting it to be taken
/// for incognito. It notes them every few seconds.
const UNNOTED: Duration = Duration::from_secs(10);

/// What a browser window holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Browser {
    /// Chromium's number for it, while it runs.
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

impl Browser {
    /// The class of its windows, for Hyprland's rules.
    pub fn class_pattern(&self) -> &'static str {
        if self.app {
            "^chrome-.*$"
        } else {
            "^chromium$"
        }
    }

    /// Whether a window of this class can be it.
    pub fn is_class(&self, class: &str) -> bool {
        is_app(class) == Some(self.app)
    }

    /// The command that opens it again, or `None` when there's nothing to
    /// open. The tab that shows goes first: that's the one Chromium shows.
    pub fn command(&self, launcher: &str) -> Option<String> {
        let mut urls: Vec<&String> = self.urls.iter().filter(|url| is_url(url)).collect();
        let shown = self.urls.get(self.tab);
        if let Some(i) = urls.iter().position(|url| Some(*url) == shown) {
            let shown = urls.remove(i);
            urls.insert(0, shown);
        }
        let first = *urls.first()?;
        let mut args = vec![format!("--profile-directory={}", self.profile)];
        if self.app {
            args.push(format!("--app={first}"));
        } else {
            args.push("--new-window".into());
            args.extend(urls.into_iter().cloned());
        }
        let args: Vec<String> = args.iter().map(|arg| shell_quote(arg)).collect();
        Some(format!("{launcher} chromium {}", args.join(" ")))
    }
}

/// Whether it's a page, and can't be taken for an option.
fn is_url(url: &str) -> bool {
    ["http://", "https://", "file://", "chrome://", "about:"]
        .iter()
        .any(|scheme| url.starts_with(scheme))
        && !url.chars().any(char::is_control)
}

/// Whether windows of this class are Chromium's: `Some(true)` a web app's.
fn is_app(class: &str) -> Option<bool> {
    match class {
        "chromium" => Some(false),
        _ => class.starts_with("chrome-").then_some(true),
    }
}

fn data_dir() -> PathBuf {
    paths::config_dir().join("chromium")
}

/// Whether the profile is set to open its last windows itself, which would
/// open them twice.
pub fn restores_itself(profile: &str) -> bool {
    let restore = || {
        let text = std::fs::read_to_string(data_dir().join(profile).join("Preferences")).ok()?;
        let prefs: serde_json::Value = serde_json::from_str(&text).ok()?;
        prefs["session"]["restore_on_startup"].as_u64()
    };
    restore() == Some(1)
}

/// Whether Chromium is running.
pub fn is_running() -> bool {
    // A link to `<host>-<pid>`, left behind if it crashed.
    std::fs::read_link(data_dir().join("SingletonLock")).is_ok_and(|lock| {
        let lock = lock.to_string_lossy();
        lock.rsplit('-')
            .next()
            .and_then(|pid| std::fs::read_to_string(format!("/proc/{pid}/comm")).ok())
            .is_some_and(|comm| comm.starts_with("chrom"))
    })
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

/// What the browser windows hold, kept current.
#[derive(Default)]
pub struct Browsers {
    files: HashMap<PathBuf, (Option<SystemTime>, Vec<Noted>)>,
    by_address: HashMap<String, Browser>,
    /// Windows Chromium doesn't note though it notes all others, since when.
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

    /// Reads what changed. A window that can't be told now, as while it
    /// closes, keeps what it held.
    pub fn refresh(&mut self, windows: &[Window], now: Instant) {
        let windows: Vec<&Window> = windows
            .iter()
            .filter(|w| is_app(&w.class).is_some())
            .collect();
        self.by_address
            .retain(|address, _| windows.iter().any(|w| w.address == *address));
        self.unnoted
            .retain(|address, _| windows.iter().any(|w| w.address == *address));
        let pids: HashSet<u32> = windows.iter().map(|w| w.pid).collect();
        let mut open = HashSet::new();
        for pid in pids {
            let files = session_files(pid);
            for app in [false, true] {
                let mut noted: Vec<Noted> = Vec::new();
                for (path, profile) in files.iter().filter(|(path, _)| is_apps(path) == app) {
                    open.insert(path.clone());
                    noted.extend(self.read(path, profile, app).iter().cloned());
                }
                let windows: Vec<&Window> = windows
                    .iter()
                    .copied()
                    .filter(|w| w.pid == pid && is_app(&w.class) == Some(app))
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

    fn read(&mut self, path: &Path, profile: &str, app: bool) -> &[Noted] {
        let at = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let entry = self.files.entry(path.to_path_buf()).or_default();
        if entry.0 != at || at.is_none() {
            let noted = std::fs::read(path)
                .ok()
                .map(|data| parse(&data, profile, app))
                .unwrap_or_default();
            *entry = (at, noted);
        }
        &entry.1
    }
}

/// The session files Chromium's process has open in its own profiles, and
/// which profile each is of. A browser run on another folder, as by a test
/// of some page, isn't yours to have back.
fn session_files(pid: u32) -> Vec<(PathBuf, String)> {
    let data = data_dir();
    let mut files: Vec<(PathBuf, String)> = std::fs::read_dir(format!("/proc/{pid}/fd"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|fd| std::fs::read_link(fd.path()).ok())
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?;
            if !(name.starts_with("Session_") || name.starts_with("Apps_")) {
                return None;
            }
            let profile = path.parent()?.parent()?;
            let name = profile.file_name()?.to_str()?.to_string();
            (profile.parent()? == data).then(|| (path.clone(), name))
        })
        .collect();
    files.sort();
    files.dedup();
    files
}

fn is_apps(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with("Apps_"))
}

/// A window as Chromium notes it, and the title of the tab that shows.
#[derive(Debug, Clone, PartialEq)]
struct Noted {
    browser: Browser,
    title: String,
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
    let mut windows: Vec<&Window> = windows.to_vec();
    let mut noted: Vec<&Noted> = noted.iter().collect();
    let mut matched = HashMap::new();

    let titled = |w: &Window, n: &Noted| {
        !n.title.is_empty() && (w.title == n.title || w.title == format!("{} - Chromium", n.title))
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
                    matched.insert(address.clone(), noted.remove(i).browser.clone());
                    windows.retain(|w| w.address != address);
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

#[derive(Default)]
struct NotedTab {
    window: i32,
    place: i32,
    /// Where it has been, as (URL, title), by place.
    pages: BTreeMap<i32, (String, String)>,
    page: i32,
}

#[derive(Default)]
struct NotedWindow {
    kind: i32,
    tab: i32,
}

/// Reads a session file: `SNSS`, its version, then commands that each change
/// something, as Chromium's `session_service_commands.cc` has them. One cut
/// short is still being written.
fn parse(data: &[u8], profile: &str, app: bool) -> Vec<Noted> {
    // 1 and 3 are in the clear, 3 with a mark between windows restored and
    // since. Others are encrypted.
    let Some(mut rest) = data
        .strip_prefix(b"SNSS\x01\0\0\0")
        .or_else(|| data.strip_prefix(b"SNSS\x03\0\0\0"))
    else {
        return Vec::new();
    };
    let mut tabs: BTreeMap<i32, NotedTab> = BTreeMap::new();
    let mut windows: BTreeMap<i32, NotedWindow> = BTreeMap::new();
    while let Some((size, after)) = rest.split_at_checked(2) {
        let size = usize::from(u16::from_le_bytes([size[0], size[1]]));
        let Some((command, after)) = after.split_at_checked(size) else {
            break;
        };
        rest = after;
        if let Some((&id, payload)) = command.split_first() {
            // One that makes no sense changes nothing.
            let _ = apply(id, payload, &mut tabs, &mut windows);
        }
    }

    let mut noted = Vec::new();
    for (id, window) in windows {
        // Others are popups, developer tools and the like.
        if (window.kind == 0) == app {
            continue;
        }
        let mut held: Vec<&NotedTab> = tabs.values().filter(|t| t.window == id).collect();
        held.sort_by_key(|t| t.place);
        let mut browser = Browser {
            window: id,
            profile: profile.to_string(),
            app,
            urls: Vec::new(),
            tab: 0,
        };
        let mut title = String::new();
        for tab in held {
            let Some((_, (url, page))) = tab.pages.range(..=tab.page).next_back() else {
                continue;
            };
            if tab.place == window.tab {
                browser.tab = browser.urls.len();
                title = page.clone();
            }
            browser.urls.push(url.clone());
        }
        if !browser.urls.is_empty() {
            noted.push(Noted { browser, title });
        }
    }
    noted
}

fn apply(
    command: u8,
    payload: &[u8],
    tabs: &mut BTreeMap<i32, NotedTab>,
    windows: &mut BTreeMap<i32, NotedWindow>,
) -> Option<()> {
    let int = |n: usize| {
        let bytes = payload.get(n * 4..n * 4 + 4)?;
        Some(i32::from_le_bytes(bytes.try_into().ok()?))
    };
    let first = int(0)?;
    match command {
        // Tab, window.
        0 => {
            let tab = tabs.entry(int(1)?).or_default();
            tab.window = first;
            windows.entry(first).or_default();
        }
        2 => tabs.entry(first).or_default().place = int(1)?,
        // A page: a size, the tab, its place, the URL and the title.
        6 => {
            let (tab, place) = (int(1)?, int(2)?);
            let length = usize::try_from(int(3)?).ok()?;
            let url = String::from_utf8(payload.get(16..16 + length)?.to_vec()).ok()?;
            // Each starts on a multiple of four.
            let at = 16 + length.next_multiple_of(4);
            let length = usize::try_from(int(at / 4)?).ok()?;
            let title = payload.get(at + 4..(at + 4).checked_add(length.checked_mul(2)?)?)?;
            let title: Vec<u16> = title
                .as_chunks()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
            let page = (url, String::from_utf16_lossy(&title));
            tabs.entry(tab).or_default().pages.insert(place, page);
        }
        7 => tabs.entry(first).or_default().page = int(1)?,
        8 => windows.entry(first).or_default().tab = int(1)?,
        9 => windows.entry(first).or_default().kind = int(1)?,
        16 => drop(tabs.remove(&first)),
        17 => drop(windows.remove(&first)),
        // Pages dropped from the middle: from where, and how many.
        24 => {
            let (from, count) = (int(1)?, int(2)?);
            let tab = tabs.get_mut(&first)?;
            let to = from.checked_add(count)?;
            if tab.page >= to {
                tab.page -= count;
            } else if tab.page >= from {
                tab.page = (from - 1).max(0);
            }
            tab.pages = std::mem::take(&mut tab.pages)
                .into_iter()
                .filter(|(place, _)| *place < from || *place >= to)
                .map(|(place, page)| (if place >= to { place - count } else { place }, page))
                .collect();
        }
        _ => {}
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(id: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = u16::try_from(payload.len() + 1)
            .unwrap()
            .to_le_bytes()
            .to_vec();
        out.push(id);
        out.extend(payload);
        out
    }

    fn ints(id: u8, values: &[i32]) -> Vec<u8> {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        command(id, &bytes)
    }

    fn page(tab: i32, place: i32, url: &str, title: &str) -> Vec<u8> {
        let mut body: Vec<u8> = [tab, place, url.len() as i32]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        body.extend(url.bytes());
        body.resize(body.len().next_multiple_of(4), 0);
        let title: Vec<u16> = title.encode_utf16().collect();
        body.extend((title.len() as i32).to_le_bytes());
        body.extend(title.iter().flat_map(|c| c.to_le_bytes()));
        let mut out = (body.len() as u32).to_le_bytes().to_vec();
        out.extend(body);
        command(6, &out)
    }

    fn file(commands: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"SNSS\x03\0\0\0".to_vec();
        out.extend(commands.concat());
        out
    }

    fn browser(window: i32, urls: &[&str], tab: usize) -> Browser {
        Browser {
            window,
            profile: "Default".into(),
            app: false,
            urls: urls.iter().map(|u| u.to_string()).collect(),
            tab,
        }
    }

    #[test]
    fn reads_what_each_window_holds() {
        let mut data = file(&[
            ints(9, &[1, 0]),
            ints(0, &[1, 10]),
            ints(2, &[10, 1]),
            page(10, 0, "https://a/", "A"),
            page(10, 1, "https://a/next", "Next – page"),
            ints(7, &[10, 1]),
            ints(0, &[1, 11]),
            ints(2, &[11, 0]),
            page(11, 0, "https://b/x", "B"),
            ints(8, &[1, 1]),
            // Closed, a popup, and one with nothing in it yet.
            ints(9, &[2, 0]),
            ints(0, &[2, 12]),
            page(12, 0, "https://c/", "C"),
            ints(17, &[2]),
            ints(9, &[3, 1]),
            ints(0, &[3, 13]),
            page(13, 0, "https://d/", "D"),
            ints(9, &[4, 0]),
            // A tab closed, and one it can't make sense of.
            ints(0, &[1, 14]),
            page(14, 0, "https://e/", "E"),
            ints(16, &[14]),
            command(6, &[1, 2, 3]),
        ]);
        let want = vec![Noted {
            browser: browser(1, &["https://b/x", "https://a/next"], 1),
            title: "Next – page".into(),
        }];
        assert_eq!(parse(&data, "Default", false), want);
        assert_eq!(parse(&data, "Default", true).len(), 1, "the popup's kind");
        // Being written.
        data.extend([200, 0, 6, 1]);
        assert_eq!(parse(&data, "Default", false), want);
        data[4] = 4;
        assert_eq!(parse(&data, "Default", false), [], "encrypted");
        assert_eq!(parse(b"", "Default", false), []);
    }

    #[test]
    fn drops_the_pages_chromium_does() {
        let data = file(&[
            ints(0, &[1, 10]),
            page(10, 0, "https://a/", ""),
            page(10, 1, "https://b/", ""),
            page(10, 2, "https://c/", ""),
            ints(7, &[10, 2]),
            ints(24, &[10, 0, 2]),
        ]);
        let noted = parse(&data, "Default", false);
        assert_eq!(noted[0].browser.urls, ["https://c/"]);
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
    fn opens_pages_and_nothing_else() {
        let mut b = browser(
            1,
            &["https://a/?x='1'", "--gpu-launcher=sh", "https://b/"],
            2,
        );
        assert_eq!(
            b.command("exec").unwrap(),
            r"exec chromium '--profile-directory=Default' '--new-window' 'https://b/' 'https://a/?x='\''1'\'''"
        );
        b.app = true;
        assert_eq!(
            b.command("exec").unwrap(),
            "exec chromium '--profile-directory=Default' '--app=https://b/'"
        );
        assert_eq!(browser(1, &["javascript:x"], 0).command("exec"), None);
    }
}
