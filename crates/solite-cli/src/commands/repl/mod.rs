pub(crate) mod completer;
mod highlighter;
use crate::cli::ReplArgs;
use crate::commands::repl::completer::ReplCompleter;
use crate::commands::repl::highlighter::{ReplHighlighter, highlight_sql};
use crate::commands::run::format_duration;
use crate::commands::tui::launch_tui;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use solite_core::dot::sh::ShellResult;
use rustyline::hint::HistoryHinter;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Completer, CompletionType, Config, EditMode, Editor, Helper, Hinter, Result, Validator};

use solite_core::dot::{DotCommand, LoadCommandSource};
use solite_core::{BlockSource, Runtime, StepError, StepResult};
use solite_table::TableConfig;
use std::borrow::Cow::{self, Borrowed, Owned};

use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Simple matching bracket validator.
#[derive(Default)]
pub struct ReplValidator {
    _priv: (),
}

impl ReplValidator {
    /// Constructor
    #[must_use]
    pub fn new() -> Self {
        Self { _priv: () }
    }
}

impl Validator for ReplValidator {
    fn validate(&self, ctx: &mut ValidationContext) -> Result<ValidationResult> {
        let input = ctx.input();
        if REPL_SPECIAL_COMMANDS.contains(&input.trim()) {
            return Ok(ValidationResult::Valid(None));
        }
        if solite_core::sqlite::input_complete(input) {
            return Ok(ValidationResult::Valid(None));
        }
        Ok(ValidationResult::Incomplete)
    }
}

#[derive(Helper, Completer, Hinter, Validator)]
struct ReplHelper {
    #[rustyline(Completer)]
    completer: ReplCompleter,
    highlighter: ReplHighlighter,
    #[rustyline(Validator)]
    validator: ReplValidator,
    #[rustyline(Hinter)]
    hinter: HistoryHinter,
    colored_prompt: String,
}

// Completer/highlighter ideas not implemented yet:
// - file-on-disk completion when a replacement scan is in play
// - history-aware completion (tables/columns/functions/urls)
impl Highlighter for ReplHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        default: bool,
    ) -> Cow<'b, str> {
        if default {
            Borrowed(&self.colored_prompt)
        } else {
            Borrowed(prompt)
        }
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Owned("\x1b[1m".to_owned() + hint + "\x1b[m")
    }

    fn highlight<'l>(&self, line: &'l str, pos: usize) -> Cow<'l, str> {
        self.highlighter.highlight(line, pos)
    }

    fn highlight_char(&self, line: &str, pos: usize) -> bool {
        self.highlighter.highlight_char(line, pos)
    }
}

