use crate::cli::TestArgs;
use console::Style;
use solite_core::dot::{DotCommand, LoadCommand};
use solite_core::{BlockSource, Runtime, StepResult};
use solite_core::sqlite::{ValueRefX, ValueRefXValue};
use std::fs::read_to_string;
use std::path::PathBuf;
use codespan_reporting::files::SimpleFiles;
use codespan_reporting::diagnostic::{Diagnostic, Label};
use codespan_reporting::term::{self, termcolor::{ColorChoice, StandardStream}};

fn parse_epilogue_comment(ep: &str) -> String {
    let s = ep.trim();
    let s = if s.starts_with("--") {
        s[2..].trim()
    } else if s.starts_with("/*") && s.ends_with("*/") {
        s[2..s.len() - 2].trim()
    } else if s.starts_with("/*") {
        // unterminated block style: strip leading /*
        s[2..].trim()
    } else {
        s
    };
    s.to_string()
}

fn value_to_string(v: &ValueRefX) -> String {
    match &v.value {
        ValueRefXValue::Null => "NULL".to_string(),
        ValueRefXValue::Int(i) => format!("{}", i),
        ValueRefXValue::Double(d) => {
            // Use a reasonably short representation
            let s = format!("{}", d);
            s
        }
        ValueRefXValue::Text(b) => {
            // return SQL single-quoted literal, escaping single quotes
            let mut s = String::from("'");
            let mut inner = String::from_utf8_lossy(b).to_string();
            inner = inner.replace("'", "''");
            s.push_str(&inner);
            s.push('\'');
            s
        }
        ValueRefXValue::Blob(_) => {
            // TODO: handle blobs
            todo!("blob comparison not implemented")
        }
    }
}

fn test_impl(args: TestArgs) -> Result<(), anyhow::Error> {
    let source_path: PathBuf = args.file;
    let content = read_to_string(&source_path)?;

    let mut rt = Runtime::new(None);
    rt.enqueue(
        &source_path.to_string_lossy(),
        &content,
        BlockSource::File(source_path.clone()),
    );

    let mut successes = 0usize;
    let mut failures = 0usize;
    let mut todos: Vec<(String, usize, usize, String)> = vec![];

    // print progress symbols inline
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

    loop {
        match rt.next_stepx() {
            None => break,
            Some(Err(e)) => {
                failures += 1;
                eprintln!("Error preparing step: {}", e);
                print!("{}", Style::new().red().apply_to("x"));
            }
            Some(Ok(step)) => match step.result {
                StepResult::DotCommand(cmd) => {
                    match cmd {
                        DotCommand::Load(cmdx) => {
                            cmdx.execute(&mut rt.connection).ok();
                        }
                        _ => todo!(),
                    }
                }
                StepResult::SqlStatement { stmt, .. } => {
                    let ep = step.epilogue.clone();
                    let ep = match ep {
                        Some(s) => parse_epilogue_comment(&s),
                        None => {
                            failures += 1;
                            print!("{}", Style::new().red().apply_to("x"));
                            continue;
                        }
                    };

                    if ep.to_uppercase().starts_with("TODO") {
                        // capture location from step.reference via its Display
                        let ref_display = format!("{}", step.reference);
                        if let Some((file, line, col)) = parse_ref_file_line_col(&ref_display) {
                            todos.push((file, line, col, ep.clone()));
                        } else {
                            todos.push((source_path.to_string_lossy().to_string(), 0, 0, ep.clone()));
                        }
                        print!("{}", Style::new().yellow().apply_to("-"));
                        continue;
                    }

                    // execute and compare
                    match stmt.next() {
                        Err(err) => {
                            // compute offset from step reference to show diagnostic
                            let ref_display = format!("{}", step.reference);
                            let maybe_offset = compute_offset_from_reference(&content, &ref_display);
                            if ep.starts_with("error:") {
                                let expected = ep["error:".len()..].trim();
                                if expected == err.message {
                                    successes += 1;
                                    print!("{}", Style::new().green().apply_to("."));
                                } else {
                                    failures += 1;
                                    print!("{}", Style::new().red().apply_to("x"));
                                    // print diagnostic for sqlite error
                                    if let Some(off) = maybe_offset {
                                        crate::errors::report_error(&source_path.to_string_lossy(), &content, &err, Some(off));
                                    } else {
                                        crate::errors::report_error(&source_path.to_string_lossy(), &content, &err, None);
                                    }
                                    if args.verbose {
                                        eprintln!("\nExpected error: '{}' got: '{}'", expected, err.message);
                                    }
                                }
                            } else {
                                failures += 1;
                                print!("{}", Style::new().red().apply_to("x"));
                                if let Some(off) = maybe_offset {
                                    crate::errors::report_error(&source_path.to_string_lossy(), &content, &err, Some(off));
                                } else {
                                    crate::errors::report_error(&source_path.to_string_lossy(), &content, &err, None);
                                }
                                if args.verbose {
                                    eprintln!("\nExecution error: {}", err.message);
                                }
                            }
                        }
                        Ok(maybe_row) => match maybe_row {
                            None => {
                                // no rows
                                if ep == "[no results]" {
                                    successes += 1;
                                    print!("{}", Style::new().green().apply_to("."));
                                } else {
                                    failures += 1;
                                    print!("{}", Style::new().red().apply_to("x"));
                                }
                            }
                            Some(row) => {
                                // single value expected
                                let v = row.get(0).unwrap();
                                let actual = value_to_string(&v);
                                if actual == ep {
                                    successes += 1;
                                    print!("{}", Style::new().green().apply_to("."));
                                } else {
                                    failures += 1;
                                    print!("{}", Style::new().red().apply_to("x"));
                                    // print a codespan diagnostic showing expected vs actual
                                    let ref_display = format!("{}", step.reference);
                                    if let Some((line, col)) = parse_line_col_from_ref(&ref_display) {
                                        report_mismatch(&source_path.to_string_lossy(), &content, line, col, &ep, &actual);
                                    } else if args.verbose {
                                        eprintln!("\nExpected: '{}' Got: '{}'", ep, actual);
                                    }
                                }
                            }
                        },
                    }
                }
            },
        }
        use std::io::Write as _;
        handle.flush()?;
    }

    println!();
    println!("{} successes", successes);
    println!("{} failures", failures);
    if !todos.is_empty() {
        println!("{} TODO(s):", todos.len());
        for (file, line, col, msg) in todos.iter() {
            println!(" - {}:{}:{} {}", file, line, col, msg);
        }
    }

    // exit non-zero if any failures or any TODOs
    if failures > 0 || !todos.is_empty() {
        if !todos.is_empty() {
            eprintln!("\nThere are {} TODO(s). Treating as failure per policy.", todos.len());
        }
        Err(anyhow::anyhow!("{} failures; {} todos", failures, todos.len()))
    } else {
        Ok(())
    }
}

