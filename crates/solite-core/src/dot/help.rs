use serde::Serialize;

/// A single entry in the `.help` command reference.
struct HelpEntry {
    name: &'static str,
    aliases: &'static [&'static str],
    usage: &'static str,
    description: &'static str,
}

/// Canonical dot command reference, shared by `.help` and `.help <command>`.
const COMMANDS: &[HelpEntry] = &[
    HelpEntry {
        name: "ask",
        aliases: &[],
        usage: ".ask <question>",
        description: "Ask the AI assistant (shorthand: ?<question>; requires OPENROUTER_API_KEY)",
    },
    HelpEntry {
        name: "bench",
        aliases: &[],
        usage: ".bench\n<query>",
        description: "Benchmark the query on the following lines",
    },
    HelpEntry {
        name: "call",
        aliases: &[],
        usage: ".call [file.sql] <procedure>",
        description: "Call a procedure defined with a `-- name:` annotation",
    },
    HelpEntry {
        name: "clear",
        aliases: &["c"],
        usage: ".clear",
        description: "Clear the screen",
    },
    HelpEntry {
        name: "dotenv",
        aliases: &["loadenv"],
        usage: ".dotenv",
        description: "Load environment variables from a .env file",
    },
    HelpEntry {
        name: "env",
        aliases: &[],
        usage: ".env set <name> <value> | unset <name>",
        description: "Manage environment variables",
    },
    HelpEntry {
        name: "export",
        aliases: &[],
        usage: ".export <path>\n<query>",
        description: "Export query results to a file (format from extension; .gz/.zst supported)",
    },
    HelpEntry {
        name: "graphviz",
        aliases: &["gv"],
        usage: ".graphviz",
        description: "Generate an ERD of the schema in Graphviz DOT format",
    },
    HelpEntry {
        name: "help",
        aliases: &[],
        usage: ".help [command]",
        description: "List dot commands, or show usage for one command",
    },
    HelpEntry {
        name: "load",
        aliases: &[],
        usage: ".load <path>",
        description: "Load a SQLite extension",
    },
    HelpEntry {
        name: "open",
        aliases: &[],
        usage: ".open <path>",
        description: "Open a different database",
    },
    HelpEntry {
        name: "param",
        aliases: &["parameter"],
        usage: ".param set <name> <value> | unset <name> | list | clear",
        description: "Manage SQL query parameters",
    },
    HelpEntry {
        name: "print",
        aliases: &[],
        usage: ".print <message>",
        description: "Print a message",
    },
    HelpEntry {
        name: "run",
        aliases: &[],
        usage: ".run <file.sql> [procedure] [--key=value ...]",
        description: "Execute a SQL file inline, optionally calling one procedure with parameters",
    },
    HelpEntry {
        name: "schema",
        aliases: &[],
        usage: ".schema [pattern]",
        description: "Show CREATE statements, optionally filtered by a LIKE pattern on object/table names",
    },
    HelpEntry {
        name: "sh",
        aliases: &[],
        usage: ".sh <command>",
        description: "Run a shell command (shorthand: !<command>)",
    },
    #[cfg(feature = "ritestream")]
    HelpEntry {
        name: "stream",
        aliases: &[],
        usage: ".stream sync <url> | restore <url>",
        description: "Sync WAL changes to a replica, or restore from one",
    },
    HelpEntry {
        name: "tables",
        aliases: &[],
        usage: ".tables [schema]",
        description: "List tables and views, optionally for an attached schema",
    },
    HelpEntry {
        name: "timer",
        aliases: &[],
        usage: ".timer on|off",
        description: "Toggle query timing",
    },
    HelpEntry {
        name: "tui",
        aliases: &[],
        usage: ".tui",
        description: "Browse the database in the TUI (REPL only)",
    },
    HelpEntry {
        name: "vegalite",
        aliases: &["vl"],
        usage: ".vegalite <mark>\n<query>",
        description: "Render a Vega-Lite chart from the query on the following lines",
    },
];

/// Primary names of all dot commands (no aliases).
pub fn command_names() -> impl Iterator<Item = &'static str> {
    COMMANDS.iter().map(|entry| entry.name)
}

/// Primary names plus aliases, sorted — the single source of truth for
/// frontends offering or styling dot-command names (completion,
/// highlighting). The parser accepts aliases, so they are offered too.
pub fn command_names_with_aliases() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = COMMANDS
        .iter()
        .flat_map(|entry| std::iter::once(entry.name).chain(entry.aliases.iter().copied()))
        .collect();
    names.sort_unstable();
    names
}

const SHORTHANDS: &str = "\
Shorthands:
  !<command>    same as .sh <command>
  ?<question>   same as .ask <question>
  \\e            open $EDITOR with a scratch buffer (REPL only)";

#[derive(Serialize, Debug, PartialEq)]
pub struct HelpCommand {
    /// Specific command to show usage for; `None` lists everything.
    pub topic: Option<String>,
}

impl HelpCommand {
    /// Render the help text. Callers print it however their context requires.
    pub fn execute(&self) -> String {
        match &self.topic {
            None => Self::listing(),
            Some(topic) => Self::topic_help(topic),
        }
    }

    fn listing() -> String {
        let mut out = String::from("Dot commands:\n");
        for entry in COMMANDS {
            // multi-line usages list only the first line; .help <cmd> has the rest
            let usage = entry.usage.lines().next().unwrap_or(entry.usage);
            let name = match entry.aliases {
                [] => usage.to_string(),
                aliases => format!("{} (alias: {})", usage, aliases.join(", ")),
            };
            out.push_str(&format!("  {name}\n      {}\n", entry.description));
        }
        out.push('\n');
        out.push_str(SHORTHANDS);
        out
    }

    fn topic_help(topic: &str) -> String {
        let topic = topic.trim().trim_start_matches('.').to_lowercase();
        match COMMANDS
            .iter()
            .find(|e| e.name == topic || e.aliases.contains(&topic.as_str()))
        {
            Some(entry) => format!("{}\n  {}", entry.usage, entry.description),
            None => format!(
                "Unknown command '{topic}'. Use .help to list all commands."
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_contains_every_command() {
        let listing = HelpCommand { topic: None }.execute();
        for entry in COMMANDS {
            assert!(listing.contains(entry.description), "missing {}", entry.name);
        }
        assert!(listing.contains("!<command>"));
        assert!(listing.contains("?<question>"));
    }

    #[test]
    fn topic_lookup_by_name_and_alias() {
        let by_name = HelpCommand { topic: Some("export".into()) }.execute();
        assert!(by_name.contains(".export <path>"));

        let by_alias = HelpCommand { topic: Some("vl".into()) }.execute();
        assert!(by_alias.contains(".vegalite <mark>"));

        let with_dot = HelpCommand { topic: Some(".tables".into()) }.execute();
        assert!(with_dot.contains(".tables [schema]"));
    }

    #[test]
    fn unknown_topic_is_graceful() {
        let out = HelpCommand { topic: Some("nope".into()) }.execute();
        assert!(out.contains("Unknown command 'nope'"));
        assert!(out.contains(".help"));
    }
}
