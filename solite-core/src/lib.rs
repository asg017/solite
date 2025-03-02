pub mod dot;
pub mod replacement_scans;
pub mod sqlite;

use crate::sqlite::Connection;
use dot::{parse_dot, ParseDotError};
use libsqlite3_sys::{
    sqlite3_db_config, SQLITE_DBCONFIG_DEFENSIVE, SQLITE_DBCONFIG_WRITABLE_SCHEMA,
};
use ropey::Rope;
use solite_stdlib::solite_stdlib_init;
use sqlite::{OwnedValue, SQLiteError, Statement};
use std::{fmt, fmt::Write, path::PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StepError {
    #[error("Error preparing SQL statement:")]
    Prepare {
        file_name: String,
        src: String,
        offset: usize,
        error: SQLiteError,
    },
    #[error("Error parsing dot command: {0}")]
    ParseDot(ParseDotError),
}

/// A "block" of commands to run - can be SQL, PRQL(?) or dot commands.
/// Can come from a SQL file ondisk, a Jupyer cell, REPL, etc.
#[derive(Debug)]
pub struct Block {
    // either file name or "[stdin]" or "[script]" or something
    name: String,
    source: BlockSource,
    contents: String,
    rope: Rope,
    offset: usize,
}

#[derive(Debug)]
pub enum BlockSource {
    File(PathBuf),
    Repl,
    JupyerCell,
}

#[derive(Default)]
pub struct State {
    //timer: bool,
    //bail: bool,
}

pub struct Runtime {
    pub connection: Connection,
    stack: Vec<Block>,
    //state: State,
    initialized_sqlite_parameters_table: bool,
}

pub struct StepReference {
  block_name: String,
  line_number: usize,
  column_number: usize,
}

impl fmt::Display for StepReference {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      write!(f, "{}:{}:{}", self.block_name, self.line_number, self.column_number)
  }
}

pub enum StepResult {
    SqlStatement{stmt: Statement, raw_sql: String},
    DotCommand(dot::DotCommand),
}
pub struct Step {
    /// Dot command or SQL
    pub result: StepResult,


    pub reference: StepReference,
}

fn error_context(error: &SQLiteError, block: &Block) -> Result<String, std::fmt::Error> {
    let mut ctx = String::new();

    let error_offset = error.offset.unwrap_or(0) + block.offset;
    let line_idx_with_error = block.rope.byte_to_line(error_offset);
    let line_error_offset = error_offset - block.rope.line_to_byte(line_idx_with_error);
    let longest_digit_length =
        ((line_idx_with_error + 1 + 4).checked_ilog10().unwrap_or(0) + 1) as usize;
    writeln!(
        &mut ctx,
        "{} ({})",
        error.code_description, error.result_code
    )?;

    for b in (1..4).rev() {
        if let Some(idx) = line_idx_with_error.checked_sub(b) {
            if let Some(line) = block.rope.get_line(idx) {
                writeln!(
                    &mut ctx,
                    "{:longest_digit_length$} | {}",
                    line_idx_with_error + 1 - b,
                    line.as_str().unwrap().trim_end_matches('\n'),
                )?;
            }
        };
    }
    writeln!(
        &mut ctx,
        "{:longest_digit_length$} | {}",
        line_idx_with_error + 1,
        block
            .rope
            .line(line_idx_with_error)
            .as_str()
            .unwrap()
            .trim_end_matches('\n'),
    )?;
    writeln!(
        &mut ctx,
        "{:longest_digit_length$} | {}^ {}",
        " ",
        " ".repeat(line_error_offset),
        error.message,
    )?;
    for b in 1..4 {
        if let Some(line) = block.rope.get_line(line_idx_with_error + b) {
            writeln!(
                &mut ctx,
                "{:longest_digit_length$} | {}",
                line_idx_with_error + 1 + b,
                line.as_str().unwrap().trim_end_matches('\n'),
            )?;
        }
    }

    writeln!(
        &mut ctx,
        "\tat {:longest_digit_length$}:{}:{}",
        match &block.source {
            BlockSource::File(p) => p.to_string_lossy().to_string(),
            BlockSource::JupyerCell => "[cell]".to_owned(),
            BlockSource::Repl => "[repl]".to_owned(),
        },
        line_idx_with_error + 1,
        line_error_offset + 1,
    )?;

    Ok(ctx)
}

