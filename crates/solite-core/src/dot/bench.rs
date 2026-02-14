//! Query benchmarking command.
//!
//! This module implements the `.bench` command which benchmarks SQL query
//! execution, running the query multiple times and reporting statistics.
//!
//! # Usage
//!
//! ```sql
//! .bench SELECT * FROM large_table
//! .bench --name "My Query" SELECT complex_query...
//! ```
//!
//! # Output
//!
//! Reports mean execution time, standard deviation, and min/max values
//! across 10 iterations, along with detailed bytecode execution statistics.

use crate::sqlite::{bytecode_steps, BytecodeStep, Statement};
use crate::{ParseDotError, Runtime};
use jiff::{Span, SpanRound, Unit};
use serde::Serialize;
use std::cmp::Ordering;
use std::ffi::OsString;

/// Command to benchmark a SQL query.
#[derive(Serialize, Debug)]
pub struct BenchCommand {
    /// Optional name for the benchmark.
    pub name: Option<String>,
    /// Optional suite name.
    pub suite: Option<String>,
    /// Prepared statement to benchmark.
    pub statement: Statement,
    /// Length consumed from rest input.
    pub rest_length: usize,
}

/// Result of a benchmark run.
pub struct BenchResult {
    /// Optional benchmark name.
    pub name: Option<String>,
    /// Optional suite name.
    pub suite: Option<String>,
    /// Execution times for each iteration.
    pub times: Vec<Span>,
    /// Number of iterations run.
    pub niter: usize,
    /// Detailed execution report.
    pub report: String,
}

impl BenchResult {
    /// Calculate the average execution time.
    pub fn average(&self) -> Span {
        average(&self.times)
    }

    /// Generate a human-readable report.
    pub fn report(&self) -> String {
        let avg = self.average();
        let std = stddev(&self.times);
        let mn = min(&self.times);
        let mx = max(&self.times);
        let niter = self.niter;

        format!(
            "{}\n  Time  (mean +/- s):   {} +/- {} ({} iterations)\n  Range (min ... max):  {} ... {}",
            match &self.name {
                Some(name) => format!("Benchmark: {}", name),
                None => "Benchmark".to_string(),
            },
            format_runtime(avg),
            format_runtime(std),
            niter,
            format_runtime(mn),
            format_runtime(mx),
        )
    }
}

/// Calculate the average of a slice of spans.
fn average(times: &[Span]) -> Span {
    if times.is_empty() {
        return Span::new();
    }

    let micros: Vec<f64> = times
        .iter()
        .filter_map(|span| span.total(Unit::Microsecond).ok())
        .collect();

    if micros.is_empty() {
        return Span::new();
    }

    Span::new().microseconds(statistical::mean(&micros) as i64)
}

/// Calculate the standard deviation of a slice of spans.
fn stddev(times: &[Span]) -> Span {
    if times.is_empty() {
        return Span::new();
    }

    let micros: Vec<f64> = times
        .iter()
        .filter_map(|span| span.total(Unit::Microsecond).ok())
        .collect();

    if micros.is_empty() {
        return Span::new();
    }

    let mean = statistical::mean(&micros);
    let std = statistical::standard_deviation(&micros, Some(mean));

    Span::new().microseconds(std as i64)
}

/// Find the minimum span.
fn min(times: &[Span]) -> Span {
    times
        .iter()
        .min_by(|a, b| compare_spans(a, b))
        .cloned()
        .unwrap_or_else(Span::new)
}

/// Find the maximum span.
fn max(times: &[Span]) -> Span {
    times
        .iter()
        .max_by(|a, b| compare_spans(a, b))
        .cloned()
        .unwrap_or_else(Span::new)
}

/// Compare two spans for ordering.
fn compare_spans(a: &Span, b: &Span) -> Ordering {
    a.compare(*b).unwrap_or(Ordering::Equal)
}

/// Format a span as a human-readable runtime string.
fn format_runtime(span: Span) -> String {
    let threshold = Span::new().milliseconds(50);
    let is_small = span.compare(threshold).map_or(true, |ord| ord.is_lt());

    if is_small {
        let total = span.total(Unit::Millisecond).unwrap_or(0.0);
        format!("{:.3}ms", total)
    } else {
        let rounded = span.round(
            SpanRound::new()
                .largest(Unit::Minute)
                .smallest(Unit::Millisecond),
        );
        match rounded {
            Ok(r) => format!("{:?}", r),
            Err(_) => format!("{:?}", span),
        }
    }
}

impl BenchCommand {
    /// Create a new bench command from arguments.
    ///
    /// # Arguments
    ///
    /// * `args` - Optional arguments like `--name "My Benchmark"`
    /// * `runtime` - The runtime context
    /// * `rest` - The SQL query to benchmark
    ///
    /// # Errors
    ///
    /// Returns `ParseDotError` if the SQL cannot be prepared.
    pub fn new(args: String, runtime: &mut Runtime, rest: &str) -> Result<Self, ParseDotError> {
        let pargs_vec: Vec<OsString> = args.split(' ').map(OsString::from).collect();
        let mut pargs = pico_args::Arguments::from_vec(pargs_vec);

        let name: Option<String> = pargs
            .opt_value_from_str("--name")
            .map_err(|e| ParseDotError::InvalidArgument(e.to_string()))?;

        let (rest_len, stmt) = runtime
            .prepare_with_parameters(rest)
            .map_err(|e| ParseDotError::Generic(format!("Failed to prepare query: {}", e)))?;

        let stmt = stmt.ok_or_else(|| ParseDotError::Generic("No SQL statement provided".into()))?;

        Ok(Self {
            name,
            suite: None,
            statement: stmt,
            rest_length: rest_len.unwrap_or(rest.len()),
        })
    }

