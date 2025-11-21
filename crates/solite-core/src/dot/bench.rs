/**
 * Implementatin of the `.bench` command. 
 * 
 * Usage:
 * 
 * ```sql
 * .bench 
 * ```
 * 
 * 
 * Available in:
 * 
 * - REPL
 * - Scripts
 * - Jupyter
 */
use std::ffi::OsString;

use serde::Serialize;
  use crate::{
      ParseDotError, Runtime, sqlite::{BytecodeStep, Statement, bytecode_steps}
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
  pub report: String,
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
          "{}\n  Time  (mean ± σ):   {} ± {} ({} iterations)\n  Range (min … max):  {} … {}\n{}",
          match self.name.clone() {
              Some(name) => format!("Benchmark: {}", name),
              None => "Benchmark".to_string(),
          },
          format_runtime(avg),
          format_runtime(stddev),
          niter,
          format_runtime(mn.clone()),
          format_runtime(mx.clone()),
          self.report
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
        let mut report = String::new();
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
            
            let steps = bytecode_steps(self.statement.pointer());
            report = render_steps(steps);
            //println!("{}", report);
            
            
        }
      Ok(BenchResult {
          name: self.name.clone(),
          suite: self.suite.clone(),
          times,
          niter,
          report
        })
    }
}

/*

pub struct BytecodeStep {
    pub addr: i64,
    pub opcode: String,
    pub p1: i64,
    pub p2: i64,
    pub p3: i64,
    pub p4: String,
    pub p5: i64,
    pub comment: String,
    pub subprog: i64,
    pub nexec: i64,
    pub ncycle: i64,
}
    */

fn render_flamegraph(steps: &[BytecodeStep], ai_indent: &[i32]) -> String {
    let mut output = String::new();
    
    output.push_str("\n");
    output.push_str("# Flamegraph Data (paste into https://www.speedscope.app/ or flamegraph.pl)\n");
    output.push_str("# Format: stack;frames cycles\n");
    output.push_str("# ----------------------------------------\n");
    
    // Build flamegraph entries with proper stack context
    // Use indentation to build hierarchical stack traces
    let mut stack: Vec<String> = vec![];
    
    for (i, step) in steps.iter().enumerate() {
        let indent = ai_indent[i];
        
        if step.ncycle == 0 {
            continue; // Skip steps with no cycles
        }
        
        // Adjust stack based on indentation changes
        let indent_level = (indent / 2) as usize;
        
        if indent_level < stack.len() {
            // Pop stack items if we've decreased indentation
            stack.truncate(indent_level);
        }
        
        // Create frame name with opcode and key info
        let frame = if !step.comment.is_empty() {
            format!("{}_{}", step.opcode, step.comment.replace(";", "").replace(" ", "_"))
        } else {
            format!("{}", step.opcode)
        };
        
        // Build the full stack trace
        let mut full_stack = stack.clone();
        full_stack.push(frame.clone());
        
        // Output in collapsed stack format: stack;frames cycles
        output.push_str(&format!("{} {}\n", full_stack.join(";"), step.ncycle));
        
        // Update stack for next iteration based on opcode type
        // Keep current frame on stack if it's a loop start
        if matches!(step.opcode.as_str(), "Rewind" | "SeekGT" | "SeekLT") {
            if indent_level >= stack.len() {
                stack.push(frame);
            }
        }
    }
    
    output.push_str("\n");
    
    output
}

fn render_steps(steps: Vec<BytecodeStep>) -> String {
    let mut output = String::new();
    
    if steps.is_empty() {
        return output;
    }
    
    // Compute indentation array
    let n_indent = steps.len();
    let mut ai_indent = vec![0i32; n_indent];
    
    // Apply indentation rules
    for (i_op, step) in steps.iter().enumerate() {
        let opcode = step.opcode.as_str();
        let i_addr = step.addr;
        let p2 = step.p2;
        let p1 = step.p1;
        
        // Calculate p2op (target address in array indices)
        let p2op = (p2 + (i_op as i64 - i_addr)) as usize;
        
        // Rule 2: Next/Prev family opcodes
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
        
        // Rule 3: Goto (backward jumps)
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
    
    // Calculate total cycles for percentage computation
    let total_cycles: i64 = steps.iter().map(|s| s.ncycle).sum();
    
    // Find the maximum width needed for comment (for alignment)
    let max_comment_width = steps.iter().map(|s| s.comment.len()).max().unwrap_or(20);
    let comment_width = max_comment_width.max(20) + 2; // Add 2 for spacing
    
    // Render output with indentation
    output.push_str(&format!(
        "QUERY PLAN (cycles={} [100%])\n",
        total_cycles
    ));
    output.push_str("addr  opcode         p1    p2    p3    p4             p5  comment\n");
    output.push_str("----  -------------  ----  ----  ----  -------------  --  -------\n");
    
    for (i, step) in steps.iter().enumerate() {
        let indent = ai_indent[i];
        
        // Calculate cycle percentage
        let cycle_pct = if total_cycles > 0 {
            ((step.ncycle as f64 / total_cycles as f64) * 100.0).round() as i64
        } else {
            0
        };
        
        // Format the line with fixed-width columns and right-aligned cycle info
        if step.ncycle > 0 {
            let cycle_info = format!("(cycles={} [{}%])", step.ncycle, cycle_pct);
            output.push_str(&format!(
                "{:<4}  {:indent$}{:<13}  {:<4}  {:<4}  {:<4}  {:<13}  {:<2}  {:<width$}{}\n",
                step.addr,
                "",
                step.opcode,
                step.p1,
                step.p2,
                step.p3,
                step.p4,
                step.p5,
                step.comment,
                cycle_info,
                indent = indent as usize,
                width = comment_width
            ));
        } else {
            output.push_str(&format!(
                "{:<4}  {:indent$}{:<13}  {:<4}  {:<4}  {:<4}  {:<13}  {:<2}  {}\n",
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
            ));
        }
    }
    
    //output.push_str(&render_flamegraph(&steps, &ai_indent));
    
    output
}