fn handle_dot_command(runtime: &mut Runtime, cmd: DotCommand, timer: &mut bool) {
    match cmd {
        DotCommand::Tui(_) => {
            if let Err(e) = launch_tui(runtime) {
                eprintln!("✗ failed to launch TUI: {}", e);
            }
        }
        DotCommand::Dotenv(cmd) => match cmd.execute() {
            Ok(result) => {
                println!("✓ loaded {} variables from {}", result.loaded.len(), result.path.display());
            }
            Err(e) => {
                eprintln!("✗ failed to load .env: {}", e);
            }
        },
        DotCommand::Clear(cmd) => {
            cmd.execute();
        }
        DotCommand::Tables(cmd) => match cmd.execute(runtime) {
            Ok(tables) => {
                for table in tables {
                    println!("{table}");
                }
            }
            Err(e) => {
                eprintln!("✗ failed to list tables: {}", e);
            }
        },
        DotCommand::Schema(cmd) => match cmd.execute(runtime) {
            Ok(creates) => {
                for create in creates {
                    println!("{}", highlight_sql(&create));
                }
            }
            Err(e) => {
                eprintln!("✗ failed to get schema: {}", e);
            }
        },
        DotCommand::Graphviz(cmd) => match cmd.execute(runtime) {
            Ok(dot) => {
                println!("{}", dot);
            }
            Err(e) => {
                eprintln!("✗ failed to generate graphviz: {}", e);
            }
        },
        DotCommand::Print(print_cmd) => print_cmd.execute(),
        DotCommand::Help(help_cmd) => println!("{}", help_cmd.execute()),
        DotCommand::Open(open_cmd) => match open_cmd.execute(runtime) {
            Ok(()) => {
                println!("✓ opened database");
            }
            Err(e) => {
                eprintln!("✗ failed to open database: {}", e);
            }
        },
        DotCommand::Load(load_cmd) => match load_cmd.execute(&mut runtime.connection) {
            Ok(source) => match source {
                LoadCommandSource::Path(path) => {
                    println!("✓ loaded extension {}", path);
                }
                LoadCommandSource::Uv { directory, package } => {
                    println!("✓ uv loaded extension {} from {}", package, directory);
                }
            },
            Err(e) => {
                eprintln!("✗ failed to load extension {}: {}", load_cmd.path, e);
            }
        },
        DotCommand::Timer(enabled) => *timer = enabled,
        DotCommand::Parameter(param_cmd) => match param_cmd {
            solite_core::dot::ParameterCommand::Set { key, value } => {
                match runtime.define_parameter(key.clone(), value) {
                    Ok(()) => println!("✓ set '{key}' parameter"),
                    Err(e) => eprintln!("✗ failed to set parameter '{key}': {}", e),
                }
            }
            solite_core::dot::ParameterCommand::Unset(key) => {
                runtime.delete_parameter(&key);
                println!("✓ unset '{key}' parameter");
            }
            solite_core::dot::ParameterCommand::List => {
                match solite_core::dot::param::list_parameters_statement(runtime) {
                    Some(mut stmt) => {
                        let config = TableConfig::terminal();
                        if let Err(e) = solite_table::print_statement(&mut stmt, &config) {
                            eprintln!("✗ failed to list parameters: {}", e);
                        }
                    }
                    None => println!("No parameters set"),
                }
            }
            solite_core::dot::ParameterCommand::Clear => {
                let cleared = solite_core::dot::param::clear_parameters(runtime);
                println!("✓ cleared {cleared} parameter(s)");
            }
        },
        DotCommand::Env(env_cmd) => {
            let action = env_cmd.execute();
            match action {
                solite_core::dot::EnvAction::Set { name, value: _ } => {
                    println!("✓ set environment variable '{name}'");
                }
                solite_core::dot::EnvAction::Unset { name } => {
                    println!("✓ unset environment variable '{name}'");
                }
            }
        }
        DotCommand::Shell(shell_cmd) => match shell_cmd.execute() {
            Ok(ShellResult::Background(child)) => {
                println!("✓ started background process with PID {}", child.id());
            }
            Ok(ShellResult::Stream(rx)) => {
                while let Ok(msg) = rx.recv() {
                    println!("{}", msg);
                }
            }
            Err(e) => {
                eprintln!("✗ shell command failed: {}", e);
            }
        },
        DotCommand::Ask(ask_command) => {
            match ask_command.execute(runtime) {
                Ok(rx) => {
                    let stdout = std::io::stdout();
                    let mut handle = stdout.lock();
                    while let Ok(msg) = rx.recv() {
                        if let Ok(text) = msg {
                            let _ = write!(handle, "{}", text);
                        }
                    }
                    let _ = handle.flush();
                    println!();
                }
                Err(e) => eprintln!("✗ ask command failed: {}", e),
            }
        }
        DotCommand::Export(mut export_command) => match export_command.execute() {
            Ok(_) => println!("✓ exported to {}", export_command.target.display()),
            Err(e) => eprintln!(
                "✗ failed to export to {}: {}",
                export_command.target.display(),
                e
            ),
        },
        DotCommand::Vegalite(mut cmd) => match cmd.execute() {
            Ok(spec) => match crate::commands::write_vegalite_spec(&spec) {
                Ok(path) => println!("✓ wrote Vega-Lite spec to {}", path.display()),
                Err(e) => eprintln!("✗ failed to write Vega-Lite spec: {}", e),
            },
            Err(e) => eprintln!("✗ vegalite command failed: {}", e),
        },
        DotCommand::Bench(mut cmd) => match cmd.execute(None) {
            Ok(result) => {
                println!("{}", result.report());
                if !result.report.is_empty() {
                    println!("{}", result.report);
                }
            }
            Err(e) => eprintln!("✗ bench failed: {}", e),
        },
        #[cfg(feature = "ritestream")]
        DotCommand::Stream(stream_cmd) => match stream_cmd.execute(runtime) {
            Ok(Some(result)) => {
                println!("✓ synced (txid={}, {} pages)", result.txid, result.page_count);
            }
            Ok(None) => {
                println!("✓ stream command completed");
            }
            Err(e) => {
                eprintln!("✗ stream command failed: {}", e);
            }
        },
        DotCommand::Call(_) => { /* resolved to SqlStatement in next_stepx() */ }
        DotCommand::Run(run_cmd) => {
            if let Some(ref proc_name) = run_cmd.procedure {
                if let Err(e) = runtime.load_file(&run_cmd.file) {
                    eprintln!("✗ failed to load file '{}': {}", run_cmd.file, e);
                    return;
                }
                let proc = match runtime.get_procedure(proc_name) {
                    Some(p) => p.clone(),
                    None => {
                        eprintln!("✗ unknown procedure: '{}'", proc_name);
                        return;
                    }
                };
                // Scope --key=val parameters to this invocation; defined only
                // after the file loads and the procedure resolves, so a failed
                // .run leaves no parameters behind.
                let saved = match runtime.save_and_define_parameters(&run_cmd.parameters) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("✗ failed to set parameters: {}", e);
                        return;
                    }
                };
                match runtime.prepare_with_parameters(&proc.sql) {
                    Ok((_, Some(mut stmt))) => {
                        let config = solite_table::TableConfig::terminal();
                        if let Err(e) = solite_table::print_statement(&mut stmt, &config) {
                            eprintln!("✗ failed to execute procedure: {}", e);
                        }
                    }
                    Ok((_, None)) => {
                        eprintln!("✗ procedure '{}' prepared to empty statement", proc_name);
                    }
                    Err(e) => {
                        eprintln!("✗ failed to prepare procedure '{}': {:?}", proc_name, e);
                    }
                }
                runtime.restore_parameters(saved);
            } else {
                let saved = match runtime.run_file_begin(&run_cmd.file, &run_cmd.parameters) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("✗ {}", e);
                        return;
                    }
                };
                step_loop(runtime, timer);
                runtime.run_file_end(saved);
            }
        }
    }
}

