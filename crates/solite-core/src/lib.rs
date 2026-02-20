pub mod dot;
pub mod procedure;
pub mod replacement_scans;
pub mod sqlite;
pub mod exporter;

use crate::dot::{DotCommand, ShellCommand, AskCommand};
use crate::procedure::Procedure;
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
use std::collections::HashMap;
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
    _source: BlockSource,
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
    CommandFlag,
    Stdin,
}

#[derive(Default)]
pub struct State {
    //timer: bool,
    //bail: bool,
}

/// Saved state for a `.run` invocation, used to restore the runtime afterwards.
#[derive(Debug)]
pub struct SavedRunState {
    stack: Vec<Block>,
    saved_params: Vec<(String, Option<OwnedValue>)>,
}

pub struct Runtime {
    pub connection: Connection,
    stack: Vec<Block>,
    //state: State,
    initialized_sqlite_parameters_table: bool,
    procedures: HashMap<String, Procedure>,
    loaded_files: std::collections::HashSet<String>,
    virtual_files: HashMap<String, String>,
    running_files: Vec<String>,
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
    ProcedureDefinition(Procedure),
}

#[derive(Serialize, Debug)]

pub struct Step {
    pub preamble: Option<String>,
    pub epilogue: Option<String>,
    /// Dot command or SQL
    pub result: StepResult,

    pub reference: StepReference,
}

