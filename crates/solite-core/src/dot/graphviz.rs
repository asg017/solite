//! Graphviz ERD (Entity-Relationship Diagram) generation.
//!
//! This module implements the `.graphviz` (or `.gv`) command which generates
//! a DOT-format entity-relationship diagram from the database schema.
//!
//! # Output
//!
//! The generated DOT file can be rendered using Graphviz tools like `dot`:
//!
//! ```bash
//! solite -cmd ".graphviz" mydb.sqlite | dot -Tpng -o erd.png
//! ```
//!
//! # Features
//!
//! - Displays tables and views with their columns
//! - Highlights primary keys (PK) and foreign keys (FK)
//! - Shows relationships with crow's foot notation
//! - Distinguishes one-to-one from one-to-many relationships
//! - Truncates columns for large tables (shows PKs/FKs + ellipsis)

use crate::dot::DotError;
use crate::sqlite::ValueRefXValue;
use crate::Runtime;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// Command to generate a Graphviz DOT diagram of the database schema.
#[derive(Serialize, Debug, PartialEq)]
pub struct GraphvizCommand {}

/// Internal representation of a database column.
#[derive(Debug)]
struct Column {
    name: String,
    col_type: String,
    is_pk: bool,
    #[allow(dead_code)]
    is_fk: bool,
}

/// Internal representation of a foreign key relationship.
#[derive(Debug)]
struct ForeignKey {
    from_table: String,
    from_column: String,
    to_table: String,
    to_column: String,
    is_unique: bool,
}

impl GraphvizCommand {
    /// Execute the graphviz command, generating a DOT-format ERD.
    ///
    /// # Arguments
    ///
    /// * `runtime` - The runtime context containing the database connection
    ///
    /// # Returns
    ///
    /// A string containing the DOT graph definition, or an error if
    /// database operations fail.
    ///
    /// # Example Output
    ///
    /// ```dot
    /// digraph ERD {
    ///   rankdir=LR;
    ///   node [shape=plaintext];
    ///   "users" [label=<...>];
    ///   "orders" [label=<...>];
    ///   "orders" -> "users" [label="...", arrowhead=normal, arrowtail=crow, dir=both];
    /// }
    /// ```
    pub fn execute(&self, runtime: &Runtime) -> Result<String, DotError> {
        let tables = self.get_tables(runtime)?;

        let mut table_columns: HashMap<String, Vec<Column>> = HashMap::new();
        let mut foreign_keys: Vec<ForeignKey> = Vec::new();

        for table in &tables {
            let columns = self.get_table_columns(runtime, table)?;
            let fks = self.get_foreign_keys(runtime, table)?;

            table_columns.insert(table.clone(), columns);
            foreign_keys.extend(fks);
        }

        Ok(self.generate_dot(&table_columns, &foreign_keys))
    }

    /// Get all user tables and views from the database.
    fn get_tables(&self, runtime: &Runtime) -> Result<Vec<String>, DotError> {
        let (_, stmt) = runtime.connection.prepare(
            r#"
            SELECT name
            FROM pragma_table_list
            WHERE "schema" = 'main'
              AND type IN ('table', 'view')
              AND name NOT LIKE 'sqlite_%'
            ORDER BY name
            "#,
        )?;

        let stmt = stmt.ok_or_else(|| DotError::InvalidData("Failed to prepare query".into()))?;

        let mut tables = Vec::new();
        while let Ok(Some(row)) = stmt.next() {
            if let Some(value) = row.first() {
                tables.push(value.as_str().to_owned());
            }
        }
        Ok(tables)
    }

    /// Get column information for a specific table.
    fn get_table_columns(&self, runtime: &Runtime, table: &str) -> Result<Vec<Column>, DotError> {
        let query = format!(
            "SELECT name, type, pk FROM pragma_table_info('{}')",
            table.replace('\'', "''")
        );

        let (_, stmt) = runtime.connection.prepare(&query)?;
        let stmt = stmt.ok_or_else(|| DotError::InvalidData("Failed to prepare query".into()))?;

        let mut columns = Vec::new();
        while let Ok(Some(row)) = stmt.next() {
            let name = row
                .first()
                .map(|v| v.as_str().to_owned())
                .unwrap_or_default();
            let col_type = row
                .get(1)
                .map(|v| v.as_str().to_owned())
                .unwrap_or_default();
            let is_pk = row
                .get(2)
                .is_some_and(|v| matches!(&v.value, ValueRefXValue::Int(i) if *i > 0));

            columns.push(Column {
                name,
                col_type,
                is_pk,
                is_fk: false,
            });
        }
        Ok(columns)
    }