/// Drain the runtime's execution stack, printing results and errors. The
/// single step loop shared by `execute()` and the `.run` file branch of
/// `handle_dot_command`.
fn step_loop(runtime: &mut Runtime, timer: &mut bool) {
    loop {
        match runtime.next_stepx() {
            None => break,
            Some(Ok(step)) => match step.result {
                StepResult::DotCommand(cmd) => handle_dot_command(runtime, cmd, timer),
                StepResult::ProcedureDefinition(ref proc) => {
                    println!("Registered procedure: {}", proc.name);
                }
                StepResult::SqlStatement { mut stmt, .. } => {
                    let start = std::time::Instant::now();
                    let config = TableConfig::terminal();
                    if let Err(e) = solite_table::print_statement(&mut stmt, &config) {
                        eprintln!("✗ failed to print table: {}", e);
                    }
                    if *timer {
                        println!(
                            "{}",
                            crate::colors::italic_gray(format_duration(start.elapsed()))
                        );
                    }
                }
            },
            Some(Err(error)) => match error {
                StepError::Prepare {
                    error,
                    file_name,
                    src,
                    offset,
                } => {
                    crate::errors::report_error(&file_name, &src, &error, Some(offset));
                }
                StepError::ParseDot(error) => eprintln!("Parse error: {}", error),
            },
        }
    }
}

