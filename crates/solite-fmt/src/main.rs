//! Solite SQL Formatter CLI
//!
//! A command-line tool for formatting SQLite SQL files.

use clap::Parser;
use solite_fmt::{check_formatted, format_sql, FormatConfig, FormatError};
use solite_schema::Document;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "solite-fmt")]
#[command(about = "A SQL formatter for SQLite")]
#[command(version)]
struct Cli {
    /// SQL files to format (reads from stdin if none provided)
    files: Vec<PathBuf>,

    /// Write formatted output back to files
    #[arg(short, long)]
    write: bool,

    /// Check if files are formatted (exit 1 if not)
    #[arg(long)]
    check: bool,

    /// Show diff of changes
    #[arg(long)]
    diff: bool,

    /// Path to config file (default: auto-discover)
    #[arg(long)]
    config: Option<PathBuf>,

    /// Override keyword case (upper, lower, preserve)
    #[arg(long)]
    keyword_case: Option<String>,

    /// Override indentation size
    #[arg(long)]
    indent_size: Option<usize>,

    /// Use tabs for indentation
    #[arg(long)]
    use_tabs: bool,

    /// Override line width
    #[arg(long)]
    line_width: Option<usize>,

    /// Disable dot command processing
    #[arg(long)]
    no_dot_commands: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Load config
    let mut config = if let Some(ref path) = cli.config {
        match FormatConfig::load(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error loading config from {:?}: {}", path, e);
                return ExitCode::from(2);
            }
        }
    } else {
        FormatConfig::discover()
    };

    // Apply CLI overrides
    if let Some(ref case) = cli.keyword_case {
        config.keyword_case = match case.to_lowercase().as_str() {
            "upper" => solite_fmt::KeywordCase::Upper,
            "lower" => solite_fmt::KeywordCase::Lower,
            "preserve" => solite_fmt::KeywordCase::Preserve,
            _ => {
                eprintln!("Invalid keyword-case: {}", case);
                return ExitCode::from(2);
            }
        };
    }

    if let Some(size) = cli.indent_size {
        config.indent_size = size;
    }

    if cli.use_tabs {
        config.indent_style = solite_fmt::IndentStyle::Tabs;
    }

    if let Some(width) = cli.line_width {
        config.line_width = width;
    }

    // Process files or stdin
    if cli.files.is_empty() {
        // Read from stdin
        let mut source = String::new();
        if let Err(e) = io::stdin().read_to_string(&mut source) {
            eprintln!("Error reading stdin: {}", e);
            return ExitCode::from(2);
        }

        let enable_dot_commands = !cli.no_dot_commands;
        match process_source(&source, &config, &cli, None, enable_dot_commands) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(e) => {
                eprintln!("Error: {}", e);
                ExitCode::from(2)
            }
        }
    } else {
        let mut any_unformatted = false;
        let mut any_error = false;

        for path in &cli.files {
            let source = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error reading {:?}: {}", path, e);
                    any_error = true;
                    continue;
                }
            };

            let enable_dot_commands = !cli.no_dot_commands;
            match process_source(&source, &config, &cli, Some(path), enable_dot_commands) {
                Ok(true) => {}
                Ok(false) => any_unformatted = true,
                Err(e) => {
                    eprintln!("Error formatting {:?}: {}", path, e);
                    any_error = true;
                }
            }
        }

        if any_error {
            ExitCode::from(2)
        } else if any_unformatted && cli.check {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        }
    }
}

fn process_source(
    source: &str,
    config: &FormatConfig,
    cli: &Cli,
    path: Option<&PathBuf>,
    enable_dot_commands: bool,
) -> Result<bool, FormatError> {
    // Parse document to separate dot commands from SQL
    let doc = Document::parse(source, enable_dot_commands);

    // Check if the document has any dot command lines (recognized or not).
    // We detect this by checking if the SQL regions don't cover the entire source.
    // A single SQL region covering [0, source.len()) means no dot commands were found.
    let has_dot_command_lines = doc.sql_regions.len() != 1
        || doc
            .sql_regions
            .first()
            .is_none_or(|r| r.start != 0 || r.end != source.len());

    // If no dot command lines or dot commands disabled, format the entire source
    if !has_dot_command_lines {
        return process_plain_sql(source, config, cli, path);
    }

    // Format with dot command handling
    process_with_dot_commands(source, &doc, config, cli, path)
}