    /// Get foreign key relationships for a specific table.
    fn get_foreign_keys(
        &self,
        runtime: &Runtime,
        table: &str,
    ) -> Result<Vec<ForeignKey>, DotError> {
        let query = format!(
            "SELECT \"from\", \"table\", \"to\" FROM pragma_foreign_key_list('{}')",
            table.replace('\'', "''")
        );

        let (_, stmt) = runtime.connection.prepare(&query)?;
        let stmt = stmt.ok_or_else(|| DotError::InvalidData("Failed to prepare query".into()))?;

        let mut fks = Vec::new();
        while let Ok(Some(row)) = stmt.next() {
            let from_column = row
                .first()
                .map(|v| v.as_str().to_owned())
                .unwrap_or_default();
            let to_table = row
                .get(1)
                .map(|v| v.as_str().to_owned())
                .unwrap_or_default();
            let to_column = row
                .get(2)
                .map(|v| v.as_str().to_owned())
                .unwrap_or_default();

            let is_unique = self.is_unique_fk(runtime, table, &from_column);

            fks.push(ForeignKey {
                from_table: table.to_owned(),
                from_column,
                to_table,
                to_column,
                is_unique,
            });
        }
        Ok(fks)
    }

    /// Check if a foreign key column is part of a unique constraint.
    fn is_unique_fk(&self, runtime: &Runtime, table: &str, column: &str) -> bool {
        let query = format!(
            r#"
            SELECT il.name
            FROM pragma_index_list('{}') AS il
            JOIN pragma_index_info(il.name) AS ii
            WHERE il."unique" = 1
              AND ii.name = '{}'
            "#,
            table.replace('\'', "''"),
            column.replace('\'', "''")
        );

        runtime
            .connection
            .prepare(&query)
            .ok()
            .and_then(|(_, stmt)| stmt)
            .is_some_and(|stmt| stmt.next().is_ok_and(|r| r.is_some()))
    }

    /// Generate the DOT graph definition.
    fn generate_dot(
        &self,
        table_columns: &HashMap<String, Vec<Column>>,
        foreign_keys: &[ForeignKey],
    ) -> String {
        let mut dot = String::from("digraph ERD {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  node [shape=plaintext];\n\n");

        // Build set of FK columns for highlighting
        let mut fk_columns: HashSet<(String, String)> = HashSet::new();
        for fk in foreign_keys {
            fk_columns.insert((fk.from_table.clone(), fk.from_column.clone()));
        }

        // Generate nodes for each table
        for (table, columns) in table_columns {
            dot.push_str(&format!("  {} [\n    label=<\n", Self::escape_id(table)));
            dot.push_str(
                "      <TABLE BORDER=\"0\" CELLBORDER=\"1\" CELLSPACING=\"0\" CELLPADDING=\"4\">\n",
            );
            dot.push_str(&format!(
                "        <TR><TD BGCOLOR=\"lightblue\"><B>{}</B></TD></TR>\n",
                Self::escape_html(table)
            ));

            let show_all = columns.len() <= 8;
            let mut shown = 0;

            for col in columns {
                let is_fk = fk_columns.contains(&(table.clone(), col.name.clone()));

                // Always show PKs and FKs, all columns for small tables
                if show_all || col.is_pk || is_fk {
                    let mut label = Self::escape_html(&col.name);

                    if col.is_pk {
                        label = format!("<B>{}</B> (PK)", label);
                    } else if is_fk {
                        label = format!("{} (FK)", label);
                    }

                    if !col.col_type.is_empty() {
                        label = format!("{}: {}", label, Self::escape_html(&col.col_type));
                    }

                    dot.push_str(&format!(
                        "        <TR><TD ALIGN=\"LEFT\">{}</TD></TR>\n",
                        label
                    ));
                    shown += 1;
                }
            }

            // Add ellipsis if not showing all columns
            if !show_all && shown < columns.len() {
                dot.push_str("        <TR><TD ALIGN=\"LEFT\">...</TD></TR>\n");
            }

            dot.push_str("      </TABLE>\n");
            dot.push_str("    >\n  ];\n\n");
        }

        // Generate edges for foreign keys
        for fk in foreign_keys {
            let from_id = Self::escape_id(&fk.from_table);
            let to_id = Self::escape_id(&fk.to_table);

            // Use different arrow styles based on relationship cardinality
            let arrow_style = if fk.is_unique {
                // One-to-one relationship
                "arrowhead=none, arrowtail=none, dir=both"
            } else {
                // Many-to-one relationship (crow's foot)
                "arrowhead=normal, arrowtail=crow, dir=both"
            };

            dot.push_str(&format!(
                "  {} -> {} [label=\"{}.{} -> {}.{}\", {}];\n",
                from_id,
                to_id,
                Self::escape_html(&fk.from_table),
                Self::escape_html(&fk.from_column),
                Self::escape_html(&fk.to_table),
                Self::escape_html(&fk.to_column),
                arrow_style
            ));
        }

        dot.push_str("}\n");
        dot
    }

