//! Test execution for mdtest
//!
//! Runs parsed tests and verifies assertions.

use crate::assertions::{parse_inline_inlay_hints, Assertion};
use crate::markers::{Marker, MarkerKind};
use crate::parser::{MdTest, TestFile};
use crate::reporter::TestFailure;
use crate::MdTestError;
use solite_analyzer::{build_schema, Schema};
use solite_lsp::completions::get_completions_for_context;
use solite_lsp::context::detect_context;
use solite_lsp::inlay_hints::get_inlay_hints;
use solite_parser::parse_program;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Result of running a test
#[derive(Debug)]
pub struct TestResult {
    /// Test name
    pub name: String,
    /// Whether all assertions passed
    pub passed: bool,
    /// Individual failures
    pub failures: Vec<TestFailure>,
    /// Source file path
    pub source_file: String,
    /// Source line number (1-indexed)
    pub source_line: usize,
}

impl TestResult {
    /// Format failures for display
    pub fn format_failures(&self) -> String {
        self.failures
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// Collect all markdown files recursively from a directory
fn collect_md_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            collect_md_files(&path, files)?;
        } else if path.extension().map(|e| e == "md").unwrap_or(false) {
            files.push(path);
        }
    }
    Ok(())
}

/// Run all tests in a directory (recursively)
pub fn run_tests(dir: &Path) -> Result<(), MdTestError> {
    let mut all_passed = true;
    let mut total_tests = 0;
    let mut passed_tests = 0;

    // Collect all markdown files recursively
    let mut md_files = Vec::new();
    collect_md_files(dir, &mut md_files)?;
    md_files.sort(); // Consistent ordering

    for path in md_files {
        // Print file header with relative path
        let relative_path = path.strip_prefix(dir).unwrap_or(&path);
        println!("\n{}:", relative_path.display());

        let content = std::fs::read_to_string(&path)?;
        let tests = crate::parse_markdown(&content, path.to_string_lossy().as_ref())?;

        for test in tests {
            total_tests += 1;
            let result = run_test(&test)?;

            if result.passed {
                passed_tests += 1;
                println!("  \x1b[32m✓\x1b[0m {} \x1b[90m({})\x1b[0m", test.name, test.assertions.len());
            } else {
                all_passed = false;
                println!("  \x1b[31m✗\x1b[0m {} ({}:{})", test.name, result.source_file, result.source_line);
                for failure in &result.failures {
                    println!("    {}", failure);
                }
            }
        }
    }

    println!("\n----------------------------------------");
    println!("Results: {} passed, {} failed, {} total", passed_tests, total_tests - passed_tests, total_tests);

    if all_passed {
        Ok(())
    } else {
        Err(MdTestError::TestFailed {
            name: "test suite".to_string(),
            details: format!("{} tests failed", total_tests - passed_tests),
        })
    }
}

/// Run a single test
pub fn run_test(test: &MdTest) -> Result<TestResult, MdTestError> {
    let mut failures = Vec::new();

    // Build combined SQL from all files
    let mut combined_sql = String::new();
    let mut all_markers: Vec<(usize, Marker)> = Vec::new(); // (file_index, marker)

    for (i, file) in test.files.iter().enumerate() {
        // Adjust marker offsets for combined SQL
        for marker in &file.markers {
            let mut adjusted_marker = marker.clone();
            adjusted_marker.clean_offset += combined_sql.len();
            all_markers.push((i, adjusted_marker));
        }

        combined_sql.push_str(&file.clean_content);
        if !combined_sql.ends_with('\n') {
            combined_sql.push('\n');
        }
    }

    // Build schema from DDL statements
    let mut schema = extract_schema_from_ddl(&combined_sql);
    let (functions, function_nargs) = discover_function_metadata();
    schema.set_functions(functions);
    schema.set_function_nargs(function_nargs);

    // Check marker assertions
    for assertion in &test.assertions {
        match assertion {
            Assertion::Autocomplete {
                marker_id,
                expected,
                strict,
            } => {
                if let Some((_file_idx, marker)) = all_markers
                    .iter()
                    .find(|(_, m)| m.kind == MarkerKind::Autocomplete && m.id == *marker_id)
                {
                    check_autocomplete(
                        &combined_sql,
                        marker,
                        &schema,
                        expected,
                        *strict || test.config.strict,
                        &mut failures,
                    );
                } else {
                    failures.push(TestFailure::MissingMarker {
                        marker_id: *marker_id,
                        kind: "ac".to_string(),
                    });
                }
            }
            Assertion::Hover {
                marker_id,
                contains,
            } => {
                if let Some((_file_idx, marker)) = all_markers
                    .iter()
                    .find(|(_, m)| m.kind == MarkerKind::Hover && m.id == *marker_id)
                {
                    check_hover(&combined_sql, marker, &schema, contains, &mut failures);
                } else {
                    failures.push(TestFailure::MissingMarker {
                        marker_id: *marker_id,
                        kind: "hv".to_string(),
                    });
                }
            }
        }
    }

    // Check inline diagnostics
    for file in &test.files {
        check_inline_diagnostics(file, &schema, &mut failures);
    }

    // Check inline inlay hints
    check_inlay_hints(&combined_sql, &mut failures);

    Ok(TestResult {
        name: test.name.clone(),
        passed: failures.is_empty(),
        failures,
        source_file: test.source_file.clone(),
        source_line: test.source_line,
    })
}

