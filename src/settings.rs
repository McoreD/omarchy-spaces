//! The settings `space config` and the sidebar's Settings screen know: one
//! list, [`SETTINGS`], that both are made from. A setting is its key in
//! `config.toml` (`section.name`), how it is shown, what it can be, its
//! default, and how to read it from [`Config`]. The panel draws whatever
//! `space config schema --json` says, so a new setting is one entry here and
//! the field in `config.rs` it reads; see docs/internals/architecture.md.
//!
//! Settings that are a path or a free string (`sounds.done`, `sounds.waiting`)
//! are not here: they are for the config file.

use std::fs;

use anyhow::{Context, Result, bail};
use serde_json::{Value as Json, json};
use toml_edit::{Array, DocumentMut, Item as Node, Table, Value as Toml};

use crate::config::Config;
use crate::newspace;
use crate::paths;

/// What a setting is now, or by default.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    /// The value of one of a choice's options.
    Choice(String),
    /// The values of the options that are on, in the options' order.
    List(Vec<String>),
}

/// A setting's default, as a constant.
#[derive(Debug, Clone, Copy)]
pub enum Initial {
    Bool(bool),
    Choice(&'static str),
    List(&'static [&'static str]),
}

impl Initial {
    pub fn value(self) -> Value {
        match self {
            Initial::Bool(on) => Value::Bool(on),
            Initial::Choice(value) => Value::Choice(value.to_string()),
            Initial::List(values) => Value::List(values.iter().map(|v| v.to_string()).collect()),
        }
    }
}

/// One option of a `Choice`, as a constant.
#[derive(Debug, Clone, Copy)]
pub struct Choice {
    pub value: &'static str,
    pub label: &'static str,
}

/// One option as the panel shows it, for a setting of several.
#[derive(Debug, Clone, PartialEq)]
pub struct Opt {
    pub value: String,
    pub label: String,
    /// A name in the icon theme, or empty.
    pub icon: String,
    /// Whether the panel offers it. One that isn't is still a value `set`
    /// takes (an agent that isn't installed yet).
    pub available: bool,
    /// The name of the group of options it is drawn in (one of the setting's
    /// `groups`), or empty for none. A group's options go together.
    pub group: String,
}

/// A group of the options of a list, and how the panel draws it.
#[derive(Debug, Clone, Copy)]
pub struct Group {
    /// Its title, which an option's `group` names.
    pub name: &'static str,
    /// Drawn as a grid of icons with short names, in place of rows, for a
    /// group of many.
    pub grid: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    /// On or off.
    Bool,
    /// One of a fixed list.
    Choice(&'static [Choice]),
    /// Any of a list that can change from one run to the next, in the order it
    /// gives. With `none`, the empty list has a word of its own, which is
    /// exclusive of the others.
    Multi {
        options: fn() -> Vec<Opt>,
        none: bool,
        /// How the options that name a group are drawn together.
        groups: &'static [Group],
        /// What `set` says of a word that isn't an option, `{}` being the word.
        unknown: &'static str,
    },
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Bool => "bool",
            Kind::Choice(_) => "choice",
            Kind::Multi { .. } => "multi",
        }
    }
}

#[derive(Debug)]
pub struct Setting {
    /// `section.name`, as in `config.toml`.
    pub key: &'static str,
    pub label: &'static str,
    /// One line under the label, or empty.
    pub help: &'static str,
    pub kind: Kind,
    pub default: Initial,
    /// The field of [`Config`] it controls.
    pub get: fn(&Config) -> Value,
}

impl Setting {
    pub fn section(&self) -> &str {
        self.key
            .split_once('.')
            .map_or(self.key, |(section, _)| section)
    }

    pub fn name(&self) -> &str {
        self.key.split_once('.').map_or("", |(_, name)| name)
    }

    /// Every option of a `Choice` or `Multi`, unavailable ones too.
    fn options(&self) -> Vec<Opt> {
        match self.kind {
            Kind::Bool => Vec::new(),
            Kind::Choice(choices) => choices
                .iter()
                .map(|choice| Opt {
                    value: choice.value.to_string(),
                    label: choice.label.to_string(),
                    icon: String::new(),
                    available: true,
                    group: String::new(),
                })
                .collect(),
            Kind::Multi { options, .. } => options(),
        }
    }
}

