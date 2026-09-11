//! HTML rendering for `.describe` in Jupyter: static `<details>`-sectioned
//! layout (design decision 6 — no JS), with a `text/plain` mirror for
//! nbconvert, `jupyter console`, and copy-as-text.
//!
//! Data is open by default; Columns / Foreign keys / Indexes / DDL are
//! collapsed. Empty Foreign keys / Indexes sections and a missing DDL are
//! omitted entirely rather than rendered empty.

use anyhow::Result;
use solite_core::dot::describe::{
    ColumnDesc, DescribeOutput, ForeignKeyDesc, IndexDesc, TableDescription,
};
use solite_table::TableConfig;
use std::sync::LazyLock;

use crate::commands::describe_text::{
    describe_footer_text, describe_header_text, describe_sections_text,
};

use super::html::{Element, HtmlDoc};
use super::syntax::render_sql_html;
use super::table::UiResponse;

static DESCRIBE_CSS: LazyLock<String> = LazyLock::new(|| {
    r#"
  .solite-describe summary { cursor: pointer; font-weight: 600; }
  .solite-describe details { margin: 0.4em 0; }
  .solite-describe-header { margin-bottom: 0.5em; }
  .solite-describe-header code { font-weight: 600; }
  .solite-describe-header .kind,
  .solite-describe-header .flag,
  .solite-describe-header .count { margin-left: 0.6em; font-size: 0.85em; opacity: 0.75; }
  .solite-describe-footer { font-size: 0.85em; opacity: 0.7; margin-top: 0.25em; }
  .solite-describe table { border-collapse: collapse; margin: 0.3em 0; }
  .solite-describe td, .solite-describe th {
    padding: 2px 10px;
    text-align: left;
    border-bottom: 1px solid currentColor;
    border-color: color-mix(in srgb, currentColor 15%, transparent);
  }
  .solite-describe .badge {
    font-size: 0.75em;
    padding: 1px 6px;
    border-radius: 3px;
    border: 1px solid currentColor;
    opacity: 0.8;
  }
"#
    .to_string()
});

