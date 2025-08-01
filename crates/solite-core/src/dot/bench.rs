  use std::ffi::OsString;

use serde::Serialize;
  use crate::{
      sqlite::Statement,
      Runtime,
      ParseDotError
  };

  use jiff::{Span, SpanRound, Unit};

#[derive(Serialize, Debug)]
pub struct BenchCommand {
    pub name: Option<String>,
    pub suite: Option<String>,
    pub statement: Statement,
    pub rest_length: usize,
}

pub struct BenchResult {
  pub name: Option<String>,
  pub suite: Option<String>,
  pub times: Vec<jiff::Span>,
  pub niter: usize,
}

impl BenchResult {
  pub fn average(&self) -> jiff::Span {
      average(&self.times)
  }

  pub fn report(&self) -> String {
      let avg = self.average();
      let stddev = stddev(&self.times);
      let mn = min(&self.times);
      let mx = max(&self.times);
      let niter = self.niter;
      format!(
          "{}\n  Time  (mean ± σ):   {} ± {} ({} iterations)\n  Range (min … max):  {} … {}",
          match self.name.clone() {
              Some(name) => format!("Benchmark: {}", name),
              None => "Benchmark".to_string(),
          },
          format_runtime(avg),
          format_runtime(stddev),
          niter,
          format_runtime(mn.clone()),
          format_runtime(mx.clone())
      )
  }
}


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
impl BenchCommand {
    pub fn new(args: String, runtime: &mut Runtime, rest: &str) -> Result<Self, ParseDotError> {
      let mut pargs = pico_args::Arguments::from_vec(args.split(" ").map(OsString::from).collect());
      println!("args: {:?}", args);
      println!("pargs: {:?}", pargs);
      let mut name = None;
      if let Some(x) = pargs.opt_value_from_str("--name").unwrap() {
        name = Some(x);
      }
        match runtime.prepare_with_parameters(rest) {
            Ok((rest2, Some(stmt))) => {
                
                Ok(Self {
                    name,
                    suite: None,
                    statement: stmt,
                    // TODO: suspicious
                    rest_length: rest2.unwrap_or(rest.len()),
                })
            }
            _ => todo!(),
        }
    }
    
    pub fn execute(&mut self, callback: Option<Box<dyn Fn(jiff::Span)>>) -> anyhow::Result<BenchResult> {
      let mut times = vec![];
        let t0 = jiff::Timestamp::now();
        let mut niter = 0;
        for _ in 0..10 {
          niter += 1;
            let tn = jiff::Timestamp::now();
            self.statement.execute().unwrap();
            self.statement.reset();
            let elapsed = jiff::Timestamp::now() - tn;
            times.push(elapsed.clone());
            if let Some(cb) = &callback {
                cb(elapsed);
            }
            
            //bytecode_steps(self.statement.pointer());
        }
      Ok(BenchResult {
          name: self.name.clone(),
          suite: self.suite.clone(),
          times,
          niter,
        })
    }
}
