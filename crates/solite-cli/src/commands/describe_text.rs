//! Shared `.describe` text rendering for the REPL and `solite run`.
//!
//! `solite test` rejects `.describe` outright (`commands/test/mod.rs`), and
//! the Jupyter kernel has its own HTML rendering (ticket 03) that mirrors
//! the same section order as a `text/plain` fallback.

use solite_core::dot::describe::{DescribeOutput, RowCount, TableDescription};
use solite_table::TableConfig;

/// Header line: `"schema.name — kind · N rows · WITHOUT ROWID · STRICT"`.
/// Shared with the Jupyter HTML/text renderer so the two front-ends read
/// identically.
pub fn describe_header_text(d: &TableDescription) -> String {
    let mut s = format!("{}.{} — {}", d.schema, d.name, d.kind_label());
    if let Some(count) = d.row_count_label() {
        s.push_str(&format!(" · {} rows", count));
    }
    if d.without_rowid {
        s.push_str(" · WITHOUT ROWID");
    }
    if d.strict {
        s.push_str(" · STRICT");
    }
    s
}

/// Sample-table footer: `"10 of 1,234 rows"`, `"10 of 100,000+ rows"`, or
/// (views, and tables where the sample already showed every row) `"N rows
/// shown"`. `shown` is the number of rows the sample actually rendered
/// (`RenderResult::total_rows` — the sample query already has `LIMIT 10`
/// baked in, so this is 0-10). Shared with the Jupyter renderer.
pub fn describe_footer_text(d: &TableDescription, shown: usize) -> String {
    match d.row_count_label() {
        None => format!("{} rows shown", shown),
        Some(_) if matches!(d.row_count, RowCount::Exact(n) if shown as u64 >= n) => {
            format!("{} rows shown", shown)
        }
        Some(label) => format!("{} of {} rows", shown, label),
    }
}

/// Columns / Foreign keys / Indexes / DDL sections, each preceded by a
/// blank line and omitted entirely when empty (DDL omitted when `None`).
/// `highlight` is applied to the DDL text only — pass a
/// `solite_table`-agnostic `highlight_sql` from the REPL, or an identity
/// closure (`|s| s.to_string()`) from `run`, Jupyter's text mirror, and
/// tests. Shared by [`render_describe_text`] and the Jupyter HTML renderer
/// so the non-table sections never drift apart.
pub fn describe_sections_text(d: &TableDescription, highlight: impl Fn(&str) -> String) -> String {
    let mut s = String::new();

    // Columns
    if !d.columns.is_empty() {
        s.push('\n');
        s.push_str("Columns\n");
        let name_width = d.columns.iter().map(|c| c.name.len()).max().unwrap_or(0);
        let multi_pk = d.columns.iter().filter(|c| c.pk > 0).count() > 1;
        for c in &d.columns {
            let mut type_part = c.declared_type.clone();
            if !c.declared_type.eq_ignore_ascii_case(c.affinity) {
                type_part.push_str(&format!(" ({})", c.affinity));
            }

            let mut flags = Vec::new();
            if c.pk > 0 {
                flags.push(if multi_pk {
                    format!("PK{}", c.pk)
                } else {
                    "PK".to_string()
                });
            }
            if c.not_null {
                flags.push("NOT NULL".to_string());
            }
            if let Some(default) = &c.default {
                flags.push(format!("default {}", default));
            }
            match c.hidden {
                1 => flags.push("hidden".to_string()),
                2 => flags.push("generated virtual".to_string()),
                3 => flags.push("generated stored".to_string()),
                _ => {}
            }

            let line = format!(
                "  {:name_width$}  {}  {}",
                c.name,
                type_part,
                flags.join("  "),
                name_width = name_width
            );
            s.push_str(line.trim_end());
            s.push('\n');
        }
    }

    // Foreign keys
    if !d.foreign_keys_out.is_empty() || !d.foreign_keys_in.is_empty() {
        s.push('\n');
        s.push_str("Foreign keys\n");
        for fk in &d.foreign_keys_out {
            let to_column = fk.to_column.as_deref().unwrap_or("<pk>");
            let mut line = format!(
                "  →  {} → {}.{}({})",
                fk.from_column, d.schema, fk.to_table, to_column
            );
            if fk.on_delete != "NO ACTION" {
                line.push_str(&format!("  ON DELETE {}", fk.on_delete));
            }
            if fk.on_update != "NO ACTION" {
                line.push_str(&format!("  ON UPDATE {}", fk.on_update));
            }
            s.push_str(&line);
            s.push('\n');
        }
        for fk in &d.foreign_keys_in {
            let to_column = fk.to_column.as_deref().unwrap_or("<pk>");
            s.push_str(&format!(
                "  ←  {}({}) → {}\n",
                fk.from_table, fk.from_column, to_column
            ));
        }
    }

    // Indexes
    if !d.indexes.is_empty() {
        s.push('\n');
        s.push_str("Indexes\n");
        let name_width = d.indexes.iter().map(|i| i.name.len()).max().unwrap_or(0);
        for idx in &d.indexes {
            let mut flags = Vec::new();
            if idx.unique {
                flags.push("unique".to_string());
            }
            if idx.partial {
                flags.push("partial".to_string());
            }
            match idx.origin.as_str() {
                "pk" => flags.push("pk".to_string()),
                "u" => flags.push("autoindex".to_string()),
                _ => {}
            }
            let line = format!(
                "  {:name_width$}  ({})  {}",
                idx.name,
                idx.columns.join(", "),
                flags.join(", "),
                name_width = name_width
            );
            s.push_str(line.trim_end());
            s.push('\n');
        }
    }

    // DDL
    if let Some(ddl) = &d.ddl {
        s.push('\n');
        s.push_str("DDL\n");
        for line in highlight(ddl).lines() {
            s.push_str("  ");
            s.push_str(line);
            s.push('\n');
        }
    }

    s
}