/// The sections, in the order the panel shows them, with their titles.
pub const SECTIONS: &[(&str, &str)] = &[
    ("new_space", "New spaces"),
    ("restore", "Restore"),
    ("sounds", "Sounds"),
    ("tabs", "Tabs"),
];

const TAB_STYLES: &[Choice] = &[
    Choice {
        value: "browser",
        label: "Like a browser",
    },
    Choice {
        value: "omarchy",
        label: "Like Omarchy",
    },
];

/// The settings, in the order the panel shows them, a section's together.
pub static SETTINGS: &[Setting] = &[
    Setting {
        key: "new_space.open",
        label: "A new space opens",
        help: "Side by side, on its first tab.",
        kind: Kind::Multi {
            options: newspace::options,
            none: true,
            groups: &[Group {
                name: "Agents",
                grid: true,
            }],
            unknown: "there's no '{}' to open: use terminal, files or an Omarchy agent's id",
        },
        default: Initial::List(&["terminal"]),
        get: |config| Value::List(config.new_space.open.clone()),
    },
    Setting {
        key: "restore.auto",
        label: "Restore on start",
        help: "Open your spaces again after you log out or restart.",
        kind: Kind::Bool,
        default: Initial::Bool(true),
        get: |config| Value::Bool(config.restore.auto),
    },
    Setting {
        key: "restore.agents",
        label: "Agents' conversations",
        help: "Pick them up again where they were.",
        kind: Kind::Bool,
        default: Initial::Bool(true),
        get: |config| Value::Bool(config.restore.agents),
    },
    Setting {
        key: "restore.browsers",
        label: "Browser windows",
        help: "Open them again; off also stops saving the pages you have open.",
        kind: Kind::Bool,
        default: Initial::Bool(true),
        get: |config| Value::Bool(config.restore.browsers),
    },
    Setting {
        key: "sounds.enabled",
        label: "Play sounds",
        help: "When an agent starts waiting for you, or finishes unseen.",
        kind: Kind::Bool,
        default: Initial::Bool(false),
        get: |config| Value::Bool(config.sounds.enabled),
    },
    Setting {
        key: "tabs.style",
        label: "Tabs",
        help: "Like a browser, numbers close up as tabs end; like Omarchy, they stay and 1-5 always show.",
        kind: Kind::Choice(TAB_STYLES),
        default: Initial::Choice("browser"),
        get: |config| Value::Choice(config.tabs.style.clone()),
    },
];

/// The setting `key` names.
pub fn find(key: &str) -> Result<&'static Setting> {
    find_in(SETTINGS, key)
}

fn find_in<'a>(settings: &'a [Setting], key: &str) -> Result<&'a Setting> {
    match settings.iter().find(|setting| setting.key == key) {
        Some(setting) => Ok(setting),
        None => {
            let keys: Vec<&str> = settings.iter().map(|setting| setting.key).collect();
            bail!(
                "there's no setting '{key}' to change: use {}",
                keys.join(", ")
            )
        }
    }
}

/// What `raw` means for a setting, or why it isn't one.
pub fn parse(setting: &Setting, raw: &str) -> Result<Value> {
    let key = setting.key;
    let word = raw.trim();
    match setting.kind {
        Kind::Bool => match word.to_lowercase().as_str() {
            "true" | "on" => Ok(Value::Bool(true)),
            "false" | "off" => Ok(Value::Bool(false)),
            _ => bail!("'{word}' isn't a value for {key}: use true, false, on or off"),
        },
        Kind::Choice(choices) => {
            let found = choices
                .iter()
                .find(|choice| choice.value.eq_ignore_ascii_case(word));
            match found {
                Some(choice) => Ok(Value::Choice(choice.value.to_string())),
                None => {
                    let values: Vec<&str> = choices.iter().map(|choice| choice.value).collect();
                    bail!(
                        "'{word}' isn't a value for {key}: use {}",
                        values.join(", ")
                    )
                }
            }
        }
        Kind::Multi { none, unknown, .. } => {
            let words: Vec<&str> = word
                .split(',')
                .map(str::trim)
                .filter(|word| !word.is_empty())
                .collect();
            if words.iter().any(|w| w.eq_ignore_ascii_case(newspace::NONE)) {
                if !none {
                    bail!("{key} can't be none");
                }
                if words.len() > 1 {
                    bail!("'none' can't go with anything else");
                }
                return Ok(Value::List(Vec::new()));
            }
            if words.is_empty() && !none {
                bail!("{key} needs at least one value");
            }
            let known = setting.options();
            let mut values = Vec::new();
            for word in words {
                let word = word.to_lowercase();
                if !known.iter().any(|opt| opt.value == word) {
                    bail!("{}", unknown.replace("{}", &word));
                }
                values.push(word);
            }
            Ok(Value::List(values))
        }
    }
}

