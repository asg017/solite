//! Format SQL files command

use crate::cli::FmtArgs;
use solite_fmt::{check_formatted, format_document, FormatConfig, FormatError};
use std::fs;
use std::io::{self, Read};

pub fn fmt(args: FmtArgs) -> Result<(), ()> {
    match fmt_impl(args) {
        Ok(success) => {
            if success {
                Ok(())
            } else {
                Err(())
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            Err(())
        }
    }
}

fn fmt_impl(args: FmtArgs) -> Result<bool, anyhow::Error> {
    let config = match &args.config {
        Some(path) => FormatConfig::load(path).map_err(|e| anyhow::anyhow!("{}", e))?,
        None => FormatConfig::discover(),
    };

    if args.files.is_empty() {
        // Read from stdin
        let mut source = String::new();
        io::stdin().read_to_string(&mut source)?;
        let formatted = format_document(&source, &config).map_err(format_error_to_anyhow)?;
        print!("{formatted}");
        return Ok(true);
    }

    let mut all_ok = true;
    for path in &args.files {
        let source = fs::read_to_string(path)?;

        if args.check {
            match check_formatted(&source, &config) {
                Ok(is_formatted) => {
                    if !is_formatted {
                        println!("{}: not formatted", path.display());
                        all_ok = false;
                    }
                }
                Err(e) => {
                    eprintln!("{}: {}", path.display(), format_error_display(&e));
                    all_ok = false;
                }
            }
        } else {
            match format_document(&source, &config) {
                Ok(formatted) => {
                    if args.diff && source != formatted {
                        print_diff(path.display().to_string().as_str(), &source, &formatted);
                    }

                    if args.write {
                        if source != formatted {
                            fs::write(path, &formatted)?;
                            println!("Formatted {}", path.display());
                        }
                    } else if !args.diff {
                        print!("{formatted}");
                    }
                }
                Err(e) => {
                    eprintln!("{}: {}", path.display(), format_error_display(&e));
                    all_ok = false;
                }
            }
        }
    }

    Ok(all_ok)
}

fn format_error_to_anyhow(e: FormatError) -> anyhow::Error {
    anyhow::anyhow!("{}", format_error_display(&e))
}

fn format_error_display(e: &FormatError) -> String {
    match e {
        FormatError::ParseError(errors) => {
            let msgs: Vec<String> = errors.iter().map(|e| format!("{}", e)).collect();
            format!("Parse error: {}", msgs.join("; "))
        }
        FormatError::IoError(e) => format!("IO error: {}", e),
    }
}

fn print_diff(filename: &str, old: &str, new: &str) {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_lines(old, new);

    println!("--- {}", filename);
    println!("+++ {}", filename);

    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            println!("...");
        }
        for op in group {
            for change in diff.iter_inline_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };
                print!("{}{}", sign, change);
            }
        }
    }
}
