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

pub mod stats;

use crate::sqlite::{bytecode_steps, BytecodeStep, Statement};
use crate::{ParseDotError, Runtime};
use jiff::Span;
use serde::Serialize;
use stats::{average, format_runtime, max, min, stddev};

/// Default number of timed iterations per benchmark.
pub const DEFAULT_ITERATIONS: usize = 10;

/// Command to benchmark a SQL query.
#[derive(Serialize, Debug)]
pub struct BenchCommand {
    /// Optional name for the benchmark.
    pub name: Option<String>,
    /// Number of timed iterations (default [`DEFAULT_ITERATIONS`], min 1).
    pub iterations: usize,
    /// Number of untimed warmup executions before measurement (default 0).
    pub warmup: usize,
    /// Prepared statement to benchmark.
    pub statement: Statement,
    /// Length consumed from rest input.
    pub rest_length: usize,
}

/// Result of a benchmark run.
pub struct BenchResult {
    /// Optional benchmark name.
    pub name: Option<String>,
    /// Execution times for each iteration.
    pub times: Vec<Span>,
    /// Number of iterations run.
    pub niter: usize,
    /// Rendered bytecode-steps table (see [`render_steps`]); the timing
    /// summary comes from [`BenchResult::report`].
    pub steps_report: String,
}

impl BenchResult {
    /// Calculate the average execution time.
    ///
    /// Returns None when no iterations were recorded.
    pub fn average(&self) -> Option<Span> {
        average(&self.times)
    }

    /// Generate a human-readable report.
    pub fn report(&self) -> String {
        let fmt = |span: Option<Span>| {
            span.map(format_runtime)
                .unwrap_or_else(|| "N/A".to_string())
        };
        let avg = fmt(self.average());
        let std = fmt(stddev(&self.times));
        let mn = fmt(min(&self.times));
        let mx = fmt(max(&self.times));
        let niter = self.niter;

        format!(
            "{}\n  Time  (mean ± σ):   {} ± {} ({} iterations)\n  Range (min … max):  {} … {}",
            match &self.name {
                Some(name) => format!("Benchmark: {}", name),
                None => "Benchmark".to_string(),
            },
            avg,
            std,
            niter,
            mn,
            mx,
        )
    }
}

/// Parse a `.bench` count flag value, surfacing the offending value in the
/// error message (pico-args' default parse error omits it).
fn bench_count(value: &str) -> Result<usize, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value '{value}'"))
}

