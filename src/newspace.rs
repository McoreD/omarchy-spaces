//! What a new space opens (`[new_space] open` in the config, `space new
//! --open`): a terminal, Omarchy's file manager, any of Omarchy's agents, or
//! nothing. Which agents there are is Omarchy's to say, so the list is read
//! from Omarchy's own `omarchy-default-agent`, not kept here, and an agent is
//! installed when its command is on the PATH, as Omarchy decides it
//! (`omarchy-install-ai-cli --check`).

use std::fs;
use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::config::Config;
use crate::launch;
use crate::paths;
use crate::settings::Opt;

pub const TERMINAL: &str = "terminal";
pub const FILES: &str = "files";
/// The word for nothing, on the command line.
pub const NONE: &str = "none";
/// The setting, as `space config` names it.
pub const KEY: &str = "new_space.open";

/// One thing a new space opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Terminal,
    Files,
    /// Omarchy's agent of this id, in a terminal.
    Agent(String),
}

impl Item {
    pub fn id(&self) -> &str {
        match self {
            Item::Terminal => TERMINAL,
            Item::Files => FILES,
            Item::Agent(id) => id,
        }
    }
}

/// An agent Omarchy knows.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Agent {
    pub id: String,
    /// What Omarchy calls it, `Claude Code`.
    pub name: String,
    /// Its icon's name in the icon theme, as Omarchy's launchers have it.
    pub icon: String,
    pub installed: bool,
}

/// Omarchy's agents, in its order, from the script that sets the default one:
/// its usage line lists the ids and its cases name them. None when Omarchy
/// isn't there.
pub fn agents() -> Vec<Agent> {
    let Some(script) = default_agent_script().and_then(|path| fs::read_to_string(path).ok()) else {
        return Vec::new();
    };
    parse_agents(&script, installed)
}

fn default_agent_script() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("omarchy-default-agent"))
            .find(|script| script.is_file())
    });
    on_path.or_else(|| {
        let shipped = PathBuf::from("/usr/share/omarchy/bin/omarchy-default-agent");
        shipped.is_file().then_some(shipped)
    })
}

/// The agents `omarchy-default-agent`'s text lists. `local` is a way to run
/// pi, not an agent of its own.
fn parse_agents(script: &str, installed: impl Fn(&str) -> bool) -> Vec<Agent> {
    let ids = script
        .lines()
        .find_map(|line| line.strip_prefix("# omarchy:args=["))
        .and_then(|args| args.strip_suffix(']'))
        .unwrap_or_default();
    // `claude | claude-code) agent="claude"; name="Claude Code" ;;`
    let name_of = |id: &str| {
        let case = format!("agent=\"{id}\"; name=\"");
        script.lines().find_map(|line| {
            let rest = &line[line.find(&case)? + case.len()..];
            Some(rest.split('"').next()?.to_string())
        })
    };
    ids.split('|')
        .map(str::trim)
        .filter(|id| !id.is_empty() && *id != "local" && is_id(id))
        .map(|id| Agent {
            id: id.to_string(),
            name: name_of(id).unwrap_or_else(|| id.to_string()),
            icon: format!("omarchy-agent-{id}"),
            installed: installed(id),
        })
        .collect()
}

fn is_id(id: &str) -> bool {
    id.bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether the agent's command is on the PATH, not counting the account
/// dispatchers Omarchy puts in front of it, which aren't the agent itself.
fn installed(id: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path)
            .filter(|dir| !dir.ends_with(".local/share/omarchy/agent-accounts/bin"))
            .any(|dir| dir.join(id).is_file())
    })
}