impl Runtime {
    pub fn new(path: Option<String>) -> Self {
        unsafe {
            libsqlite3_sys::sqlite3_auto_extension(Some(std::mem::transmute(
                solite_stdlib_init as *const (),
            )));
        }
        let connection = match path {
            Some(path) => Connection::open(path.as_str()).unwrap(),
            None => Connection::open_in_memory().unwrap(),
        };
        Runtime {
            connection,
            stack: vec![],
            //state: State::default(),
            initialized_sqlite_parameters_table: false,
        }
    }
    pub fn enqueue(&mut self, name: &str, code: &str, source: BlockSource) {
        self.stack.push(Block {
            name: name.to_string(),
            source,
            contents: code.to_string(),
            rope: Rope::from_str(code),
            offset: 0,
        });
    }
    
    // temporary for snapshot
    pub fn next_sql_step(&mut self) -> Result<Option<Step>, StepError> {
      let mut current = match self.stack.pop() {
        Some(x) => x,
        None => return Ok(None),
      };
      let full = (current.contents).get(current.offset..).unwrap();
      let code = full.to_owned();
      let local_offset = 0;
      match self.prepare_with_parameters(&code) {
        Ok((rest, Some(stmt))) => {
            let stmt_offset_idx = if (current.offset - local_offset) == 0 {
                0
            } else {
                current.offset - local_offset + 1
            };
            let line_idx = current.rope.byte_to_line(stmt_offset_idx);
            let column_idx = stmt_offset_idx - current.rope.line_to_byte(line_idx);
            let block_name = current.name.clone();
            if let Some(rest) = rest {
                current.offset += rest;
                self.stack.insert(0, current);
            }
            return Ok(Some(Step {
                reference: StepReference {
                  block_name,
                  line_number: line_idx + 1,
                  column_number: column_idx + 1,
                },
                result: StepResult::SqlStatement{stmt, raw_sql: code.to_owned()},
            }));
        }
        Ok((_rest, None)) => {
            return Ok(None)
        }
        Err(error) => todo!(),
    }
    }
    pub fn next_step(&mut self) -> Result<Option<Step>, StepError> {
        // loop here handles:
        // 1. Dot commands
        // 2. Running SQL statements
        // 3. Potential replacement scans
        while let Some(mut current) = self.stack.pop() {
            // "code" that starts with whitespace/comments makes it hard to parse
            // out dot commands correctly. So we advance the offset until we reach
            // the first "real" code.
            //let code = (current.contents).get(current.offset..).unwrap();
            let full = (current.contents).get(current.offset..).unwrap();
            let code = advance_through_ignorable(full).to_owned();
            let local_offset = full.len() - code.len();
            current.offset += local_offset;
            if code.starts_with('.') {
                let end_idx = code.find('\n').unwrap_or(code.len());
                let dot_line = code.get(0..end_idx).unwrap();
                let sep_idx = code.find(' ').unwrap_or(code.len());
                let dot_command = dot_line.get(1..sep_idx).unwrap().trim().to_string();
                let dot_args = dot_line.get(sep_idx..).unwrap().trim().to_string();
                let rest = code.get(end_idx..).unwrap();
                let source = current.name.to_string();
                if !rest.is_empty() {
                    current.offset += end_idx + 1;
                    self.stack.push(current);
                }
                let cmd = parse_dot(dot_command, dot_args).map_err(StepError::ParseDot)?;
                return Ok(Some(Step {
                    reference: StepReference {
                      block_name: source,
                      // TODO: why hardcode here?
                      line_number: 1,
                      column_number: 1,
                    },
                    result: StepResult::DotCommand(cmd),
                }));
            } else {
                match self.prepare_with_parameters(&code) {
                    Ok((rest, Some(stmt))) => {
                        let stmt_offset_idx = if (current.offset - local_offset) == 0 {
                            0
                        } else {
                            current.offset - local_offset + 1
                        };
                        let line_idx = current.rope.byte_to_line(stmt_offset_idx);
                        let column_idx = stmt_offset_idx - current.rope.line_to_byte(line_idx);
                        let block_name = current.name.clone();
                        if let Some(rest) = rest {
                            current.offset += rest;
                            self.stack.insert(0, current);
                        }
                        return Ok(Some(Step {
                            reference: StepReference {
                              block_name,
                              line_number: line_idx + 1,
                              column_number: column_idx + 1,
                            },
                            result: StepResult::SqlStatement{stmt, raw_sql: code.to_owned()},
                        }));
                    }
                    Ok((_rest, None)) => {
                        continue;
                    }
                    Err(error) => {
                        match replacement_scans::replacement_scan(&error, &self.connection) {
                            Some(Ok(stmt)) => {
                                stmt.execute().unwrap();
                                self.stack.push(current);
                            }
                            Some(Err(_)) => todo!(),
                            None => {
                                let context = error_context(&error, &current)
                                    .map_err(|_e| "error_context error???".to_owned())
                                    .unwrap();
                                return Err(StepError::Prepare {
                                    error,
                                    file_name: current.name,
                                    src: current.contents,
                                    offset: current.offset,
                                });
                            }
                        };
                    }
                }
            }
        }

        Ok(None)
    }

