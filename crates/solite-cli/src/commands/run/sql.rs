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
pub fn handle_sql(
    runtime: &mut Runtime,
    stmt: &Statement,
    step_reference: &str,
    is_trace: bool,
    timer: bool,
) {
    let pb = create_progress_bar();

    // Set up tracing if enabled
    let trace_stmt_id = if is_trace {
        setup_trace_statement(runtime, stmt)
    } else {
        None
    };

    let start = jiff::Timestamp::now();
    let p = stmt.pointer();
    let r = step_reference.to_string();
    let pbx = pb.clone();

    // Set up progress handler
    runtime.connection.set_progress_handler(
        500_000,
        Some(move |(stmt_ptr, start_time): &(*mut solite_core::sqlite::sqlite3_stmt, Timestamp)| {
            handle_progress(*stmt_ptr, *start_time, &pbx, &r)
        }),
        (p, start),
    );

    let execution_start = std::time::Instant::now();
    pb.finish_and_clear();

    // Display results as table
    let config = TableConfig::terminal();
    match solite_table::print_statement(stmt, &config) {
        Ok(_) => {}
        Err(err) => {
            eprintln!("Error: {}", err);
        }
    }

    // Print status message with timing
    if timer {
        print_completion_status(stmt, step_reference, execution_start.elapsed());
    }

    // Record trace data if enabled
    if let Some(trace_id) = trace_stmt_id {
        record_trace_steps(runtime, stmt, trace_id);
    }
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
    let insert_stmt = match runtime
        .connection
        .prepare("INSERT INTO solite_trace.statements (sql) VALUES (?) RETURNING id")
    {
        Ok((_, Some(s))) => s,
        _ => {
            eprintln!("Warning: Failed to prepare trace statement");
            return None;
        }
    };

    insert_stmt.bind_text(1, stmt.sql());

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
    let elapsed = start - Timestamp::now();
    let should_update = elapsed.compare(42.milliseconds()).unwrap_or(std::cmp::Ordering::Less).is_gt();

    if !should_update {
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

    trace_stmt.bind_int64(1, trace_stmt_id);
    unsafe { trace_stmt.bind_pointer(2, stmt.pointer().cast(), c"stmt-pointer") };

    if let Err(e) = trace_stmt.execute() {
        eprintln!("Warning: Failed to record trace steps: {:?}", e);
    }
}
