//! Markdown parser for mdtest files
//!
//! Parses markdown files into structured test cases.

use crate::assertions::{parse_assertions, parse_inline_diagnostics, Assertion, InlineDiagnostic};
use crate::markers::{extract_markers, Marker};
use crate::MdTestError;
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

/// A parsed test from markdown
#[derive(Debug)]
pub struct MdTest {
    /// Test name (from headers)
    pub name: String,
    /// Files in this test
    pub files: Vec<TestFile>,
    /// Assertions about markers
    pub assertions: Vec<Assertion>,
    /// Configuration (from TOML blocks)
    pub config: TestConfig,
    /// Source file path
    pub source_file: String,
    /// Source line number (1-indexed)
    pub source_line: usize,
}

/// A file in the test
#[derive(Debug)]
pub struct TestFile {
    /// Optional path (if labeled, e.g., `schema.sql:`)
    pub path: Option<String>,
    /// Original SQL content (with markers)
    pub original_content: String,
    /// Clean SQL content (markers removed)
    pub clean_content: String,
    /// Markers found in this file
    pub markers: Vec<Marker>,
    /// Inline diagnostic assertions
    pub inline_diagnostics: Vec<InlineDiagnostic>,
}

/// Test configuration from TOML blocks
#[derive(Debug, Default, Clone)]
pub struct TestConfig {
    /// Lint rule overrides
    pub lint_rules: Vec<(String, String)>,
    /// Whether assertions must be strict by default
    pub strict: bool,
}

/// Parse a markdown file into test cases
pub fn parse_markdown(content: &str, file_name: &str) -> Result<Vec<MdTest>, MdTestError> {
    use pulldown_cmark::Options;

    let parser = Parser::new_ext(content, Options::empty()).into_offset_iter();
    let mut tests = Vec::new();
    let mut current_headers: Vec<String> = Vec::new();
    let mut current_files: Vec<TestFile> = Vec::new();
    let mut current_config = TestConfig::default();
    let mut pending_label: Option<String> = None;
    let mut assertion_text = String::new();
    let mut in_code_block = false;
    let mut code_block_lang: Option<String> = None;
    let mut code_block_content = String::new();

    // Track header levels for proper nesting
    let mut header_levels: Vec<usize> = Vec::new();
    let mut in_heading = false;

    // Track test source location (line number of header that defines the test)
    let mut current_test_line: usize = 1;

    // Helper to convert byte offset to line number (1-indexed)
    let offset_to_line = |offset: usize| -> usize {
        content[..offset].matches('\n').count() + 1
    };

    for (event, range) in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                // Finalize previous test if we have files
                if !current_files.is_empty() {
                    let test = finalize_test(
                        &current_headers,
                        std::mem::take(&mut current_files),
                        &assertion_text,
                        current_config.clone(),
                        file_name,
                        current_test_line,
                    );
                    tests.push(test);
                    assertion_text.clear();
                }

                // Record line number of this header (potential test start)
                current_test_line = offset_to_line(range.start);

                // Pop headers until we're at the right level
                let level_num = level as usize;
                while header_levels.last().map(|&l| l >= level_num).unwrap_or(false) {
                    header_levels.pop();
                    current_headers.pop();
                }
                header_levels.push(level_num);
                current_headers.push(String::new()); // Placeholder
                in_heading = true;
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = false;
            }
            Event::Text(text) => {
                if in_heading {
                    // Update the current header text
                    if let Some(last) = current_headers.last_mut() {
                        last.push_str(&text);
                    }
                } else if in_code_block {
                    code_block_content.push_str(&text);
                } else {
                    // Collect text for assertion parsing
                    assertion_text.push_str(&text);
                    assertion_text.push('\n');
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_code_block {
                    code_block_content.push('\n');
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code_block = true;
                code_block_content.clear();
                code_block_lang = match kind {
                    CodeBlockKind::Fenced(lang) => {
                        let lang = lang.to_string();
                        if lang.is_empty() {
                            None
                        } else {
                            Some(lang)
                        }
                    }
                    CodeBlockKind::Indented => None,
                };
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;

                if let Some(ref lang) = code_block_lang {
                    match lang.as_str() {
                        "sql" => {
                            // Process SQL file
                            let extract_result = extract_markers(&code_block_content);
                            let inline_diagnostics =
                                parse_inline_diagnostics(&extract_result.clean_sql);

                            current_files.push(TestFile {
                                path: pending_label.take(),
                                original_content: code_block_content.clone(),
                                clean_content: extract_result.clean_sql,
                                markers: extract_result.markers,
                                inline_diagnostics,
                            });
                        }
                        "toml" => {
                            // Parse config (simplified - just extract lint rules)
                            current_config = parse_toml_config(&code_block_content);
                        }
                        "assertions" => {
                            // Assertion code block
                            assertion_text.push_str(&code_block_content);
                        }
                        _ => {
                            // Ignore other code blocks
                        }
                    }
                }

                code_block_lang = None;
            }
            Event::Start(Tag::Paragraph) => {
                // Check if this paragraph is a file label (ends with .sql:)
            }
            Event::End(TagEnd::Paragraph) => {}
            Event::Start(Tag::Item) => {
                // Add dash for list items (needed for assertion parsing)
                assertion_text.push_str("- ");
            }
            Event::End(TagEnd::Item) => {
                assertion_text.push('\n');
            }
            Event::Code(code) => {
                // Check for file labels like `schema.sql`:
                let code_str = code.to_string();
                let is_file_label = code_str.ends_with(".sql");
                // Add to assertion text
                assertion_text.push('`');
                assertion_text.push_str(&code_str);
                assertion_text.push('`');
                // Set as pending label if it's a file label
                if is_file_label {
                    pending_label = Some(code_str);
                }
            }
            _ => {}
        }
    }

    // Finalize last test
    if !current_files.is_empty() {
        let test = finalize_test(
            &current_headers,
            std::mem::take(&mut current_files),
            &assertion_text,
            current_config.clone(),
            file_name,
            current_test_line,
        );
        tests.push(test);
    }

    // If no headers were found, create a single test with the file name
    if tests.is_empty() && !current_files.is_empty() {
        tests.push(MdTest {
            name: file_name.to_string(),
            files: current_files,
            assertions: parse_assertions(&assertion_text),
            config: current_config.clone(),
            source_file: file_name.to_string(),
            source_line: 1,
        });
    }

    Ok(tests)
}