fn extract_epilogue(code: &str, rest_index: usize) -> Option<String> {
    if rest_index >= code.len() {
        return None;
    }
    let rest = &code[rest_index..];
    let mut first_non_ws_idx: Option<usize> = None;
    for (idx, c) in rest.char_indices() {
        if c == '\n' {
            // newline before any comment -> not an epilogue
            return None;
        }
        if c.is_whitespace() {
            continue;
        }
        first_non_ws_idx = Some(idx);
        break;
    }
    let idx = first_non_ws_idx?;
    let rem = &rest[idx..];
    if rem.starts_with("--") {
        // include leading whitespace before comment
        if let Some(pos) = rem.find('\n') {
            return Some(rest[..idx + pos].to_string());
        } else {
            return Some(rest.to_string());
        }
    }
    if rem.starts_with("/*") {
        if let Some(endpos) = rem.find("*/") {
            let end = endpos + 2;
            return Some(rest[..idx + end].to_string());
        } else {
            // unterminated block comment: take rest
            return Some(rest.to_string());
        }
    }
    None
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
            procedures: HashMap::new(),
            loaded_files: std::collections::HashSet::new(),
            virtual_files: HashMap::new(),
            running_files: Vec::new(),
        }
    }

    pub fn new_readonly(path: &str) -> Self {
        let connection = Connection::open_readonly(path).unwrap();
        unsafe {
            solite_stdlib_init(connection.db(), std::ptr::null_mut(), std::ptr::null_mut());
        }
        Runtime {
            connection,
            stack: vec![],
            initialized_sqlite_parameters_table: false,
            procedures: HashMap::new(),
            loaded_files: std::collections::HashSet::new(),
            virtual_files: HashMap::new(),
            running_files: Vec::new(),
        }
    }

    pub fn enqueue(&mut self, name: &str, code: &str, source: BlockSource) {
        self.stack.push(Block {
            name: name.to_string(),
            _source: source,
            contents: code.to_string(),
            rope: Rope::from_str(code),
            offset: 0,
            regions: vec![],
        });
    }
    pub fn add_virtual_file(&mut self, path: &str, content: &str) {
        self.virtual_files.insert(path.to_string(), content.to_string());
    }

    pub fn read_file(&self, path: &str) -> Result<String, String> {
        if let Some(content) = self.virtual_files.get(path) {
            return Ok(content.clone());
        }
        std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read '{}': {}", path, e))
    }

    pub fn register_procedure(&mut self, proc: Procedure) {
        self.procedures.insert(proc.name.clone(), proc);
    }

    pub fn get_procedure(&self, name: &str) -> Option<&Procedure> {
        self.procedures.get(name)
    }

    pub fn procedures(&self) -> &HashMap<String, Procedure> {
        &self.procedures
    }

    pub fn next_stepx(&mut self) -> Option<Result<Step, StepError>> {
        while let Some(mut block) = self.stack.pop() {
            let regions = block.regions.clone();
            let current = block.contents.get(block.offset..)?;
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

            if code.starts_with('.') || code.starts_with("!")  || code.starts_with('?') {
                let end_idx = code.find('\n').unwrap_or(code.len());
                let dot_line = code.get(0..end_idx).unwrap();
                let source = block.name.to_string();
                let rest: &str = code.get(end_idx..).unwrap();
                    

                let mut cmd = if code.starts_with('!') {
                  DotCommand::Shell(ShellCommand {
                        command: dot_line.get(1..end_idx).unwrap().trim().to_string(),
                    })
                }else if code.starts_with('?') {
                  DotCommand::Ask(AskCommand {
                        message: dot_line.get(1..end_idx).unwrap().trim().to_string(),
                    })
                }
                else {
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
                            DotCommand::Bench(cmd) => {
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
                };

                // Resolve .call to a SqlStatement so test/snapshot/run all
                // get a prepared statement they already know how to handle.
                if let DotCommand::Call(ref call_cmd) = cmd {
                    let stmt_offset_idx = block.offset;
                    let block_name = block.name.clone();
                    let line_idx: usize = block.rope.byte_to_line(stmt_offset_idx);
                    let column_idx = stmt_offset_idx - block.rope.line_to_byte(line_idx);

                    // Extract epilogue from the dot_line (e.g. ".call proc -- 4" → "-- 4")
                    let epilogue_owned = dot_line.find(" --").map(|idx| dot_line[idx..].to_string());

                    if !rest.is_empty() {
                        block.offset += dot_line.len();
                        self.stack.push(block);
                    }

                    // Load file if specified, resolving relative to the calling file's directory
                    if let Some(ref file) = call_cmd.file {
                        let resolved = PathBuf::from(&block_name)
                            .parent()
                            .map(|dir| dir.join(file))
                            .unwrap_or_else(|| PathBuf::from(file));
                        let resolved_str = resolved.to_string_lossy().to_string();
                        if let Err(e) = self.load_file(&resolved_str) {
                            return Some(Err(StepError::ParseDot(
                                dot::ParseDotError::InvalidArgument(e),
                            )));
                        }
                    }

                    // Look up and prepare the procedure
                    let proc = match self.get_procedure(&call_cmd.procedure_name) {
                        Some(p) => p.clone(),
                        None => {
                            return Some(Err(StepError::ParseDot(
                                dot::ParseDotError::InvalidArgument(format!(
                                    "Unknown procedure: '{}'",
                                    call_cmd.procedure_name
                                )),
                            )));
                        }
                    };

                    match self.prepare_with_parameters(&proc.sql) {
                        Ok((_, Some(stmt))) => {
                            let raw_sql = stmt.sql();
                            return Some(Ok(Step {
                                preamble: None,
                                epilogue: epilogue_owned,
                                reference: StepReference {
                                    block_name,
                                    line_number: line_idx + 1,
                                    column_number: column_idx + 1,
                                    region: regions,
                                },
                                result: StepResult::SqlStatement { stmt, raw_sql },
                            }));
                        }
                        Ok((_, None)) => {
                            return Some(Err(StepError::ParseDot(
                                dot::ParseDotError::InvalidArgument(format!(
                                    "Procedure '{}' prepared to empty statement",
                                    call_cmd.procedure_name
                                )),
                            )));
                        }
                        Err(error) => {
                            return Some(Err(StepError::Prepare {
                                error,
                                file_name: block_name,
                                src: String::new(),
                                offset: 0,
                            }));
                        }
                    }
                }

                // Resolve .run file path relative to the calling file's directory
                if let DotCommand::Run(ref mut run_cmd) = cmd {
                    let resolved = PathBuf::from(&block.name)
                        .parent()
                        .map(|dir| dir.join(&run_cmd.file))
                        .unwrap_or_else(|| PathBuf::from(&run_cmd.file));
                    run_cmd.file = resolved.to_string_lossy().to_string();
                }

                if !rest.is_empty() {
                      block.offset +=  dot_line.len(); //preamble preamble_offset + end_idx + 1;
                      self.stack.push(block);
                  }

                return Some(Ok(Step {
                    preamble: None,
                    epilogue: None,
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

            match self.prepare_with_parameters(code) {
                Ok((rest, Some(stmt))) => {
                    let stmt_offset_idx = block.offset; // + preamble.map_or(0, |p| p.len()) + 1;
                    let block_name = block.name.clone();
                    let line_idx: usize = block.rope.byte_to_line(stmt_offset_idx);
                    let column_idx = stmt_offset_idx - block.rope.line_to_byte(line_idx);
                    let raw_sql = stmt.sql(); //code.to_owned();
                    let preamble_owned = preamble.map(|p| p.to_string());
                    let epilogue_owned = rest.and_then(|r| extract_epilogue(code, r));

                    if let Some(rest) = rest {
                        block.offset += rest; // + preamble.map_or(0, |p| p.len()) + 1;
                        self.stack.insert(0, block);
                    }

                    // Check if this is a procedure definition
                    // The preamble may contain multiple comment lines (regions, separators, etc.)
                    // so we look for a `-- name:` line anywhere in the preamble.
                    if let Some(ref preamble_str) = preamble_owned {
                        let name_line = preamble_str
                            .lines()
                            .map(|l| l.trim())
                            .find(|l| l.starts_with("-- name:"));
                        if let Some(name_line) = name_line {
                            if let Some((name, annotations)) = procedure::parse_name_line(name_line) {
                                let columns = stmt.column_meta();
                                let parameters: Vec<_> = stmt
                                    .parameter_info()
                                    .iter()
                                    .map(|p| procedure::parse_parameter(p))
                                    .collect();
                                let result_type = procedure::determine_result_type(&annotations, columns.len());

                                let proc = Procedure {
                                    name: name.clone(),
                                    sql: stmt.sql(),
                                    result_type,
                                    parameters,
                                    columns,
                                };
                                self.procedures.insert(name, proc.clone());

                                return Some(Ok(Step {
                                    preamble: preamble_owned,
                                    epilogue: epilogue_owned,
                                    reference: StepReference {
                                        block_name,
                                        line_number: line_idx + 1,
                                        column_number: column_idx + 1,
                                        region: regions,
                                    },
                                    result: StepResult::ProcedureDefinition(proc),
                                }));
                            }
                        }
                    }

                    return Some(Ok(Step {
                        preamble: preamble_owned,
                        epilogue: epilogue_owned,
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

    #[allow(clippy::result_large_err)]
    pub fn execute_to_completion(&mut self) -> Result<(), StepError> {
        loop {
            match self.next_stepx() {
                None => return Ok(()),
                Some(Ok(step)) => {
                    match step.result {
                        StepResult::SqlStatement { stmt, .. } => { stmt.execute().unwrap(); }
                        StepResult::DotCommand(_cmd) => todo!(),
                        StepResult::ProcedureDefinition(_) => { /* already registered */ }
                    }
                    continue;
                }
                Some(Err(err)) => return Err(err),
            }
        }
    }
    /// Load and execute a SQL file, registering any procedures it defines.
    /// Setup statements (CREATE TABLE, INSERT, etc.) are executed immediately.
    pub fn load_file(&mut self, path: &str) -> Result<(), String> {
        if self.loaded_files.contains(path) {
            return Ok(());
        }
        let content = self.read_file(path)?;
        let path_buf = PathBuf::from(path);
        self.loaded_files.insert(path.to_string());
        // Temporarily save and clear the stack so next_stepx() only processes
        // the loaded file (the stack inserts blocks at position 0 after SQL
        // statements, which would interleave with the caller's blocks).
        let saved_stack: Vec<Block> = self.stack.drain(..).collect();
        self.enqueue(path, &content, BlockSource::File(path_buf));
        let result = loop {
            match self.next_stepx() {
                None => break Ok(()),
                Some(Ok(step)) => match step.result {
                    StepResult::SqlStatement { stmt, .. } => {
                        if let Err(e) = stmt.execute() {
                            break Err(format!("Error executing '{}': {:?}", path, e));
                        }
                    }
                    StepResult::ProcedureDefinition(_) => { /* already registered */ }
                    StepResult::DotCommand(_) => { /* skip dot commands in loaded files */ }
                },
                Some(Err(err)) => break Err(format!("Error loading '{}': {}", path, err)),
            }
        };
        self.stack.extend(saved_stack);
        result
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
    pub fn lookup_parameter<S: AsRef<str>>(&self, key: S) -> Option<OwnedValue> {
        let stmt = self
            .connection
            .prepare("SELECT value FROM temp.sqlite_parameters WHERE key = ?1")
            .ok()?
            .1?;
        stmt.bind_text(1, key);
        stmt.next()
            .unwrap()
            .map(|v| OwnedValue::from_value_ref(v.first().unwrap()))
    }

    pub fn delete_parameter(&mut self, key: &str) {
        self.init_sqlite_parameters_table();
        let stmt = self
            .connection
            .prepare("DELETE FROM temp.sqlite_parameters WHERE key = ?1")
            .unwrap()
            .1
            .unwrap();
        stmt.bind_text(1, key);
        stmt.execute().unwrap();
    }

    /// Begin a `.run` invocation: check for cycles, read the file,
    /// set parameters, save and clear the stack, and enqueue the file.
    pub fn run_file_begin(
        &mut self,
        path: &str,
        params: &HashMap<String, String>,
    ) -> Result<SavedRunState, String> {
        // Cycle detection
        if self.running_files.contains(&path.to_string()) {
            let mut cycle = self.running_files.clone();
            cycle.push(path.to_string());
            return Err(format!(
                "Recursive .run cycle detected: {}",
                cycle.join(" -> ")
            ));
        }

        let content = self.read_file(path)?;
        self.running_files.push(path.to_string());

        // Save current param values and set new ones
        let mut saved_params = Vec::new();
        for (key, value) in params {
            let old_value = self.lookup_parameter(key);
            saved_params.push((key.clone(), old_value));
            self.define_parameter(key.clone(), value.clone()).unwrap();
        }

        // Save and clear the stack
        let saved_stack: Vec<Block> = self.stack.drain(..).collect();

        // Enqueue the file
        let path_buf = PathBuf::from(path);
        self.enqueue(path, &content, BlockSource::File(path_buf));

        Ok(SavedRunState {
            stack: saved_stack,
            saved_params,
        })
    }

    /// End a `.run` invocation: restore parameters and the stack.
    pub fn run_file_end(&mut self, saved: SavedRunState) {
        // Pop from running_files
        self.running_files.pop();

        // Restore parameters
        for (key, old_value) in saved.saved_params {
            match old_value {
                Some(OwnedValue::Text(s)) => {
                    self.define_parameter(key, std::str::from_utf8(&s).unwrap().to_string()).unwrap();
                }
                Some(OwnedValue::Integer(v)) => {
                    self.define_parameter(key, v.to_string()).unwrap();
                }
                Some(OwnedValue::Double(v)) => {
                    self.define_parameter(key, v.to_string()).unwrap();
                }
                Some(_) => {
                    self.delete_parameter(&key);
                }
                None => {
                    self.delete_parameter(&key);
                }
            }
        }

        // Restore the stack
        self.stack.extend(saved.stack);
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
    use insta::assert_yaml_snapshot;
    use solite_stdlib::BUILTIN_FUNCTIONS;

    use super::*;
    use crate::dot::DotCommand;
    use crate::sqlite::Connection;

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
            fns.push(row.first().unwrap().as_str().to_string());
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
            fns.push(row.first().unwrap().as_str().to_string());
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
            stmt.next().unwrap().unwrap().first().unwrap().as_str(),
            "3.52.0"
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
            if step.is_none() {
                break;
            }
        }
    }

    #[test]
    fn test_extract_epilogue_line_comment_same_line() {
        let code = "select 1; -- epilogue\nselect 2;";
        let rest_index = "select 1;".len();
        let ep = super::extract_epilogue(code, rest_index);
        assert_eq!(ep, Some(" -- epilogue".to_string()));
    }

    #[test]
    fn test_extract_epilogue_block_comment_same_line() {
        let code = "select 1; /* block */\nselect 2;";
        let rest_index = "select 1;".len();
        let ep = super::extract_epilogue(code, rest_index);
        assert_eq!(ep, Some(" /* block */".to_string()));
    }

    #[test]
    fn test_no_epilogue_newline_before_comment() {
        let code = "select 1;\n-- not an epilogue\n";
        let rest_index = "select 1;".len();
        let ep = super::extract_epilogue(code, rest_index);
        assert_eq!(ep, None);
    }

    #[test]
    fn test_extract_epilogue_no_space_before_comment() {
        let code = "select 1;--tight\n";
        let rest_index = "select 1;".len();
        let ep = super::extract_epilogue(code, rest_index);
        assert_eq!(ep, Some("--tight".to_string()));
    }

    #[test]
    fn test_new_readonly_allows_reads() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();

        // Seed a database
        {
            let rt = Runtime::new(Some(db_str.to_string()));
            rt.connection
                .execute_script("CREATE TABLE t(x TEXT); INSERT INTO t VALUES ('hi');")
                .unwrap();
        }

        // Open readonly and read
        let rt = Runtime::new_readonly(db_str);
        let (_, stmt) = rt.connection.prepare("SELECT x FROM t").unwrap();
        let stmt = stmt.unwrap();
        let row = stmt.next().unwrap().unwrap();
        assert_eq!(row.first().unwrap().as_str(), "hi");
    }

    #[test]
    fn test_new_readonly_blocks_writes() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();

        // Seed a database
        {
            let rt = Runtime::new(Some(db_str.to_string()));
            rt.connection
                .execute_script("CREATE TABLE t(x TEXT)")
                .unwrap();
        }

        // Open readonly and try to write
        let rt = Runtime::new_readonly(db_str);
        let result = rt.connection.execute_script("INSERT INTO t VALUES ('nope')");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .message
                .contains("readonly")
        );
    }

    #[test]
    fn test_virtual_file_read() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/test.sql", "select 42;");
        assert_eq!(rt.read_file("/test.sql").unwrap(), "select 42;");
    }

    #[test]
    fn test_virtual_file_fallback() {
        let rt = Runtime::new(None);
        let result = rt.read_file("/nonexistent_file_12345.sql");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_file_uses_virtual_fs() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/helper.sql", "create table vtest(x); insert into vtest values (99);");
        rt.load_file("/helper.sql").unwrap();
        let (_, stmt) = rt.connection.prepare("select x from vtest").unwrap();
        let stmt = stmt.unwrap();
        let row = stmt.next().unwrap().unwrap();
        assert_eq!(row.first().unwrap().as_str(), "99");
    }

    #[test]
    fn test_run_file_begin_reads_virtual_file() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/test.sql", "select 1;");
        let saved = rt.run_file_begin("/test.sql", &HashMap::new()).unwrap();
        // Stack should have one block (the file)
        assert!(!rt.stack.is_empty());
        rt.run_file_end(saved);
    }

    #[test]
    fn test_run_file_begin_cycle_self() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/self.sql", ".run /self.sql");
        rt.running_files.push("/self.sql".to_string());
        let result = rt.run_file_begin("/self.sql", &HashMap::new());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cycle"));
    }

    #[test]
    fn test_run_file_begin_cycle_mutual() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/a.sql", ".run /b.sql");
        rt.add_virtual_file("/b.sql", ".run /a.sql");
        rt.running_files.push("/a.sql".to_string());
        rt.running_files.push("/b.sql".to_string());
        let result = rt.run_file_begin("/a.sql", &HashMap::new());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("/a.sql"));
        assert!(err.contains("/b.sql"));
    }

    #[test]
    fn test_run_file_begin_cycle_deep() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/a.sql", "");
        rt.running_files.push("/a.sql".to_string());
        rt.running_files.push("/b.sql".to_string());
        rt.running_files.push("/c.sql".to_string());
        let result = rt.run_file_begin("/a.sql", &HashMap::new());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("/a.sql -> /b.sql -> /c.sql -> /a.sql"));
    }

    #[test]
    fn test_run_file_end_restores_stack() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/test.sql", "select 1;");
        // Set up some existing stack
        rt.enqueue("[outer]", "select 'outer';", BlockSource::Repl);
        let saved = rt.run_file_begin("/test.sql", &HashMap::new()).unwrap();
        // Stack now has the file, outer was saved
        assert_eq!(rt.stack.len(), 1);
        // Drain the file's steps
        while rt.next_stepx().is_some() {}
        rt.run_file_end(saved);
        // Stack should have the outer block restored
        assert_eq!(rt.stack.len(), 1);
    }

    #[test]
    fn test_run_file_end_restores_params() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/test.sql", "select 1;");
        rt.define_parameter("name".to_string(), "original".to_string()).unwrap();

        let mut params = HashMap::new();
        params.insert("name".to_string(), "override".to_string());

        let saved = rt.run_file_begin("/test.sql", &params).unwrap();
        // During run, param should be overridden
        assert_eq!(
            rt.lookup_parameter("name").map(|v| match v {
                OwnedValue::Text(s) => std::str::from_utf8(&s).unwrap().to_string(),
                _ => String::new(),
            }),
            Some("override".to_string())
        );
        rt.run_file_end(saved);
        // After run, param should be restored
        assert_eq!(
            rt.lookup_parameter("name").map(|v| match v {
                OwnedValue::Text(s) => std::str::from_utf8(&s).unwrap().to_string(),
                _ => String::new(),
            }),
            Some("original".to_string())
        );
    }

    #[test]
    fn test_run_path_resolution() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/dir/helper.sql", "select 42;");
        rt.add_virtual_file("/dir/main.sql", ".run helper.sql\n");
        rt.enqueue("/dir/main.sql", ".run helper.sql\n", BlockSource::File(PathBuf::from("/dir/main.sql")));

        // Get the next step which should be a Run command with resolved path
        let step = rt.next_stepx().unwrap().unwrap();
        match step.result {
            StepResult::DotCommand(DotCommand::Run(run_cmd)) => {
                assert_eq!(run_cmd.file, "/dir/helper.sql");
            }
            other => panic!("Expected DotCommand::Run, got {:?}", other),
        }
    }

    // Full stepping tests using virtual FS

    /// Collect results from stepping, handling .run commands inline.
    fn collect_sql_results(rt: &mut Runtime) -> Vec<String> {
        let mut results = Vec::new();
        collect_sql_results_inner(rt, &mut results);
        results
    }

    fn collect_sql_results_inner(rt: &mut Runtime, results: &mut Vec<String>) {
        loop {
            match rt.next_stepx() {
                None => break,
                Some(Ok(step)) => match step.result {
                    StepResult::SqlStatement { stmt, .. } => {
                        match stmt.next() {
                            Ok(Some(row)) => {
                                results.push(row.first().unwrap().as_str().to_string());
                            }
                            Ok(None) => {
                                let _ = stmt.execute();
                            }
                            Err(_) => {}
                        }
                    }
                    StepResult::DotCommand(DotCommand::Run(run_cmd)) => {
                        if run_cmd.procedure.is_some() {
                            // For simplicity in tests, skip procedure calls
                        } else {
                            let saved = rt.run_file_begin(&run_cmd.file, &run_cmd.parameters).unwrap();
                            collect_sql_results_inner(rt, results);
                            rt.run_file_end(saved);
                        }
                    }
                    StepResult::DotCommand(DotCommand::Print(ref p)) => {
                        results.push(p.message.clone());
                    }
                    StepResult::DotCommand(_) => {}
                    StepResult::ProcedureDefinition(_) => {}
                },
                Some(Err(_)) => break,
            }
        }
    }

    fn collect_steps_with_errors(rt: &mut Runtime) -> (Vec<String>, Vec<String>) {
        let mut results = Vec::new();
        let mut errors = Vec::new();
        collect_steps_inner(rt, &mut results, &mut errors);
        (results, errors)
    }

    fn collect_steps_inner(rt: &mut Runtime, results: &mut Vec<String>, errors: &mut Vec<String>) {
        loop {
            match rt.next_stepx() {
                None => break,
                Some(Ok(step)) => match step.result {
                    StepResult::SqlStatement { stmt, .. } => {
                        match stmt.next() {
                            Ok(Some(row)) => {
                                results.push(row.first().unwrap().as_str().to_string());
                            }
                            Ok(None) => {
                                let _ = stmt.execute();
                            }
                            Err(_) => {}
                        }
                    }
                    StepResult::DotCommand(DotCommand::Run(run_cmd)) => {
                        if run_cmd.procedure.is_some() {
                            // skip for now
                        } else {
                            match rt.run_file_begin(&run_cmd.file, &run_cmd.parameters) {
                                Ok(saved) => {
                                    collect_steps_inner(rt, results, errors);
                                    rt.run_file_end(saved);
                                }
                                Err(e) => {
                                    errors.push(e);
                                }
                            }
                        }
                    }
                    StepResult::DotCommand(DotCommand::Print(ref p)) => {
                        results.push(p.message.clone());
                    }
                    StepResult::DotCommand(_) => {}
                    StepResult::ProcedureDefinition(_) => {}
                },
                Some(Err(e)) => {
                    errors.push(format!("{}", e));
                    break;
                }
            }
        }
    }

    #[test]
    fn test_run_basic_execution() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/helper.sql", "select 42;");
        rt.add_virtual_file("/main.sql", ".run /helper.sql\n");
        rt.enqueue("/main.sql", ".run /helper.sql\n", BlockSource::File(PathBuf::from("/main.sql")));
        let results = collect_sql_results(&mut rt);
        assert_eq!(results, vec!["42"]);
    }

    #[test]
    fn test_run_multiple_statements() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/helper.sql", "select 'a';\nselect 'b';\nselect 'c';");
        rt.add_virtual_file("/main.sql", ".run /helper.sql\n");
        rt.enqueue("/main.sql", ".run /helper.sql\n", BlockSource::File(PathBuf::from("/main.sql")));
        let results = collect_sql_results(&mut rt);
        assert_eq!(results, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_run_with_dot_commands() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/helper.sql", ".print hello\n");
        rt.add_virtual_file("/main.sql", ".run /helper.sql\n");
        rt.enqueue("/main.sql", ".run /helper.sql\n", BlockSource::File(PathBuf::from("/main.sql")));
        let results = collect_sql_results(&mut rt);
        assert_eq!(results, vec!["hello"]);
    }

    #[test]
    fn test_run_step_ordering() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/helper.sql", "select 'b';");
        let code = "select 'a';\n.run /helper.sql\nselect 'c';";
        rt.add_virtual_file("/main.sql", code);
        rt.enqueue("/main.sql", code, BlockSource::File(PathBuf::from("/main.sql")));
        let results = collect_sql_results(&mut rt);
        assert_eq!(results, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_run_nested() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/c.sql", "select 'c';");
        rt.add_virtual_file("/b.sql", "select 'b';\n.run /c.sql");
        let code = "select 'a';\n.run /b.sql\nselect 'd';";
        rt.add_virtual_file("/main.sql", code);
        rt.enqueue("/main.sql", code, BlockSource::File(PathBuf::from("/main.sql")));
        let results = collect_sql_results(&mut rt);
        assert_eq!(results, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn test_run_params_scoped() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/helper.sql", "select :name;");
        let code = ".run /helper.sql --name=alex\nselect :name;";
        rt.add_virtual_file("/main.sql", code);
        rt.enqueue("/main.sql", code, BlockSource::File(PathBuf::from("/main.sql")));
        // Set up param table first by defining and deleting
        rt.define_parameter("name".to_string(), "__placeholder__".to_string()).unwrap();
        rt.delete_parameter("name");
        rt.enqueue("/main.sql", code, BlockSource::File(PathBuf::from("/main.sql")));
        let results = collect_sql_results(&mut rt);
        // First result from helper should be "alex", second from main should be unbound (NULL)
        assert_eq!(results[0], "alex");
    }

    #[test]
    fn test_run_params_override_restored() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/helper.sql", "select :name;");
        rt.define_parameter("name".to_string(), "original".to_string()).unwrap();
        let code = ".run /helper.sql --name=override";
        rt.add_virtual_file("/main.sql", code);
        rt.enqueue("/main.sql", code, BlockSource::File(PathBuf::from("/main.sql")));
        let results = collect_sql_results(&mut rt);
        assert_eq!(results, vec!["override"]);
        // After .run, param should be restored
        let val = rt.lookup_parameter("name").map(|v| match v {
            OwnedValue::Text(s) => std::str::from_utf8(&s).unwrap().to_string(),
            _ => String::new(),
        });
        assert_eq!(val, Some("original".to_string()));
    }

    #[test]
    fn test_run_cycle_error_message() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/a.sql", ".run /b.sql");
        rt.add_virtual_file("/b.sql", ".run /a.sql");
        rt.enqueue("/a.sql", ".run /b.sql", BlockSource::File(PathBuf::from("/a.sql")));

        let (_, errors) = collect_steps_with_errors(&mut rt);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("cycle"));
        assert!(errors[0].contains("/a.sql"));
        assert!(errors[0].contains("/b.sql"));
    }

    #[test]
    fn test_run_file_not_found_error() {
        let mut rt = Runtime::new(None);
        let code = ".run /nonexistent.sql";
        rt.enqueue("/main.sql", code, BlockSource::File(PathBuf::from("/main.sql")));

        let (_, errors) = collect_steps_with_errors(&mut rt);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Failed to read"));
    }

    #[test]
    fn test_run_traceback_shows_correct_file() {
        let mut rt = Runtime::new(None);
        rt.add_virtual_file("/helper.sql", "select 42;");
        rt.add_virtual_file("/main.sql", ".run /helper.sql\n");
        rt.enqueue("/main.sql", ".run /helper.sql\n", BlockSource::File(PathBuf::from("/main.sql")));

        // Get the .run step
        let step = rt.next_stepx().unwrap().unwrap();
        assert_eq!(step.reference.to_string(), "/main.sql:1:1");

        // Handle the .run
        if let StepResult::DotCommand(DotCommand::Run(run_cmd)) = step.result {
            let saved = rt.run_file_begin(&run_cmd.file, &run_cmd.parameters).unwrap();
            // The SQL step inside helper.sql should reference helper.sql
            let inner_step = rt.next_stepx().unwrap().unwrap();
            assert!(inner_step.reference.to_string().contains("/helper.sql"));
            // Consume remaining
            while rt.next_stepx().is_some() {}
            rt.run_file_end(saved);
        }
    }
}
