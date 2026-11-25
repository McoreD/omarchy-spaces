//! Chromium, and the browsers made from it. They note what their windows hold
//! as they change, in files they keep open in their profile
//! (`Sessions/Session_*`, and `Apps_*` for web apps). Incognito windows aren't
//! in them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{Browser, Noted, desktop};
use crate::paths;
use crate::terminal::shell_quote;

/// The ones Omarchy offers (`omarchy default browser`): their desktop entry,
/// which starts them, and where they have their profiles. Nothing says which
/// entry a running one is of.
const FAMILY: &[(&str, &str)] = &[
    ("chromium", "chromium"),
    ("google-chrome", "google-chrome"),
    ("brave-browser", "BraveSoftware/Brave-Browser"),
    ("brave-origin", "BraveSoftware/Brave-Origin"),
    ("microsoft-edge", "microsoft-edge"),
];

fn data_dir(name: &str) -> Option<PathBuf> {
    let (_, dir) = FAMILY.iter().find(|(known, _)| *known == name)?;
    Some(paths::config_dir().join(dir))
}

/// A file that says what a browser's windows hold.
pub struct SessionFile {
    pub path: PathBuf,
    /// Whose it is, by its desktop entry, and its profile's folder.
    name: &'static str,
    profile: String,
    /// Whether it's of web apps.
    pub app: bool,
}

/// The session files a process has open, if it's one of these. A browser run
/// on another folder, as by a test of some page, isn't yours to have back.
pub fn session_files(pid: u32) -> Vec<SessionFile> {
    let config = paths::config_dir();
    let mut files: Vec<SessionFile> = open_files(pid)
        .filter_map(|path| session_file(&config, path))
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files.dedup_by(|a, b| a.path == b.path);
    files
}

fn session_file(config: &Path, path: PathBuf) -> Option<SessionFile> {
    let file = path.file_name()?.to_str()?;
    let app = file.starts_with("Apps_");
    if !app && !file.starts_with("Session_") {
        return None;
    }
    let profile = path.parent()?.parent()?;
    let data = profile.parent()?;
    let (name, _) = FAMILY.iter().find(|(_, dir)| config.join(dir) == data)?;
    Some(SessionFile {
        name,
        profile: profile.file_name()?.to_str()?.to_string(),
        app,
        path,
    })
}

fn open_files(pid: u32) -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|fd| std::fs::read_link(fd.path()).ok())
}

pub fn read(file: &SessionFile) -> Vec<Noted> {
    std::fs::read(&file.path)
        .map(|data| parse(&data, file.name, &file.profile, file.app))
        .unwrap_or_default()
}

/// Whether the profile is set to open its last windows itself, which would
/// open them twice.
fn restores_itself(data: &Path, profile: &str) -> bool {
    let restore = || {
        let text = std::fs::read_to_string(data.join(profile).join("Preferences")).ok()?;
        let prefs: serde_json::Value = serde_json::from_str(&text).ok()?;
        prefs["session"]["restore_on_startup"].as_u64()
    };
    restore() == Some(1)
}

/// Whether it's running, with its profiles there.
fn is_running(data: &Path) -> bool {
    // A link to `<host>-<pid>`, left behind if it crashed.
    std::fs::read_link(data.join("SingletonLock")).is_ok_and(|lock| {
        let lock = lock.to_string_lossy();
        let pid = lock.rsplit('-').next().and_then(|pid| pid.parse().ok());
        pid.is_some_and(|pid| open_files(pid).any(|path| path.starts_with(data)))
    })
}

/// Whether opening its windows again is left to it: it's about to.
pub fn opens_them_itself(browser: &Browser) -> bool {
    data_dir(&browser.name)
        .is_some_and(|data| restores_itself(&data, &browser.profile) && !is_running(&data))
}

/// The command that opens it again, or `None` when there's nothing to open,
/// or nothing to open it with. The tab that shows goes first: that's the one
/// it then shows.
pub fn command(browser: &Browser, launcher: &str) -> Option<String> {
    // Only one of these: a saved session can't have another program run.
    data_dir(&browser.name)?;
    let program = shell_quote(&desktop::program(&browser.name)?);
    Some(format!("{launcher} {program} {}", arguments(browser)?))
}

