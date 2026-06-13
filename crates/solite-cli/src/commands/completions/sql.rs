//! Best-effort SQL completion inside SQL-string arguments
//! (`solite query "SELECT * FROM <TAB>"`, `solite run -c "SELECT <TAB>"`).
//!
//! Reuses the same engine that powers the REPL and LSP
//! ([`solite_completion`]) plus the REPL's [`LiveSchemaSource`], so keyword,
//! function, and table-name candidates come from one source of truth.
//!
//! This is deliberately **best-effort**: shells tokenize on whitespace, so a
//! multi-word SQL string is a single quoted argument and completing mid-value
//! is shell- and cursor-dependent. Scope for v1:
//! - The cursor is treated as the end of the value.
//! - Candidates are returned as the *whole* reconstructed string (head + the
//!   completed token), because the engine does not post-filter custom value
//!   completers — the shell replaces the entire quoted word, so a bare
//!   `users` would clobber the SQL typed so far.
//! - Table names require a sibling database path on the command line that opens
//!   read-only; otherwise only keywords/functions are offered. A database is
//!   never created or written.
//!
//! Out of scope (zsh/fish cope with quoted values better than bash): mid-string
//! completion when the cursor isn't at the end, and column completion that
//! needs a database not present on the command line.

use std::ffi::OsStr;
use std::path::PathBuf;

use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};
use solite_completion::{detect_context, get_completions};
use solite_core::Runtime;

use super::files::script_or_database_candidates;
use crate::cli::is_database_path;
use crate::commands::repl::completer::{
    find_completion_start, item_matches_prefix, LiveSchemaSource,
};

/// SQL keyword/function/table candidates for the SQL string `current`.
fn sql_candidates(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(source) = current.to_str() else {
        return Vec::new();
    };
    let cursor = source.len();
    let start = find_completion_start(source, cursor);
    let prefix = &source[start..cursor];
    let prefix_opt = if prefix.is_empty() { None } else { Some(prefix) };

    let context = detect_context(source, cursor);

    // Best-effort table completion: open a sibling db read-only if one is on
    // the command line. Falls back to keyword/function-only on any error.
    let runtime = open_sibling_db_readonly();
    let items = match &runtime {
        Some(rt) => {
            let schema = LiveSchemaSource::new(rt);
            get_completions(&context, Some(&schema), prefix_opt)
        }
        None => get_completions(&context, None, prefix_opt),
    };

    let head = &source[..start];
    items
        .into_iter()
        .filter(|item| item_matches_prefix(item, prefix))
        .map(|item| {
            let insert = item.insert_text.as_deref().unwrap_or(item.label.as_str());
            CompletionCandidate::new(format!("{head}{insert}"))
        })
        .collect()
}

/// Open the first existing database path on the current completion command line
/// read-only, for schema-aware (table-name) completion. Returns `None` on any
/// error — completion must never create/write a database or fail.
fn open_sibling_db_readonly() -> Option<Runtime> {
    let db = std::env::args_os()
        .map(PathBuf::from)
        .find(|p| is_database_path(p) && p.is_file())?;
    Runtime::new_readonly(db.to_str()?).ok()
}

/// SQL-only completer (for `run -c <value>`, which is always inline SQL).
pub(crate) fn sql_completer() -> ArgValueCompleter {
    ArgValueCompleter::new(sql_candidates)
}

/// Completer for args that accept SQL *or* a path (`query.statement`,
/// `execute.args`, `bench.sql`): file candidates plus SQL candidates.
pub(crate) fn script_database_or_sql_completer() -> ArgValueCompleter {
    ArgValueCompleter::new(|current: &OsStr| {
        let mut candidates = script_or_database_candidates(current);
        candidates.extend(sql_candidates(current));
        candidates
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    fn complete(value: &str) -> Vec<String> {
        sql_candidates(&OsString::from(value))
            .into_iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn completes_keywords_without_schema() {
        // Statement-start keyword completion (no db needed). The engine emits
        // lowercase keywords; what matters is the keyword is offered.
        let sel = complete("SEL");
        assert!(sel.iter().any(|c| c.eq_ignore_ascii_case("select")), "{sel:?}");
        // Candidates are whole reconstructed strings: the head before the
        // completed token is preserved, so a mid-statement keyword keeps the
        // SQL typed so far (critical — the shell replaces the whole word).
        let got = complete("SELECT 1 FROM t WHE");
        assert!(
            got.iter().any(|c| c.eq_ignore_ascii_case("SELECT 1 FROM t WHERE")),
            "{got:?}"
        );
    }

    #[test]
    fn completes_table_names_with_sibling_db() {
        // Build a db with a `users` table, then point the argv scan at it.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("app.db");
        {
            let rt = Runtime::new(Some(db.to_string_lossy().into_owned())).unwrap();
            rt.connection
                .execute_script("CREATE TABLE users(id, name);")
                .unwrap();
        }

        // open_sibling_db_readonly reads process args; the test binary's argv
        // won't contain the db, so exercise sql_candidates via a constructed
        // Runtime + LiveSchemaSource directly to assert the table-name path.
        let rt = Runtime::new_readonly(db.to_str().unwrap()).unwrap();
        let schema = LiveSchemaSource::new(&rt);
        let source = "SELECT * FROM us";
        let cursor = source.len();
        let start = find_completion_start(source, cursor);
        let prefix = &source[start..cursor];
        let ctx = detect_context(source, cursor);
        let items = get_completions(&ctx, Some(&schema), Some(prefix));
        assert!(
            items.iter().any(|i| i.label == "users"),
            "expected `users` table, got {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }
}