/// Render `.describe` output as `{ text, html }` — sectioned `<details>`
/// HTML plus a same-order `text/plain` mirror.
pub fn render_describe(mut out: DescribeOutput) -> Result<UiResponse> {
    // Buffer the sample once (a `Statement` can't be iterated twice), then
    // render it into both HTML and plain text — same pattern as
    // `render/table.rs`. The describe-specific footer replaces
    // solite-table's own footer, so both configs suppress it.
    let html_config = crate::colors::html_table_config().with_footer(false);
    let buffered = solite_table::buffer_statement(
        &mut out.sample,
        html_config.head_rows,
        html_config.tail_rows,
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;
    let d = &out.description;

    let (sample_html, sample_plain, shown) = match &buffered {
        Some(b) => {
            let html = solite_table::render_buffered(b, &html_config).output;
            let plain_config = TableConfig::plain().with_footer(false);
            let plain = solite_table::render_buffered(b, &plain_config);
            (html, plain.output, plain.total_rows)
        }
        None => (String::new(), String::new(), 0),
    };
    let footer = describe_footer_text(d, shown);

    let text = render_text(d, &sample_plain, &footer);
    let html = render_html(d, &sample_html, &footer);

    Ok(UiResponse { text, html })
}

fn render_text(d: &TableDescription, sample_plain: &str, footer: &str) -> String {
    let mut s = String::new();
    s.push_str(&describe_header_text(d));
    s.push('\n');
    s.push_str(sample_plain);
    if !sample_plain.is_empty() && !sample_plain.ends_with('\n') {
        s.push('\n');
    }
    s.push_str(footer);
    s.push('\n');
    s.push_str(&describe_sections_text(d, |s| s.to_string()));
    s
}

fn render_html(d: &TableDescription, sample_html: &str, footer: &str) -> String {
    let doc = HtmlDoc::new();
    let mut root = doc.div();
    root.attr("class", "solite-describe");
    root.child("style").set_text(DESCRIBE_CSS.clone());

    {
        let header = root.div();
        header.attr("class", "solite-describe-header");
        header
            .child("code")
            .set_text(format!("{}.{}", d.schema, d.name));
        header
            .child("span")
            .attr("class", "kind")
            .set_text(d.kind_label());
        if d.without_rowid {
            header
                .child("span")
                .attr("class", "flag")
                .set_text("WITHOUT ROWID");
        }
        if d.strict {
            header
                .child("span")
                .attr("class", "flag")
                .set_text("STRICT");
        }
        if let Some(count) = d.row_count_label() {
            header
                .child("span")
                .attr("class", "count")
                .set_text(format!("{} rows", count));
        }
    }

    {
        let details = root.child("details");
        details.attr("open", "open");
        details.child("summary").set_text("Data");
        details
            .child("div")
            .style("overflow-x", "auto")
            .raw(sample_html.to_string());
        details
            .child("div")
            .attr("class", "solite-describe-footer")
            .set_text(footer);
    }

    {
        let details = root.child("details");
        details
            .child("summary")
            .set_text(format!("Columns ({})", d.columns.len()));
        columns_table(details, &d.columns);
    }

    if !d.foreign_keys_out.is_empty() || !d.foreign_keys_in.is_empty() {
        let details = root.child("details");
        details.child("summary").set_text(format!(
            "Foreign keys ({})",
            d.foreign_keys_out.len() + d.foreign_keys_in.len()
        ));
        if !d.foreign_keys_out.is_empty() {
            details.child("h4").set_text("Outgoing");
            outgoing_fk_table(details, &d.schema, &d.foreign_keys_out);
        }
        if !d.foreign_keys_in.is_empty() {
            details.child("h4").set_text("Incoming");
            incoming_fk_table(details, &d.foreign_keys_in);
        }
    }

    if !d.indexes.is_empty() {
        let details = root.child("details");
        details
            .child("summary")
            .set_text(format!("Indexes ({})", d.indexes.len()));
        indexes_table(details, &d.indexes);
    }

    if let Some(ddl) = &d.ddl {
        let details = root.child("details");
        details.child("summary").set_text("DDL");
        details.raw(render_sql_html(ddl, crate::colors::theme()));
    }

    root.to_html()
}

fn columns_table(parent: &mut Element, columns: &[ColumnDesc]) {
    let table = parent.child("table");
    {
        let tr = table.child("thead").child("tr");
        for h in [
            "Name", "Type", "Affinity", "Not null", "PK", "Default", "Flags",
        ] {
            tr.child("th").set_text(h);
        }
    }
    let tbody = table.child("tbody");
    let multi_pk = columns.iter().filter(|c| c.pk > 0).count() > 1;
    for c in columns {
        let tr = tbody.child("tr");
        tr.child("td").set_text(c.name.clone());
        tr.child("td").set_text(c.declared_type.clone());
        tr.child("td").set_text(c.affinity);
        tr.child("td").set_text(if c.not_null { "yes" } else { "" });
        let pk_text = if c.pk > 0 {
            if multi_pk {
                format!("PK {}", c.pk)
            } else {
                "PK".to_string()
            }
        } else {
            String::new()
        };
        tr.child("td").set_text(pk_text);
        tr.child("td")
            .set_text(c.default.clone().unwrap_or_default());
        let flags_td = tr.child("td");
        match c.hidden {
            1 => {
                flags_td
                    .child("span")
                    .attr("class", "badge")
                    .set_text("hidden");
            }
            2 => {
                flags_td
                    .child("span")
                    .attr("class", "badge")
                    .set_text("generated (virtual)");
            }
            3 => {
                flags_td
                    .child("span")
                    .attr("class", "badge")
                    .set_text("generated (stored)");
            }
            _ => {}
        }
    }
}

fn outgoing_fk_table(parent: &mut Element, schema: &str, fks: &[ForeignKeyDesc]) {
    let table = parent.child("table");
    {
        let tr = table.child("thead").child("tr");
        for h in ["From", "To", "On update", "On delete"] {
            tr.child("th").set_text(h);
        }
    }
    let tbody = table.child("tbody");
    for fk in fks {
        let tr = tbody.child("tr");
        tr.child("td").set_text(fk.from_column.clone());
        let to_column = fk.to_column.as_deref().unwrap_or("<pk>");
        tr.child("td")
            .set_text(format!("{}.{}({})", schema, fk.to_table, to_column));
        tr.child("td").set_text(fk.on_update.clone());
        tr.child("td").set_text(fk.on_delete.clone());
    }
}

fn incoming_fk_table(parent: &mut Element, fks: &[ForeignKeyDesc]) {
    let table = parent.child("table");
    {
        let tr = table.child("thead").child("tr");
        for h in ["From", "To"] {
            tr.child("th").set_text(h);
        }
    }
    let tbody = table.child("tbody");
    for fk in fks {
        let tr = tbody.child("tr");
        tr.child("td")
            .set_text(format!("{}({})", fk.from_table, fk.from_column));
        let to_column = fk.to_column.as_deref().unwrap_or("<pk>");
        tr.child("td").set_text(to_column.to_string());
    }
}

fn indexes_table(parent: &mut Element, indexes: &[IndexDesc]) {
    let table = parent.child("table");
    {
        let tr = table.child("thead").child("tr");
        for h in ["Name", "Columns", "Unique", "Partial", "Origin"] {
            tr.child("th").set_text(h);
        }
    }
    let tbody = table.child("tbody");
    for idx in indexes {
        let tr = tbody.child("tr");
        tr.child("td").set_text(idx.name.clone());
        tr.child("td").set_text(idx.columns.join(", "));
        tr.child("td").set_text(if idx.unique { "yes" } else { "" });
        tr.child("td")
            .set_text(if idx.partial { "yes" } else { "" });
        tr.child("td").set_text(idx.origin.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solite_core::dot::describe::DescribeCommand;
    use solite_core::Runtime;

    #[test]
    fn escapes_a_column_named_with_html_markup() {
        let runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(r#"CREATE TABLE t ("<b>x</b>" INTEGER PRIMARY KEY);"#)
            .unwrap();

        let out = DescribeCommand {
            schema: None,
            name: "t".to_string(),
        }
        .execute(&runtime)
        .unwrap();

        let ui = render_describe(out).unwrap();
        assert!(!ui.html.contains("<b>x</b>"));
        assert!(ui.html.contains("&lt;b&gt;x&lt;/b&gt;"));
    }

    #[test]
    fn omits_empty_foreign_keys_and_indexes_sections() {
        let runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script("CREATE TABLE plain (id INTEGER PRIMARY KEY, name TEXT);")
            .unwrap();

        let out = DescribeCommand {
            schema: None,
            name: "plain".to_string(),
        }
        .execute(&runtime)
        .unwrap();

        let ui = render_describe(out).unwrap();
        assert!(!ui.html.contains("Foreign keys"));
        assert!(!ui.html.contains("Indexes ("));
        assert!(ui.html.contains(">DDL<"));
    }

    #[test]
    fn view_footer_has_no_of_count() {
        let runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(
                "CREATE TABLE base (id INTEGER);
                 INSERT INTO base VALUES (1), (2);
                 CREATE VIEW v_base AS SELECT * FROM base;",
            )
            .unwrap();

        let out = DescribeCommand {
            schema: None,
            name: "v_base".to_string(),
        }
        .execute(&runtime)
        .unwrap();

        let ui = render_describe(out).unwrap();
        assert!(ui.html.contains("rows shown"));
        assert!(!ui.html.contains(" of "));
        assert!(ui.text.contains("rows shown"));
    }
}
