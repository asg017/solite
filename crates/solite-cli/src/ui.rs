use std::sync::LazyLock;
use std::io::Write;

use cli_table::format::{Border, HorizontalLine, Separator, VerticalLine};
use cli_table::{format::Justify, Cell, CellStruct, Style, Table, TableStruct};
use solite_core::sqlite::{SQLiteError, Statement, ValueRefX, ValueRefXValue, JSON_SUBTYPE};
use termcolor::{Ansi, Color, ColorSpec, WriteColor};

use crate::themes::ctp_mocha_colors;

lazy_static::lazy_static! {
  pub static ref BORDER: Border = Border::builder()
  .top(HorizontalLine::new('┌', '┐', '┬', '─'))
  .bottom(HorizontalLine::new('└', '┘', '┴', '─'))
  .left(VerticalLine::new('│'))
  .right(VerticalLine::new('│'))
  .build();
  static ref COLUMN_SEPARATOR: VerticalLine = VerticalLine::new('│');
  static ref TITLE_SEPARATOR: HorizontalLine = HorizontalLine::new('├', '┤', '┼', '─');
  pub static ref SEPARATOR: Separator =Separator::builder()
  .column(Some(*COLUMN_SEPARATOR))
  .row(Some(HorizontalLine::new('├', '┤', '┼', '─')))
  .title(Some(*TITLE_SEPARATOR))
  .build();
}

pub(crate) struct CliTheme {
    pub null: Color,
    pub integer: Color,
    pub double: Color,
    pub text: Color,
    pub blob: Color,
    pub blue: Color,
    pub green: Color,
}

pub(crate) static CTP_MOCHA_THEME: LazyLock<CliTheme> = LazyLock::new(|| CliTheme {
    null: ctp_mocha_colors::SUBTEXT1.clone().into(),
    integer: ctp_mocha_colors::PEACH.clone().into(),
    double: ctp_mocha_colors::PEACH.clone().into(),
    text: ctp_mocha_colors::TEXT.clone().into(),
    blob: ctp_mocha_colors::TEAL.clone().into(),
    blue: ctp_mocha_colors::BLUE.clone().into(),
    green: ctp_mocha_colors::GREEN.clone().into(),
});

/*
  - [ ] JSON
  - [ ] sqlite-vec types
  - [ ] sqlite-tg types
  - [ ] sqlite-img?
  - [ ] sqlite-html?
  - [ ] pointer types?
*/
pub(crate) fn ui_row(row: &Vec<ValueRefX>, theme: Option<&CliTheme>) -> Vec<CellStruct> {
    let mut ui_row: Vec<CellStruct> = Vec::with_capacity(row.len());
    for valref in row {
        let cell = match valref.value {
            ValueRefXValue::Null => "".cell(),
            ValueRefXValue::Int(value) => value
                .cell()
                .justify(Justify::Right)
                .foreground_color(theme.map(|t| t.integer.clone())),
            ValueRefXValue::Double(value) => value
                .cell()
                .justify(Justify::Right)
                .foreground_color(theme.map(|t| t.double.clone())),
            ValueRefXValue::Text(value) => {
                if valref.subtype().unwrap_or(0) == JSON_SUBTYPE {
                    let contents = unsafe { String::from_utf8_unchecked(value.to_vec()) };
                    let tokens = solite_lexer::json::tokenize(&contents);
                    let mut output = String::new();
                    for token in tokens {
                        match token.kind {
                            solite_lexer::json::Kind::String => {
                                if let Some(t) = theme {
                                    let mut v = Vec::new();
                                    let mut ansi_writer = Ansi::new(&mut v);
                                    let mut colorspec = ColorSpec::new();
                                    if token.string_context == Some(solite_lexer::json::StringContext::Key) {
                                        colorspec.set_fg(Some(t.blue.clone()));
                                    } else {
                                        colorspec.set_fg(Some(t.green.clone()));
                                    }
                                    ansi_writer.set_color(&colorspec).unwrap();
                                    ansi_writer.write_all(token.text.as_bytes()).unwrap();
                                    ansi_writer.reset().unwrap();
                                    output.push_str(&String::from_utf8_lossy(&v));
                                } else {
                                    output.push_str(token.text);
                                }
                            }
                            solite_lexer::json::Kind::Number => {
                                if let Some(t) = theme {
                                    let mut v = Vec::new();
                                    let mut ansi_writer = Ansi::new(&mut v);
                                    let mut colorspec = ColorSpec::new();
                                    colorspec.set_fg(Some(t.integer.clone()));
                                    ansi_writer.set_color(&colorspec).unwrap();
                                    ansi_writer.write_all(token.text.as_bytes()).unwrap();
                                    ansi_writer.reset().unwrap();
                                    output.push_str(&String::from_utf8_lossy(&v));
                                } else {
                                    output.push_str(token.text);
                                }
                            }
                            solite_lexer::json::Kind::Null => {
                                output.push_str("null");
                            }
                            solite_lexer::json::Kind::Eof => {}
                            solite_lexer::json::Kind::LBrace => output.push_str("{"),
                            solite_lexer::json::Kind::RBrace => output.push_str("}"),
                            solite_lexer::json::Kind::LBracket => output.push_str("["),
                            solite_lexer::json::Kind::RBracket => output.push_str("]"),
                            solite_lexer::json::Kind::Colon => output.push_str(":"),
                            solite_lexer::json::Kind::Comma => output.push_str(","),
                            solite_lexer::json::Kind::True => output.push_str("true"),
                            solite_lexer::json::Kind::False => output.push_str("false"),
                            solite_lexer::json::Kind::Whitespace => output.push_str(" "),
                            solite_lexer::json::Kind::Unknown => todo!(),
                        }
                    }
                    output.cell().justify(Justify::Center)
                } else {
                    (unsafe { String::from_utf8_unchecked(value.to_vec()) })
                        .cell()
                        .justify(Justify::Left)
                        .foreground_color(theme.map(|t| t.text.clone()))
                }
            }
            ValueRefXValue::Blob(value) => format!("Blob<{}>", value.len())
                .cell()
                .justify(Justify::Center)
                .foreground_color(theme.map(|t| t.blob.clone())),
        };
        ui_row.push(cell);
    }
    ui_row
}

pub(crate) fn ui_table(columns: Vec<String>, ui_rows: Vec<Vec<CellStruct>>) -> TableStruct {
    ui_rows
        .table()
        .title(columns.iter().map(crate::colors::bold))
        .border(*BORDER)
        .separator(*SEPARATOR)
}

pub(crate) fn table_from_statement(
    stmt: &Statement,
    theme: Option<&CliTheme>,
) -> Result<Option<TableStruct>, SQLiteError> {
    let num_display_rows = match term_size::dimensions() {
        Some((_w, h)) => {
            h
              - 1 // TODO
              - 1 // TODO
              - 1 // TODO
              - 1 // TODO
        }
        None => 20,
    };

    let columns = stmt.column_names().unwrap();
    let mut ui_rows = vec![];
    loop {
        match stmt.next() {
            Ok(Some(row)) => ui_rows.push(ui_row(&row, theme)),
            Ok(None) => break,
            Err(error) => {
                return Err(error);
            }
        }
    }
    if columns.is_empty() {
        Ok(None)
    } else {
        Ok(Some(ui_table(columns, ui_rows)))
    }
}
