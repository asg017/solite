use crate::cli::ReplFlags;
use crate::colors::bold;
use crate::run::format_duration;

use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::HistoryHinter;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{
    Completer, CompletionType, Config, EditMode, Editor, Helper, Hinter, Result, Validator,
};

use cli_table::print_stdout;
use solite_core::dot::DotCommand;
use solite_core::{BlockSource, Runtime, StepError, StepResult};
use solite_stdlib::BUILTIN_FUNCTIONS;
use std::borrow::Cow::{self, Borrowed, Owned};
use std::cell::{RefCell, RefMut};
use std::rc::Rc;
use tokio::time::error::Elapsed;

fn is_all_whitespace(s: &str) -> bool {
    for c in s.chars() {
        if !c.is_whitespace() {
            return false;
        }
    }
    true
}

mod sql_highlighter {
    use core::fmt;
    use std::io::Write;
    use std::sync::OnceLock;
    use termcolor::Ansi;
    use termcolor::WriteColor;
    use termcolor::{Color, ColorSpec};

    const KEYWORD_COLOR: Color = Color::Rgb(203, 166, 247);

    pub fn style<S: AsRef<str>>(s: S, colorspec: &ColorSpec) -> impl fmt::Display {
        let mut v = Vec::new();
        let mut ansi_writer = Ansi::new(&mut v);
        ansi_writer.set_color(colorspec).unwrap();
        ansi_writer.write_all(s.as_ref().as_bytes()).unwrap();
        ansi_writer.reset().unwrap();
        String::from_utf8_lossy(&v).into_owned()
    }

    pub(crate) fn keyword<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static KEYWORD: OnceLock<ColorSpec> = OnceLock::new();
        let k = KEYWORD.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(KEYWORD_COLOR)); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn keywordx<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static KEYWORD: OnceLock<ColorSpec> = OnceLock::new();
        let k = KEYWORD.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec
                .set_bg(Some(KEYWORD_COLOR))
                .set_fg(Some(Color::White)); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn dot<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static DOT: OnceLock<ColorSpec> = OnceLock::new();
        let k = DOT.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(139, 213, 202))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn comment<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static COMMENT: OnceLock<ColorSpec> = OnceLock::new();
        let k = COMMENT.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(148, 156, 187))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn parameter<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static PARAMETER: OnceLock<ColorSpec> = OnceLock::new();
        let k = PARAMETER.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(235, 160, 172))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn types<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static TYPES: OnceLock<ColorSpec> = OnceLock::new();
        let k = TYPES.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(249, 226, 175))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn string<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static STRING: OnceLock<ColorSpec> = OnceLock::new();
        let k = STRING.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(166, 209, 137))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn function<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static FUNCTION: OnceLock<ColorSpec> = OnceLock::new();
        let k = FUNCTION.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(138, 173, 244))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn builtin<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static BUILTIN: OnceLock<ColorSpec> = OnceLock::new();
        let k = BUILTIN.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec
                .set_fg(Some(Color::Rgb(138, 173, 244)))
                .set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn paren<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static PAREN: OnceLock<ColorSpec> = OnceLock::new();
        let k = PAREN.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(243, 139, 168))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn operator<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static OPERATOR: OnceLock<ColorSpec> = OnceLock::new();
        let k = OPERATOR.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(137, 220, 235))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
    pub(crate) fn number<S: AsRef<str>>(s: S) -> impl fmt::Display {
        static NUMBER: OnceLock<ColorSpec> = OnceLock::new();
        let k = NUMBER.get_or_init(|| {
            let mut style_spec = ColorSpec::new();
            style_spec.set_fg(Some(Color::Rgb(245, 169, 127))); //.set_bold(true);
            style_spec
        });
        style(s, k)
    }
}

#[derive(Default)]
pub struct ReplHighlighter {}

impl ReplHighlighter {
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }
}