/// A value as `space config get` and `set` say it.
pub fn show(value: &Value) -> String {
    match value {
        Value::Bool(on) => on.to_string(),
        Value::Choice(value) => value.clone(),
        Value::List(values) if values.is_empty() => newspace::NONE.to_string(),
        Value::List(values) => values.join(","),
    }
}

fn json(value: &Value) -> Json {
    match value {
        Value::Bool(on) => json!(on),
        Value::Choice(value) => json!(value),
        Value::List(values) => json!(values),
    }
}

/// `space config get <key>`: what the config has now, or the default.
pub fn get(key: &str, as_json: bool) -> Result<String> {
    let setting = find(key)?;
    let value = (setting.get)(&Config::load()?);
    Ok(if as_json {
        serde_json::to_string(&json(&value))?
    } else {
        show(&value)
    })
}

/// `space config set <key> <value>`: writes the setting, touching nothing else
/// in the file, comments included. A value the setting can't have is refused,
/// so a mistake never gets in.
pub fn set(key: &str, raw: &str) -> Result<()> {
    let setting = find(key)?;
    let value = parse(setting, raw)?;
    edit(setting, Some(&value))
}

/// `space config unset <key>`: takes the setting out of the file, so it is
/// the default again.
pub fn unset(key: &str) -> Result<()> {
    edit(find(key)?, None)
}

fn edit(setting: &Setting, value: Option<&Value>) -> Result<()> {
    let path = paths::config_file();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("can't read {}", paths::tilde(&path))),
    };
    let edited = with_value(&text, setting.section(), setting.name(), value)
        .with_context(|| format!("can't read {}", paths::tilde(&path)))?;
    if edited == text {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("can't make {}", paths::tilde(dir)))?;
    }
    let was = crate::config::tab_style();
    // Through a symlink to the real file, keeping its mode.
    crate::state::write_atomic(&path, &edited)?;
    // The style of the tabs counts from now: the Lua in Hyprland goes by what
    // `space` has noted of it. Going back to a browser's, the gaps close once
    // (as `space watch` does too, if it's running, so that one's enough).
    let now = crate::config::tab_style();
    if was == crate::config::TabStyle::Omarchy
        && now == crate::config::TabStyle::Browser
        && let Err(e) = crate::commands::tab::renumber()
    {
        eprintln!("space: warning: the tabs couldn't be numbered again: {e:#}");
    }
    Ok(())
}

fn toml(value: &Value) -> Toml {
    match value {
        Value::Bool(on) => Toml::from(*on),
        Value::Choice(value) => Toml::from(value.as_str()),
        Value::List(values) => {
            let mut array = Array::new();
            for value in values {
                array.push(value.as_str());
            }
            Toml::Array(array)
        }
    }
}

/// The file's text with `[section] name` set, or taken out with `None`, the
/// rest as it was.
fn with_value(text: &str, section: &str, name: &str, value: Option<&Value>) -> Result<String> {
    let mut doc: DocumentMut = text.parse()?;
    let Some(value) = value else {
        let emptied = match doc.get_mut(section).and_then(Node::as_table_like_mut) {
            Some(table) => table.remove(name).is_some() && table.is_empty(),
            None => false,
        };
        if emptied {
            drop_empty(&mut doc, section);
        }
        return Ok(doc.to_string());
    };
    let table = match doc
        .entry(section)
        .or_insert_with(|| Node::Table(Table::new()))
        .as_table_like_mut()
    {
        Some(table) => table,
        None => bail!("{section} isn't a table"),
    };
    let mut new = toml(value);
    // Keeps what is around the old value, such as a comment after it.
    if let Some(old) = table.get(name).and_then(Node::as_value) {
        *new.decor_mut() = old.decor().clone();
    }
    match table.get_mut(name) {
        // In place, so the key keeps the comment above it.
        Some(old) => *old = Node::Value(new),
        None => {
            table.insert(name, Node::Value(new));
        }
    }
    Ok(doc.to_string())
}

