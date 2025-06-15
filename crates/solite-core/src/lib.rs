pub mod dot;
pub mod load_uv;
pub mod replacement_scans;
pub mod sqlite;
pub mod exporter;

use crate::dot::{DotCommand, ShellCommand};
use crate::sqlite::Connection;
use dot::{parse_dot, ParseDotError};
use libsqlite3_sys::{
    sqlite3_db_config, SQLITE_DBCONFIG_DEFENSIVE, SQLITE_DBCONFIG_WRITABLE_SCHEMA,
};
use regex::Regex;
use ropey::Rope;
use serde::{Deserialize, Serialize};
use solite_stdlib::solite_stdlib_init;
use sqlite::{OwnedValue, SQLiteError, Statement};
use std::sync::LazyLock;
use std::{fmt, path::PathBuf};
use thiserror::Error;

static SQL_COMMENT_REGION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*--\s*#region\s+(\w*)").unwrap());
static SQL_COMMENT_ENDREGION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*--\s*#endregion").unwrap());

fn sql_comment_region_name(sql: &str) -> Option<&str> {
    //SQL_COMMENT_REGION.captures_at(sql, 1).map(|x| x)
    SQL_COMMENT_REGION
        .captures(sql)
        .and_then(|captures| captures.get(1).map(|m| m.as_str()))
}

#[derive(Serialize, Deserialize, Error, Debug)]
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
    regions: Vec<String>,
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

#[derive(Serialize, Deserialize, Debug)]
pub struct StepReference {
    block_name: String,
    line_number: usize,
    column_number: usize,
    pub region: Vec<String>,
}

impl fmt::Display for StepReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.block_name, self.line_number, self.column_number
        )
    }
}

#[derive(Serialize, Debug)]
pub enum StepResult {
    SqlStatement { stmt: Statement, raw_sql: String },
    DotCommand(dot::DotCommand),
}

#[derive(Serialize, Debug)]

pub struct Step {
    preamble: Option<String>,
    /// Dot command or SQL
    pub result: StepResult,

    pub reference: StepReference,
}

fn extract_preamble(code: &str) -> (&str, Option<&str>) {
    let codex = advance_through_ignorable(code);
    let preamble_offset = unsafe { codex.as_ptr().offset_from(code.as_ptr()) } as usize;
    if preamble_offset > 0 {
        (codex, Some(&code[0..preamble_offset]))
    } else {
        (code, None)
    }
}