/// Process a plain SQL file (no dot commands)
fn process_plain_sql(
    source: &str,
    config: &FormatConfig,
    cli: &Cli,
    path: Option<&PathBuf>,
) -> Result<bool, FormatError> {
    // Check mode
    if cli.check {
        let is_formatted = check_formatted(source, config)?;
        if !is_formatted {
            if let Some(p) = path {
                eprintln!("Would reformat: {:?}", p);
            } else {
                eprintln!("Would reformat: <stdin>");
            }
        }
        return Ok(is_formatted);
    }

    // Format
    let formatted = format_sql(source, config)?;

    // Diff mode
    if cli.diff {
        if source != formatted {
            if let Some(p) = path {
                println!("--- {:?}", p);
                println!("+++ {:?} (formatted)", p);
            } else {
                println!("--- <stdin>");
                println!("+++ <stdin> (formatted)");
            }
            print_diff(source, &formatted);
        }
        return Ok(source == formatted);
    }

    // Write mode
    if cli.write {
        if let Some(p) = path {
            if source != formatted {
                if let Err(e) = fs::write(p, &formatted) {
                    eprintln!("Error writing {:?}: {}", p, e);
                    return Err(FormatError::IoError(e));
                }
                eprintln!("Formatted: {:?}", p);
            }
        } else {
            // Can't write back to stdin
            print!("{}", formatted);
        }
    } else {
        // Print to stdout
        print!("{}", formatted);
        let _ = io::stdout().flush();
    }

    Ok(true)
}

/// Process a document with dot commands, formatting only SQL regions
fn process_with_dot_commands(
    source: &str,
    doc: &Document,
    config: &FormatConfig,
    cli: &Cli,
    path: Option<&PathBuf>,
) -> Result<bool, FormatError> {
    // Build formatted output by:
    // 1. Preserving dot command lines as-is
    // 2. Formatting SQL regions
    let formatted = reconstruct_with_formatted_sql(source, doc, config)?;

    // Check mode
    if cli.check {
        let is_formatted = source == formatted;
        if !is_formatted {
            if let Some(p) = path {
                eprintln!("Would reformat: {:?}", p);
            } else {
                eprintln!("Would reformat: <stdin>");
            }
        }
        return Ok(is_formatted);
    }

    // Diff mode
    if cli.diff {
        if source != formatted {
            if let Some(p) = path {
                println!("--- {:?}", p);
                println!("+++ {:?} (formatted)", p);
            } else {
                println!("--- <stdin>");
                println!("+++ <stdin> (formatted)");
            }
            print_diff(source, &formatted);
        }
        return Ok(source == formatted);
    }

    // Write mode
    if cli.write {
        if let Some(p) = path {
            if source != formatted {
                if let Err(e) = fs::write(p, &formatted) {
                    eprintln!("Error writing {:?}: {}", p, e);
                    return Err(FormatError::IoError(e));
                }
                eprintln!("Formatted: {:?}", p);
            }
        } else {
            // Can't write back to stdin
            print!("{}", formatted);
        }
    } else {
        // Print to stdout
        print!("{}", formatted);
        let _ = io::stdout().flush();
    }

    Ok(true)
}

/// Reconstruct the document with formatted SQL regions while preserving dot commands
fn reconstruct_with_formatted_sql(
    source: &str,
    doc: &Document,
    config: &FormatConfig,
) -> Result<String, FormatError> {
    // If there are no SQL regions, just return the source as-is
    if doc.sql_regions.is_empty() {
        return Ok(source.to_string());
    }

    // Build a list of (start, end, content) for all regions
    // We need to track what's a dot command line vs SQL region
    let mut result = String::new();
    let mut pos = 0;

    // Process the source line by line, formatting SQL regions
    for line in source.lines() {
        let line_start = pos;
        let line_end = pos + line.len();

        if line.starts_with('.') {
            // Dot command line - preserve as-is
            result.push_str(line);
            result.push('\n');
        } else {
            // Check if this line is part of an SQL region
            let in_sql_region = doc.sql_regions.iter().any(|r| {
                // Line overlaps with SQL region
                line_start < r.end && line_end > r.start
            });

            if in_sql_region {
                // This line is SQL - we'll format the entire region at once
                // Check if this is the start of a new SQL region
                let is_region_start = doc.sql_regions.iter().any(|r| {
                    r.start == line_start || (r.start < line_start && !source[r.start..line_start].contains('\n'))
                });

                if is_region_start {
                    // Find the SQL region that starts here or contains this line
                    if let Some(region) = doc.sql_regions.iter().find(|r| {
                        line_start >= r.start && line_start < r.end
                    }) {
                        // Get the full SQL text for this region
                        let sql_text = &source[region.start..region.end];

                        // Format the SQL
                        let formatted_sql = format_sql(sql_text.trim(), config)?;

                        // Add formatted SQL (it already ends with newline from formatter)
                        result.push_str(&formatted_sql);

                        // Skip ahead in the source to after this region
                        // (we'll handle this by tracking which regions we've processed)
                    }
                }
            } else if line.trim().is_empty() {
                // Empty line between regions
                result.push('\n');
            }
            // For lines within SQL regions (not at the start), we skip them
            // because we process the entire region when we hit the start
        }

        // Move position past the line and newline
        pos = line_end;
        if pos < source.len() {
            if source[pos..].starts_with("\r\n") {
                pos += 2;
            } else if source[pos..].starts_with('\n') || source[pos..].starts_with('\r') {
                pos += 1;
            }
        }
    }

    // Ensure the output ends with a newline if the source did
    if source.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }

    Ok(result)
}

fn print_diff(old: &str, new: &str) {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_lines(old, new);

    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        print!("{}{}", sign, change);
    }
}
