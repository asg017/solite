use super::highlighter::CTP_MOCHA_THEME;
use rustyline::completion::{Completer, Pair};
use rustyline::Result;
use solite_completion::{
    detect_context, get_completions, CompletionItem, CompletionKind, SchemaSource,
};
use solite_core::sqlite::quote_identifier;
use solite_core::Runtime;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

/// A SchemaSource implementation that queries a live SQLite database connection.
/// Shared with the Jupyter kernel's complete_request handler.
pub(crate) struct LiveSchemaSource<'a> {
    runtime: &'a Runtime,
}

impl<'a> LiveSchemaSource<'a> {
    pub(crate) fn new(runtime: &'a Runtime) -> Self {
        Self { runtime }
    }
}

impl SchemaSource for LiveSchemaSource<'_> {
    fn table_names(&self) -> Vec<String> {
        let mut stmt = match self.runtime.connection.prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        ) {
            Ok((_, Some(stmt))) => stmt,
            _ => return vec![],
        };

        let mut names = vec![];
        while let Ok(Some(row)) = stmt.next() {
            if let Some(first) = row.first() {
                names.push(first.as_str().to_string());
            }
        }
        names
    }

    fn columns_for_table(&self, table: &str) -> Option<Vec<String>> {
        // Use PRAGMA table_info to get column names
        let sql = format!("PRAGMA table_info({})", quote_identifier(table));
        let mut stmt = match self.runtime.connection.prepare(&sql) {
            Ok((_, Some(stmt))) => stmt,
            _ => return None,
        };

        let mut columns = vec![];
        while let Ok(Some(row)) = stmt.next() {
            // PRAGMA table_info returns: cid, name, type, notnull, dflt_value, pk
            if let Some(name_col) = row.get(1) {
                columns.push(name_col.as_str().to_string());
            }
        }

        if columns.is_empty() {
            None
        } else {
            Some(columns)
        }
    }

    fn columns_for_table_with_rowid(&self, table: &str) -> Option<Vec<String>> {
        let mut columns = self.columns_for_table(table)?;

        // Check if the table is WITHOUT ROWID
        let sql = format!(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name = {}",
            quote_identifier(table)
        );
        let mut stmt = match self.runtime.connection.prepare(&sql) {
            Ok((_, Some(stmt))) => stmt,
            _ => return Some(columns),
        };

        if let Ok(Some(row)) = stmt.next() {
            if let Some(sql_col) = row.first() {
                let sql_text = sql_col.as_str().to_uppercase();
                if !sql_text.contains("WITHOUT ROWID") {
                    columns.insert(0, "rowid".to_string());
                }
            }
        }

        Some(columns)
    }

    fn index_names(&self) -> Vec<String> {
        let mut stmt = match self.runtime.connection.prepare(
            "SELECT name FROM sqlite_master WHERE type='index' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        ) {
            Ok((_, Some(stmt))) => stmt,
            _ => return vec![],
        };

        let mut names = vec![];
        while let Ok(Some(row)) = stmt.next() {
            if let Some(first) = row.first() {
                names.push(first.as_str().to_string());
            }
        }
        names
    }

    fn view_names(&self) -> Vec<String> {
        let mut stmt = match self.runtime.connection.prepare(
            "SELECT name FROM sqlite_master WHERE type='view' ORDER BY name",
        ) {
            Ok((_, Some(stmt))) => stmt,
            _ => return vec![],
        };

        let mut names = vec![];
        while let Ok(Some(row)) = stmt.next() {
            if let Some(first) = row.first() {
                names.push(first.as_str().to_string());
            }
        }
        names
    }

    fn function_names(&self) -> Vec<String> {
        let mut stmt = match self.runtime.connection.prepare(
            "SELECT DISTINCT name FROM pragma_function_list ORDER BY name",
        ) {
            Ok((_, Some(stmt))) => stmt,
            _ => return vec![],
        };

        let mut names = vec![];
        while let Ok(Some(row)) = stmt.next() {
            if let Some(first) = row.first() {
                names.push(first.as_str().to_string());
            }
        }
        names
    }

    fn function_nargs(&self, name: &str) -> Option<Vec<i32>> {
        let sql = "SELECT DISTINCT narg FROM pragma_function_list WHERE lower(name) = lower(?) ORDER BY narg";
        let mut stmt = match self.runtime.connection.prepare(sql) {
            Ok((_, Some(stmt))) => stmt,
            _ => return None,
        };
        stmt.bind_text(1, name).ok()?;

        let mut nargs = vec![];
        while let Ok(Some(row)) = stmt.next() {
            if let Some(val) = row.first() {
                nargs.push(val.as_int64() as i32);
            }
        }

        if nargs.is_empty() {
            None
        } else {
            Some(nargs)
        }
    }
}