fn finalize_test(
    headers: &[String],
    files: Vec<TestFile>,
    assertion_text: &str,
    config: TestConfig,
    source_file: &str,
    source_line: usize,
) -> MdTest {
    let name = headers
        .iter()
        .filter(|h| !h.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" - ");

    MdTest {
        name: if name.is_empty() {
            "unnamed".to_string()
        } else {
            name
        },
        files,
        assertions: parse_assertions(assertion_text),
        config,
        source_file: source_file.to_string(),
        source_line,
    }
}

fn parse_toml_config(toml_content: &str) -> TestConfig {
    let mut config = TestConfig::default();

    // Simple TOML parsing for [lint] section
    let mut in_lint_section = false;

    for line in toml_content.lines() {
        let line = line.trim();

        if line == "[lint]" {
            in_lint_section = true;
            continue;
        }

        if line.starts_with('[') {
            in_lint_section = false;
            continue;
        }

        if in_lint_section {
            // Parse "rule-id" = "level"
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().trim_matches('"');
                let value = value.trim().trim_matches('"');
                config.lint_rules.push((key.to_string(), value.to_string()));
            }
        }

        if line == "strict = true" {
            config.strict = true;
        }
    }

    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_test() {
        let md = r#"
# Simple Test

```sql
select * from <ac1>;
```

- `<ac1>`: users, tables
"#;

        let tests = parse_markdown(md, "test.md").unwrap();
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "Simple Test");
        assert_eq!(tests[0].files.len(), 1);
        assert_eq!(tests[0].files[0].clean_content, "select * from ;\n");
        assert_eq!(tests[0].files[0].markers.len(), 1);
        assert_eq!(tests[0].assertions.len(), 1);
    }

    #[test]
    fn test_parse_nested_headers() {
        let md = r#"
# Autocomplete

## Tables

### After FROM

```sql
select * from <ac1>;
```

- `<ac1>`: users

### After JOIN

```sql
select * from a join <ac1>;
```

- `<ac1>`: tables
"#;

        let tests = parse_markdown(md, "test.md").unwrap();
        assert_eq!(tests.len(), 2);
        assert_eq!(tests[0].name, "Autocomplete - Tables - After FROM");
        assert_eq!(tests[1].name, "Autocomplete - Tables - After JOIN");
    }

    #[test]
    fn test_parse_multiple_files() {
        let md = r#"
# Multi-file Test

`schema.sql`:

```sql
create table users(id, name);
```

`query.sql`:

```sql
select <ac1> from users;
```

- `<ac1>`: id, name
"#;

        let tests = parse_markdown(md, "test.md").unwrap();
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].files.len(), 2);
        assert_eq!(tests[0].files[0].path, Some("schema.sql".to_string()));
        assert_eq!(tests[0].files[1].path, Some("query.sql".to_string()));
    }

    #[test]
    fn test_parse_inline_diagnostics() {
        let md = r#"
# Diagnostics Test

```sql
select "value"; -- error: [double-quoted-string]
select 'ok';    -- ok
```
"#;

        let tests = parse_markdown(md, "test.md").unwrap();
        assert_eq!(tests[0].files[0].inline_diagnostics.len(), 2);
        assert!(!tests[0].files[0].inline_diagnostics[0].is_ok);
        assert!(tests[0].files[0].inline_diagnostics[1].is_ok);
    }

    #[test]
    fn test_parse_toml_config() {
        let md = r#"
# Config Test

```toml
[lint]
"double-quoted-string" = "off"

[test]
strict = true
```

```sql
select "value"; -- ok
```
"#;

        let tests = parse_markdown(md, "test.md").unwrap();
        assert_eq!(tests[0].config.lint_rules.len(), 1);
        assert_eq!(
            tests[0].config.lint_rules[0],
            ("double-quoted-string".to_string(), "off".to_string())
        );
    }
}