/// Discover function names and argument counts from a live SQLite connection with stdlib loaded.
fn discover_function_metadata() -> (Vec<String>, HashMap<String, Vec<i32>>) {
    let Ok(conn) = rusqlite::Connection::open_in_memory() else {
        return (vec![], HashMap::new());
    };
    unsafe {
        solite_stdlib::solite_stdlib_init(
            conn.handle(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
    let mut functions = Vec::new();
    let mut function_nargs: HashMap<String, Vec<i32>> = HashMap::new();
    let Ok(mut stmt) = conn.prepare("SELECT name, narg FROM pragma_function_list ORDER BY name")
    else {
        return (vec![], HashMap::new());
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?))
    }) else {
        return (vec![], HashMap::new());
    };
    for (name, narg) in rows.flatten() {
        let key = name.to_lowercase();
        let entry = function_nargs.entry(key).or_default();
        if !entry.contains(&narg) {
            entry.push(narg);
        }
        if !functions.contains(&name) {
            functions.push(name);
        }
    }
    for nargs in function_nargs.values_mut() {
        nargs.sort();
    }
    (functions, function_nargs)
}

/// Extract schema from DDL statements (CREATE TABLE, etc.)
/// This handles the case where DML statements might be invalid due to marker removal
fn extract_schema_from_ddl(sql: &str) -> Schema {
    // Try parsing the full SQL first
    if let Ok(program) = parse_program(sql) {
        return build_schema(&program);
    }

    // If that fails, try to extract and parse individual CREATE statements
    let mut schema = Schema::default();
    let sql_lower = sql.to_lowercase();
    let mut offset = 0;

    while let Some(create_pos) = sql_lower[offset..].find("create table") {
        let start = offset + create_pos;
        let rest = &sql[start..];
        let end = if let Some(semi_pos) = rest.find(';') {
            start + semi_pos + 1
        } else {
            sql.len()
        };

        let stmt_sql = &sql[start..end];

        if let Ok(program) = parse_program(stmt_sql) {
            let stmt_schema = build_schema(&program);
            for name in stmt_schema.table_names() {
                if let Some(cols) = stmt_schema.columns_for_table(name) {
                    let without_rowid = stmt_schema
                        .columns_for_table_with_rowid(name)
                        .map(|c| c.len() == cols.len())
                        .unwrap_or(false);

                    if let Some(info) = stmt_schema.get_table(name) {
                        schema.add_table_with_doc(
                            name,
                            cols.to_vec(),
                            without_rowid,
                            info.doc.clone(),
                            info.column_docs.clone(),
                        );
                    } else {
                        schema.add_table(name, cols.to_vec(), without_rowid);
                    }
                }
            }
        }

        offset = end;
    }

    schema
}

