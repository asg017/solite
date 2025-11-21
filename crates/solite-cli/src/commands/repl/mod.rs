mod completer;
mod highlighter;
use crate::cli::ReplArgs;
use crate::commands::repl::completer::ReplCompleter;
use crate::commands::repl::highlighter::{ReplHighlighter, highlight_sql};
use crate::commands::run::format_duration;
use crate::commands::tui::launch_tui;
use crate::ui::CTP_MOCHA_THEME;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use solite_core::dot::sh::ShellResult;
use rustyline::hint::HistoryHinter;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{
    Completer, CompletionType, Config, EditMode, Editor, Helper, Hinter, Result, Validator,
};

use cli_table::print_stdout;
use solite_core::dot::{DotCommand, LoadCommandSource};
use solite_core::{BlockSource, Runtime, StepError, StepResult};
use std::borrow::Cow::{self, Borrowed, Owned};

use std::cell::{RefCell, RefMut};
use std::io::Write;
use std::rc::Rc;

fn is_all_whitespace(s: &str) -> bool {
    for c in s.chars() {
        if !c.is_whitespace() {
            return false;
        }
    }
    true
}

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
        if is_all_whitespace(input) {
            return Ok(ValidationResult::Valid(None));
        }
        if REPL_SPECIAL_COMMANDS.contains(&input.trim()) {
            return Ok(ValidationResult::Valid(None));
        }
        if solite_core::sqlite::complete(input) {
            return Ok(ValidationResult::Valid(None));
        }
        if input.trim_start().starts_with(".export") {
            match input.trim_start().splitn(2, '\n').nth(1) {
                Some(rest) => {
                    if solite_core::sqlite::complete(rest) {
                        return Ok(ValidationResult::Valid(None));
                    } else {
                        return Ok(ValidationResult::Incomplete);
                    }
                }
                None => {
                    return Ok(ValidationResult::Incomplete);
                }
            }
        }
        // dot commands and special prefixes
        if input.trim_start().starts_with(['.', '!', '?']) {
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

/** TODO
 * - completer
 *   - SQL syntax
 *   - database, table, column names
 *   - SQL functions/table functions
 *   - if replacement scan, then files on disk
 *   - history, tables/columns/functions/urls?
 * - syntax colors
 *   - more complete SQL
 *   - strings
 *   - numbers
 *   - comments
 */

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
            launch_tui(runtime).unwrap();
        }
        DotCommand::Dotenv(cmd) => {
            cmd.execute();
        }
        
        DotCommand::Clear(cmd) => {
            cmd.execute();
        }

        DotCommand::Tables(cmd) => {
            let tables = cmd.execute(runtime);
            for table in tables {
                println!("{table}");
            }
        }
        DotCommand::Schema(cmd) => {
            let creates = cmd.execute(runtime);
            for create in creates {
                println!("{}", highlight_sql(&mut create.clone()));
            }
        }
        DotCommand::Graphviz(cmd) => {
            let dot = cmd.execute(runtime);
            println!("{}", dot);
        }
        DotCommand::Print(print_cmd) => print_cmd.execute(),
        DotCommand::Open(open_cmd) => open_cmd.execute(runtime),
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
                runtime.define_parameter(key.clone(), value).unwrap();
                println!("✓ set '{key}' parameter");
            }
            solite_core::dot::ParameterCommand::Unset(_) => todo!(),
            solite_core::dot::ParameterCommand::List => todo!(),
            solite_core::dot::ParameterCommand::Clear => todo!(),
        },
        DotCommand::Shell(shell_cmd) => {
            match shell_cmd.execute() {
              ShellResult::Background(child) => {
                println!("✓ started background process with PID {}", child.id());
              }
              ShellResult::Stream(rx) => {
                while let Ok(msg) = rx.recv() {
                    println!("{}", msg);
                }
              }
            }
        }
        DotCommand::Ask(ask_command) => {
            let rx = ask_command.execute(runtime).unwrap();
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();

            while let Ok(msg) = rx.recv() {
              let msg = msg.unwrap();
              write!(handle, "{}", msg).unwrap();
            }

            handle.flush().unwrap();
            println!();
        }
        DotCommand::Export(mut export_command) => match export_command.execute() {
            Ok(_) => println!("✓ exported to {}", export_command.target.display()),
            Err(e) => eprintln!(
                "✗ failed to export to {}: {}",
                export_command.target.display(),
                e
            ),
        },
        DotCommand::Vegalite(_vega_lite_command) => {
            eprintln!("Vega-Lite command is not supported in the REPL yet.");
        }
        DotCommand::Bench(_bench_command) => {
            eprintln!("Bench command is not supported in the REPL yet.");
        }
    }
}

