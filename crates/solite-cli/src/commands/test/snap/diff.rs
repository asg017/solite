//! Diff printing utilities for snapshot comparisons.

use similar::{Algorithm, ChangeTag, TextDiff};
use std::time::Duration;

/// Print a diff between two snapshot contents.
pub fn print_diff(original: &str, new: &str) {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .timeout(Duration::from_millis(500))
        .diff_lines(original, new);

    let width = console::Term::stdout().size().1 as usize;
    println!("────────────┬{:─^1$}", "", width.saturating_sub(13));

    for (idx, group) in diff.grouped_ops(4).iter().enumerate() {
        if idx > 0 {
            println!("┈┈┈┈┈┈┈┈┈┈┈┈┼{:┈^1$}", "", width.saturating_sub(13));
        }
        for op in group {
            for change in diff.iter_inline_changes(op) {
                match change.tag() {
                    ChangeTag::Insert => {
                        let line_num = change
                            .new_index()
                            .map(|i| i.to_string())
                            .unwrap_or_default();
                        print!(
                            "{:>5} {:>5} │{}",
                            "",
                            console::style(line_num).cyan().bold().dim(),
                            console::style("+").green(),
                        );
                        for &(emphasized, text) in change.values() {
                            if emphasized {
                                print!("{}", console::style(text).green().underlined());
                            } else {
                                print!("{}", console::style(text).green());
                            }
                        }
                    }
                    ChangeTag::Delete => {
                        let line_num = change
                            .old_index()
                            .map(|i| i.to_string())
                            .unwrap_or_default();
                        print!(
                            "{:>5} {:>5} │{}",
                            console::style(line_num).cyan().dim(),
                            "",
                            console::style("-").red(),
                        );
                        for &(emphasized, text) in change.values() {
                            if emphasized {
                                print!("{}", console::style(text).red().underlined());
                            } else {
                                print!("{}", console::style(text).red());
                            }
                        }
                    }
                    ChangeTag::Equal => {
                        let old_num = change
                            .old_index()
                            .map(|i| i.to_string())
                            .unwrap_or_default();
                        let new_num = change
                            .new_index()
                            .map(|i| i.to_string())
                            .unwrap_or_default();
                        print!(
                            "{:>5} {:>5} │ ",
                            console::style(&old_num).cyan().dim(),
                            console::style(&new_num).cyan().dim().bold(),
                        );
                        for &(_, text) in change.values() {
                            print!("{}", console::style(text).dim());
                        }
                    }
                }
            }
        }
    }
    println!("────────────┴{:─^1$}", "", width.saturating_sub(13));
}

/// Print the decision prompt for accepting/rejecting a snapshot.
pub fn print_decision() {
    println!(
        "  {} accept     {}",
        console::style("a").green().bold(),
        console::style("keep the new snapshot").dim()
    );
    println!(
        "  {} reject     {}",
        console::style("r").red().bold(),
        console::style("reject the new snapshot").dim()
    );
}