/// Convert a CompletionItem to a rustyline Pair for display.
fn to_pair(item: CompletionItem) -> Pair {
    let display = match item.kind {
        CompletionKind::Column => format!("ᶜ {}", item.label),
        CompletionKind::Table => format!("ᵗ {}", item.label),
        CompletionKind::Cte => format!("ᵗ {}", item.label),
        CompletionKind::Keyword => CTP_MOCHA_THEME.style_keyword(&item.label),
        CompletionKind::Index => format!("ⁱ {}", item.label),
        CompletionKind::View => format!("ᵛ {}", item.label),
        CompletionKind::Function => format!("ᶠ {}", item.label),
        CompletionKind::Operator => item.label.clone(),
    };

    let replacement = item.insert_text.unwrap_or(item.label);

    Pair {
        display,
        replacement,
    }
}

/// Dot command names (primary names plus aliases) offered by tab completion
/// (REPL and Jupyter) and styled by the REPL highlighter. Derived from the
/// canonical registry in `solite_core::dot::help` so new commands and
/// feature-gated ones (e.g. `stream`) appear automatically.
pub(crate) static DOT_COMMAND_NAMES: std::sync::LazyLock<Vec<&'static str>> =
    std::sync::LazyLock::new(solite_core::dot::command_names_with_aliases);

/// Find the start position for completion replacement.
/// Returns the byte position of the start of the current word being typed.
/// `.` is a word boundary so a qualified reference like `t.na` completes
/// only the column part, preserving the `t.` qualifier.
pub(crate) fn find_completion_start(line: &str, pos: usize) -> usize {
    let line_before_cursor = &line[..pos];

    // Find the last whitespace or operator before cursor
    line_before_cursor
        .rfind(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')' || c == '.')
        .map(|idx| idx + 1)
        .unwrap_or(0)
}

/// Case-insensitive prefix filter for completion items. The completion
/// engine returns unfiltered candidate lists for most contexts (the LSP
/// client filters on its side), so REPL/Jupyter frontends filter here.
pub(crate) fn item_matches_prefix(item: &CompletionItem, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    let prefix = prefix.to_lowercase();
    item.label.to_lowercase().starts_with(&prefix)
        || item
            .insert_text
            .as_deref()
            .is_some_and(|text| text.to_lowercase().starts_with(&prefix))
}

/// What a dot command's argument at a given position should complete to.
/// Pure decision (no runtime/filesystem access) so it can be unit-tested; the
/// actual candidate production lives in `ReplCompleter::complete_dot`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DotArgKind {
    /// Nothing to complete at this position.
    None,
    /// Any file path in the current directory.
    AnyFile,
    /// Script files only (`.sql`/`.ipynb`).
    ScriptFile,
    /// Script files *or* an already-registered procedure name (`.call <proc>`).
    ScriptFileOrProcedure,
    /// Procedure names (from the referenced file and/or the live runtime).
    Procedure,
    /// A fixed set of subcommand keywords (e.g. `set`/`unset`).
    Keywords(&'static [&'static str]),
    /// Names of currently-defined SQL parameters.
    ParamName,
    /// Attached database/schema names.
    SchemaName,
    /// Dot-command names (for `.help`).
    DotCommandName,
}