/// What `values` mean, as items. A value Spaces doesn't know, or an agent or
/// Files that isn't installed, has its say in `problems`, and isn't opened.
/// Each opens once.
pub fn resolve(
    values: &[String],
    agents: &[Agent],
    files_installed: bool,
) -> (Vec<Item>, Vec<Problem>) {
    let mut items: Vec<Item> = Vec::new();
    let mut problems = Vec::new();
    for value in values {
        let word = value.trim().to_lowercase();
        let item = match word.as_str() {
            TERMINAL => Some(Item::Terminal),
            FILES if files_installed => Some(Item::Files),
            FILES => {
                problems.push(Problem::NotInstalled(value.clone()));
                None
            }
            _ => match agents.iter().find(|agent| agent.id == word) {
                Some(agent) if agent.installed => Some(Item::Agent(agent.id.clone())),
                Some(_) => {
                    problems.push(Problem::NotInstalled(value.clone()));
                    None
                }
                None => {
                    problems.push(Problem::Unknown(value.clone()));
                    None
                }
            },
        };
        if let Some(item) = item
            && !items.contains(&item)
        {
            items.push(item);
        }
    }
    (items, problems)
}

/// Why a value isn't opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    Unknown(String),
    NotInstalled(String),
}

impl Problem {
    pub fn said(&self) -> String {
        match self {
            Problem::Unknown(value) => format!(
                "there's no '{value}' to open in {KEY}: use terminal, files or an Omarchy agent's id"
            ),
            Problem::NotInstalled(value) => {
                format!("'{value}' in {KEY} isn't installed, so a new space skips it")
            }
        }
    }
}

/// What `space new` opens: `--open`'s words, else the config's setting, which
/// is for a space you open yourself (the `+`, a key): a script's `--silent` or
/// `-- <cmd>` gets just a terminal, as it always has, unless it says
/// `--open`. A command to run needs a terminal, so one is added, first, if
/// there isn't. A setting with items of which none could be opened gives a
/// terminal rather than an empty space, which only `open = []` asks for.
pub fn for_new_space(arg: Option<&str>, command: bool, silent: bool) -> Result<Vec<Item>> {
    let (values, from_arg) = match arg {
        Some(raw) => (parse_arg(raw)?, true),
        None if command || silent => return Ok(vec![Item::Terminal]),
        None => (Config::load_or_default().new_space.open, false),
    };
    let agents = agents();
    let (mut items, problems) = resolve(&values, &agents, launch::on_path("folio"));
    for problem in &problems {
        if from_arg {
            bail!("{}", problem.said().replace(KEY, "--open"));
        }
        eprintln!(
            "space: warning: {}, in {}",
            problem.said(),
            paths::tilde(&paths::config_file())
        );
    }
    if items.is_empty() && !values.is_empty() && !from_arg {
        items.push(Item::Terminal);
    }
    if command && !items.contains(&Item::Terminal) {
        items.insert(0, Item::Terminal);
    }
    Ok(items)
}

/// `--open`'s words: comma-separated, `none` or nothing for an empty space.
pub fn parse_arg(raw: &str) -> Result<Vec<String>> {
    let words: Vec<String> = raw
        .split(',')
        .map(|word| word.trim().to_string())
        .filter(|word| !word.is_empty())
        .collect();
    if words.iter().any(|word| word.eq_ignore_ascii_case(NONE)) {
        if words.len() > 1 {
            bail!("'none' can't go with anything else to open");
        }
        return Ok(Vec::new());
    }
    Ok(words)
}

/// What `new_space.open` offers, for the settings panel and `set`: Terminal,
/// Files, then Omarchy's agents by name, A to Z whatever order Omarchy lists
/// them in. An agent that isn't installed is a value `set` takes but the panel
/// doesn't offer.
pub fn options() -> Vec<Opt> {
    options_with(agents())
}

fn options_with(mut agents: Vec<Agent>) -> Vec<Opt> {
    agents.sort_by_key(|agent| agent.name.to_lowercase());
    let mut options = vec![
        Opt {
            value: TERMINAL.into(),
            label: "Terminal".into(),
            icon: "utilities-terminal".into(),
            available: true,
            group: String::new(),
        },
        Opt {
            value: FILES.into(),
            label: "Files".into(),
            icon: "folio".into(),
            available: true,
            group: String::new(),
        },
    ];
    options.extend(agents.into_iter().map(|agent| Opt {
        value: agent.id,
        label: agent.name,
        icon: agent.icon,
        available: agent.installed,
        // Drawn together, as a grid (`settings.rs`).
        group: "Agents".into(),
    }));
    options
}