// TODO: more special commands
static REPL_SPECIAL_COMMANDS: [&str; 1] = ["\\e"];

/// Open `$EDITOR` (default `vi`) on a scratch buffer seeded with `initial`
/// (the most recently executed input, psql-style). The buffer is a unique
/// per-invocation temp file so concurrent REPL sessions don't clobber each
/// other; it is removed afterwards.
fn repl_editor_command(initial: &str) -> anyhow::Result<String> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let tmpfile = tempfile::Builder::new()
        .prefix("solite-repl-")
        .suffix(".sql")
        .tempfile()?;
    std::fs::write(tmpfile.path(), initial)?;
    let status = std::process::Command::new(&editor)
        .arg(tmpfile.path())
        .status()?;
    let code = std::fs::read_to_string(tmpfile.path());
    if status.success() {
        Ok(code?)
    } else {
        Err(anyhow::anyhow!("Editor '{}' exited with non-zero status", editor))
    }
}

/// Execute one submitted line. Returns the code that actually ran (the
/// editor buffer's contents for `\e`) so the caller can record it in
/// history, or `None` if nothing was executed.
fn execute(runtime: &mut Runtime, timer: &mut bool, code: &str, last_input: &str) -> Option<String> {
    // repl specific commands
    let mut code = code.to_owned();
    if REPL_SPECIAL_COMMANDS.contains(&code.trim()) {
        match code.trim() {
            "\\e" => match repl_editor_command(last_input) {
                Ok(editor_code) => code = editor_code,
                Err(e) => {
                    eprintln!("✗ editor command failed: {}", e);
                    return None;
                }
            },
            _ => unreachable!(),
        }
    }
    runtime.enqueue("[repl]", &code, BlockSource::Repl);
    step_loop(runtime, timer);
    Some(code)
}

// possible arrows: › ❱ ❯
// possible dots: •*
//
const PROMPT: &str = "❱ ";
const PROMPT_TRANSACTION: &str = "❱• ";