    /// Execute the benchmark.
    ///
    /// # Arguments
    ///
    /// * `callback` - Optional callback invoked after each iteration
    ///
    /// # Returns
    ///
    /// Benchmark results including timing statistics and execution report.
    pub fn execute(
        &mut self,
        callback: Option<Box<dyn Fn(Span)>>,
    ) -> anyhow::Result<BenchResult> {
        let mut times = Vec::new();
        let mut niter = 0;
        let mut report = String::new();

        for _ in 0..10 {
            niter += 1;
            let start = jiff::Timestamp::now();

            self.statement
                .execute()
                .map_err(|e| anyhow::anyhow!("Query execution failed: {}", e))?;
            self.statement.reset();

            let elapsed = jiff::Timestamp::now() - start;
            times.push(elapsed);

            if let Some(ref cb) = callback {
                cb(elapsed);
            }

            let steps = unsafe { bytecode_steps(self.statement.pointer()) };
            report = render_steps(steps);
        }

        Ok(BenchResult {
            name: self.name.clone(),
            suite: self.suite.clone(),
            times,
            niter,
            report,
        })
    }
}

/// Render bytecode execution steps as a formatted report.
pub fn render_steps(steps: Vec<BytecodeStep>) -> String {
    let mut output = String::new();

    if steps.is_empty() {
        return output;
    }

    let term_width = term_size::dimensions().map(|(w, _)| w).unwrap_or(120);

    // Compute indentation array
    let n_indent = steps.len();
    let mut ai_indent = vec![0i32; n_indent];

    for (i_op, step) in steps.iter().enumerate() {
        let opcode = step.opcode.as_str();
        let i_addr = step.addr;
        let p2 = step.p2;
        let p1 = step.p1;

        let p2op = (p2 + (i_op as i64 - i_addr)) as usize;

        // Next/Prev family opcodes
        if matches!(
            opcode,
            "Next" | "Prev" | "VNext" | "VPrev" | "SorterNext" | "NextIfOpen" | "PrevIfOpen"
        ) {
            if p2op < n_indent {
                for i in p2op..i_op {
                    ai_indent[i] += 2;
                }
            }
        }

        // Goto (backward jumps)
        if opcode == "Goto" && p2op < n_indent {
            let target_opcode = steps[p2op].opcode.as_str();
            let is_loop_target = matches!(
                target_opcode,
                "Yield" | "SeekLT" | "SeekGT" | "RowSetRead" | "Rewind"
            );

            if is_loop_target || p1 != 0 {
                for i in p2op..i_op {
                    ai_indent[i] += 2;
                }
            }
        }
    }

    let total_cycles: i64 = steps.iter().map(|s| s.ncycle).sum();

    output.push_str(&format!("QUERY PLAN (cycles={} [100%])\n", total_cycles));
    output.push_str("addr  opcode         p1    p2    p3    p4             p5  comment\n");
    output.push_str("----  -------------  ----  ----  ----  -------------  --  -------\n");

    for (i, step) in steps.iter().enumerate() {
        let indent = ai_indent[i];

        let cycle_pct = if total_cycles > 0 {
            ((step.ncycle as f64 / total_cycles as f64) * 100.0).round() as i64
        } else {
            0
        };

        let base_line = format!(
            "{:<4}  {:indent$}{:<13}  {:<4}  {:<4}  {:<4}  {:<13}  {:<2}  {}",
            step.addr,
            "",
            step.opcode,
            step.p1,
            step.p2,
            step.p3,
            step.p4,
            step.p5,
            step.comment,
            indent = indent as usize
        );

        if step.ncycle > 0 {
            let cycle_info = format!("(cycles={} [{}%])", step.ncycle, cycle_pct);
            let content_len = base_line.len();
            let cycle_len = cycle_info.len();
            let total_needed = content_len + 1 + cycle_len;

            let padding = if term_width > total_needed {
                term_width - total_needed
            } else {
                1
            };

            output.push_str(&format!(
                "{}{:>pad$}{}\n",
                base_line,
                "",
                cycle_info,
                pad = padding + 1
            ));
        } else {
            output.push_str(&format!("{}\n", base_line));
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_average_empty() {
        let times: Vec<Span> = vec![];
        let avg = average(&times);
        // Empty times should produce an empty span
        let total = avg.total(Unit::Microsecond).unwrap_or(0.0);
        assert_eq!(total, 0.0);
    }

    #[test]
    fn test_average_single() {
        let times = vec![Span::new().milliseconds(100)];
        let avg = average(&times);
        // Should be approximately 100ms
        let total = avg.total(Unit::Millisecond).unwrap();
        assert!((total - 100.0).abs() < 0.1);
    }

    #[test]
    fn test_format_runtime_small() {
        let span = Span::new().milliseconds(10);
        let formatted = format_runtime(span);
        assert!(formatted.contains("ms"));
    }

    #[test]
    fn test_format_runtime_large() {
        let span = Span::new().seconds(5);
        let formatted = format_runtime(span);
        // Should format as seconds, not milliseconds
        assert!(formatted.contains("s") || formatted.contains("second"));
    }

    #[test]
    fn test_bench_result_report() {
        let result = BenchResult {
            name: Some("Test Query".to_string()),
            suite: None,
            times: vec![
                Span::new().milliseconds(10),
                Span::new().milliseconds(12),
                Span::new().milliseconds(11),
            ],
            niter: 3,
            report: String::new(),
        };

        let report = result.report();
        assert!(report.contains("Benchmark: Test Query"));
        assert!(report.contains("3 iterations"));
    }

    #[test]
    fn test_render_steps_empty() {
        let steps: Vec<BytecodeStep> = vec![];
        let output = render_steps(steps);
        assert!(output.is_empty());
    }
}