fn check_autocomplete(
    sql: &str,
    marker: &Marker,
    schema: &Schema,
    expected: &[String],
    strict: bool,
    failures: &mut Vec<TestFailure>,
) {
    // Use the real LSP context detection
    let ctx = detect_context(sql, marker.clean_offset);

    // Extract the prefix (partial word being typed at cursor)
    let before = &sql[..marker.clean_offset];
    let prefix_start = before
        .rfind(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
        .map(|i| i + 1)
        .unwrap_or(0);
    let prefix = &sql[prefix_start..marker.clean_offset];
    let prefix_opt = if prefix.is_empty() { None } else { Some(prefix) };

    // Use the shared completion function from solite_lsp
    let completions = get_completions_for_context(&ctx, Some(schema), prefix_opt);
    // Preserve order for display, use set for membership checks
    let completion_labels: Vec<String> = completions.into_iter().map(|c| c.label).collect();
    let completion_set: HashSet<String> = completion_labels.iter().cloned().collect();

    // Check expected items are present (preserving order from assertion)
    let missing: Vec<_> = expected
        .iter()
        .filter(|item| !completion_set.contains(*item))
        .cloned()
        .collect();

    if !missing.is_empty() {
        failures.push(TestFailure::AutocompleteMissing {
            marker_id: marker.id,
            missing,
            got: completion_labels.clone(),
        });
    }

    let expected_set: HashSet<String> = expected.iter().cloned().collect();

    // Empty assertion means "expect no completions" - treat as strict
    // In strict mode, check no extra items
    if strict || expected.is_empty() {
        let extra: Vec<_> = completion_labels
            .iter()
            .filter(|item| !expected_set.contains(*item))
            .cloned()
            .collect();
        if !extra.is_empty() {
            failures.push(TestFailure::AutocompleteExtra {
                marker_id: marker.id,
                extra,
            });
        }
    }
}

fn check_hover(
    sql: &str,
    marker: &Marker,
    schema: &Schema,
    expected_contains: &[String],
    failures: &mut Vec<TestFailure>,
) {
    use solite_analyzer::symbols::{
        find_statement_at_offset, find_symbol_at_offset, format_hover_content,
    };

    let program = match parse_program(sql) {
        Ok(p) => p,
        Err(_) => {
            failures.push(TestFailure::HoverFailed {
                marker_id: marker.id,
                reason: "SQL parse error".to_string(),
            });
            return;
        }
    };

    // The hover marker is placed AFTER the token, so look slightly before
    let hover_offset = if marker.clean_offset > 0 {
        marker.clean_offset - 1
    } else {
        marker.clean_offset
    };

    let stmt = match find_statement_at_offset(&program, hover_offset) {
        Some(s) => s,
        None => {
            failures.push(TestFailure::HoverFailed {
                marker_id: marker.id,
                reason: "No statement at position".to_string(),
            });
            return;
        }
    };

    let symbol_result = find_symbol_at_offset(stmt, sql, hover_offset, Some(schema));

    match symbol_result {
        Some((symbol, _span)) => {
            let hover_content = format_hover_content(&symbol, Some(schema));

            for expected in expected_contains {
                if !hover_content.contains(expected) {
                    failures.push(TestFailure::HoverMissing {
                        marker_id: marker.id,
                        expected: expected.clone(),
                        got: hover_content.clone(),
                    });
                }
            }
        }
        None => {
            failures.push(TestFailure::HoverFailed {
                marker_id: marker.id,
                reason: "No symbol found at position".to_string(),
            });
        }
    }
}

fn check_inline_diagnostics(file: &TestFile, schema: &Schema, failures: &mut Vec<TestFailure>) {
    use solite_analyzer::analyze_with_schema;

    let program = match parse_program(&file.clean_content) {
        Ok(p) => p,
        Err(errs) => {
            let has_parse_error_assertion = file
                .inline_diagnostics
                .iter()
                .any(|d| !d.is_ok && d.rule.as_deref() == Some("parse-error"));

            if !has_parse_error_assertion && !file.inline_diagnostics.is_empty() {
                failures.push(TestFailure::UnexpectedDiagnostic {
                    line: 0,
                    message: format!("Parse error: {:?}", errs),
                });
            }
            return;
        }
    };

    let diagnostics = analyze_with_schema(&program, Some(schema));
    let mut matched_diagnostics: HashSet<usize> = HashSet::new();

    for inline in &file.inline_diagnostics {
        if inline.is_ok {
            let line_errors: Vec<_> = diagnostics
                .iter()
                .enumerate()
                .filter(|(_, d)| {
                    let line = file.clean_content[..d.span.start].matches('\n').count() as u32;
                    line == inline.line
                })
                .collect();

            if !line_errors.is_empty() {
                failures.push(TestFailure::UnexpectedDiagnostic {
                    line: inline.line,
                    message: line_errors
                        .iter()
                        .map(|(_, d)| d.message.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                });
            }
        } else {
            let matching: Vec<_> = diagnostics
                .iter()
                .enumerate()
                .filter(|(idx, d)| {
                    if matched_diagnostics.contains(idx) {
                        return false;
                    }

                    let line = file.clean_content[..d.span.start].matches('\n').count() as u32;

                    if line != inline.line {
                        return false;
                    }

                    if let Some(ref msg) = inline.message {
                        if !d.message.contains(msg) {
                            return false;
                        }
                    }

                    true
                })
                .collect();

            if matching.is_empty() {
                failures.push(TestFailure::MissingDiagnostic {
                    line: inline.line,
                    rule: inline.rule.clone(),
                    message: inline.message.clone(),
                });
            } else {
                for (idx, _) in matching {
                    matched_diagnostics.insert(idx);
                }
            }
        }
    }

    if !file.inline_diagnostics.is_empty() {
        for (idx, diag) in diagnostics.iter().enumerate() {
            if !matched_diagnostics.contains(&idx) {
                let line = file.clean_content[..diag.span.start].matches('\n').count() as u32;
                failures.push(TestFailure::UnexpectedDiagnostic {
                    line,
                    message: diag.message.clone(),
                });
            }
        }
    }
}

/// Check inlay hint assertions in the SQL
fn check_inlay_hints(sql: &str, failures: &mut Vec<TestFailure>) {
    let expected_hints = parse_inline_inlay_hints(sql);
    if expected_hints.is_empty() {
        return; // No inlay assertions in this test
    }

    // Parse and get actual hints
    let program = match parse_program(sql) {
        Ok(p) => p,
        Err(_) => return, // Can't check hints if we can't parse
    };

    let actual_hints = get_inlay_hints(&program);

    // Convert actual hints to a map by line number
    // Multiple hints can be on the same line
    let actual_by_line: HashMap<u32, Vec<String>> = {
        let mut map: HashMap<u32, Vec<String>> = HashMap::new();
        for hint in &actual_hints {
            let line = offset_to_line(sql, hint.position);
            map.entry(line).or_default().push(hint.label.clone());
        }
        map
    };

    // Check each expected hint
    for expected in &expected_hints {
        let actual_labels = actual_by_line.get(&expected.line);
        match actual_labels {
            Some(labels) if labels.contains(&expected.label) => {
                // Match!
            }
            Some(labels) => {
                failures.push(TestFailure::InlayHintMismatch {
                    line: expected.line,
                    expected: expected.label.clone(),
                    actual: labels.join(", "),
                });
            }
            None => {
                failures.push(TestFailure::InlayHintMissing {
                    line: expected.line,
                    expected: expected.label.clone(),
                });
            }
        }
    }
}

/// Convert byte offset to line number (0-indexed)
fn offset_to_line(text: &str, offset: usize) -> u32 {
    text[..offset.min(text.len())].matches('\n').count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markers::extract_markers;
    use solite_lsp::context::CompletionContext;

    #[test]
    fn test_create_table_column_context() {
        // Inside CREATE TABLE column definitions should return no completions
        let sql_with_marker = "create table t(\n <ac1>\n)";
        let extract_result = extract_markers(sql_with_marker);

        let marker = &extract_result.markers[0];

        // Check what context is detected
        let ctx = detect_context(&extract_result.clean_sql, marker.clean_offset);

        // Inside column definitions should return None context
        assert_eq!(ctx, CompletionContext::None,
            "Inside CREATE TABLE () should return None context");

        // Get completions - should be empty
        let schema = Schema::default();
        let completions = get_completions_for_context(&ctx, Some(&schema), None);
        assert!(completions.is_empty(),
            "Should have no completions inside CREATE TABLE column definitions");
    }
}