/// `space config agents`: Omarchy's agents as JSON, for the settings panel.
pub fn agents_json() -> Result<String> {
    Ok(serde_json::to_string(&agents())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = "#!/bin/bash\n\n# omarchy:summary=Set\n\
        # omarchy:args=[local|pi|claude|agy|hermes]\n\
        case \"$1\" in\n\
        local) agent=\"local\"; name=\"Local model\" ;;\n\
        pi) agent=\"pi\"; name=\"Pi\" ;;\n\
        claude | claude-code) agent=\"claude\"; name=\"Claude Code\" ;;\n\
        agy | antigravity) agent=\"agy\"; name=\"Antigravity\" ;;\n\
        esac\n";

    fn agents_here() -> Vec<Agent> {
        parse_agents(SCRIPT, |id| id != "agy")
    }

    fn words(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn reads_omarchys_agents_from_its_script() {
        let agents = agents_here();
        let ids: Vec<&str> = agents.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["pi", "claude", "agy", "hermes"], "local isn't one");
        assert_eq!(agents[1].name, "Claude Code");
        assert_eq!(agents[1].icon, "omarchy-agent-claude");
        assert_eq!(agents[3].name, "hermes", "unnamed: its id");
        assert!(agents[1].installed && !agents[2].installed);
        assert!(parse_agents("nothing here", |_| true).is_empty());
    }

    #[test]
    fn says_what_a_new_space_opens() {
        let agents = agents_here();
        let (items, problems) = resolve(
            &words(&["Terminal", "files", "claude", "claude", "claud", "agy"]),
            &agents,
            true,
        );
        assert_eq!(
            items,
            [Item::Terminal, Item::Files, Item::Agent("claude".into())]
        );
        assert_eq!(
            problems,
            [
                Problem::Unknown("claud".into()),
                Problem::NotInstalled("agy".into())
            ]
        );
        assert!(problems[0].said().contains("no 'claud' to open"));

        let (items, problems) = resolve(&words(&["files"]), &agents, false);
        assert!(items.is_empty());
        assert_eq!(problems, [Problem::NotInstalled("files".into())]);
        assert_eq!(resolve(&[], &agents, true), (Vec::new(), Vec::new()));
    }

    #[test]
    fn reads_the_open_argument() {
        assert_eq!(parse_arg("files,claude").unwrap(), ["files", "claude"]);
        assert_eq!(parse_arg(" files , ,claude ").unwrap(), ["files", "claude"]);
        assert!(parse_arg("").unwrap().is_empty());
        assert!(parse_arg("none").unwrap().is_empty());
        assert!(parse_arg("NONE").unwrap().is_empty());
        assert!(parse_arg("none,files").is_err());
    }

    #[test]
    fn offers_terminal_files_then_agents_by_name() {
        let agent = |id: &str, name: &str, installed| Agent {
            id: id.into(),
            name: name.into(),
            icon: format!("omarchy-agent-{id}"),
            installed,
        };
        let options = options_with(vec![
            agent("pi", "Pi", true),
            agent("crush", "crush", true),
            agent("claude", "Claude Code", true),
            agent("agy", "Antigravity", false),
        ]);
        let ids: Vec<&str> = options.iter().map(|o| o.value.as_str()).collect();
        assert_eq!(ids, ["terminal", "files", "agy", "claude", "crush", "pi"]);
        let offered: Vec<bool> = options.iter().map(|o| o.available).collect();
        assert_eq!(offered, [true, true, false, true, true, true]);
        assert_eq!(options[3].label, "Claude Code");
        assert_eq!(options[3].icon, "omarchy-agent-claude");
        let groups: Vec<&str> = options.iter().map(|o| o.group.as_str()).collect();
        assert_eq!(groups, ["", "", "Agents", "Agents", "Agents", "Agents"]);
    }
}