use solite_lexer::{tokenize, Kind, Token};
fn highlight_sql(copy: &mut String) -> String {
    let tokens = tokenize(copy.as_str());
    let mut hl = String::new();
    let mut iter = tokens.iter().peekable();
    let mut prevs: Vec<&Token> = vec![];
    while let Some(token) = iter.next() {
        let s = match token.kind {
            Kind::Comment => sql_highlighter::comment(&copy[token.start..token.end]).to_string(),
            Kind::Parameter => {
                sql_highlighter::parameter(&copy[token.start..token.end]).to_string()
            }
            Kind::Text | Kind::Int | Kind::Float | Kind::Blob | Kind::Bit => {
                sql_highlighter::types(&copy[token.start..token.end]).to_string()
            }
            Kind::Number => sql_highlighter::number(&copy[token.start..token.end]).to_string(),
            Kind::Plus | Kind::Minus | Kind::Eof | Kind::Pipe | Kind::Div | Kind::Lt | Kind::Gt => {
                sql_highlighter::operator(&copy[token.start..token.end]).to_string()
            }
            Kind::String => sql_highlighter::string(&copy[token.start..token.end]).to_string(),
            Kind::Asterisk
            | Kind::LBracket
            | Kind::RBracket
            | Kind::Comma
            | Kind::Semicolon
            | Kind::Dot
            | Kind::Unknown => (&copy[token.start..token.end]).to_string(),
            Kind::LParen | Kind::RParen => {
                sql_highlighter::paren(&copy[token.start..token.end]).to_string()
            }
            Kind::SingleArrowOperator | Kind::DoubleArrowOperator => {
                sql_highlighter::builtin(&copy[token.start..token.end]).to_string()
            }
            Kind::Identifier => {
                // if the next token is a '('
                if matches!(iter.peek().map(|v| v.kind), Some(Kind::LParen))
                // and the previous token is NOT 'using' or 'table'
                    && !(matches!(prevs.last().map(|t| t.kind), Some(Kind::Using) | Some(Kind::Table)))
                {
                    if BUILTIN_FUNCTIONS
                        .iter()
                        .position(|r| *r == (&copy[token.start..token.end]).trim())
                        .is_some()
                    {
                        sql_highlighter::builtin(&copy[token.start..token.end]).to_string()
                    } else {
                        sql_highlighter::function(&copy[token.start..token.end]).to_string()
                    }
                } else {
                    (&copy[token.start..token.end]).to_string()
                }
            }
            _ => sql_highlighter::keyword(&copy[token.start..token.end]).to_string(),
        };
        hl.push_str(s.as_str());
        prevs.push(token);
    }
    hl

    /*
    let keywords = [
        "select",
        "from",
        "where",
        "group by",
        "order by",
        "limit",
        "offset",
        "with",
        "create",
        "table",
        "insert",
        "into",
        "returning",
    ];
    for kw in keywords.iter() {
        if let Some(s) = copy.find(kw) {
            //copy.replace_range(s..s + kw.len(), &format!("\x1b[1;34m{}\x1b[0m", kw));
            copy.replace_range(
                s..s + kw.len(),
                sql_highlighter::keyword(kw).to_string().as_str(),
            );
        }
    }
     */
}
fn highlight_dot(copy: &mut String) {
    let keywords = ["load", "tables", "open"];
    for kw in keywords.iter() {
        if let Some(s) = copy.find(kw) {
            //copy.replace_range(s..s + kw.len(), &format!("\x1b[1;34m{}\x1b[0m", kw));
            copy.replace_range(
                s..s + kw.len(),
                sql_highlighter::dot(kw).to_string().as_str(),
            );
        }
    }
}
impl Highlighter for ReplHighlighter {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        //println!("highlight pos={}", pos);
        if line.len() <= 1 {
            return Borrowed(line);
        }
        let mut copy = line.to_owned();
        if line.starts_with('.') {
            highlight_dot(&mut copy);
        } else {
            return Owned(highlight_sql(&mut copy));
        }
        return Owned(copy);
        //Borrowed(line)
    }

    fn highlight_char(&self, _line: &str, _pos: usize) -> bool {
        true
    }
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
        if solite_core::sqlite::complete(input) {
            return Ok(ValidationResult::Valid(None));
        }
        // dot commands
        if input.trim_start().starts_with('.') {
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

fn execute(runtime: &mut Runtime, timer: &mut bool, code: &str) {
    runtime.enqueue("[repl]", code, BlockSource::Repl);

    loop {
        match runtime.next_step() {
            Ok(Some(step)) => match step.result {
                StepResult::DotCommand(cmd) => match cmd {
                    DotCommand::Tables(cmd) => cmd.execute(runtime),
                    DotCommand::Print(print_cmd) => print_cmd.execute(),
                    DotCommand::Open(open_cmd) => open_cmd.execute(runtime),
                    DotCommand::Load(load_cmd) => load_cmd.execute(&mut runtime.connection),
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
                },
                StepResult::SqlStatement(stmt) => {
                    let start = std::time::Instant::now();

                    if let Some(table) = crate::ui::table_from_statement(stmt, true) {
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
            Ok(None) => break,
            Err(error) => match error {
                StepError::Prepare { error, file_name, src, offset } => {
                    crate::errors::report_error(&file_name, &src, &error, Some(offset));
                }
                StepError::ParseDot(_error) => eprintln!("todo parse dot error"),
            },
        }
    }
}

// https://github.com/sqlite/sqlite/blob/cd889c7a88b2bd23ac71a897c54c43c84eee972d/ext/misc/completion.c#L74-L85
struct ReplCompleter {
    runtime: Rc<RefCell<Runtime>>,
}

impl ReplCompleter {
    pub fn new(runtime: Rc<RefCell<Runtime>>) -> Self {
        Self { runtime }
    }

    fn complete_dot(
        &self,
        line: &str,
        pos: usize,
        ctx: &rustyline::Context<'_>,
    ) -> Result<(usize, Vec<Pair>)> {
        let dots = ["load", "tables", "open"];
        if line.contains(' ') || !line.starts_with('.') {
            return Ok((0, vec![]));
        }
        let prefix = &line[1..];

        let x = dots
            .iter()
            .filter_map(|v| {
                if v.starts_with(prefix) {
                    Some(Pair {
                        display: sql_highlighter::dot(v).to_string(),
                        replacement: format!("{v} "),
                    })
                } else {
                    None
                }
            })
            .collect();
        Ok((1, x))
    }
    fn complete_sql(
        &self,
        line: &str,
        pos: usize,
        ctx: &rustyline::Context<'_>,
    ) -> Result<(usize, Vec<Pair>)> {
        let rt = self.runtime.borrow();
        let (last_word, last_word_idx) = line
            .trim_end()
            .rfind(|c: char| c.is_whitespace())
            .map(|x| (&line[(x + 1)..], x + 1))
            .unwrap_or((line, 0));
        let stmt = rt
            .connection
            .prepare(
                r#"
              select
                case
                  when phase == 1 then lower(candidate)
                  else candidate
                end as candidate,
                phase,
                case
                  when phase == 8 then 1 /* tables */
                  when phase == 9 then 2 /* columns */
                  when phase == 3 then 3 /* functions */
                  when phase == 1 then 4 /* keywords */
                  when phase == 10 then 5 /* modules */
                  when phase == 7 then 6 /* databases */
                  when phase == 2 then 7 /* pragmas */
                  when phase == 4 then 8 /* collations */
                  when phase == 5 then 9 /* indexes */
                  when phase == 6 then 10 /* triggers */
                  else 100
                end as rank
              from completion(?, ?)
              group by 1
              order by rank, candidate
              limit 20
            "#,
            )
            .unwrap()
            .1
            .unwrap();
        stmt.bind_text(1, last_word);
        stmt.bind_text(2, line);

        //stmt.bind_text(2, line);
        let mut candidates: Vec<Pair> = vec![];
        while let Ok(Some(row)) = stmt.next() {
            let candidate = row.first().unwrap().as_str().to_string();
            let phase = row.get(1).unwrap().as_int64();
            let display = if phase == 9 {
                format!("ᶜ {}", (candidate.clone()))
            } else if phase == 1 {
                format!("{}", sql_highlighter::keyword(candidate.clone()))
            } else {
                format!("ᵗ {}", candidate.clone())
            };
            candidates.push(Pair {
                display,
                replacement: candidate.clone(),
            });
        }
        Ok((last_word_idx, candidates))
    }
}

impl Completer for ReplCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &rustyline::Context<'_>,
    ) -> Result<(usize, Vec<Self::Candidate>)> {
        if line.starts_with('.') {
            self.complete_dot(line, pos, ctx)
        } else {
            self.complete_sql(line, pos, ctx)
        }
    }
}

// possible arrows: › ❱ ❯
// possible dots: •*
//
const PROMPT: &str = "❱ ";
const PROMPT_TRANSACTION: &str = "❱• ";

pub fn repl_launch(flags: ReplFlags) -> Result<()> {
    let runtime = Runtime::new(flags.database.clone());
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

    let connection_info = match &flags.database {
        None => "Connected to a transient in-memory database.".to_string(),
        Some(db) => format!("Connected to {}", bold(db)),
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
pub(crate) fn repl(flags: ReplFlags) -> std::result::Result<(), ()> {
    match repl_launch(flags) {
        Ok(_) => Ok(()),
        Err(_) => Err(()),
    }
}
