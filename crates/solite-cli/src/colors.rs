#![allow(dead_code)]

// Borrowed from https://github.com/denoland/deno/blob/main/runtime/colors.rs
use std::fmt;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use solite_theme::Theme;
use termcolor::Ansi;
use termcolor::Color::Ansi256;
use termcolor::Color::Black;
use termcolor::Color::Blue;
use termcolor::Color::Cyan;
use termcolor::Color::Green;
use termcolor::Color::Magenta;
use termcolor::Color::Red;
use termcolor::Color::White;
use termcolor::Color::Yellow;
use termcolor::ColorSpec;
use termcolor::WriteColor;

#[cfg(windows)]
use termcolor::BufferWriter;
#[cfg(windows)]
use termcolor::ColorChoice as TermcolorChoice;

/// The env/tty-independent half of the color decision: either forced to a
/// fixed value (by `--color always/never` or one of the standard env
/// overrides) or left to each stream's own `is_terminal()` check.
///
/// `NO_COLOR`/`CLICOLOR_FORCE`/`CLICOLOR=0`/`TERM=dumb` apply identically to
/// stdout and stderr (they're environment-wide signals), so only this part
/// needs to be resolved once; the per-stream part (`is_terminal()`) is cheap
/// enough to check at each call site instead of caching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resolution {
    Forced(bool),
    Auto,
}

static RESOLUTION: OnceLock<Resolution> = OnceLock::new();
static THEME: OnceLock<Theme> = OnceLock::new();

fn env_non_empty(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

fn env_is(name: &str, value: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| v == value)
}

/// Resolve the non-tty-dependent part of the standard color precedence:
/// `--color always/never` > `NO_COLOR` (non-empty ⇒ off) > `CLICOLOR_FORCE`
/// (set and not "0" ⇒ on) > `CLICOLOR=0` (⇒ off) > `TERM=dumb` (⇒ off) >
/// defer to `is_terminal()` per stream.
fn resolve(choice: clap::ColorChoice) -> Resolution {
    match choice {
        clap::ColorChoice::Always => return Resolution::Forced(true),
        clap::ColorChoice::Never => return Resolution::Forced(false),
        clap::ColorChoice::Auto => {}
    }
    if env_non_empty("NO_COLOR") {
        return Resolution::Forced(false);
    }
    if std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| v != "0") {
        return Resolution::Forced(true);
    }
    if env_is("CLICOLOR", "0") {
        return Resolution::Forced(false);
    }
    if env_is("TERM", "dumb") {
        return Resolution::Forced(false);
    }
    Resolution::Auto
}

/// Resolve and store the color decision for the whole process. Call exactly
/// once, as early as possible (from `run_main`), before any output is
/// printed. Also toggles the `console` crate's global switch when the
/// choice is forced on or off, so `console::style(..)` (test dots, snapshot
/// diffs) follows the same decision; `console`'s own NO_COLOR/tty detection
/// is left in place for `auto`.
pub fn init(choice: clap::ColorChoice, theme_name: Option<&str>) {
    let resolution = resolve(choice);
    let _ = RESOLUTION.set(resolution);
    if let Resolution::Forced(enabled) = resolution {
        console::set_colors_enabled(enabled);
        console::set_colors_enabled_stderr(enabled);
    }

    // Truecolor is not universally supported; when the terminal doesn't
    // advertise it, `solite-theme` downgrades Rgb values to the nearest
    // 256-color index at emission time.
    solite_theme::set_color_depth(solite_theme::detect_color_depth());

    // `--theme` wins over `$SOLITE_THEME`; neither means the built-in
    // terminal theme.
    let name = theme_name
        .map(str::to_string)
        .or_else(|| std::env::var("SOLITE_THEME").ok().filter(|v| !v.is_empty()));
    let theme = match name {
        None => Theme::terminal(),
        Some(name) => match resolve_theme(&name) {
            Ok(theme) => theme,
            Err(message) => {
                eprintln!("error: {message}");
                std::process::exit(1);
            }
        },
    };
    let _ = THEME.set(theme);
}