/// Decide what the current dot-command argument should complete to, given the
/// command name (without the leading `.`), the zero-based index of the argument
/// being typed, and the arguments already completed before it.
pub(crate) fn classify_dot_arg(
    command: &str,
    arg_index: usize,
    prior_args: &[&str],
) -> DotArgKind {
    use DotArgKind::*;
    match command {
        "open" | "load" => {
            if arg_index == 0 {
                AnyFile
            } else {
                None
            }
        }
        // `.run file.sql [proc]`
        "run" => match arg_index {
            0 => ScriptFile,
            1 => Procedure,
            _ => None,
        },
        // `.call [file.sql] proc` — the first arg may be a file or the proc itself.
        "call" => match arg_index {
            0 => ScriptFileOrProcedure,
            1 => Procedure,
            _ => None,
        },
        "param" | "parameter" => match arg_index {
            0 => Keywords(&["set", "unset", "list", "clear"]),
            1 if prior_args.first() == Some(&"unset") => ParamName,
            _ => None,
        },
        "timer" => {
            if arg_index == 0 {
                Keywords(&["on", "off"])
            } else {
                None
            }
        }
        "env" => {
            if arg_index == 0 {
                Keywords(&["set", "unset"])
            } else {
                None
            }
        }
        "tables" => {
            if arg_index == 0 {
                SchemaName
            } else {
                None
            }
        }
        "help" => {
            if arg_index == 0 {
                DotCommandName
            } else {
                None
            }
        }
        // `.export <path>` then a SQL body on following lines; only the path
        // (first arg) is completable here.
        "export" => {
            if arg_index == 0 {
                AnyFile
            } else {
                None
            }
        }
        _ => None,
    }
}

/// A parsed dot-command line, split at the cursor.
struct DotLine<'a> {
    /// Command name without the leading `.`.
    command: &'a str,
    /// Arguments fully entered before the one being typed.
    prior_args: Vec<&'a str>,
    /// The partial argument under the cursor (may be empty after a space).
    current: &'a str,
    /// Byte offset where `current` begins (the rustyline replacement start).
    current_start: usize,
}

/// Split a dot line (`.cmd arg1 arg2 cur`) at `pos` into command, prior args,
/// and the current token. Returns `None` when still typing the command name
/// itself (no space yet), which keeps today's command-name completion path.
fn parse_dot_line(line: &str, pos: usize) -> Option<DotLine<'_>> {
    let before = &line[..pos];
    if !before.starts_with('.') || !before.contains(' ') {
        return None;
    }
    let current_start = before
        .rfind(char::is_whitespace)
        .map(|i| i + 1)
        .unwrap_or(0);
    let current = &before[current_start..];

    let head = before[..current_start].trim_start_matches('.');
    let mut tokens = head.split_whitespace();
    let command = tokens.next().unwrap_or("");
    let prior_args: Vec<&str> = tokens.collect();

    Some(DotLine {
        command,
        prior_args,
        current,
        current_start,
    })
}

/// Script files solite can run: `.sql` / `.ipynb` (case-insensitive). Local to
/// the REPL completer; the shell-completion subsystem has its own copy.
fn is_script_file(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "sql" | "ipynb"))
}

/// List cwd entries matching the partial `token`, keeping files for which
/// `keep` returns true and always keeping directories (with a trailing `/` so
/// multi-segment paths can be navigated). Dependency-free (`std::fs::read_dir`).
/// Returned strings include any directory prefix already typed in `token`.
fn complete_path_token(token: &str, keep: impl Fn(&Path) -> bool) -> Vec<String> {
    // Split the typed token into an already-entered directory prefix and the
    // partial final component.
    let (dir_prefix, partial) = match token.rfind('/') {
        Some(i) => (&token[..=i], &token[i + 1..]),
        None => ("", token),
    };
    let read_root = if dir_prefix.is_empty() {
        Path::new(".")
    } else {
        Path::new(dir_prefix)
    };

    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(read_root) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(partial) {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                out.push(format!("{dir_prefix}{name}/"));
            } else if keep(&path) {
                out.push(format!("{dir_prefix}{name}"));
            }
        }
    }
    out.sort();
    out
}

/// Procedure names declared via `-- name:` lines in a script file. Best-effort:
/// returns an empty vec on any read error (completion must never fail/panic).
fn procedure_names_in_file(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| solite_core::procedure::parse_name_line(line).map(|(name, ..)| name))
        .collect()
}

pub(crate) struct ReplCompleter {
    runtime: Rc<RefCell<Runtime>>,
}