impl BenchCommand {
    /// Create a new bench command from arguments.
    ///
    /// # Arguments
    ///
    /// * `args` - Same-line text after `.bench`: optional flags like
    ///   `--name "My Benchmark"`
    /// * `runtime` - The runtime context
    /// * `rest` - The SQL query to benchmark (the lines following the dot
    ///   line)
    ///
    /// # Errors
    ///
    /// Returns `ParseDotError` if a flag is unknown or malformed, or if the
    /// SQL cannot be prepared.
    pub fn new(args: String, runtime: &mut Runtime, rest: &str) -> Result<Self, ParseDotError> {
        // Tokenize the dot-line arguments shell-style so quoted values like
        // `--name "My Query"` survive as one argument — a plain `split(' ')`
        // would break them apart. The SQL itself is never tokenized (it stays
        // in `rest`), so string literals are left untouched.
        let tokens = shlex::split(&args).ok_or_else(|| {
            ParseDotError::InvalidArgument("malformed quoting in .bench arguments".into())
        })?;
        let mut pargs = pico_args::Arguments::from_vec(
            tokens.into_iter().map(std::ffi::OsString::from).collect(),
        );

        let name: Option<String> = pargs
            .opt_value_from_str("--name")
            .map_err(|e| ParseDotError::InvalidArgument(e.to_string()))?;
        let iterations: Option<usize> = pargs
            .opt_value_from_fn("--iterations", bench_count)
            .map_err(|e| ParseDotError::InvalidArgument(e.to_string()))?;
        let warmup: Option<usize> = pargs
            .opt_value_from_fn("--warmup", bench_count)
            .map_err(|e| ParseDotError::InvalidArgument(e.to_string()))?;

        // Anything left over is an unknown flag or stray argument — reject it
        // rather than silently ignore. SQL goes on the line after `.bench`.
        if let Some(unexpected) = pargs.finish().into_iter().next() {
            return Err(ParseDotError::InvalidArgument(format!(
                "unexpected .bench argument '{}' (put SQL on the line after .bench)",
                unexpected.to_string_lossy()
            )));
        }

        let iterations = iterations.unwrap_or(DEFAULT_ITERATIONS);
        if iterations == 0 {
            return Err(ParseDotError::InvalidArgument(
                "--iterations must be at least 1".to_string(),
            ));
        }
        let warmup = warmup.unwrap_or(0);

        let (rest_len, stmt) = runtime
            .prepare_with_parameters(rest)
            .map_err(|e| ParseDotError::Generic(format!("Failed to prepare query: {}", e)))?;

        let stmt = stmt.ok_or_else(|| ParseDotError::Generic("No SQL statement provided".into()))?;

        Ok(Self {
            name,
            iterations,
            warmup,
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
        // Untimed warmup executions: absorb cold-cache costs (page cache,
        // statement first run) before measurement begins.
        for _ in 0..self.warmup {
            self.statement
                .execute()
                .map_err(|e| anyhow::anyhow!("Query execution failed: {}", e))?;
            self.statement.reset();
        }

        let mut times = Vec::with_capacity(self.iterations);

        for _ in 0..self.iterations {
            let start = jiff::Timestamp::now();

            self.statement
                .execute()
                .map_err(|e| anyhow::anyhow!("Query execution failed: {}", e))?;

            // Stop the clock before resetting, mirroring the CLI bench loop —
            // reset overhead would otherwise inflate every sample.
            let elapsed = jiff::Timestamp::now() - start;
            self.statement.reset();
            times.push(elapsed);

            if let Some(ref cb) = callback {
                cb(elapsed);
            }
        }

        // Fetch and render the bytecode trace once, after all iterations.
        // Per-opcode ncycle counters accumulate across executions (only
        // sqlite3_stmt_scanstatus_reset clears them), so the rendered cycle
        // counts are totals over all iterations; the percentages are
        // unaffected.
        let steps = unsafe { bytecode_steps(self.statement.pointer()) }?;
        let steps_report = render_steps(steps);

        Ok(BenchResult {
            name: self.name.clone(),
            times,
            niter: self.iterations,
            steps_report,
        })
    }
}

/// Render bytecode execution steps as a formatted report.
///
/// `cycles=` values come straight from the bytecode table's per-opcode
/// `ncycle` counters, which accumulate over every execution of the
/// statement — when sampled after a benchmark loop they are totals across
/// all iterations, not a single run.
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
        ) && p2op < n_indent
        {
            for item in &mut ai_indent[p2op..i_op] {
                *item += 2;
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
                for item in &mut ai_indent[p2op..i_op] {
                    *item += 2;
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
    fn test_bench_result_report() {
        let result = BenchResult {
            name: Some("Test Query".to_string()),
            times: vec![
                Span::new().milliseconds(10),
                Span::new().milliseconds(12),
                Span::new().milliseconds(11),
            ],
            niter: 3,
            steps_report: String::new(),
        };

        let report = result.report();
        assert!(report.contains("Benchmark: Test Query"));
        assert!(report.contains("3 iterations"));
        assert!(report.contains("mean ± σ"));
        assert!(report.contains("min … max"));
    }

    #[test]
    fn test_bench_result_report_single_iteration_has_na_stddev() {
        // n=1: sample stddev is undefined — report N/A, never panic
        let result = BenchResult {
            name: None,
            times: vec![Span::new().milliseconds(10)],
            niter: 1,
            steps_report: String::new(),
        };

        let report = result.report();
        assert!(report.contains("± N/A"), "got: {report}");
        assert!(report.contains("1 iterations"));
    }

    #[test]
    fn test_render_steps_empty() {
        let steps: Vec<BytecodeStep> = vec![];
        let output = render_steps(steps);
        assert!(output.is_empty());
    }

    #[test]
    fn test_new_sql_on_following_line() {
        // The standard form: flags (none here) on the `.bench` line, SQL on
        // the following line(s).
        let mut rt = Runtime::new(None).unwrap();
        let rest = "\nSELECT 1;";
        let cmd = BenchCommand::new(String::new(), &mut rt, rest).unwrap();
        assert_eq!(cmd.name, None);
        assert_eq!(cmd.statement.sql().trim(), "SELECT 1;");
        // Single statement: the whole `rest` is consumed.
        assert_eq!(cmd.rest_length, rest.len());
    }

    #[test]
    fn test_new_multiline_sql_consumes_only_first_statement() {
        let mut rt = Runtime::new(None).unwrap();
        let rest = "\nSELECT 1;\nSELECT 2;";
        let cmd = BenchCommand::new(String::new(), &mut rt, rest).unwrap();
        assert_eq!(cmd.statement.sql().trim(), "SELECT 1;");
        // Consumed through the end of `SELECT 1;` — `SELECT 2;` remains.
        assert_eq!(cmd.rest_length, "\nSELECT 1;".len());
        assert_eq!(&rest[cmd.rest_length..], "\nSELECT 2;");
    }

    #[test]
    fn test_new_quoted_name_with_spaces() {
        let mut rt = Runtime::new(None).unwrap();
        let cmd =
            BenchCommand::new("--name \"My Query\"".to_string(), &mut rt, "\nSELECT 1;").unwrap();
        assert_eq!(cmd.name, Some("My Query".to_string()));
        assert_eq!(cmd.statement.sql().trim(), "SELECT 1;");
    }

    #[test]
    fn test_new_name_eq_form() {
        let mut rt = Runtime::new(None).unwrap();
        let cmd =
            BenchCommand::new("--name='My Query'".to_string(), &mut rt, "\nSELECT 1;").unwrap();
        assert_eq!(cmd.name, Some("My Query".to_string()));
        assert_eq!(cmd.statement.sql().trim(), "SELECT 1;");
    }

    #[test]
    fn test_new_unknown_flag_is_an_error() {
        let mut rt = Runtime::new(None).unwrap();
        let err = BenchCommand::new("--nmae foo".to_string(), &mut rt, "\nSELECT 1;").unwrap_err();
        assert!(err.to_string().contains("--nmae"), "got: {err}");
    }

    #[test]
    fn test_new_malformed_quote_is_an_error() {
        let mut rt = Runtime::new(None).unwrap();
        let err =
            BenchCommand::new("--name \"My Query".to_string(), &mut rt, "\nSELECT 1;").unwrap_err();
        assert!(err.to_string().contains("malformed quoting"), "got: {err}");
    }

    #[test]
    fn test_new_iterations_and_warmup_flags() {
        let mut rt = Runtime::new(None).unwrap();
        let cmd = BenchCommand::new(
            "--iterations 3 --warmup 2".to_string(),
            &mut rt,
            "\nSELECT 1;",
        )
        .unwrap();
        assert_eq!(cmd.iterations, 3);
        assert_eq!(cmd.warmup, 2);
    }

    #[test]
    fn test_new_default_iterations() {
        let mut rt = Runtime::new(None).unwrap();
        let cmd = BenchCommand::new(String::new(), &mut rt, "\nSELECT 1;").unwrap();
        assert_eq!(cmd.iterations, DEFAULT_ITERATIONS);
        assert_eq!(cmd.warmup, 0);
    }

    #[test]
    fn test_new_zero_iterations_is_an_error() {
        let mut rt = Runtime::new(None).unwrap();
        let err =
            BenchCommand::new("--iterations 0".to_string(), &mut rt, "\nSELECT 1;").unwrap_err();
        assert!(err.to_string().contains("at least 1"), "got: {err}");
    }

    #[test]
    fn test_new_non_numeric_iterations_is_an_error() {
        let mut rt = Runtime::new(None).unwrap();
        let err =
            BenchCommand::new("--iterations lots".to_string(), &mut rt, "\nSELECT 1;").unwrap_err();
        assert!(err.to_string().contains("lots"), "got: {err}");
    }

    #[test]
    fn test_single_iteration_executes_without_panicking() {
        // n=1: stddev is undefined — must report N/A, not panic
        let mut rt = Runtime::new(None).unwrap();
        let mut cmd =
            BenchCommand::new("--iterations 1".to_string(), &mut rt, "\nSELECT 1;").unwrap();
        let result = cmd.execute(None).unwrap();
        assert_eq!(result.times.len(), 1);
        assert!(result.report().contains("± N/A"));
    }

    #[test]
    fn test_new_no_sql_is_an_error() {
        let mut rt = Runtime::new(None).unwrap();
        let err = BenchCommand::new(String::new(), &mut rt, "").unwrap_err();
        assert!(err.to_string().contains("No SQL statement"), "got: {err}");
    }
}