/// The directories searched for `<name>.toml` user themes, most specific
/// first: `$XDG_CONFIG_HOME/solite/themes`, then `~/.config/solite/themes`
/// (the bat/lazygit convention — `~/.config` on macOS too, not
/// `~/Library/Application Support`).
pub fn theme_search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(xdg).join("solite").join("themes"));
    }
    if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        let dir = PathBuf::from(home)
            .join(".config")
            .join("solite")
            .join("themes");
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// Resolve a `--theme` value: a built-in name, a `<name>.toml` in one of the
/// [`theme_search_dirs`], or — when the value contains a path separator or
/// ends in `.toml` — a file path.
pub fn resolve_theme(name: &str) -> Result<Theme, String> {
    let is_path = name.ends_with(".toml")
        || name.contains('/')
        || name.contains(std::path::MAIN_SEPARATOR);
    if is_path {
        return solite_theme::load_theme_file(Path::new(name)).map_err(|e| e.to_string());
    }
    if let Some(theme) = Theme::builtin(name) {
        return Ok(theme);
    }
    let dirs = theme_search_dirs();
    for dir in &dirs {
        let candidate = dir.join(format!("{name}.toml"));
        if candidate.is_file() {
            return solite_theme::load_theme_file(&candidate).map_err(|e| e.to_string());
        }
    }
    let searched = if dirs.is_empty() {
        "  (no config directory: neither $XDG_CONFIG_HOME nor $HOME is set)".to_string()
    } else {
        dirs.iter()
            .map(|d| format!("  searched: {}", d.join(format!("{name}.toml")).display()))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Err(format!(
        "no theme named `{name}`\n  built-in themes: {}\n{searched}\n  (a value containing `/` or ending in `.toml` is loaded as a file path)",
        Theme::BUILTIN_NAMES.join(", ")
    ))
}

/// The theme resolved for this process. Defaults to [`Theme::terminal`] when
/// [`init`] hasn't run (tests, benches).
pub fn theme() -> &'static Theme {
    THEME.get_or_init(Theme::terminal)
}

fn resolution() -> Resolution {
    RESOLUTION.get().copied().unwrap_or(Resolution::Auto)
}

/// Best-effort scan of raw argv for `--color <value>` / `--color=<value>`,
/// used to style clap's own `--help`/usage/error output before full parsing
/// (and its resulting authoritative value) is available. Does not attempt
/// to distinguish a `--color` meant for a pass-through subcommand (e.g.
/// `solite sqlite3 --color ...`) from the top-level flag.
pub fn scan_color_flag(args: &[String]) -> clap::ColorChoice {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(value) = arg.strip_prefix("--color=") {
            if let Ok(choice) = value.parse() {
                return choice;
            }
        } else if arg == "--color" {
            if let Some(value) = iter.next() {
                if let Ok(choice) = value.parse() {
                    return choice;
                }
            }
        }
    }
    clap::ColorChoice::Auto
}

/// Best-effort scan of raw argv for `--theme <value>` / `--theme=<value>`,
/// used on the fallback paths in `run_main` where clap never produced a `Cli`
/// (bare `solite`, `solite foo.db`).
pub fn scan_theme_flag(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(value) = arg.strip_prefix("--theme=") {
            return Some(value.to_string());
        } else if arg == "--theme" {
            return iter.next().cloned();
        }
    }
    None
}

/// Whether stdout is a terminal.
pub fn is_tty() -> bool {
    std::io::stdout().is_terminal()
}

/// Whether stdout output should be colored, per the resolved precedence.
pub fn use_color() -> bool {
    match resolution() {
        Resolution::Forced(v) => v,
        Resolution::Auto => std::io::stdout().is_terminal(),
    }
}

/// Whether stderr output should be colored, per the resolved precedence.
pub fn use_color_stderr() -> bool {
    match resolution() {
        Resolution::Forced(v) => v,
        Resolution::Auto => std::io::stderr().is_terminal(),
    }
}

/// The `TableConfig` to use for a table printed to stdout: the normal
/// terminal config, minus the theme when color is gated off.
pub fn table_config() -> solite_table::TableConfig {
    let config = solite_table::TableConfig::terminal();
    if use_color() {
        config.with_theme(Some(*theme()))
    } else {
        config.with_theme(None)
    }
}

/// The `TableConfig` for HTML output (Jupyter), carrying the resolved theme.
/// Color gating doesn't apply: HTML is never a terminal stream, and the
/// theme's `Default` roles render as `currentColor`.
pub fn html_table_config() -> solite_table::TableConfig {
    solite_table::TableConfig::html().with_theme(Some(*theme()))
}