impl Runtime {
    pub fn new(path: Option<String>) -> Self {
        unsafe {
            //libsqlite3_sys::sqlite3_auto_extension(Some(std::mem::transmute(solite_stdlib_init as *const (),)));
        }
        let connection = match path {
            Some(path) => Connection::open(path.as_str()).unwrap(),
            None => Connection::open_in_memory().unwrap(),
        };
        unsafe {
            solite_stdlib_init(connection.db(), std::ptr::null_mut(), std::ptr::null_mut());
        }
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
            regions: vec![],
        });
    }
    pub fn next_stepx(&mut self) -> Option<Result<Step, StepError>> {
        while let Some(mut block) = self.stack.pop() {
            let regions = block.regions.clone();
            let current = match (&block.contents).get(block.offset..) {
                Some(code) => code,
                None => return None,
            };
            let (code, preamble) = extract_preamble(current);
            if let Some(preamble) = preamble {
                block.offset += code.as_ptr() as usize - preamble.as_ptr() as usize;
                /*if !preamble.is_empty() {
                }*/
                for line in preamble.lines() {
                    if let Some(region) = sql_comment_region_name(line) {
                        block.regions.push(region.to_string());
                    } else if SQL_COMMENT_ENDREGION.is_match(line) {
                        block.regions.pop();
                    }
                }
            }

            if code.starts_with('.') || code.starts_with("!") {
                let end_idx = code.find('\n').unwrap_or(code.len());
                let dot_line = code.get(0..end_idx).unwrap();
                let source = block.name.to_string();
                let rest: &str = code.get(end_idx..).unwrap();
                    

                let cmd = if !code.starts_with('!') {
                    let sep_idx = dot_line.find(' ').unwrap_or(dot_line.len());
                    let dot_command = dot_line.get(1..sep_idx).unwrap().trim().to_string();
                    let dot_args = dot_line.get(sep_idx..).unwrap().trim().to_string();
                    
                    match parse_dot(dot_command, dot_args, rest, self) {
                        Ok(cmd) => {
                          match &cmd {
                            DotCommand::Export(cmd) => {
                              block.offset +=  cmd.rest_length;
                            }
                            DotCommand::Vegalite(cmd) => {
                              block.offset +=  cmd.rest_length;
                            }
                            _ => (),
                          }
                          cmd
                        },
                        Err(err) => {
                            return Some(Err(StepError::ParseDot(err)));
                      }
                    }
                } else {
                    DotCommand::Shell(ShellCommand {
                        command: dot_line.get(1..end_idx).unwrap().trim().to_string(),
                    })
                };

                if !rest.is_empty() {
                      block.offset +=  dot_line.len(); //preamble preamble_offset + end_idx + 1;
                      self.stack.push(block);
                  }

                return Some(Ok(Step {
                    preamble: None,
                    reference: StepReference {
                        block_name: source,
                        // TODO: why hardcode here?
                        line_number: 1,
                        column_number: 1,
                        region: regions,
                    },
                    result: StepResult::DotCommand(cmd),
                }));
            }

            match self.prepare_with_parameters(&code) {
                Ok((rest, Some(stmt))) => {
                    let stmt_offset_idx = block.offset; // + preamble.map_or(0, |p| p.len()) + 1;
                    let block_name = block.name.clone();
                    let line_idx: usize = block.rope.byte_to_line(stmt_offset_idx);
                    let column_idx = stmt_offset_idx - block.rope.line_to_byte(line_idx);
                    let raw_sql = stmt.sql(); //code.to_owned();
                    let preamble_owned = preamble.map(|p| p.to_string());

                    if let Some(rest) = rest {
                        block.offset += rest; // + preamble.map_or(0, |p| p.len()) + 1;
                        self.stack.insert(0, block);
                    }

                    return Some(Ok(Step {
                        preamble: preamble_owned,
                        reference: StepReference {
                            block_name,
                            line_number: line_idx + 1,
                            column_number: column_idx + 1,
                            region: regions,
                        },
                        result: StepResult::SqlStatement { stmt, raw_sql },
                    }));
                }
                Ok((_rest, None)) => {
                    return None;
                }
                Err(error) => {
                    match replacement_scans::replacement_scan(&error, &self.connection) {
                        Some(Ok(stmt)) => {
                            stmt.execute().unwrap();
                            self.stack.push(block);
                        }
                        Some(Err(_)) => todo!(),
                        None => {
                            return Some(Err(StepError::Prepare {
                                error,
                                file_name: block.name,
                                src: block.contents,
                                offset: block.offset,
                            }));
                        }
                    };
                }
            }
        }
        None
    }

    pub fn execute_to_completion(&mut self) -> Result<(), StepError> {
        loop {
            match self.next_stepx() {
                None => return Ok(()),
                Some(Ok(step)) => {
                    match step.result {
                        StepResult::SqlStatement { stmt, .. } => stmt.execute().unwrap(),
                        StepResult::DotCommand(_cmd) => todo!(),
                    };
                    continue;
                }
                Some(Err(err)) => return Err(err),
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
    use insta::{assert_debug_snapshot, assert_yaml_snapshot};
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
            "3.50.1"
        );
        insta::assert_yaml_snapshot!(functions_of(&runtime.connection));
        insta::assert_yaml_snapshot!(modules_of(&runtime.connection));
        insta::assert_yaml_snapshot!(version_functions_of(&runtime.connection));
        insta::assert_yaml_snapshot!(BUILTIN_FUNCTIONS);
    }
    
    #[test]
    fn snap2() {
        let mut rt = Runtime::new(None);
        rt.enqueue(
            "[input]",
            "
-- preamble1
select 1;
select 2;
-- another preamble
select 3.1;select 3.2;
/* inline! */ select 4;

-- what!
select not_exist();",
            BlockSource::File(PathBuf::new()),
        );
        let mut idx = 0;
        loop {
            let step = rt.next_stepx();
            assert_yaml_snapshot!(format!("step-{idx}"), step);
            idx += 1;
            match step {
                None => break,
                _ => (),
            }
        }
    }
}