fn arguments(browser: &Browser) -> Option<String> {
    let mut urls: Vec<&String> = browser.urls.iter().filter(|url| is_url(url)).collect();
    let shown = browser.urls.get(browser.tab);
    if let Some(i) = urls.iter().position(|url| Some(*url) == shown) {
        let shown = urls.remove(i);
        urls.insert(0, shown);
    }
    let first = *urls.first()?;
    let mut args = vec![format!("--profile-directory={}", browser.profile)];
    if browser.app {
        args.push(format!("--app={first}"));
    } else {
        args.push("--new-window".into());
        args.extend(urls.into_iter().cloned());
    }
    let args: Vec<String> = args.iter().map(|arg| shell_quote(arg)).collect();
    Some(args.join(" "))
}

/// Whether it's a page, and can't be taken for an option.
fn is_url(url: &str) -> bool {
    ["http://", "https://", "file://", "chrome://", "about:"]
        .iter()
        .any(|scheme| url.starts_with(scheme))
        && !url.chars().any(char::is_control)
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
fn parse(data: &[u8], name: &str, profile: &str, app: bool) -> Vec<Noted> {
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
            name: name.to_string(),
            class: String::new(),
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
    use super::super::tests::browser;
    use super::*;

    fn note(id: u8, payload: &[u8]) -> Vec<u8> {
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
        note(id, &bytes)
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
        note(6, &out)
    }

    fn file(commands: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"SNSS\x03\0\0\0".to_vec();
        out.extend(commands.concat());
        out
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
            note(6, &[1, 2, 3]),
        ]);
        let want = vec![Noted {
            browser: browser(1, &["https://b/x", "https://a/next"], 1),
            title: "Next – page".into(),
        }];
        assert_eq!(parse(&data, "chromium", "Default", false), want);
        assert_eq!(
            parse(&data, "chromium", "Default", true).len(),
            1,
            "the popup's kind"
        );
        // Being written.
        data.extend([200, 0, 6, 1]);
        assert_eq!(parse(&data, "chromium", "Default", false), want);
        data[4] = 4;
        assert_eq!(parse(&data, "chromium", "Default", false), [], "encrypted");
        assert_eq!(parse(b"", "chromium", "Default", false), []);
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
        let noted = parse(&data, "chromium", "Default", false);
        assert_eq!(noted[0].browser.urls, ["https://c/"]);
    }

    #[test]
    fn knows_whose_a_session_file_is() {
        let of = |path: &str| {
            let file = session_file(Path::new("/h/.config"), path.into())?;
            Some((file.name, file.profile, file.app))
        };
        assert_eq!(
            of("/h/.config/chromium/Default/Sessions/Session_1"),
            Some(("chromium", "Default".into(), false))
        );
        assert_eq!(
            of("/h/.config/BraveSoftware/Brave-Browser/Profile 1/Sessions/Apps_1"),
            Some(("brave-browser", "Profile 1".into(), true))
        );
        assert_eq!(of("/h/.config/google-chrome/Default/Sessions/Tabs_1"), None);
        assert_eq!(of("/tmp/test/Default/Sessions/Session_1"), None);
        assert_eq!(of("/Session_1"), None);
    }

    #[test]
    fn opens_pages_and_nothing_else() {
        let mut b = browser(
            1,
            &["https://a/?x='1'", "--gpu-launcher=sh", "https://b/"],
            2,
        );
        assert_eq!(
            arguments(&b).unwrap(),
            r"'--profile-directory=Default' '--new-window' 'https://b/' 'https://a/?x='\''1'\'''"
        );
        b.app = true;
        assert_eq!(
            arguments(&b).unwrap(),
            "'--profile-directory=Default' '--app=https://b/'"
        );
        assert_eq!(arguments(&browser(1, &["javascript:x"], 0)), None);
        b.name = "sh".into();
        assert_eq!(command(&b, "exec"), None, "not a browser");
    }

    #[test]
    fn tells_whether_it_runs() {
        use std::os::unix::fs::symlink;
        let data = tempfile::tempdir().unwrap();
        let data = data.path().canonicalize().unwrap();
        assert!(!is_running(&data));
        // Another process has its number by now.
        let lock = format!("my-host-{}", std::process::id());
        symlink(lock, data.join("SingletonLock")).unwrap();
        assert!(!is_running(&data));
        let _open = std::fs::File::create(data.join("History")).unwrap();
        assert!(is_running(&data));
    }
}