// TODO: more special commands
// TODO: \e should get previously ran SQL
static REPL_SPECIAL_COMMANDS: [&str; 1] = ["\\e"];

fn repl_editor_command() -> anyhow::Result<String> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let mut tmpfile = std::env::temp_dir();
    tmpfile.push("solite_repl.sql");
    std::fs::write(&tmpfile, "").unwrap();
    let status = std::process::Command::new(editor)
        .arg(&tmpfile)
        .status()
        .unwrap();
    if status.success() {
        let code = std::fs::read_to_string(&tmpfile).unwrap();
        let _ = std::fs::remove_file(&tmpfile);
        Ok(code)
    } else {
        let _ = std::fs::remove_file(&tmpfile);
        eprintln!("Editor exited with non-zero status");
        Err(anyhow::anyhow!("Editor exited with non-zero status"))
    }
}

fn execute(runtime: &mut Runtime, timer: &mut bool, code: &str) {
    // repl specific commands
    let mut code = code.to_owned();
    if REPL_SPECIAL_COMMANDS.contains(&code.trim()) {
        match code.trim() {
            "\\e" => {
                code = repl_editor_command().unwrap();
            }
            _ => unreachable!(),
        }
        return;
    }
    runtime.enqueue("[repl]", &code, BlockSource::Repl);

    loop {
        match runtime.next_stepx() {
            Some(Ok(step)) => match step.result {
                StepResult::DotCommand(cmd) => handle_dot_command(runtime, cmd, timer),
                StepResult::SqlStatement { stmt, .. } => {
                    let start = std::time::Instant::now();

                    // TODO error handle
                    if let Ok(Some(table)) = crate::ui::table_from_statement(&stmt, Some(&CTP_MOCHA_THEME)) {
                        print_stdout(table).unwrap();
                    }
                    if *timer {
                        println!(
                            "{}",
                            crate::colors::italic_gray(format_duration(start.elapsed()))
                        );
                    }
                }
            },
            None => break,
            Some(Err(error)) => match error {
                StepError::Prepare {
                    error,
                    file_name,
                    src,
                    offset,
                } => {
                    crate::errors::report_error(&file_name, &src, &error, Some(offset));
                }
                StepError::ParseDot(error) => eprintln!("todo parse dot error {error:?}"),
            },
        }
    }
}

// possible arrows: › ❱ ❯
// possible dots: •*
//
const PROMPT: &str = "❱ ";
const PROMPT_TRANSACTION: &str = "❱• ";

pub fn launch_repl(args: ReplArgs) -> Result<()> {
    let runtime = Runtime::new(
        args.database
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
    );
    let rc_runtime = Rc::new(RefCell::new(runtime));

    let mut timer = true;
    // `()` can be used when no completer is required
    let config = Config::builder()
        .completion_type(CompletionType::List)
        .edit_mode(EditMode::Emacs)
        .build();
    let mut rl = Editor::with_config(config)?;
    let helper = ReplHelper {
        completer: ReplCompleter::new(Rc::clone(&rc_runtime)),
        highlighter: ReplHighlighter::new(),
        hinter: HistoryHinter {},
        colored_prompt: "".to_owned(),
        validator: ReplValidator::new(),
    };
    let x = Rc::clone(&rc_runtime);
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

    let solite_history_path =
        std::path::Path::new(std::env::var("HOME").unwrap().as_str()).join(".solite_history");

    let _ = std::fs::File::create_new(&solite_history_path);

    loop {
        let prompt = if x.borrow().connection.in_transaction() {
            PROMPT_TRANSACTION
        } else {
            PROMPT
        };
        rl.helper_mut().unwrap().colored_prompt = crate::colors::cyan(prompt).to_string();

        rl.load_history(&solite_history_path).unwrap();
        let readline = rl.readline(prompt);
        match readline {
            Ok(line) => {
                let line = line
                    .as_str()
                    .strip_prefix(PROMPT)
                    .or_else(|| line.as_str().strip_prefix(PROMPT_TRANSACTION))
                    .unwrap_or(&line);
                {
                    let mut q: RefMut<'_, Runtime> = RefCell::borrow_mut(&x); //x.borrow_mut();
                    execute(&mut q, &mut timer, line);
                }
                rl.add_history_entry(line).unwrap();
                rl.append_history(&solite_history_path).unwrap();
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                x.borrow().connection.interrupt();
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("^D");
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }
    Ok(())
}
pub fn repl(args: ReplArgs) -> std::result::Result<(), ()> {
    launch_repl(args).map_err(|_| ())
}