    pub fn execute_to_completion(&mut self) -> Result<(), StepError> {
        loop {
            match self.next_step() {
                Ok(None) => return Ok(()),
                Ok(Some(step)) => {
                    match step.result {
                        StepResult::SqlStatement{stmt, ..} => stmt.execute().unwrap(),
                        StepResult::DotCommand(_cmd) => todo!(),
                    };
                    continue;
                }
                Err(err) => return Err(err),
            }
        }
    }
    pub fn has_next(&self) -> bool {
        !self.stack.is_empty()
    }

    fn init_sqlite_parameters_table(&mut self) {
        if self.initialized_sqlite_parameters_table {
            return;
        }
        unsafe {
            let original_writable = 0;
            let original_defensive = 0;
            let db = self.connection.db();
            sqlite3_db_config(db, SQLITE_DBCONFIG_DEFENSIVE, -1, &original_defensive);
            sqlite3_db_config(db, SQLITE_DBCONFIG_DEFENSIVE, 0, 0);
            sqlite3_db_config(db, SQLITE_DBCONFIG_WRITABLE_SCHEMA, -1, &original_writable);
            sqlite3_db_config(db, SQLITE_DBCONFIG_WRITABLE_SCHEMA, 1, 0);
            let result = self.connection.prepare("CREATE TABLE IF NOT EXISTS temp.sqlite_parameters(key TEXT PRIMARY KEY, value) WITHOUT ROWID");
            result.unwrap().1.expect("TODO").execute().unwrap();
            sqlite3_db_config(db, SQLITE_DBCONFIG_DEFENSIVE, original_defensive, 0);
            sqlite3_db_config(db, SQLITE_DBCONFIG_WRITABLE_SCHEMA, original_writable, 0);
        }
        self.initialized_sqlite_parameters_table = true;
        //self.connection.prepare("CREATE TABLE TEMP.sqlite_parameters(key text, value any ) WITHOUT ROWID")
    }

    pub fn define_parameter(&mut self, key: String, value: String) -> Result<(), String> {
        self.init_sqlite_parameters_table();
        let stmt = self
            .connection
            .prepare("INSERT OR REPLACE INTO temp.sqlite_parameters(key, value) VALUES (?1, ?2)")
            .unwrap()
            .1
            .unwrap();
        stmt.bind_text(1, key);
        stmt.bind_text(2, value);
        stmt.execute().unwrap();
        Ok(())
    }

