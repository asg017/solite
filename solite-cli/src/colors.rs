#![allow(dead_code)]

// Borrowed from https://github.com/denoland/deno/blob/main/runtime/colors.rs
use std::fmt;
use std::io::Write;
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
use termcolor::ColorChoice;

pub fn is_tty() -> bool {
    true
}

pub fn use_color() -> bool {
    true
}

#[cfg(windows)]
pub fn enable_ansi() {
    BufferWriter::stdout(ColorChoice::AlwaysAnsi);
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