#[cfg(windows)]
pub fn enable_ansi() {
    BufferWriter::stdout(TermcolorChoice::AlwaysAnsi);
}

pub fn style<S: AsRef<str>>(s: S, colorspec: ColorSpec) -> impl fmt::Display {
    if !use_color() {
        return String::from(s.as_ref());
    }
    let mut v = Vec::new();
    let mut ansi_writer = Ansi::new(&mut v);
    ansi_writer.set_color(&colorspec).unwrap();
    ansi_writer.write_all(s.as_ref().as_bytes()).unwrap();
    ansi_writer.reset().unwrap();
    String::from_utf8_lossy(&v).into_owned()
}

pub fn lol<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec
        .set_fg(Some(termcolor::Color::Rgb(234, 118, 203)))
        .set_bold(true);
    style(s, style_spec)
}

pub fn red_bold<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(Red)).set_bold(true);
    style(s, style_spec)
}

pub fn green_bold<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(Green)).set_bold(true);
    style(s, style_spec)
}

pub fn italic<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_italic(true);
    style(s, style_spec)
}

pub fn italic_gray<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(Ansi256(8))).set_italic(true);
    style(s, style_spec)
}

pub fn italic_bold<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_bold(true).set_italic(true);
    style(s, style_spec)
}

pub fn white_on_red<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_bg(Some(Red)).set_fg(Some(White));
    style(s, style_spec)
}

pub fn black_on_green<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_bg(Some(Green)).set_fg(Some(Black));
    style(s, style_spec)
}

pub fn yellow<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(Yellow));
    style(s, style_spec)
}

pub fn cyan<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(Cyan));
    style(s, style_spec)
}
pub fn cyan_bold<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(Cyan)).set_bold(true);
    style(s, style_spec)
}

pub fn magenta<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(Magenta));
    style(s, style_spec)
}
pub fn magenta_bold<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(Magenta)).set_bold(true);
    style(s, style_spec)
}

pub fn red<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(Red));
    style(s, style_spec)
}

pub fn green<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(Green));
    style(s, style_spec)
}

/// The green "✓" used for every "this operation succeeded" status line
/// (run's dot commands, statement completion, backup, vacuum, stream).
pub fn checkmark() -> impl fmt::Display {
    green("✓")
}

pub fn bold<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_bold(true);
    style(s, style_spec)
}

pub fn gray<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(Ansi256(245)));
    style(s, style_spec)
}

/// Matches crossterm's `Color::Grey` (ANSI 37 / 256-color index 7).
pub fn grey<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(Ansi256(7)));
    style(s, style_spec)
}

/// Matches crossterm's `Color::DarkGrey` (ANSI 90 / 256-color index 8).
pub fn dark_gray<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(Ansi256(8)));
    style(s, style_spec)
}

pub fn white<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(White));
    style(s, style_spec)
}

pub fn intense_blue<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec.set_fg(Some(Blue)).set_intense(true);
    style(s, style_spec)
}

pub fn white_bold_on_red<S: AsRef<str>>(s: S) -> impl fmt::Display {
    let mut style_spec = ColorSpec::new();
    style_spec
        .set_bold(true)
        .set_bg(Some(Red))
        .set_fg(Some(White));
    style(s, style_spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_color_flag_equals_form() {
        let args: Vec<String> = ["solite", "run", "--color=always", "x.sql"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(scan_color_flag(&args), clap::ColorChoice::Always);
    }

    #[test]
    fn scan_color_flag_space_form() {
        let args: Vec<String> = ["solite", "--color", "never", "run", "x.sql"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(scan_color_flag(&args), clap::ColorChoice::Never);
    }

    #[test]
    fn scan_color_flag_absent_defaults_auto() {
        let args: Vec<String> = ["solite", "run", "x.sql"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(scan_color_flag(&args), clap::ColorChoice::Auto);
    }

    #[test]
    fn resolve_precedence() {
        assert_eq!(
            resolve(clap::ColorChoice::Always),
            Resolution::Forced(true)
        );
        assert_eq!(
            resolve(clap::ColorChoice::Never),
            Resolution::Forced(false)
        );
    }
}