/// Render a `.describe` result as psql-`\d`-style text: header, sample
/// rows, columns, foreign keys (out then in), indexes, DDL — in that order.
/// Empty sections ("Foreign keys", "Indexes") are omitted entirely, as is
/// "DDL" when there is none (eponymous virtual tables).
///
/// Steps `out.sample` through [`solite_table::render_statement`] with
/// `config`, so callers don't have to. `highlight` is applied to the DDL
/// text only — pass `solite_table`-agnostic `highlight_sql` from the REPL,
/// or an identity closure (`|s| s.to_string()`) from `run`/tests.
pub fn render_describe_text(
    mut out: DescribeOutput,
    config: &TableConfig,
    highlight: impl Fn(&str) -> String,
) -> String {
    let d = &out.description;
    let mut s = String::new();

    s.push_str(&describe_header_text(d));
    s.push('\n');

    // Sample
    let render = solite_table::render_statement(&mut out.sample, config);
    let shown = match render {
        Ok(result) => {
            s.push_str(&result.output);
            if !result.output.ends_with('\n') {
                s.push('\n');
            }
            result.total_rows
        }
        Err(e) => {
            s.push_str(&format!("(failed to render sample: {})\n", e));
            0
        }
    };
    s.push_str(&describe_footer_text(d, shown));
    s.push('\n');
    s.push_str(&describe_sections_text(d, highlight));

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use solite_core::dot::describe::DescribeCommand;
    use solite_core::Runtime;

    fn rt() -> Runtime {
        Runtime::new(None).unwrap()
    }

    fn describe(runtime: &Runtime, name: &str) -> DescribeOutput {
        DescribeCommand {
            schema: None,
            name: name.to_string(),
        }
        .execute(runtime)
        .unwrap()
    }

    #[test]
    fn test_render_table_with_fk_in_out_and_partial_unique_index() {
        let runtime = rt();
        runtime
            .connection
            .execute_script(
                "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
                 CREATE TABLE accounts (
                    id INTEGER PRIMARY KEY,
                    owner_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
                    label VARCHAR(20) DEFAULT 'default'
                 );
                 CREATE UNIQUE INDEX idx_accounts_owner ON accounts(owner_id) WHERE owner_id IS NOT NULL;
                 CREATE TABLE orders (
                    id INTEGER PRIMARY KEY,
                    account_id INTEGER REFERENCES accounts(id)
                 );
                 INSERT INTO users (name) VALUES ('alice'), ('bob');
                 INSERT INTO accounts (owner_id, label) VALUES (1, 'a1'), (2, 'a2');
                 INSERT INTO orders (account_id) VALUES (1), (1), (2);",
            )
            .unwrap();

        let out = describe(&runtime, "accounts");
        let text = render_describe_text(out, &TableConfig::plain(), |s| s.to_string());
        insta::assert_snapshot!(text);
    }

    #[test]
    fn test_render_virtual_table_hidden_column() {
        let runtime = rt();
        runtime
            .connection
            .execute_script(
                "CREATE VIRTUAL TABLE notes USING fts5(body);
                 INSERT INTO notes(body) VALUES ('hello world'), ('another note');",
            )
            .unwrap();

        let out = describe(&runtime, "notes");
        let text = render_describe_text(out, &TableConfig::plain(), |s| s.to_string());
        insta::assert_snapshot!(text);
    }

    #[test]
    fn test_render_view_has_no_row_count_or_fk_or_index_sections() {
        let runtime = rt();
        runtime
            .connection
            .execute_script(
                "CREATE TABLE base (id INTEGER PRIMARY KEY, val TEXT);
                 INSERT INTO base VALUES (1, 'a'), (2, 'b');
                 CREATE VIEW v_base AS SELECT * FROM base;",
            )
            .unwrap();

        let out = describe(&runtime, "v_base");
        let text = render_describe_text(out, &TableConfig::plain(), |s| s.to_string());
        insta::assert_snapshot!(text);
    }
}
