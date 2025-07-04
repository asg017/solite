use cli_table::format::{Border, HorizontalLine, Separator, VerticalLine};
use cli_table::{format::Justify, Cell, CellStruct, Style, Table, TableStruct};
use solite_core::sqlite::{SQLiteError, Statement, ValueRefX, ValueRefXValue};
use termcolor::Color;

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
  .row(None)
  .title(Some(*TITLE_SEPARATOR))
  .build();
}

const COLOR_NUMBER: Color = Color::Rgb(250, 179, 135);
const COLOR_BLOB: Color = Color::Rgb(137, 220, 235);

/*
  - [ ] JSON
  - [ ] sqlite-vec types
  - [ ] sqlite-tg types
  - [ ] sqlite-img?
  - [ ] sqlite-html?
  - [ ] pointer types?
*/
pub(crate) fn ui_row(row: &Vec<ValueRefX>, color: bool) -> Vec<CellStruct> {
    let mut ui_row: Vec<CellStruct> = Vec::with_capacity(row.len());
    for value in row {
        let cell = match value.value {
            ValueRefXValue::Null => "".cell(),
            ValueRefXValue::Int(value) => value
                .cell()
                .justify(Justify::Right)
                .foreground_color(if color { Some(COLOR_NUMBER) } else { None }),
            ValueRefXValue::Double(value) => value
                .cell()
                .justify(Justify::Right)
                .foreground_color(if color { Some(COLOR_NUMBER) } else { None }),
            ValueRefXValue::Text(value) => (unsafe { String::from_utf8_unchecked(value.to_vec()) })
                .cell()
                .justify(Justify::Left)
                .foreground_color(None),
            ValueRefXValue::Blob(value) => format!("Blob<{}>", value.len())
                .cell()
                .justify(Justify::Center)
                .foreground_color(if color { Some(COLOR_BLOB) } else { None }),
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
    color: bool,
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
            Ok(Some(row)) => ui_rows.push(ui_row(&row, color)),
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
