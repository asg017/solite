use std::path::Path;

use crossterm::style::Stylize;
use indicatif::ProgressBar;
use jiff::{Span, SpanRound, Unit};
use solite_core::{sqlite::bytecode_steps, Runtime};

use crate::cli::BenchArgs;

fn average(times: &[jiff::Span]) -> jiff::Span {
    let times: Vec<f64> = times
        .iter()
        .map(|span| span.total(jiff::Unit::Microsecond).unwrap())
        .collect();
    jiff::Span::new().microseconds(statistical::mean(&times) as i64)
}
fn stddev(times: &[jiff::Span]) -> jiff::Span {
    let times: Vec<f64> = times
        .iter()
        .map(|span| span.total(jiff::Unit::Microsecond).unwrap())
        .collect();
    jiff::Span::new()
        .microseconds(
            statistical::standard_deviation(&times, Some(statistical::mean(&times))) as i64,
        )
}

fn min(times: &[jiff::Span]) -> jiff::Span {
    times
        .iter()
        .min_by(|a, b| a.compare(*b).unwrap())
        .unwrap()
        .clone()
}
fn max(times: &[jiff::Span]) -> jiff::Span {
    times
        .iter()
        .max_by(|a, b| a.compare(*b).unwrap())
        .unwrap()
        .clone()
}

fn format_runtime(span: jiff::Span) -> String {
    if span.compare(Span::new().milliseconds(50)).unwrap().is_lt() {
        let total = span.total(Unit::Millisecond).unwrap();
        format!("{total}ms")
    } else {
        let rounded = span
            .round(
                SpanRound::new()
                    .largest(Unit::Minute)
                    .smallest(Unit::Millisecond),
            )
            .unwrap();
        format!("{rounded:?}")
    }
}

pub fn bench(args: BenchArgs) -> std::result::Result<(), ()> {
    let runtime = Runtime::new(None);
    /*let connection = match path {
        Some(path) => Connection::open(path.as_str()).unwrap(),
        None => Connection::open_in_memory().unwrap(),
    };
    unsafe {
        solite_stdlib_init(connection.db(), std::ptr::null_mut(), std::ptr::null_mut());
    }*/

    if let Some(extensions) = args.load_extension {
        for extension in extensions {
            runtime
                .connection
                .load_extension(&extension.as_os_str().to_string_lossy(), &None)
                .map_err(|err| {
                    eprintln!("Error loading extension {}: {err}", extension.display());
                    ()
                })?;
        }
    }
    let pb = ProgressBar::new(1);
    pb.set_style(
        indicatif::ProgressStyle::with_template(
            " {spinner} {msg:<30} {wide_bar} ETA {eta_precise} ",
        )
        .unwrap(),
    );
    for sql in args.sql {
        let mut sql = sql;
        if sql.ends_with(".sql") && Path::new(sql.as_str()).exists() {
            sql = std::fs::read_to_string(sql).unwrap();
            pb.set_message(format!("Reading SQL file: {}", sql));
        } else {
            pb.set_message(format!("SQL: {sql}"));
        }
        //println!("Benchmarking: {sql}");
        let stmt = runtime.connection.prepare(&sql).unwrap().1.unwrap();
        let mut times = vec![];
        pb.reset();
        pb.set_length(10);
        let t0 = jiff::Timestamp::now();
        let mut niter = 0;
        for _ in 0..10 {
            pb.inc(1);
            let tn = jiff::Timestamp::now();
            stmt.execute().unwrap();
            stmt.reset();
            times.push(jiff::Timestamp::now() - tn);

            pb.set_message(format!(
                "Current estimate: {}",
                format_runtime(average(&times)).green()
            ));

            bytecode_steps(stmt.pointer());
        }
        pb.finish_and_clear();

        let avg = format_runtime(average(&times));
        let stddev = format_runtime(stddev(&times));
        let mn = format_runtime(min(&times));
        let mx = format_runtime(max(&times));
        let longest = [avg.len(), stddev.len(), mn.len(), mx.len()]
            .into_iter()
            .max()
            .unwrap()
            + 1;
        println!("{sql}:");
        println!(
            "  Time  (mean ± σ ):  {} ± {}",
            avg.clone().green().bold(),
            stddev.clone().green(),
        );
        println!(
            "  Range (min … max):  {} … {}",
            mn.clone().cyan(),
            mx.clone().magenta(),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_bench() {
        insta::assert_snapshot!(format_runtime(Span::new().microseconds(1000)), @"1ms");
        insta::assert_snapshot!(format_runtime("4ms 4us".parse().unwrap()), @"4.004ms");
        insta::assert_snapshot!(format_runtime("49ms 999us".parse().unwrap()), @"49.999ms");
        insta::assert_snapshot!(format_runtime("50ms 999us".parse().unwrap()), @"51ms");
        insta::assert_snapshot!(format_runtime("989ms 999us".parse().unwrap()), @"990ms");
        insta::assert_snapshot!(format_runtime("1s 1ms 999us".parse().unwrap()), @"1s 2ms");
        insta::assert_snapshot!(format_runtime("2s 1ms 999us".parse().unwrap()), @"2s 2ms");
        insta::assert_snapshot!(format_runtime("61s".parse().unwrap()), @"1m 1s");
    }
}