fn parse_line_col_from_ref(ref_display: &str) -> Option<(usize, usize)> {
    // ref_display is "block:line:column". Use rsplitn to get last two parts.
    let mut parts: Vec<&str> = ref_display.rsplitn(3, ':').collect();
    if parts.len() < 2 {
        return None;
    }
    let col = parts[0].parse::<usize>().ok()?;
    let line = parts[1].parse::<usize>().ok()?;
    Some((line, col))
}

fn parse_ref_file_line_col(ref_display: &str) -> Option<(String, usize, usize)> {
    let mut parts: Vec<&str> = ref_display.splitn(3, ':').collect();
    if parts.len() < 3 {
        return None;
    }
    let file = parts[0].to_string();
    let line = parts[1].parse::<usize>().ok()?;
    let col = parts[2].parse::<usize>().ok()?;
    Some((file, line, col))
}

fn compute_offset_from_reference(content: &str, ref_display: &str) -> Option<usize> {
    if let Some((line, col)) = parse_line_col_from_ref(ref_display) {
        let lines: Vec<&str> = content.lines().collect();
        if line == 0 || line > lines.len() {
            return None;
        }
        let mut offset = 0usize;
        for i in 0..(line - 1) {
            offset += lines[i].as_bytes().len();
            offset += 1; // newline
        }
        let col0 = if col == 0 { 0 } else { col - 1 };
        offset += col0;
        Some(offset)
    } else {
        None
    }
}

fn report_mismatch(file_name: &str, content: &str, line: usize, _column: usize, expected: &str, actual: &str) {
    let mut files = SimpleFiles::new();
    let id = files.add(file_name.to_string(), content.to_string());
    let lines: Vec<&str> = content.lines().collect();
    let start = if line == 0 || line > lines.len() { 0 } else {
        let mut off = 0usize;
        for i in 0..(line - 1) {
            off += lines[i].as_bytes().len();
            off += 1;
        }
        off
    };
    let end = start + if line == 0 || line > lines.len() { 1 } else { lines[line - 1].as_bytes().len() };

    let diagnostic = Diagnostic::error()
        .with_message("Test assertion failed: expected vs actual mismatch")
        .with_labels(vec![Label::primary(id, start..end).with_message(format!("expected: {}\nactual: {}", expected, actual))]);

    let writer = StandardStream::stderr(ColorChoice::Auto);
    let config = term::Config::default();
    term::emit(&mut writer.lock(), &config, &files, &diagnostic).ok();
}


pub fn test(args: TestArgs) -> Result<(), ()> {
    match test_impl(args) {
        Ok(_) => Ok(()),
        Err(_) => Err(()),
    }
} 
 