impl ReplCompleter {
    pub fn new(runtime: Rc<RefCell<Runtime>>) -> Self {
        Self { runtime }
    }

    fn complete_dot(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> Result<(usize, Vec<Pair>)> {
        if !line.starts_with('.') {
            return Ok((0, vec![]));
        }

        // No space yet: still completing the command name itself.
        let Some(dot) = parse_dot_line(line, pos) else {
            let prefix = &line[1..pos];
            let candidates = DOT_COMMAND_NAMES
                .iter()
                .filter(|v| v.starts_with(prefix))
                .map(|v| Pair {
                    display: CTP_MOCHA_THEME.style_dot(v),
                    replacement: format!("{v} "),
                })
                .collect();
            return Ok((1, candidates));
        };

        let kind = classify_dot_arg(dot.command, dot.prior_args.len(), &dot.prior_args);
        let values = self.dot_arg_values(kind, &dot);

        // Directories keep their trailing `/` (so the user can descend);
        // every other candidate gets a trailing space to start the next token.
        let pairs = values
            .into_iter()
            .map(|v| {
                let replacement = if v.ends_with('/') {
                    v.clone()
                } else {
                    format!("{v} ")
                };
                Pair {
                    display: v,
                    replacement,
                }
            })
            .collect();
        Ok((dot.current_start, pairs))
    }

    /// Produce the candidate strings for a classified dot argument, pulling
    /// from the filesystem and the live runtime as needed.
    fn dot_arg_values(&self, kind: DotArgKind, dot: &DotLine) -> Vec<String> {
        let current = dot.current;
        match kind {
            DotArgKind::None => vec![],
            DotArgKind::AnyFile => complete_path_token(current, |_| true),
            DotArgKind::ScriptFile => complete_path_token(current, is_script_file),
            DotArgKind::ScriptFileOrProcedure => {
                let mut out = complete_path_token(current, is_script_file);
                out.extend(self.runtime_procedure_names(current));
                out
            }
            DotArgKind::Procedure => {
                // Procedures from the referenced script file (so procs in a
                // not-yet-run file still complete) plus any the runtime has
                // already registered.
                let mut out = vec![];
                if let Some(file) = dot
                    .prior_args
                    .iter()
                    .find(|a| is_script_file(Path::new(a)))
                {
                    out.extend(
                        procedure_names_in_file(Path::new(file))
                            .into_iter()
                            .filter(|n| n.starts_with(current)),
                    );
                }
                out.extend(self.runtime_procedure_names(current));
                out.sort();
                out.dedup();
                out
            }
            DotArgKind::Keywords(kws) => kws
                .iter()
                .filter(|k| k.starts_with(current))
                .map(|k| k.to_string())
                .collect(),
            DotArgKind::ParamName => self.param_names(current),
            DotArgKind::SchemaName => self.schema_names(current),
            DotArgKind::DotCommandName => DOT_COMMAND_NAMES
                .iter()
                .filter(|v| v.starts_with(current))
                .map(|v| v.to_string())
                .collect(),
        }
    }

    /// Registered procedure names matching `prefix`, from the live runtime.
    fn runtime_procedure_names(&self, prefix: &str) -> Vec<String> {
        let rt = self.runtime.borrow();
        let mut names: Vec<String> = rt
            .procedures()
            .keys()
            .filter(|n| n.starts_with(prefix))
            .cloned()
            .collect();
        names.sort();
        names
    }

    /// Names of defined SQL parameters matching `prefix`.
    fn param_names(&self, prefix: &str) -> Vec<String> {
        let rt = self.runtime.borrow();
        let mut stmt = match rt
            .connection
            .prepare("SELECT key FROM temp.sqlite_parameters ORDER BY key")
        {
            Ok((_, Some(stmt))) => stmt,
            _ => return vec![],
        };
        let mut out = vec![];
        while let Ok(Some(row)) = stmt.next() {
            if let Some(first) = row.first() {
                let key = first.as_str().to_string();
                if key.starts_with(prefix) {
                    out.push(key);
                }
            }
        }
        out
    }

    /// Attached database/schema names matching `prefix` (via `PRAGMA
    /// database_list`).
    fn schema_names(&self, prefix: &str) -> Vec<String> {
        let rt = self.runtime.borrow();
        let mut stmt = match rt.connection.prepare("PRAGMA database_list") {
            Ok((_, Some(stmt))) => stmt,
            _ => return vec![],
        };
        let mut out = vec![];
        while let Ok(Some(row)) = stmt.next() {
            // database_list columns: seq, name, file
            if let Some(name) = row.get(1) {
                let name = name.as_str().to_string();
                if name.starts_with(prefix) {
                    out.push(name);
                }
            }
        }
        out
    }

    fn complete_sql(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> Result<(usize, Vec<Pair>)> {
        let rt = self.runtime.borrow();
        let schema = LiveSchemaSource::new(&rt);

        // Detect the completion context
        let context = detect_context(line, pos);

        // Find the start position for replacement
        let start = find_completion_start(line, pos);

        // Extract the prefix (partial word being typed)
        let prefix = &line[start..pos];
        let prefix_opt = if prefix.is_empty() { None } else { Some(prefix) };

        // Get completions from the shared engine, keeping only candidates
        // that match what has been typed so far.
        let items = get_completions(&context, Some(&schema), prefix_opt);

        // Convert to rustyline Pairs
        let pairs: Vec<Pair> = items
            .into_iter()
            .filter(|item| item_matches_prefix(item, prefix))
            .map(to_pair)
            .collect();

        Ok((start, pairs))
    }
}

impl Completer for ReplCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &rustyline::Context<'_>,
    ) -> Result<(usize, Vec<Self::Candidate>)> {
        if line.starts_with('.') {
            self.complete_dot(line, pos, ctx)
        } else {
            self.complete_sql(line, pos, ctx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_completion_start_plain_word() {
        let line = "select na";
        assert_eq!(find_completion_start(line, line.len()), "select ".len());
    }

    #[test]
    fn test_find_completion_start_line_start() {
        assert_eq!(find_completion_start("sel", 3), 0);
    }

    #[test]
    fn test_find_completion_start_after_comma_and_paren() {
        let line = "select a,b";
        assert_eq!(find_completion_start(line, line.len()), "select a,".len());
        let line = "select count(x";
        assert_eq!(find_completion_start(line, line.len()), "select count(".len());
    }

    #[test]
    fn test_find_completion_start_qualified_column() {
        // `t.na` completes only `na`, preserving the `t.` qualifier
        let line = "select t.na";
        assert_eq!(find_completion_start(line, line.len()), "select t.".len());
    }

    #[test]
    fn test_item_matches_prefix() {
        let name = CompletionItem::new("name", CompletionKind::Column);
        assert!(item_matches_prefix(&name, ""));
        assert!(item_matches_prefix(&name, "na"));
        assert!(item_matches_prefix(&name, "NA")); // case-insensitive
        assert!(!item_matches_prefix(&name, "id"));

        // insert_text is matched too (quoted identifiers)
        let quoted = CompletionItem::new("my table", CompletionKind::Table)
            .with_insert_text("\"my table\"");
        assert!(item_matches_prefix(&quoted, "my"));
        assert!(item_matches_prefix(&quoted, "\"my"));
    }

    // --- dot-command argument completion (ticket 06) ---

    #[test]
    fn parse_dot_line_splits_command_and_current_token() {
        // No space yet → command-name completion path (None).
        assert!(parse_dot_line(".ope", 4).is_none());

        let d = parse_dot_line(".param ", 7).unwrap();
        assert_eq!(d.command, "param");
        assert!(d.prior_args.is_empty());
        assert_eq!(d.current, "");
        assert_eq!(d.current_start, 7);

        let d = parse_dot_line(".timer o", 8).unwrap();
        assert_eq!((d.command, d.current), ("timer", "o"));

        let d = parse_dot_line(".run q.sql ", 11).unwrap();
        assert_eq!(d.command, "run");
        assert_eq!(d.prior_args, vec!["q.sql"]);
        assert_eq!(d.current, "");
    }

    #[test]
    fn classify_dot_arg_dispatches_per_command() {
        use DotArgKind::*;
        assert_eq!(
            classify_dot_arg("param", 0, &[]),
            Keywords(&["set", "unset", "list", "clear"])
        );
        assert_eq!(classify_dot_arg("timer", 0, &[]), Keywords(&["on", "off"]));
        assert_eq!(classify_dot_arg("run", 0, &[]), ScriptFile);
        assert_eq!(classify_dot_arg("run", 1, &["q.sql"]), Procedure);
        assert_eq!(classify_dot_arg("call", 0, &[]), ScriptFileOrProcedure);
        assert_eq!(classify_dot_arg("param", 1, &["unset"]), ParamName);
        assert_eq!(classify_dot_arg("param", 1, &["set"]), None);
        assert_eq!(classify_dot_arg("help", 0, &[]), DotCommandName);
        assert_eq!(classify_dot_arg("open", 0, &[]), AnyFile);
        assert_eq!(classify_dot_arg("nope", 0, &[]), None);
    }

    #[test]
    fn complete_path_token_lists_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.sql"), "SELECT 1;").unwrap();
        std::fs::write(dir.path().join("b.db"), "").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        // Use an absolute dir prefix so the test doesn't depend on/ mutate cwd.
        let prefix = format!("{}/", dir.path().display());

        let any: Vec<String> = complete_path_token(&prefix, |_| true)
            .into_iter()
            .map(|p| p.trim_start_matches(&prefix).to_string())
            .collect();
        assert!(any.contains(&"a.sql".to_string()), "{any:?}");
        assert!(any.contains(&"b.db".to_string()), "{any:?}");
        assert!(any.contains(&"sub/".to_string()), "{any:?}");

        let scripts: Vec<String> = complete_path_token(&prefix, is_script_file)
            .into_iter()
            .map(|p| p.trim_start_matches(&prefix).to_string())
            .collect();
        assert!(scripts.contains(&"a.sql".to_string()), "{scripts:?}");
        assert!(scripts.contains(&"sub/".to_string()), "{scripts:?}");
        assert!(!scripts.contains(&"b.db".to_string()), "{scripts:?}");
    }

    #[test]
    fn procedure_names_in_file_parses_name_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queries.sql");
        std::fs::write(
            &path,
            "-- name: getUser :row\nSELECT 1;\n-- name: listUsers :rows\nSELECT 2;\n",
        )
        .unwrap();
        assert_eq!(
            procedure_names_in_file(&path),
            vec!["getUser".to_string(), "listUsers".to_string()]
        );
        // Missing file → empty, no panic.
        assert!(procedure_names_in_file(Path::new("/no/such.sql")).is_empty());
    }

    fn completer_with_runtime() -> ReplCompleter {
        let runtime = Runtime::new(None).expect("in-memory runtime");
        ReplCompleter::new(Rc::new(RefCell::new(runtime)))
    }

    #[test]
    fn param_and_schema_names_from_runtime() {
        let completer = completer_with_runtime();
        completer
            .runtime
            .borrow_mut()
            .define_parameter("userId".to_string(), "42".to_string())
            .unwrap();

        assert_eq!(completer.param_names(""), vec!["userId".to_string()]);
        assert!(completer.param_names("user").contains(&"userId".to_string()));
        assert!(completer.param_names("zzz").is_empty());

        // The in-memory connection always has a `main` schema.
        assert!(completer.schema_names("").contains(&"main".to_string()));
    }

    #[test]
    fn call_first_arg_offers_registered_procedures() {
        use solite_core::procedure::{Procedure, ResultType};
        let completer = completer_with_runtime();
        completer.runtime.borrow_mut().register_procedure(Procedure {
            name: "getUser".to_string(),
            sql: "SELECT 1".to_string(),
            result_type: ResultType::Row,
            annotations: vec!["row".to_string()],
            parameters: vec![],
            columns: vec![],
            result_class: None,
        });

        // `.call <prefix>` (ScriptFileOrProcedure) offers the registered proc.
        let dot = parse_dot_line(".call get", 9).unwrap();
        let kind = classify_dot_arg(dot.command, dot.prior_args.len(), &dot.prior_args);
        let values = completer.dot_arg_values(kind, &dot);
        assert!(values.contains(&"getUser".to_string()), "{values:?}");
    }
}
