//! Lint SQL files command

use crate::cli::LintArgs;
use solite_analyzer::{lint_with_config, LintConfig, RuleSeverity};
use solite_parser::parse_program;
use std::fs;
use std::io::{self, Read};

pub fn lint(args: LintArgs) -> Result<(), ()> {
    match lint_impl(args) {
        Ok(has_errors) => {
            if has_errors {
                Err(())
            } else {
                Ok(())
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            Err(())
        }
    }
}

fn lint_impl(args: LintArgs) -> Result<bool, anyhow::Error> {
    let config = match &args.config {
        Some(path) => LintConfig::load(path).map_err(|e| anyhow::anyhow!("{}", e))?,
        None => LintConfig::discover(),
    };

    let mut has_errors = false;

    if args.files.is_empty() {
        // Read from stdin
        let mut source = String::new();
        io::stdin().read_to_string(&mut source)?;
        has_errors |= lint_source("<stdin>", &source, &config, args.fix)?;
    } else {
        for path in &args.files {
            let source = fs::read_to_string(path)?;
            let path_str = path.display().to_string();
            let (file_has_errors, fixed_source) = lint_source_with_fix(&path_str, &source, &config)?;
            has_errors |= file_has_errors;

            // If --fix and we have fixes, write back
            if args.fix {
                if let Some(fixed) = fixed_source {
                    if fixed != source {
                        fs::write(path, &fixed)?;
                        eprintln!("Fixed {}", path.display());
                    }
                }
            }
        }
    }

    Ok(has_errors)
}

fn lint_source(filename: &str, source: &str, config: &LintConfig, _fix: bool) -> Result<bool, anyhow::Error> {
    let (has_errors, _) = lint_source_with_fix(filename, source, config)?;
    Ok(has_errors)
}

fn lint_source_with_fix(
    filename: &str,
    source: &str,
    config: &LintConfig,
) -> Result<(bool, Option<String>), anyhow::Error> {
    let program = match parse_program(source) {
        Ok(p) => p,
        Err(errors) => {
            for err in &errors {
                let (line, col) = offset_to_line_col(source, err.position());
                eprintln!("{}:{}:{}: error: {}", filename, line, col, err);
            }
            return Ok((true, None));
        }
    };

    let results = lint_with_config(&program, source, config, None);

    let mut has_errors = false;
    let mut fixes_to_apply: Vec<_> = vec![];

    for result in &results {
        let severity_str = match result.diagnostic.severity {
            RuleSeverity::Error => {
                has_errors = true;
                "error"
            }
            RuleSeverity::Warning => "warning",
            RuleSeverity::Off => continue,
        };

        // Calculate line/column from span
        let (line, col) = offset_to_line_col(source, result.diagnostic.span.start);

        eprintln!(
            "{}:{}:{}: {}[{}]: {}",
            filename,
            line,
            col,
            severity_str,
            result.diagnostic.rule_id,
            result.diagnostic.message
        );

        // Collect fixes
        if let Some(fix) = &result.fix {
            fixes_to_apply.push(fix.clone());
        }
    }

    // Apply fixes if any (in reverse order to maintain offsets)
    let fixed_source = if !fixes_to_apply.is_empty() {
        let mut fixed = source.to_string();
        // Sort by start position descending so we can apply fixes without offset issues
        fixes_to_apply.sort_by(|a, b| b.span.start.cmp(&a.span.start));
        for fix in fixes_to_apply {
            fixed.replace_range(fix.span.start..fix.span.end, &fix.replacement);
        }
        Some(fixed)
    } else {
        None
    };

    Ok((has_errors, fixed_source))
}

/// Convert a byte offset to (line, column), both 1-indexed
fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;

    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }

    (line, col)
}
