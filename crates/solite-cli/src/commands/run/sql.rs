//! SQL statement execution and progress tracking.

use std::io::stdout;
use std::time::Duration;

use crossterm::{
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
};
use jiff::fmt::friendly::{FractionalUnit, SpanPrinter};
use jiff::{Timestamp, ToSpan};
use solite_core::sqlite::Statement;
use solite_core::Runtime;
use solite_table::TableConfig;

use super::format::format_duration;
use super::status::get_statement_status;

/// Execute a SQL statement with progress tracking and output.
///
/// Returns `false` when execution failed (the error is reported here).
pub fn handle_sql(
    runtime: &mut Runtime,
    stmt: &mut Statement,
    step_reference: &str,
    is_trace: bool,
    timer: bool,
) -> bool {
    let pb = create_progress_bar();

    // Set up tracing if enabled
    let trace_stmt_id = if is_trace {
        setup_trace_statement(runtime, stmt)
    } else {
        None
    };

    let start = jiff::Timestamp::now();
    // Captured as usize so the closure is Send; the handler is cleared below
    // before the statement is dropped.
    let p = stmt.pointer() as usize;
    let r = step_reference.to_string();
    let pbx = pb.clone();

    // Set up progress handler
    runtime.connection.set_progress_handler(500_000, move || {
        handle_progress(p as *mut solite_core::sqlite::sqlite3_stmt, start, &pbx, &r)
    });

    let execution_start = std::time::Instant::now();

    // Display results as table
    let config = TableConfig::terminal();
    let success = match solite_table::print_statement(stmt, &config) {
        Ok(_) => true,
        Err(err) => {
            eprintln!("Error: {}", err);
            false
        }
    };

    // The handler captures this statement's raw pointer; unregister as soon
    // as execution finishes, before the statement can be dropped and before
    // any further SQL (trace recording below) could trip the opcode
    // threshold. Only then clear the spinner so no late callback redraws it.
    runtime.connection.clear_progress_handler();
    pb.finish_and_clear();

    // Print status message with timing
    if timer {
        print_completion_status(stmt, step_reference, execution_start.elapsed());
    }

    // Record trace data if enabled
    if let Some(trace_id) = trace_stmt_id {
        record_trace_steps(runtime, stmt, trace_id);
    }

    success
}

/// Create a styled progress bar.
fn create_progress_bar() -> indicatif::ProgressBar {
    let pb = indicatif::ProgressBar::new_spinner();
    if let Ok(style) = indicatif::ProgressStyle::with_template("{spinner:.cyan} {elapsed} {wide_msg}")
    {
        pb.set_style(style.tick_chars("⣾⣷⣯⣟⡿⢿⣻⣽"));
    }
    pb
}

/// Set up trace statement and return its ID.
fn setup_trace_statement(runtime: &Runtime, stmt: &Statement) -> Option<i64> {
    let mut insert_stmt = match runtime
        .connection
        .prepare("INSERT INTO solite_trace.statements (sql) VALUES (?) RETURNING id")
    {
        Ok((_, Some(s))) => s,
        _ => {
            eprintln!("Warning: Failed to prepare trace statement");
            return None;
        }
    };

    if let Err(e) = insert_stmt.bind_text(1, stmt.sql()) {
        eprintln!("Warning: Failed to bind trace statement: {:?}", e);
        return None;
    }

    match insert_stmt.nextx() {
        Ok(Some(row)) => Some(row.value_at(0).as_int64()),
        _ => {
            eprintln!("Warning: Failed to insert trace statement");
            None
        }
    }
}

/// Handle progress callback.
fn handle_progress(
    stmt: *mut solite_core::sqlite::sqlite3_stmt,
    start: Timestamp,
    pb: &indicatif::ProgressBar,
    reference: &str,
) -> bool {
    // Only update progress if more than 42ms has elapsed
    if !should_render_progress(start, Timestamp::now()) {
        return false;
    }

    let status = get_statement_status(stmt);
    let msg = status.progress_message();

    let duration = Timestamp::now() - start;
    let printer = SpanPrinter::new()
        .hours_minutes_seconds(true)
        .fractional(Some(FractionalUnit::Second));
    let mut buf = String::new();
    let _ = printer.print_span(&duration, &mut buf);

    let ms = duration.total(jiff::Unit::Millisecond).unwrap_or(0.0) as u64;
    pb.clone()
        .with_elapsed(Duration::from_millis(ms))
        .with_message(format!("{} {}", reference, msg))
        .tick();

    false
}

/// True once enough time has elapsed since statement start for the spinner
/// to be worth drawing; keeps fast statements from flickering a spinner.
fn should_render_progress(start: Timestamp, now: Timestamp) -> bool {
    (now - start)
        .compare(42.milliseconds())
        .unwrap_or(std::cmp::Ordering::Less)
        .is_gt()
}

/// Print completion status with timing.
fn print_completion_status(stmt: &Statement, reference: &str, elapsed: Duration) {
    let status = get_statement_status(stmt.pointer());
    let msg = status.completion_message();

    let _ = execute!(
        stdout(),
        SetForegroundColor(Color::Green),
        Print("✓ "),
        SetForegroundColor(Color::Grey),
        Print(format!("{} ", reference)),
        SetForegroundColor(Color::White),
        Print(msg),
        Print(format!("in {}", format_duration(elapsed))),
        ResetColor,
        Print("\n")
    );

    for line in status.trigger_effect_lines() {
        let _ = execute!(
            stdout(),
            SetForegroundColor(Color::DarkGrey),
            Print(format!("  ↳ {}\n", line)),
            ResetColor,
        );
    }
}

/// Record trace steps to the trace database.
fn record_trace_steps(runtime: &Runtime, stmt: &Statement, trace_stmt_id: i64) {
    let trace_stmt = match runtime.connection.prepare(
        r#"INSERT INTO solite_trace.steps
           (statement_id, addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle)
           SELECT ?, addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle
           FROM bytecode(?)"#,
    ) {
        Ok((_, Some(s))) => s,
        _ => {
            eprintln!("Warning: Failed to prepare trace step insertion");
            return;
        }
    };

    let bound = trace_stmt
        .bind_int64(1, trace_stmt_id)
        .and_then(|_| unsafe { trace_stmt.bind_pointer(2, stmt.pointer().cast(), c"stmt-pointer") });
    if let Err(e) = bound {
        eprintln!("Warning: Failed to bind trace step: {:?}", e);
        return;
    }

    if let Err(e) = trace_stmt.execute() {
        eprintln!("Warning: Failed to record trace steps: {:?}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::should_render_progress;
    use jiff::{Timestamp, ToSpan};

    #[test]
    fn renders_after_threshold() {
        let start = Timestamp::now();
        assert!(should_render_progress(start, start + 100.milliseconds()));
    }

    #[test]
    fn quiet_before_threshold() {
        let start = Timestamp::now();
        assert!(!should_render_progress(start, start + 10.milliseconds()));
    }

    #[test]
    fn quiet_at_exact_threshold() {
        let start = Timestamp::now();
        assert!(!should_render_progress(start, start + 42.milliseconds()));
    }

    #[test]
    fn quiet_when_clock_goes_backwards() {
        // Regression: the original code computed `start - now`, which made
        // the elapsed span negative and the spinner never render.
        let start = Timestamp::now();
        assert!(!should_render_progress(start, start - 100.milliseconds()));
    }
}
