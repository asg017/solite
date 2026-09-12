#![allow(dead_code)]

// Borrowed from https://github.com/denoland/deno/blob/main/runtime/colors.rs
use std::fmt;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use solite_theme::Theme;
use terminal_colorsaurus::{QueryOptions, ThemeMode};
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
///
/// `theme_name` (`--theme`/`$SOLITE_THEME`) always wins outright, with no
/// background detection performed at all. Otherwise, when either half of
/// `theme_dark`/`theme_light` (`--theme-dark`/`--theme-light` or their env
/// equivalents) is set, the terminal's background is queried once (OSC 11,
/// via `terminal-colorsaurus`) to pick between them — see
/// [`resolve_theme_pair`]. With neither the explicit theme nor the pair set,
/// this is the plain [`Theme::terminal`] default and no query is ever sent.
pub fn init(
    choice: clap::ColorChoice,
    theme_name: Option<&str>,
    theme_dark: Option<&str>,
    theme_light: Option<&str>,
) {
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

    // `--theme` wins over `$SOLITE_THEME`; the `--theme-dark`/`--theme-light`
    // pair (and its envs) is consulted only when that's absent.
    let explicit = theme_name
        .map(str::to_string)
        .or_else(|| env_var_nonempty("SOLITE_THEME"));
    let dark = theme_dark
        .map(str::to_string)
        .or_else(|| env_var_nonempty("SOLITE_THEME_DARK"));
    let light = theme_light
        .map(str::to_string)
        .or_else(|| env_var_nonempty("SOLITE_THEME_LIGHT"));

    // Lazy by construction: the OSC 11 query only ever runs when it could
    // change the outcome (no explicit theme, and at least one of the pair
    // is configured) — never on the common default path, and never when
    // `--theme` bypasses the pair entirely.
    let detected = if explicit.is_none() && (dark.is_some() || light.is_some()) {
        detect_background_mode()
    } else {
        None
    };
    let name = resolve_theme_pair(explicit.as_deref(), dark.as_deref(), light.as_deref(), detected);

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

fn env_var_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Pure resolution of a `--theme`/`--theme-dark`/`--theme-light` set against
/// a detected terminal background mode (`None` meaning "not queried, or the
/// query failed") into the theme name to load (`None` meaning
/// [`Theme::terminal`]):
///
/// - `explicit` set → that name, unconditionally (the caller never even
///   queries `detected` in this case).
/// - `explicit` unset and both `dark`/`light` unset → `None` (today's
///   default; the caller never queries in this case either).
/// - otherwise, `detected == Some(Light)` picks `light` (or `None` — i.e.
///   `terminal` — if `light` wasn't configured); `Some(Dark)` *or* `None`
///   (detection unavailable/failed) picks `dark` (or `None` if `dark` wasn't
///   configured) — a failed detection is treated the same as a dark
///   terminal, per industry convention (bat, delta, yazi).
fn resolve_theme_pair(
    explicit: Option<&str>,
    dark: Option<&str>,
    light: Option<&str>,
    detected: Option<ThemeMode>,
) -> Option<String> {
    if let Some(name) = explicit {
        return Some(name.to_string());
    }
    if dark.is_none() && light.is_none() {
        return None;
    }
    match detected {
        Some(ThemeMode::Light) => light.map(str::to_string),
        Some(ThemeMode::Dark) | None => dark.map(str::to_string),
    }
}

/// Query the terminal's background via OSC 11 (`terminal-colorsaurus`, the
/// same crate bat and delta use — it sends the query alongside a DA1
/// sentinel so there's no arbitrary timeout on terminals that reply, and it
/// already declines to query at all on `TERM=dumb` or known-unsupported
/// terminals). Returns `None` on any failure: unsupported terminal, no
/// reply, timeout, or — checked here explicitly rather than relying solely
/// on the crate — stdin/stdout not both being a tty (a pipe) or
/// `TERM=dumb`. The caller treats `None` the same as an explicit `Dark`
/// result.
///
/// Only ever called from [`init`], and only when a `--theme-dark`/
/// `--theme-light` pair is actually configured — never on the default
/// no-pair path, so a plain piped `solite query ...` never pays for this.
fn detect_background_mode() -> Option<ThemeMode> {
    if env_is("TERM", "dumb") {
        return None;
    }
    if !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal()) {
        return None;
    }
    terminal_colorsaurus::theme_mode(QueryOptions::default()).ok()
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

/// Best-effort scan of raw argv for `--theme`/`--theme-dark`/`--theme-light`
/// (space or `=` form), used on the fallback paths in `run_main` where clap
/// never produced a `Cli` (bare `solite`, `solite foo.db`). Returns
/// `(theme, theme_dark, theme_light)`.
pub fn scan_theme_flags(args: &[String]) -> (Option<String>, Option<String>, Option<String>) {
    let mut theme = None;
    let mut theme_dark = None;
    let mut theme_light = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(value) = arg.strip_prefix("--theme-dark=") {
            theme_dark = Some(value.to_string());
        } else if arg == "--theme-dark" {
            theme_dark = iter.next().cloned();
        } else if let Some(value) = arg.strip_prefix("--theme-light=") {
            theme_light = Some(value.to_string());
        } else if arg == "--theme-light" {
            theme_light = iter.next().cloned();
        } else if let Some(value) = arg.strip_prefix("--theme=") {
            theme = Some(value.to_string());
        } else if arg == "--theme" {
            theme = iter.next().cloned();
        }
    }
    (theme, theme_dark, theme_light)
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

    #[test]
    fn scan_theme_flags_equals_and_space_forms() {
        let args: Vec<String> = [
            "solite",
            "--theme-dark=mocha",
            "run",
            "--theme-light",
            "latte",
            "x.sql",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        assert_eq!(
            scan_theme_flags(&args),
            (None, Some("mocha".to_string()), Some("latte".to_string()))
        );
    }

    #[test]
    fn scan_theme_flags_plain_theme() {
        let args: Vec<String> = ["solite", "--theme", "terminal"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(
            scan_theme_flags(&args),
            (Some("terminal".to_string()), None, None)
        );
    }

    #[test]
    fn scan_theme_flags_all_absent() {
        let args: Vec<String> = ["solite", "run", "x.sql"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(scan_theme_flags(&args), (None, None, None));
    }

    // Full matrix for `resolve_theme_pair`: explicit-wins, both-unset ->
    // terminal, detection picking each side, a detection failure (`None`)
    // treated as dark, and the "only one side of the pair configured"
    // fallback to terminal when detection points at the *other* side.
    mod resolve_theme_pair_matrix {
        use super::*;

        #[test]
        fn explicit_always_wins() {
            for (dark, light, detected) in [
                (None, None, None),
                (Some("d"), Some("l"), Some(ThemeMode::Light)),
                (Some("d"), None, Some(ThemeMode::Dark)),
            ] {
                assert_eq!(
                    resolve_theme_pair(Some("x"), dark, light, detected),
                    Some("x".to_string())
                );
            }
        }

        #[test]
        fn both_unset_is_terminal_regardless_of_detection() {
            for detected in [None, Some(ThemeMode::Dark), Some(ThemeMode::Light)] {
                assert_eq!(resolve_theme_pair(None, None, None, detected), None);
            }
        }

        #[test]
        fn dark_only_detected_dark() {
            assert_eq!(
                resolve_theme_pair(None, Some("d"), None, Some(ThemeMode::Dark)),
                Some("d".to_string())
            );
        }

        #[test]
        fn dark_only_detection_failed_falls_back_to_dark() {
            assert_eq!(
                resolve_theme_pair(None, Some("d"), None, None),
                Some("d".to_string())
            );
        }

        #[test]
        fn dark_only_detected_light_falls_back_to_terminal() {
            assert_eq!(
                resolve_theme_pair(None, Some("d"), None, Some(ThemeMode::Light)),
                None
            );
        }

        #[test]
        fn light_only_detected_light() {
            assert_eq!(
                resolve_theme_pair(None, None, Some("l"), Some(ThemeMode::Light)),
                Some("l".to_string())
            );
        }

        #[test]
        fn light_only_detected_dark_falls_back_to_terminal() {
            assert_eq!(
                resolve_theme_pair(None, None, Some("l"), Some(ThemeMode::Dark)),
                None
            );
        }

        #[test]
        fn light_only_detection_failed_falls_back_to_terminal() {
            // Failure is treated as "dark", and there's no dark theme
            // configured on this side, so the terminal default wins.
            assert_eq!(resolve_theme_pair(None, None, Some("l"), None), None);
        }

        #[test]
        fn both_set_detected_dark() {
            assert_eq!(
                resolve_theme_pair(None, Some("d"), Some("l"), Some(ThemeMode::Dark)),
                Some("d".to_string())
            );
        }

        #[test]
        fn both_set_detected_light() {
            assert_eq!(
                resolve_theme_pair(None, Some("d"), Some("l"), Some(ThemeMode::Light)),
                Some("l".to_string())
            );
        }

        #[test]
        fn both_set_detection_failed_falls_back_to_dark() {
            assert_eq!(
                resolve_theme_pair(None, Some("d"), Some("l"), None),
                Some("d".to_string())
            );
        }
    }
}
