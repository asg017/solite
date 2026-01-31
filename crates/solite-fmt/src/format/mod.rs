//! Format implementations for AST nodes
//!
//! This module contains the FormatNode trait and implementations
//! for all AST types.

pub mod clause;
pub mod ddl;
pub mod expr;
pub mod stmt;

use crate::printer::Printer;

/// Trait for AST nodes that can be formatted
pub trait FormatNode {
    /// Format this node using the given printer
    fn format(&self, p: &mut Printer);
}

/// Format a qualified name (schema.name or just name)
pub fn format_qualified_name(p: &mut Printer, schema: &Option<String>, name: &str) {
    if let Some(s) = schema {
        format_identifier(p, s);
        p.write(".");
    }
    format_identifier(p, name);
}

/// Format an identifier, quoting if necessary
pub fn format_identifier(p: &mut Printer, name: &str) {
    // Check if identifier needs quoting
    if needs_quoting(name) {
        p.write("\"");
        // Escape any double quotes in the name
        p.write(&name.replace("\"", "\"\""));
        p.write("\"");
    } else {
        p.write(name);
    }
}

/// Check if an identifier needs to be quoted
fn needs_quoting(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }

    // Must start with letter or underscore
    let first = name.chars().next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' {
        return true;
    }

    // Must contain only alphanumeric or underscore
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return true;
    }

    // Check if it's a reserved keyword
    is_reserved_keyword(name)
}

/// Check if a name is a reserved SQL keyword
fn is_reserved_keyword(name: &str) -> bool {
    let upper = name.to_uppercase();
    matches!(
        upper.as_str(),
        "SELECT"
            | "FROM"
            | "WHERE"
            | "AND"
            | "OR"
            | "NOT"
            | "NULL"
            | "TRUE"
            | "FALSE"
            | "INSERT"
            | "UPDATE"
            | "DELETE"
            | "CREATE"
            | "DROP"
            | "TABLE"
            | "INDEX"
            | "VIEW"
            | "TRIGGER"
            | "AS"
            | "ON"
            | "JOIN"
            | "LEFT"
            | "RIGHT"
            | "INNER"
            | "OUTER"
            | "FULL"
            | "CROSS"
            | "NATURAL"
            | "ORDER"
            | "BY"
            | "GROUP"
            | "HAVING"
            | "LIMIT"
            | "OFFSET"
            | "UNION"
            | "INTERSECT"
            | "EXCEPT"
            | "ALL"
            | "DISTINCT"
            | "BEGIN"
            | "COMMIT"
            | "ROLLBACK"
            | "SAVEPOINT"
            | "RELEASE"
            | "CASE"
            | "WHEN"
            | "THEN"
            | "ELSE"
            | "END"
            | "IN"
            | "BETWEEN"
            | "LIKE"
            | "GLOB"
            | "IS"
            | "EXISTS"
            | "CAST"
            | "PRIMARY"
            | "KEY"
            | "FOREIGN"
            | "REFERENCES"
            | "UNIQUE"
            | "CHECK"
            | "DEFAULT"
            | "CONSTRAINT"
            | "COLLATE"
            | "ASC"
            | "DESC"
            | "NULLS"
            | "FIRST"
            | "LAST"
            | "WITH"
            | "RECURSIVE"
            | "VALUES"
            | "SET"
            | "INTO"
            | "RETURNING"
    )
}

/// Format a list of items with proper separators
pub fn format_list<T, F>(p: &mut Printer, items: &[T], multiline: bool, format_item: F)
where
    F: Fn(&mut Printer, &T),
{
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            p.list_separator(multiline);
        }
        format_item(p, item);
    }
}

/// Format an optional alias
pub fn format_alias(p: &mut Printer, alias: &Option<String>, has_as: bool) {
    if let Some(name) = alias {
        p.space();
        if has_as {
            p.keyword("AS");
            p.space();
        }
        format_identifier(p, name);
    }
}
