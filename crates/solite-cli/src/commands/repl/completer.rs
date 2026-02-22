use super::highlighter::CTP_MOCHA_THEME;
use rustyline::completion::{Completer, Pair};
use rustyline::Result;
use solite_completion::{
    detect_context, get_completions, CompletionItem, CompletionKind, SchemaSource,
};
use solite_core::Runtime;
use std::cell::RefCell;
use std::rc::Rc;

/// A SchemaSource implementation that queries a live SQLite database connection.
struct LiveSchemaSource<'a> {
    runtime: &'a Runtime,
}

impl<'a> LiveSchemaSource<'a> {
    fn new(runtime: &'a Runtime) -> Self {
        Self { runtime }
    }
}

impl SchemaSource for LiveSchemaSource<'_> {
    fn table_names(&self) -> Vec<String> {
        let stmt = match self.runtime.connection.prepare(
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
        let sql = format!("PRAGMA table_info(\"{}\")", table.replace("\"", "\"\""));
        let stmt = match self.runtime.connection.prepare(&sql) {
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
            "SELECT sql FROM sqlite_master WHERE type='table' AND name = \"{}\"",
            table.replace("\"", "\"\"")
        );
        let stmt = match self.runtime.connection.prepare(&sql) {
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
        let stmt = match self.runtime.connection.prepare(
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
        let stmt = match self.runtime.connection.prepare(
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
        let stmt = match self.runtime.connection.prepare(
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
        let stmt = match self.runtime.connection.prepare(sql) {
            Ok((_, Some(stmt))) => stmt,
            _ => return None,
        };
        stmt.bind_text(1, name);

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

/// Find the start position for completion replacement.
/// Returns the position of the start of the current word being typed.
fn find_completion_start(line: &str, pos: usize) -> usize {
    let line_before_cursor = &line[..pos];

    // Find the last whitespace or operator before cursor
    line_before_cursor
        .rfind(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
        .map(|idx| idx + 1)
        .unwrap_or(0)
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
        _pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> Result<(usize, Vec<Pair>)> {
        let dots = ["load", "tables", "open", "schema", "timer", "help"];
        if line.contains(' ') || !line.starts_with('.') {
            return Ok((0, vec![]));
        }
        let prefix = &line[1..];

        let candidates = dots
            .iter()
            .filter_map(|v| {
                if v.starts_with(prefix) {
                    Some(Pair {
                        display: CTP_MOCHA_THEME.style_dot(v),
                        replacement: format!("{v} "),
                    })
                } else {
                    None
                }
            })
            .collect();
        Ok((1, candidates))
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

        // Get completions from the shared engine
        let items = get_completions(&context, Some(&schema), prefix_opt);

        // Convert to rustyline Pairs
        let pairs: Vec<Pair> = items.into_iter().map(to_pair).collect();

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