    pub fn prepare_with_parameters(
        &self,
        sql: &str,
    ) -> Result<(Option<usize>, Option<Statement>), SQLiteError> {
        let (rest, stmt) = self.connection.prepare(sql)?;
        if let Some(stmt) = stmt {
            let params = stmt.bind_parameters();
            for (idx, param) in params.iter().enumerate() {
                let param = if let Some(param) = param.strip_prefix(':') {
                    param
                } else if let Some(param) = param.strip_prefix('@') {
                    param
                } else {
                    param
                };
                match self.lookup_parameter(param) {
                    Some(OwnedValue::Text(s)) => {
                        stmt.bind_text((idx + 1) as i32, std::str::from_utf8(&s).unwrap())
                    }
                    Some(OwnedValue::Integer(v)) => stmt.bind_int64((idx + 1) as i32, v),
                    Some(OwnedValue::Double(v)) => stmt.bind_double((idx + 1) as i32, v),
                    Some(OwnedValue::Blob(v)) => stmt.bind_blob((idx + 1) as i32, v.as_ref()),
                    Some(OwnedValue::Null) => stmt.bind_null((idx + 1) as i32),

                    None => (),
                }
            }
            Ok((rest, Some(stmt)))
        } else {
            Ok((rest, stmt))
        }
    }
    fn lookup_parameter<S: AsRef<str>>(&self, key: S) -> Option<OwnedValue> {
        let stmt = self
            .connection
            .prepare("SELECT value FROM temp.sqlite_parameters WHERE key = ?1")
            .ok()?
            .1?;
        stmt.bind_text(1, key);
        stmt.next()
            .unwrap()
            .map(|v| OwnedValue::from_value_ref(v.get(0).unwrap()))
    }
}

pub fn advance_through_ignorable(contents: &str) -> &str {
    let mut chars = contents.char_indices();
    let mut latest = 0;

    while let Some((idx, c)) = chars.next() {
        latest = idx;
        if c.is_whitespace() {
            continue;
        }
        if c == '-' {
            if let Some((_, '-')) = chars.next() {
                loop {
                    match chars.next() {
                        Some((idx, '\n')) => {
                            latest = idx;
                            break;
                        }
                        Some((_, _)) => continue,
                        None => break,
                    }
                }
            } else {
                break;
            }
        } else if c == '/' {
            if let Some((_idx, '*')) = chars.next() {
                loop {
                    match chars.next() {
                        Some((idx, '*')) => match chars.next() {
                            Some((idx, '/')) => {
                                latest = idx;
                                break;
                            }
                            Some((idx, _)) => {
                                latest = idx;
                                continue;
                            }
                            None => {
                                latest = idx;
                                break;
                            }
                        },
                        Some((idx, _)) => {
                            latest = idx;
                            continue;
                        }
                        None => break,
                    }
                }
            } else {
                latest = idx;
                break;
            }
        } else if c == '#' {
            loop {
                match chars.next() {
                    Some((idx, '\n')) => {
                        latest = idx;
                        break;
                    }
                    Some((_, _)) => continue,
                    None => break,
                }
            }
        } else {
            break;
        }
    }
    contents.get(latest..).unwrap()
}
#[cfg(test)]
mod tests {
    use solite_stdlib::BUILTIN_FUNCTIONS;

    use super::*;
    use crate::{dot::DotCommand, sqlite::Connection};

    #[test]
    fn test_advance_through_ignorable() {
        assert_eq!(advance_through_ignorable("4"), "4");
        assert_eq!(advance_through_ignorable("    4"), "4");

        assert_eq!(advance_through_ignorable("--\n4"), "4");
        assert_eq!(advance_through_ignorable("-- skip me\n4"), "4");
        assert_eq!(
            advance_through_ignorable("-- skip me\n  -- 2nd line \n 4"),
            "4"
        );
        assert_eq!(advance_through_ignorable("-\n4"), "-\n4");

        assert_eq!(advance_through_ignorable("#\n4"), "4");
        assert_eq!(advance_through_ignorable("# skip me\n4"), "4");
        assert_eq!(advance_through_ignorable("# skip me\n  #\n 4"), "4");
        assert_eq!(advance_through_ignorable("#\n--\n4"), "4");

        assert_eq!(advance_through_ignorable("/**/4"), "4");
        assert_eq!(advance_through_ignorable("/* skip me */ 4"), "4");
        assert_eq!(advance_through_ignorable("/** skip me */ 4"), "4");
        assert_eq!(advance_through_ignorable("/* skip me **/ 4"), "4");
    }