    /// Escape a string for use as a DOT identifier.
    fn escape_id(s: &str) -> String {
        format!("\"{}\"", s.replace('"', "\\\""))
    }

    /// Escape a string for use in HTML-like labels.
    fn escape_html(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_id() {
        assert_eq!(GraphvizCommand::escape_id("users"), "\"users\"");
        assert_eq!(
            GraphvizCommand::escape_id("my\"table"),
            "\"my\\\"table\""
        );
    }

    #[test]
    fn test_escape_html() {
        assert_eq!(GraphvizCommand::escape_html("a<b>c"), "a&lt;b&gt;c");
        assert_eq!(GraphvizCommand::escape_html("a&b"), "a&amp;b");
        assert_eq!(GraphvizCommand::escape_html("a\"b"), "a&quot;b");
    }

    #[test]
    fn test_generate_dot_empty() {
        let cmd = GraphvizCommand {};
        let result = cmd.generate_dot(&HashMap::new(), &[]);
        assert!(result.contains("digraph ERD"));
        assert!(result.contains("rankdir=LR"));
    }

    #[test]
    fn test_generate_dot_single_table() {
        let cmd = GraphvizCommand {};
        let mut tables = HashMap::new();
        tables.insert(
            "users".to_string(),
            vec![
                Column {
                    name: "id".to_string(),
                    col_type: "INTEGER".to_string(),
                    is_pk: true,
                    is_fk: false,
                },
                Column {
                    name: "name".to_string(),
                    col_type: "TEXT".to_string(),
                    is_pk: false,
                    is_fk: false,
                },
            ],
        );

        let result = cmd.generate_dot(&tables, &[]);
        assert!(result.contains("\"users\""));
        assert!(result.contains("<B>id</B> (PK)"));
        assert!(result.contains("name: TEXT"));
    }

    #[test]
    fn test_generate_dot_with_fk() {
        let cmd = GraphvizCommand {};
        let mut tables = HashMap::new();
        tables.insert(
            "users".to_string(),
            vec![Column {
                name: "id".to_string(),
                col_type: "INTEGER".to_string(),
                is_pk: true,
                is_fk: false,
            }],
        );
        tables.insert(
            "orders".to_string(),
            vec![
                Column {
                    name: "id".to_string(),
                    col_type: "INTEGER".to_string(),
                    is_pk: true,
                    is_fk: false,
                },
                Column {
                    name: "user_id".to_string(),
                    col_type: "INTEGER".to_string(),
                    is_pk: false,
                    is_fk: true,
                },
            ],
        );

        let fks = vec![ForeignKey {
            from_table: "orders".to_string(),
            from_column: "user_id".to_string(),
            to_table: "users".to_string(),
            to_column: "id".to_string(),
            is_unique: false,
        }];

        let result = cmd.generate_dot(&tables, &fks);
        assert!(result.contains("\"orders\" -> \"users\""));
        assert!(result.contains("arrowtail=crow"));
    }
}