/// Strip leading prompt markers from every line of the submitted input, so
/// text copied from another REPL session (prompts included) paste-executes
/// cleanly. Input that legitimately starts with `❱ ` is vanishingly rare.
fn strip_prompt_prefixes(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            line.strip_prefix(PROMPT)
                .or_else(|| line.strip_prefix(PROMPT_TRANSACTION))
                .unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn launch_repl(args: ReplArgs) -> Result<()> {
    let runtime = Runtime::new_with_options(
        args.database
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        args.remote.remote_bin.as_deref(),
        args.remote.transport.as_deref(),
        args.remote.allow_ssh,
    ).map_err(|e| ReadlineError::Io(std::io::Error::other(e.to_string())))?;
    let rc_runtime = Rc::new(RefCell::new(runtime));

    let mut timer = true;
    let config = Config::builder()
        .completion_type(CompletionType::List)
        .edit_mode(EditMode::Emacs)
        // Skip consecutive duplicate entries and cap in-memory history so
        // the history file doesn't grow per repeated command.
        .history_ignore_dups(true)?
        .max_history_size(10_000)?
        .build();
    let mut rl = Editor::with_config(config)?;
    let helper = ReplHelper {
        completer: ReplCompleter::new(Rc::clone(&rc_runtime)),
        highlighter: ReplHighlighter::new(),
        hinter: HistoryHinter {},
        colored_prompt: String::new(),
        validator: ReplValidator::new(),
    };
    let runtime_ref = Rc::clone(&rc_runtime);
    rl.set_helper(Some(helper));

    let connection_info = match &args.database {
        None => "Connected to a transient in-memory database.".to_string(),
        Some(db) => format!("Connected to {:?}", db),
    };

    let prelude = format!(
        "Solite {} (SQLite {})
Enter \".help\" for usage hints.
{}",
        env!("CARGO_PKG_VERSION"),
        solite_core::sqlite::sqlite_version(),
        connection_info
    );
    println!("{prelude}");

    // History lives at $SOLITE_HISTORY if set, else $HOME/.solite_history.
    // With neither set, skip persistence entirely rather than silently
    // littering the current directory.
    let solite_history_path = std::env::var("SOLITE_HISTORY")
        .map(std::path::PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::var("HOME")
                .map(|home| std::path::PathBuf::from(home).join(".solite_history"))
                .ok()
        });

    if let Some(path) = &solite_history_path {
        // Create iff missing; ignore errors (e.g. read-only home)
        let _ = std::fs::File::create_new(path);
        let _ = rl.load_history(path);
    }

    // Ctrl-C while a statement is running raises SIGINT (rustyline is not
    // reading, so the terminal is in its normal mode). The handler just sets
    // a flag; a SQLite progress handler polls it and aborts the running
    // statement with SQLITE_INTERRUPT so the REPL survives.
    let interrupted = Arc::new(AtomicBool::new(false));
    {
        let flag = Arc::clone(&interrupted);
        // Failure to install the handler (e.g. another handler already
        // registered) only loses query cancellation, not the REPL itself.
        let _ = ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst));
    }

    // Most recently executed input; seeds the `\e` editor scratch buffer.
    let mut last_input = String::new();

    loop {
        let prompt = if runtime_ref.borrow().connection.in_transaction() {
            PROMPT_TRANSACTION
        } else {
            PROMPT
        };
        if let Some(helper) = rl.helper_mut() {
            helper.colored_prompt = crate::colors::cyan(prompt).to_string();
        }

        let readline = rl.readline(prompt);
        match readline {
            Ok(line) => {
                let line = strip_prompt_prefixes(&line);
                let line = line.as_str();
                let executed = {
                    let mut rt = runtime_ref.borrow_mut();
                    // Re-register every iteration: `.open` swaps the
                    // connection out, which drops any previous registration.
                    interrupted.store(false, Ordering::SeqCst);
                    let flag = Arc::clone(&interrupted);
                    rt.connection
                        .set_progress_handler(1000, move || flag.load(Ordering::SeqCst));
                    execute(&mut rt, &mut timer, line, &last_input)
                };
                // Record what actually ran (for `\e`, the editor buffer's
                // SQL rather than the literal `\e`) so up-arrow recalls it.
                // Failed statements are intentionally kept: recalling and
                // fixing a typo'd query is the main use of history.
                if let Some(code) = executed {
                    let _ = rl.add_history_entry(code.as_str());
                    if let Some(path) = &solite_history_path {
                        let _ = rl.append_history(path);
                    }
                    last_input = code;
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C at the prompt discards the current input line and
                // re-prompts (like sqlite3/psql). Ctrl-D exits.
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("^D");
                break;
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        }
    }
    Ok(())
}
pub fn repl(args: ReplArgs) -> std::result::Result<(), ()> {
    launch_repl(args).map_err(|err| eprintln!("Error: {err}"))
}

#[cfg(test)]
mod tests {
    use super::strip_prompt_prefixes;

    #[test]
    fn test_strip_prompt_prefixes() {
        // single line, both prompt flavors
        assert_eq!(strip_prompt_prefixes("❱ select 1;"), "select 1;");
        assert_eq!(strip_prompt_prefixes("❱• commit;"), "commit;");
        // untouched input passes through
        assert_eq!(strip_prompt_prefixes("select 1;"), "select 1;");
        // multi-line paste strips every line's prompt
        assert_eq!(
            strip_prompt_prefixes("❱ select\n❱ 1 + 1\n❱ as x;"),
            "select\n1 + 1\nas x;"
        );
        // mixed prompted/unprompted lines
        assert_eq!(
            strip_prompt_prefixes("❱ begin;\ninsert into t values (1);\n❱• commit;"),
            "begin;\ninsert into t values (1);\ncommit;"
        );
    }
}