/// Takes out `section`, a table or an inline table with nothing left in it,
/// unless a comment is on its own line (`[restore] # why`). What was written
/// above it, a comment on the file, goes to what follows, so it stays.
fn drop_empty(doc: &mut DocumentMut, section: &str) {
    let (prefix, suffix) = match doc.get(section) {
        Some(Node::Table(table)) => (
            table.decor().prefix().and_then(|p| p.as_str()),
            table.decor().suffix().and_then(|p| p.as_str()),
        ),
        Some(Node::Value(Toml::InlineTable(table))) => (
            table.decor().prefix().and_then(|p| p.as_str()),
            table.decor().suffix().and_then(|p| p.as_str()),
        ),
        _ => return,
    };
    if suffix.is_some_and(|text| text.contains('#')) {
        return;
    }
    let above = prefix.filter(|text| text.contains('#')).map(str::to_string);
    let keys: Vec<String> = doc.iter().map(|(key, _)| key.to_string()).collect();
    let next = keys
        .iter()
        .position(|key| key == section)
        .and_then(|at| keys.get(at + 1));
    doc.remove(section);
    let Some(above) = above else { return };
    let following = next.and_then(|key| match doc.get_mut(key) {
        Some(Node::Table(table)) => Some(table.decor_mut()),
        _ => None,
    });
    match following {
        Some(decor) => {
            let old = decor.prefix().and_then(|p| p.as_str()).unwrap_or("");
            decor.set_prefix(format!("{above}{old}"));
        }
        None => {
            let old = doc.trailing().as_str().unwrap_or("");
            let text = format!("{old}{above}");
            doc.set_trailing(text);
        }
    }
}

/// `space config schema --json`: every setting, with what it is now, for the
/// panel to draw.
pub fn schema_json() -> Result<String> {
    let path = paths::config_file();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("can't read {}", paths::tilde(&path))),
    };
    Ok(serde_json::to_string(&schema(SETTINGS, &text))?)
}

/// What the config text says of a setting, or the default when it says
/// nothing; or why it can't be read, in which case a bad value anywhere else
/// in the file doesn't count.
fn read_value(setting: &Setting, table: &toml::Table) -> Result<Value> {
    let found = match table.get(setting.section()) {
        None => None,
        Some(toml::Value::Table(section)) => section.get(setting.name()),
        Some(_) => bail!("{} isn't a table", setting.section()),
    };
    let Some(found) = found else {
        return Ok((setting.get)(&Config::default()));
    };
    let mut section = toml::Table::new();
    section.insert(setting.name().to_string(), found.clone());
    let mut only = toml::Table::new();
    only.insert(setting.section().to_string(), toml::Value::Table(section));
    let config: Config = toml::Value::Table(only).try_into()?;
    Ok((setting.get)(&config))
}