    fn functions_of(db: &Connection) -> Vec<String> {
        let stmt = db
            .prepare("select distinct name from pragma_function_list order by 1")
            .unwrap()
            .1
            .unwrap();
        let mut fns = vec![];
        while let Ok(Some(row)) = stmt.next() {
            fns.push(row.get(0).unwrap().as_str().to_string());
        }
        fns
    }
    fn version_functions_of(db: &Connection) -> Vec<String> {
        let stmt = db
            .prepare("select distinct name from pragma_function_list where name like '%_version' order by 1")
            .unwrap()
            .1
            .unwrap();
        let mut fns = vec![];
        while let Ok(Some(row)) = stmt.next() {
            fns.push(row.get(0).unwrap().as_str().to_string());
        }
        let mut sql = String::new();
        sql += "select ";
        sql += &fns
            .iter()
            .map(|v| format!("{v}()"))
            .collect::<Vec<String>>()
            .join(", ");
        let stmt = db.prepare(&sql).unwrap().1.unwrap();
        let row = stmt.next().unwrap().unwrap();
        let vers = row.iter().map(|v| v.as_str().to_string());
        assert_eq!(fns.len(), vers.len());
        //fns.iter().zip(vers).collect()
        fns.iter()
            .zip(vers)
            .map(|(func, version)| format!("{func} == {version}"))
            .collect()
    }
    fn modules_of(db: &Connection) -> Vec<String> {
        let stmt = db
            .prepare("select distinct name from pragma_module_list order by 1")
            .unwrap()
            .1
            .unwrap();
        let mut mods = vec![];
        while let Ok(Some(row)) = stmt.next() {
            mods.push(row.first().unwrap().as_str().to_string());
        }
        mods
    }

    #[test]
    fn core_basic() {
        let runtime = Runtime::new(None);
        let stmt = runtime
            .connection
            .prepare("select sqlite_version();")
            .unwrap()
            .1
            .unwrap();
        assert_eq!(
            stmt.next().unwrap().unwrap().get(0).unwrap().as_str(),
            "3.49.1"
        );
        insta::assert_yaml_snapshot!(functions_of(&runtime.connection));
        insta::assert_yaml_snapshot!(modules_of(&runtime.connection));
        insta::assert_yaml_snapshot!(version_functions_of(&runtime.connection));
        insta::assert_yaml_snapshot!(BUILTIN_FUNCTIONS);
    }
    #[test]
    fn core_stack() {
        let mut runtime = Runtime::new(None);
        runtime.enqueue(
            "[input]",
            "create table t(a);
                insert into t select 1;
                insert into t select 2; ",
            BlockSource::File(PathBuf::new()),
        );
        runtime.execute_to_completion().unwrap();
        let binding = runtime
            .connection
            .prepare("select json_group_array(a) from t")
            .unwrap()
            .1
            .unwrap();
        let binding = binding.next().unwrap().unwrap();
        let x = binding.get(0).unwrap().as_str();
        assert_eq!(x, "[1,2]");

        runtime.enqueue(
            "[input]",
            ".print yo!
.print ok
select 4;",
            BlockSource::Repl,
        );

        match runtime.next_step().unwrap().unwrap().result {
            StepResult::DotCommand(cmd) => {
                assert_eq!(
                    cmd,
                    DotCommand::Print(dot::PrintCommand {
                        message: "yo!".to_owned()
                    })
                )
            }
            _ => panic!("fail"),
        };
        match runtime.next_step().unwrap().unwrap().result {
            StepResult::DotCommand(cmd) => {
                assert_eq!(
                    cmd,
                    DotCommand::Print(dot::PrintCommand {
                        message: "ok".to_owned()
                    })
                )
            }
            _ => panic!("fail"),
        };
        match runtime.next_step().unwrap().unwrap().result {
            StepResult::SqlStatement{stmt, ..} => {
                let row = stmt.next().unwrap().unwrap();
                assert_eq!(row.first().unwrap().as_str(), "4");
            }
            _ => panic!("fail"),
        };
        assert!(runtime.next_step().unwrap().is_none())
        /*runtime
        .execute_to_completion(
            "[input]",
            "select * from \"sample.csv\"",
            BlockSource::File(PathBuf::new()),
        )
        .unwrap();*/
    }
}