fn schema(settings: &[Setting], text: &str) -> Json {
    let parsed: Result<toml::Table, String> = text.parse().map_err(|e: toml::de::Error| {
        format!(
            "config.toml can't be read ({}), so nothing here changes until it is fixed",
            e.message()
        )
    });
    let sections: Vec<Json> = SECTIONS
        .iter()
        .map(|(id, title)| json!({ "id": id, "title": title }))
        .collect();
    let settings: Vec<Json> = settings
        .iter()
        .map(|setting| {
            let section = setting.section();
            let title = SECTIONS
                .iter()
                .find(|(id, _)| *id == section)
                .map_or(section, |(_, title)| *title);
            let (value, problem) = match &parsed {
                Ok(table) => match read_value(setting, table) {
                    Ok(value) => (value, None),
                    Err(_) => (
                        (setting.get)(&Config::default()),
                        Some(
                            "config.toml has a value here that isn't valid, so the default shows; changing it replaces it"
                                .to_string(),
                        ),
                    ),
                },
                Err(why) => ((setting.get)(&Config::default()), Some(why.clone())),
            };
            let mut entry = json!({
                "key": setting.key,
                "section": section,
                "section_title": title,
                "label": setting.label,
                "help": setting.help,
                "type": setting.kind.name(),
                "default": json(&setting.default.value()),
                "value": json(&value),
            });
            if let Some(problem) = problem {
                entry["problem"] = json!(problem);
            }
            if !matches!(setting.kind, Kind::Bool) {
                let options: Vec<Json> = setting
                    .options()
                    .into_iter()
                    .filter(|opt| opt.available)
                    .map(|opt| {
                        let mut option = json!({ "value": opt.value, "label": opt.label });
                        if !opt.icon.is_empty() {
                            option["icon"] = json!(opt.icon);
                        }
                        if !opt.group.is_empty() {
                            option["group"] = json!(opt.group);
                        }
                        option
                    })
                    .collect();
                entry["options"] = json!(options);
            }
            if let Kind::Multi { none, groups, .. } = setting.kind {
                entry["none"] = json!(none);
                let groups: Vec<Json> = groups
                    .iter()
                    .map(|group| json!({ "name": group.name, "grid": group.grid }))
                    .collect();
                entry["groups"] = json!(groups);
            }
            entry
        })
        .collect();
    json!({ "version": 1, "sections": sections, "settings": settings })
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLES: &[Choice] = &[
        Choice {
            value: "browser",
            label: "Like a browser",
        },
        Choice {
            value: "omarchy",
            label: "Like Omarchy",
        },
    ];

    fn words(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn options() -> Vec<Opt> {
        let opt = |value: &str, label: &str, available: bool| Opt {
            value: value.to_string(),
            label: label.to_string(),
            icon: String::new(),
            available,
            group: if value == "b" {
                "Bs".into()
            } else {
                String::new()
            },
        };
        vec![
            opt("a", "A", true),
            opt("b", "B", true),
            opt("gone", "Gone", false),
        ]
    }

    /// One of each kind, for what the real settings don't have yet.
    static SAMPLE: &[Setting] = &[
        Setting {
            key: "sounds.enabled",
            label: "On",
            help: "",
            kind: Kind::Bool,
            default: Initial::Bool(false),
            get: |config| Value::Bool(config.sounds.enabled),
        },
        Setting {
            key: "tabs.style",
            label: "Tabs",
            help: "How they look.",
            kind: Kind::Choice(STYLES),
            default: Initial::Choice("browser"),
            get: |_| Value::Choice("omarchy".into()),
        },
        Setting {
            key: "tabs.pick",
            label: "Pick",
            help: "",
            kind: Kind::Multi {
                options,
                none: false,
                groups: &[],
                unknown: "no '{}'",
            },
            default: Initial::List(&["a"]),
            get: |_| Value::List(vec!["a".into(), "b".into()]),
        },
        Setting {
            key: "tabs.some",
            label: "Some",
            help: "",
            kind: Kind::Multi {
                options,
                none: true,
                groups: &[Group {
                    name: "Bs",
                    grid: true,
                }],
                unknown: "no '{}'",
            },
            default: Initial::List(&[]),
            get: |_| Value::List(Vec::new()),
        },
    ];

    fn sample(key: &str) -> &'static Setting {
        find_in(SAMPLE, key).unwrap()
    }

    #[test]
    fn every_registered_default_is_the_configs() {
        let config = Config::default();
        for setting in SETTINGS {
            assert_eq!(
                setting.default.value(),
                (setting.get)(&config),
                "{}'s default isn't Config's",
                setting.key
            );
        }
    }

    #[test]
    fn the_registry_is_well_formed() {
        let mut keys = std::collections::BTreeSet::new();
        for setting in SETTINGS {
            assert!(keys.insert(setting.key), "{} twice", setting.key);
            assert!(!setting.section().is_empty() && !setting.name().is_empty());
            assert!(
                SECTIONS.iter().any(|(id, _)| *id == setting.section()),
                "{} has no section",
                setting.key
            );
            assert!(!setting.label.is_empty());
            if let Kind::Choice(choices) = setting.kind {
                let Initial::Choice(default) = setting.default else {
                    panic!("{}: a choice's default is one of its options", setting.key);
                };
                assert!(choices.iter().any(|choice| choice.value == default));
            }
        }
        // A section's settings are together.
        let sections: Vec<&str> = SETTINGS.iter().map(|s| s.section()).collect();
        let mut seen: Vec<&str> = Vec::new();
        for section in sections {
            if seen.last() != Some(&section) {
                assert!(!seen.contains(&section), "{section} is split");
                seen.push(section);
            }
        }
    }

    #[test]
    fn the_tabs_style_is_the_configs_own() {
        let setting = find("tabs.style").unwrap();
        assert_eq!((setting.get)(&Config::default()), setting.default.value());
        for word in ["browser", "omarchy"] {
            let value = parse(setting, word).unwrap();
            let text = with_value("", "tabs", "style", Some(&value)).unwrap();
            let config: Config = toml::from_str(&text).unwrap();
            assert_eq!((setting.get)(&config), value, "{text}");
        }
        assert!(parse(setting, "chrome").is_err());
        assert_eq!(
            parse(setting, "Omarchy").unwrap(),
            Value::Choice("omarchy".into())
        );
    }

    #[test]
    fn reads_what_each_kind_takes() {
        let parse = |key: &str, raw: &str| parse(sample(key), raw);
        for (raw, on) in [
            ("true", true),
            ("ON", true),
            ("false", false),
            (" off ", false),
        ] {
            assert_eq!(
                parse("sounds.enabled", raw).unwrap(),
                Value::Bool(on),
                "{raw}"
            );
        }
        assert!(parse("sounds.enabled", "maybe").is_err());
        assert!(parse("sounds.enabled", "").is_err());

        assert_eq!(
            parse("tabs.style", "Omarchy").unwrap(),
            Value::Choice("omarchy".into())
        );
        let err = parse("tabs.style", "tabbed").unwrap_err().to_string();
        assert!(err.contains("browser, omarchy"), "{err}");

        let list = |values: &[&str]| Value::List(words(values));
        assert_eq!(parse("tabs.some", "A, b").unwrap(), list(&["a", "b"]));
        assert_eq!(
            parse("tabs.some", "gone").unwrap(),
            list(&["gone"]),
            "known, not offered"
        );
        assert_eq!(parse("tabs.some", "none").unwrap(), list(&[]));
        assert_eq!(parse("tabs.some", "").unwrap(), list(&[]));
        assert!(parse("tabs.some", "none,a").is_err());
        assert_eq!(parse("tabs.some", "c").unwrap_err().to_string(), "no 'c'");
        assert!(parse("tabs.pick", "none").is_err(), "this one has no none");
        assert!(parse("tabs.pick", "").is_err());
        assert_eq!(parse("tabs.pick", "b").unwrap(), list(&["b"]));
    }

    #[test]
    fn shows_values_as_they_are_set() {
        assert_eq!(show(&Value::Bool(true)), "true");
        assert_eq!(show(&Value::Choice("omarchy".into())), "omarchy");
        assert_eq!(
            show(&Value::List(words(&["files", "claude"]))),
            "files,claude"
        );
        assert_eq!(show(&Value::List(Vec::new())), "none");
    }

    #[test]
    fn an_unknown_key_says_which_there_are() {
        let err = find("restore.nothing").unwrap_err().to_string();
        assert!(err.contains("no setting 'restore.nothing'"), "{err}");
        assert!(
            err.contains("restore.auto") && err.contains("new_space.open"),
            "{err}"
        );
        assert!(find("sounds.done").is_err(), "file only");
    }

    #[test]
    fn edits_only_the_setting() {
        let set = |text: &str, key: &str, value: Value| {
            let setting = sample(key);
            with_value(text, setting.section(), setting.name(), Some(&value)).unwrap()
        };
        let open = |text: &str, values: &[&str]| {
            with_value(text, "new_space", "open", Some(&Value::List(words(values)))).unwrap()
        };
        assert_eq!(open("", &["files"]), "[new_space]\nopen = [\"files\"]\n");
        let text = "# mine\n[restore]\nauto = false # keep\n\n\
                    [new_space]\n# what opens\nopen = [\"terminal\"] # default\nother = 1\n\n\
                    [sounds]\nenabled = true\n";
        let edited = open(text, &["files", "claude"]);
        assert_eq!(
            edited,
            text.replace(
                "open = [\"terminal\"] # default",
                "open = [\"files\", \"claude\"] # default"
            )
        );
        assert!(open(text, &[]).contains("open = [] # default\n"));
        // A table that isn't there yet comes after the rest.
        let edited = open("[sounds]\nenabled = true # on\n", &["terminal"]);
        assert_eq!(
            edited,
            "[sounds]\nenabled = true # on\n\n[new_space]\nopen = [\"terminal\"]\n"
        );
        // Dotted and inline forms are edited in place.
        assert_eq!(
            open("new_space = { open = [\"files\"] }\n", &["terminal"]),
            "new_space = { open = [\"terminal\"] }\n"
        );
        assert!(
            with_value(
                "new_space = 3\n",
                "new_space",
                "open",
                Some(&Value::List(vec![]))
            )
            .is_err()
        );
        assert!(
            with_value(
                "[new_space\n",
                "new_space",
                "open",
                Some(&Value::List(vec![]))
            )
            .is_err()
        );

        // Other kinds, with their comments.
        assert_eq!(
            set(
                "[sounds]\n# loud?\nenabled = false # no\n",
                "sounds.enabled",
                Value::Bool(true)
            ),
            "[sounds]\n# loud?\nenabled = true # no\n"
        );
        assert_eq!(
            set("", "tabs.style", Value::Choice("omarchy".into())),
            "[tabs]\nstyle = \"omarchy\"\n"
        );
    }

    #[test]
    fn unsets_only_the_setting() {
        let unset =
            |text: &str, section: &str, name: &str| with_value(text, section, name, None).unwrap();
        let text = "# mine\n[restore]\nauto = false # keep\nbrowsers = false\n\n[sounds]\nenabled = true\n";
        assert_eq!(
            unset(text, "restore", "browsers"),
            "# mine\n[restore]\nauto = false # keep\n\n[sounds]\nenabled = true\n"
        );
        // Not there: as it was.
        assert_eq!(unset(text, "restore", "agents"), text);
        assert_eq!(unset(text, "new_space", "open"), text);
        // A table with nothing left goes, unless it has a comment of its own.
        assert_eq!(
            unset(
                "[sounds]\nenabled = true\n\n[restore]\nauto = false\n",
                "restore",
                "auto"
            ),
            "[sounds]\nenabled = true\n"
        );
        assert_eq!(unset("", "restore", "auto"), "");
        assert!(with_value("[restore\n", "restore", "auto", None).is_err());
    }

    #[test]
    fn the_schema_says_everything_the_panel_draws() {
        let schema = schema(SAMPLE, "");
        assert_eq!(schema["version"], 1);
        let settings = schema["settings"].as_array().unwrap();
        assert_eq!(settings.len(), 4);

        assert_eq!(
            settings[0],
            json!({
                "key": "sounds.enabled", "section": "sounds", "section_title": "Sounds",
                "label": "On", "help": "", "type": "bool", "default": false, "value": false,
            })
        );
        assert_eq!(
            settings[1],
            json!({
                "key": "tabs.style", "section": "tabs", "section_title": "Tabs",
                "label": "Tabs", "help": "How they look.", "type": "choice",
                "default": "browser", "value": "omarchy",
                "options": [
                    { "value": "browser", "label": "Like a browser" },
                    { "value": "omarchy", "label": "Like Omarchy" },
                ],
            })
        );
        assert_eq!(settings[2]["type"], "multi");
        assert_eq!(settings[2]["none"], false);
        assert_eq!(settings[2]["value"], json!(["a", "b"]));
        assert_eq!(
            settings[2]["options"],
            json!([{ "value": "a", "label": "A" }, { "value": "b", "label": "B", "group": "Bs" }]),
            "what isn't available isn't offered, and a group is named"
        );
        assert_eq!(settings[2]["groups"], json!([]));
        assert_eq!(settings[3]["none"], true);
        assert_eq!(
            settings[3]["groups"],
            json!([{ "name": "Bs", "grid": true }])
        );
    }

    #[test]
    fn every_setting_round_trips_through_the_file_and_config() {
        for setting in SETTINGS {
            let default = setting.default.value();
            let other = match setting.kind {
                Kind::Bool => Value::Bool(default != Value::Bool(true)),
                Kind::Choice(choices) => Value::Choice(
                    choices
                        .iter()
                        .map(|choice| choice.value.to_string())
                        .find(|value| Value::Choice(value.clone()) != default)
                        .expect("a choice has two options"),
                ),
                Kind::Multi { .. } => {
                    let first = setting
                        .options()
                        .into_iter()
                        .find(|opt| Value::List(vec![opt.value.clone()]) != default);
                    Value::List(vec![first.expect("an option to differ").value])
                }
            };
            assert_ne!(other, default, "{}", setting.key);
            let text = with_value("", setting.section(), setting.name(), Some(&other)).unwrap();
            let config: Config =
                toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {text}: {e}", setting.key));
            assert_eq!(
                (setting.get)(&config),
                other,
                "{} isn't {text}",
                setting.key
            );
        }
    }

    #[test]
    fn a_bad_value_doesnt_blank_the_other_settings() {
        let schema = schema(
            SETTINGS,
            "[sounds]\nenabled = \"yes\"\n[restore]\nauto = false\n",
        );
        let settings = schema["settings"].as_array().unwrap();
        assert_eq!(settings.len(), SETTINGS.len());
        let by = |key: &str| settings.iter().find(|s| s["key"] == key).unwrap();
        let sounds = by("sounds.enabled");
        assert_eq!(sounds["value"], false, "the default shows");
        assert!(sounds["problem"].as_str().unwrap().contains("isn't valid"));
        assert_eq!(by("restore.auto")["value"], false, "the others are read");
        assert!(by("restore.auto").get("problem").is_none());
        assert!(by("new_space.open").get("problem").is_none());

        // Setting it replaces the bad value, and the rest stays.
        let fixed = with_value(
            "[sounds]\nenabled = \"yes\" # hm\n[restore]\nauto = false\n",
            "sounds",
            "enabled",
            Some(&Value::Bool(true)),
        )
        .unwrap();
        assert_eq!(
            fixed,
            "[sounds]\nenabled = true # hm\n[restore]\nauto = false\n"
        );

        // A file that isn't TOML says so on every setting.
        let broken = schema_of_broken();
        assert!(
            broken
                .iter()
                .all(|s| s["problem"].as_str().unwrap().contains("can't be read"))
        );
    }

    fn schema_of_broken() -> Vec<Json> {
        schema(SETTINGS, "[restore\nauto = 1\n")["settings"]
            .as_array()
            .unwrap()
            .clone()
    }

    #[test]
    fn unsetting_the_last_key_drops_the_table_but_keeps_the_file_comment() {
        let unset =
            |text: &str, section: &str, name: &str| with_value(text, section, name, None).unwrap();
        assert_eq!(
            unset("# mine\n[restore]\nauto = false\n", "restore", "auto"),
            "# mine\n"
        );
        assert_eq!(
            unset(
                "# mine\n[restore]\nauto = false\n\n[sounds]\nenabled = true\n",
                "restore",
                "auto"
            ),
            "# mine\n\n[sounds]\nenabled = true\n"
        );
        assert_eq!(unset("restore = { auto = false }\n", "restore", "auto"), "");
        assert_eq!(
            unset(
                "a = 1\nrestore = { auto = false }\nb = 2\n",
                "restore",
                "auto"
            ),
            "a = 1\nb = 2\n"
        );
        assert_eq!(
            unset(
                "restore = { auto = false, agents = true }\n",
                "restore",
                "auto"
            ),
            "restore = { agents = true }\n"
        );
        // A comment on the header's own line stays with it.
        assert_eq!(
            unset("[restore] # why\nauto = false\n", "restore", "auto"),
            "[restore] # why\n"
        );
    }
}
