use core::fmt;
use libsqlite3_sys::*;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::ffi::{c_int, c_void, CStr};
use std::ptr::{self};
use std::str::Utf8Error;
use std::{ffi::CString, os::raw::c_char};

pub use libsqlite3_sys::sqlite3_stmt;
// https://github.com/sqlite/sqlite/blob/853fb5e723a284347051756157a42bd65b53ebc4/src/json.c#L126
pub const JSON_SUBTYPE: u32 = 74;

// https://github.com/sqlite/sqlite/blob/853fb5e723a284347051756157a42bd65b53ebc4/src/vdbeapi.c#L212
pub const POINTER_SUBTYPE: u32 = 112;

/// Abstraction of a SQLite error.
#[derive(Serialize, Deserialize, Debug)]
pub struct SQLiteError {
    pub result_code: i32,
    pub code_description: String,
    pub message: String,
    pub offset: Option<usize>,
}

impl SQLiteError {
    pub fn from_latest(db: *mut sqlite3, result_code: i32) -> Self {
        let message = unsafe {
            let message = sqlite3_errmsg(db);
            let message = CStr::from_ptr(message);
            message.to_string_lossy().to_string()
        };

        let code_description = unsafe {
            let code_description = sqlite3_errstr(result_code);
            let code_description = CStr::from_ptr(code_description);
            code_description.to_string_lossy().to_string()
        };
        let offset = unsafe {
            match sqlite3_error_offset(db) {
                -1 => None,
                offset => Some(offset as usize),
            }
        };
        Self {
            result_code,
            code_description,
            message,
            offset,
        }
    }
}

impl fmt::Display for SQLiteError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "[{}] {}: {}",
            self.result_code, self.code_description, self.message
        )
    }
}

fn escape_identifier(identifer: &str) -> String {
    let n = identifer.len();
    let s = CString::new(identifer).unwrap();
    unsafe {
        let x = sqlite3_str_new(ptr::null_mut());
        sqlite3_str_appendf(x, c"%w".as_ptr(), s.as_ptr());
        let s = sqlite3_str_finish(x);
        let cpy = CStr::from_ptr(s).to_string_lossy().into_owned();
        sqlite3_free(s.cast());
        cpy
    }
    //let _rc = unsafe { sqlite3_bind_text(self.statement, i, s.as_ptr(), n as i32, SQLITE_TRANSIENT()) };
}

// Use the SQLite printf '%Q' conversion to escape a string as a SQL string literal, surrounded by single quotes.
pub fn escape_string(value: &str) -> String {
    let n = value.len();
    let s = CString::new(value).unwrap();
    unsafe {
        let x = sqlite3_str_new(ptr::null_mut());
        sqlite3_str_appendf(x, c"%Q".as_ptr(), s.as_ptr());
        let s = sqlite3_str_finish(x);
        let cpy = CStr::from_ptr(s).to_string_lossy().into_owned();
        sqlite3_free(s.cast());
        cpy
    }
}

// https://www.sqlite.org/c3ref/value.html
pub struct ValueRefX<'a> {
    raw: *mut sqlite3_value,
    pub value: ValueRefXValue<'a>,
}
pub enum ValueRefXValue<'a> {
    Null,
    Int(i64),
    Double(f64),
    Text(&'a [u8]),
    Blob(&'a [u8]),
}

impl<'a> ValueRefX<'a> {
    /// # Safety
    ///
    /// Only use on real sqlite3_value bro
    pub unsafe fn from_value(value: *mut sqlite3_value) -> Self {
        let v = match sqlite3_value_type(value) {
            SQLITE_INTEGER => ValueRefXValue::Int(sqlite3_value_int64(value)),
            SQLITE_FLOAT => ValueRefXValue::Double(sqlite3_value_double(value)),
            SQLITE_TEXT => {
                let (s, len) = (sqlite3_value_text(value), sqlite3_value_bytes(value));
                ValueRefXValue::Text(std::slice::from_raw_parts(s, len as usize))
            }
            SQLITE_BLOB => {
                let (b, len) = (sqlite3_value_blob(value), sqlite3_value_bytes(value));
                ValueRefXValue::Blob(std::slice::from_raw_parts(b.cast::<u8>(), len as usize))
            }
            _ => ValueRefXValue::Null,
        };
        Self {
            raw: value,
            value: v,
        }
    }
    pub fn as_str(&self) -> &str {
        unsafe {
            let n = sqlite3_value_bytes(self.raw);
            let s = sqlite3_value_text(self.raw);
            if n == 0 {
                ""
            } else {
                std::str::from_utf8(std::slice::from_raw_parts(s, n as usize)).unwrap()
            }
        }
    }
    pub fn as_int64(&self) -> i64 {
        unsafe { sqlite3_value_int64(self.raw) }
    }
}

impl ValueRefX<'_> {
    pub fn subtype(&self) -> Option<u32> {
        let subtype = unsafe { sqlite3_value_subtype(self.raw) };
        if subtype == 0 {
            None
        } else {
            Some(subtype)
        }
    }
}

pub enum OwnedValue {
    Null,
    Integer(i64),
    Double(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}
impl OwnedValue {
    pub fn from_value_ref(v: &'_ ValueRefX) -> Self {
        match v.value {
            ValueRefXValue::Null => OwnedValue::Null,
            ValueRefXValue::Int(v) => OwnedValue::Integer(v),
            ValueRefXValue::Double(v) => OwnedValue::Double(v),
            ValueRefXValue::Text(v) => OwnedValue::Text(v.to_vec()),
            ValueRefXValue::Blob(v) => OwnedValue::Blob(v.to_vec()),
        }
    }
}

pub enum ValueType {
    Null,
    Integer,
    Double,
    Text,
    Blob,
}

pub struct Row<'a> {
    statement: *mut sqlite3_stmt,
    phantom: std::marker::PhantomData<&'a ()>,
}
impl<'a> Row<'a> {
    #[inline(always)]
    pub fn count(&self) -> usize {
        unsafe { sqlite3_column_count(self.statement) as usize }
    }

    #[inline(always)]
    pub fn value_at(&self, at: usize) -> ValueRefX<'a> {
        unsafe { ValueRefX::from_value(sqlite3_column_value(self.statement, at as i32)) }
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct ColumnMeta {
    pub name: String,
    pub origin_database: Option<String>,
    pub origin_table: Option<String>,
    pub origin_column: Option<String>,
    pub decltype: Option<String>,
}

pub enum IsExplain{
  Explain,
  ExplainQueryPlan,
}
/// https://www.sqlite.org/c3ref/stmt.html
#[derive(Serialize, Debug)]
pub struct Statement {
    #[serde(skip)]
    statement: *mut sqlite3_stmt,
}
impl Statement {
    pub fn sql(&self) -> String {
        unsafe {
            // Freed when statement is finalized, but we make copy here anyway
            let z = sqlite3_sql(self.statement);
            let s = CStr::from_ptr(z);
            s.to_str().unwrap().to_string()
        }
    }
    /// https://www.sqlite.org/c3ref/expanded_sql.html
    pub fn expanded_sql(&self) -> Result<String, ()> {
        let result = unsafe { sqlite3_expanded_sql(self.statement) };
        if result.is_null() {
            Err(())
        } else {
            unsafe {
                let s = CStr::from_ptr(result);
                let expanded = s.to_str().unwrap().to_string();
                sqlite3_free(result.cast());
                Ok(expanded.to_owned())
            }
        }
    }

    pub fn is_explain(&self) -> Option<IsExplain> {
        let result = unsafe {
          sqlite3_stmt_isexplain(self.statement)
        };
        match result {
          0 => None,
          1 => Some(IsExplain::Explain),
          2 => Some(IsExplain::ExplainQueryPlan),
          _ => None,
        }
    }
    pub fn explain(&self, emode: i32) {
        unsafe {
            //sqlite3_stmt_explain(self.statement, emode);
        }
    }
    pub fn column_names(&self) -> Result<Vec<String>, Utf8Error> {
        unsafe {
            let mut columns = vec![];
            let n = sqlite3_column_count(self.statement);
            for i in 0..n {
                let s = CStr::from_ptr(sqlite3_column_name(self.statement, i));
                columns.push(s.to_str()?.to_string());
            }
            Ok(columns)
        }
    }

    pub fn column_meta(&self) -> Vec<ColumnMeta> {
        unsafe {
            let mut columns = vec![];
            let n = sqlite3_column_count(self.statement);
            for i in 0..n {
                let name = CStr::from_ptr(sqlite3_column_name(self.statement, i));
                let origin_database = sqlite3_column_database_name(self.statement, i);
                let origin_table = sqlite3_column_table_name(self.statement, i);
                let origin_column = sqlite3_column_origin_name(self.statement, i);

                let decltype = sqlite3_column_decltype(self.statement, i);
                columns.push(ColumnMeta {
                    name: name.to_string_lossy().to_string(),
                    origin_database: origin_database
                        .as_ref()
                        .map(|s| CStr::from_ptr(s).to_string_lossy().to_string()),
                    origin_table: origin_table
                        .as_ref()
                        .map(|s| CStr::from_ptr(s).to_string_lossy().to_string()),
                    origin_column: origin_column
                        .as_ref()
                        .map(|s| CStr::from_ptr(s).to_string_lossy().to_string()),
                    decltype: decltype
                        .as_ref()
                        .map(|s| CStr::from_ptr(s).to_string_lossy().to_string()),
                });
            }
            columns
        }
    }
    pub fn next(&self) -> Result<Option<Vec<ValueRefX>>, SQLiteError> {
        let rc = unsafe { sqlite3_step(self.statement) };
        match rc {
            SQLITE_DONE => Ok(None),
            SQLITE_ROW => {
                let n = unsafe { sqlite3_column_count(self.statement) };
                let mut row = Vec::with_capacity(n as usize);
                for i in 0..n {
                    row.push(
                        /*ValueRef {
                            value: unsafe { sqlite3_column_value(self.statement, i) },
                        }*/
                        unsafe { ValueRefX::from_value(sqlite3_column_value(self.statement, i)) },
                    );
                }
                Ok(Some(row))
            }
            rc => Err(SQLiteError::from_latest(
                unsafe { sqlite3_db_handle(self.statement) },
                rc,
            )),
        }
    }

    #[inline(always)]
    pub fn nextx<'a>(&self) -> Result<Option<Row<'a>>, SQLiteError> {
        let rc = unsafe { sqlite3_step(self.statement) };
        match rc {
            SQLITE_DONE => Ok(None),
            SQLITE_ROW => Ok(Some(Row {
                statement: self.statement,
                phantom: std::marker::PhantomData,
            })),
            rc => Err(SQLiteError::from_latest(
                unsafe { sqlite3_db_handle(self.statement) },
                rc,
            )),
        }
    }

    /// Execute statement until completion, ie SQLITE_DONE.
    /// Returns the number of SQLITE_ROW results yeilded by sqlite3_step(),
    /// or an error.
    pub fn execute(&self) -> Result<usize, SQLiteError> {
        let mut n = 0;
        loop {
            let rc = unsafe { sqlite3_step(self.statement) };
            n += 1;
            match rc {
                SQLITE_DONE => break,
                SQLITE_ROW => continue,
                _ => {
                    return Err(SQLiteError::from_latest(
                        unsafe { sqlite3_db_handle(self.statement) },
                        rc,
                    ))
                }
            }
        }
        Ok(n)
    }

    pub fn bind_int64(&self, i: i32, value: i64) {
        unsafe { sqlite3_bind_int64(self.statement, i, value) };
    }
    pub fn bind_double(&self, i: i32, value: f64) {
        unsafe { sqlite3_bind_double(self.statement, i, value) };
    }
    pub fn bind_null(&self, i: i32) {
        unsafe { sqlite3_bind_null(self.statement, i) };
    }
    pub fn bind_blob(&self, i: i32, value: &[u8]) {
        unsafe {
            sqlite3_bind_blob(
                self.statement,
                i,
                value.as_ptr().cast(),
                value.len().try_into().unwrap(),
                SQLITE_TRANSIENT(),
            )
        };
    }

    // TODO expose destructor interface here?
    pub fn bind_pointer(&self, i: i32, p: *mut c_void, name: &CStr) {
        unsafe { sqlite3_bind_pointer(self.statement, i, p, name.as_ptr(), None) };
    }
    pub fn bind_text<S: AsRef<str>>(&self, i: i32, value: S) {
        let n = value.as_ref().len();
        let s = CString::new(value.as_ref()).unwrap();
        let _rc = unsafe {
            sqlite3_bind_text(self.statement, i, s.as_ptr(), n as i32, SQLITE_TRANSIENT())
        };
        // TODO error check
    }

    pub fn bind_parameters(&self) -> Vec<String> {
        unsafe {
            let n = sqlite3_bind_parameter_count(self.statement);
            let mut bind_parameters = vec![];
            for i in 0..n {
                let name = sqlite3_bind_parameter_name(self.statement, i + 1);
                let name = CStr::from_ptr(name).to_string_lossy().to_string();
                bind_parameters.push(name);
            }
            bind_parameters
        }
    }

    pub fn parameter_info(&self) -> Vec<String> {
        unsafe {
            let n = sqlite3_bind_parameter_count(self.statement);
            let mut bind_parameters = vec![];
            for i in 0..n {
                let name = sqlite3_bind_parameter_name(self.statement, i + 1);
                let name = CStr::from_ptr(name).to_string_lossy().to_string();
                bind_parameters.push(format!("{}", name));
            }
            bind_parameters
        }
    }
    pub fn reset(&self) {
        unsafe { sqlite3_reset(self.statement) };
    }
    pub fn readonly(&self) -> bool {
        unsafe { sqlite3_stmt_readonly(self.statement) != 0 }
    }

    pub fn pointer(&self) -> *mut sqlite3_stmt {
        self.statement
    }
}
impl Drop for Statement {
    fn drop(&mut self) {
        // https://www.sqlite.org/c3ref/finalize.html
        unsafe {
            sqlite3_finalize(self.statement);
        }
    }
}

#[derive(Debug)]
pub struct BytecodeStep {
    pub addr: i64,
    pub opcode: String,
    pub p1: i64,
    pub p2: i64,
    pub p3: i64,
    pub p4: String,
    pub p5: i64,
    pub comment: String,
    pub subprog: i64,
    pub nexec: i64,
    pub ncycle: i64,
}
pub fn bytecode_steps(pstmt: *mut sqlite3_stmt) -> Vec<BytecodeStep> {
    let mut steps = vec![];
    unsafe {
        let db: *mut sqlite3 = sqlite3_db_handle(pstmt);
        let db = Connection {
            connection: db,
            owned: false,
        };

        let stmt = db
            .prepare(
                "SELECT addr, opcode, p1, p2, p3, p4, p5, comment, subprog, nexec, ncycle 
    FROM bytecode(?)
    ",
            )
            .unwrap()
            .1
            .unwrap();
        stmt.bind_pointer(1, pstmt.cast(), c"stmt-pointer");
        loop {
            let rc = stmt.nextx();
            match rc {
                Ok(Some(row)) => {
                    let addr = row.value_at(0).as_int64();
                    let opcode = row.value_at(1).as_str().to_owned();
                    let p1 = row.value_at(2).as_int64();
                    let p2 = row.value_at(3).as_int64();
                    let p3 = row.value_at(4).as_int64();
                    let p4 = row.value_at(5).as_str().to_owned();
                    let p5 = row.value_at(6).as_int64();
                    let comment = row.value_at(7).as_str().to_owned();
                    let subprog = row.value_at(8).as_int64();
                    let nexec = row.value_at(9).as_int64();
                    let ncycle = row.value_at(10).as_int64();
                    steps.push(BytecodeStep {
                        addr,
                        opcode: opcode.to_string(),
                        p1,
                        p2,
                        p3,
                        p4: p4.to_string(),
                        p5: p5,
                        comment: comment.to_string(),
                        subprog,
                        nexec,
                        ncycle,
                    });
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    }
    // TODO
    steps
}
/// https://www.sqlite.org/c3ref/sqlite3.html
pub struct Connection {
    connection: *mut sqlite3,
    owned: bool,
}

// NOT Sync, sqlite limitation
unsafe impl std::marker::Send for Connection {}
unsafe impl std::marker::Send for Statement {}

#[derive(Debug)]
pub struct PrepareError {
    pub code: i32,
    pub code_str: String,
    pub message: String,
}
impl Connection {
    pub fn open(path: &str) -> Result<Self, SQLiteError> {
        let mut connection: *mut sqlite3 = ptr::null_mut();
        let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_FULLMUTEX | SQLITE_OPEN_CREATE;
        let filename = CString::new(path).unwrap();
        let rc =
            unsafe { sqlite3_open_v2(filename.as_ptr(), &mut connection, flags, ptr::null_mut()) };
        if rc == SQLITE_OK {
            unsafe {
                sqlite3_enable_load_extension(connection, 1);
            }
            Ok(Connection {
                connection,
                owned: true,
            })
        } else {
            let err = SQLiteError::from_latest(connection, rc);
            unsafe {
                sqlite3_close(connection);
            }
            Err(err)
        }
    }

    pub fn open_in_memory() -> Result<Self, SQLiteError> {
        let mut connection: *mut sqlite3 = ptr::null_mut();
        let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_FULLMUTEX | SQLITE_OPEN_CREATE;
        let filename = CString::new(":memory:").unwrap();
        let rc =
            unsafe { sqlite3_open_v2(filename.as_ptr(), &mut connection, flags, ptr::null_mut()) };
        if rc == SQLITE_OK {
            unsafe {
                let v = 1;
                let x = sqlite3_db_config(connection, SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION, 1, &v);
                sqlite3_db_config(connection, 1018, 1, &v);
                //sqlite3_db_config(connection, 1018, 0, &v);
                //let x = sqlite3_enable_load_extension(connection, 1);
                //sqlite3_db_config(connection, SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION);
            }
            Ok(Connection {
                connection,
                owned: true,
            })
        } else {
            let err = SQLiteError::from_latest(connection, rc);
            unsafe {
                sqlite3_close(connection);
            }
            Err(err)
        }
    }
    pub unsafe fn db(&self) -> *mut sqlite3 {
        self.connection
    }

    pub fn db_name(&self)-> Option<String> {
        unsafe {
            let filename: *const sqlite3_filename = sqlite3_db_filename(self.connection, c"main".as_ptr()).cast();
            if !filename.is_null() {
                // TODO: lol
                let s = sqlite3_filename_database(filename.cast());
                let s = CStr::from_ptr(s).to_string_lossy();
                Some(s.to_string())
            } else {
                None
            }
        }

    }

    pub fn in_transaction(&self) -> bool {
        unsafe { sqlite3_get_autocommit(self.connection) == 0 }
    }

    pub fn load_extension(&self, path: &str, entrypoint: &Option<String>) -> anyhow::Result<()> {
        let p = CString::new(path).map_err(|_| anyhow::anyhow!("Invalid path"))?;

        let entrypoint_cstr = entrypoint
            .as_ref()
            .map(|e| CString::new(e.clone()).map_err(|_| anyhow::anyhow!("Invalid entrypoint")))
            .transpose()?;
        let entrypoint_ptr = entrypoint_cstr.as_ref().map_or(ptr::null(), |e| e.as_ptr());
        let mut pz_err_msg: *mut c_char = ptr::null_mut();
        let rc = unsafe {
            sqlite3_load_extension(self.db(), p.as_ptr(), entrypoint_ptr, &mut pz_err_msg)
        };
        if rc != SQLITE_OK {
            let s = unsafe { CStr::from_ptr(pz_err_msg).to_string_lossy() };
            //println!("Loading extension failed: {s}");
            return Err(anyhow::anyhow!("Loading extension failed: {s}",));
        }
        Ok(())
    }

    pub fn execute(&self, sql: &str) -> Result<usize, /* TODO */ ()> {
        let stmt = self.prepare(sql).unwrap().1.unwrap();
        Ok(stmt.execute().unwrap())
    }

    pub fn execute_script(&self, sql: &str) -> Result<(), SQLiteError> {
        let z_sql = CString::new(sql).unwrap();
        // TODO unfurl manually
        let rc = unsafe { sqlite3_exec(self.connection, z_sql.as_ptr(), None, ptr::null_mut(), ptr::null_mut()) };
        if rc == SQLITE_OK {
            Ok(())
        } else {
            Err(SQLiteError::from_latest(self.connection, rc))
        }
    }

    pub fn set_progress_handler<F, T>(&self, ops: i32, handle: Option<F>, aux: T)
    where
        F: FnMut(&T) -> bool + Send + 'static,
    {
        unsafe extern "C" fn call_boxed_closure<F, T>(p_arg: *mut c_void) -> c_int
        where
            F: FnMut(&T) -> bool,
        {
            //let boxed_handler: *mut F = p_arg.cast::<(F, T)>();
            //let x = p_arg.cast::<(F, T)>();
            //let r = ((*x).0)(&(*x).1);
            let x = p_arg.cast::<(*mut F, *mut T)>();
            let r = (*((*x).0))(&(*(*x).1));
            return if r { 1 } else { 0 };
        }
        if let Some(handle) = handle {
            unsafe {
                //let boxed_handler = Box::new(handle);
                let x: *mut F = Box::into_raw(Box::new(handle));
                let y: *mut T = Box::into_raw(Box::new(aux));
                ///let boxed_handler = Box::new((handle, aux));
                let boxed_handler = Box::into_raw(Box::new((x, y)));
                sqlite3_progress_handler(
                    self.connection,
                    ops,
                    Some(call_boxed_closure::<F, T>),
                    boxed_handler.cast(),
                    //&*boxed_handler as *const (F, T) as *mut _,
                    //&*boxed_handler as *const F as *mut _,
                );
            }
        }
    }

    pub fn serialize(&self) -> Result<Vec<u8>, SQLiteError> {
        unsafe {
          let mut sz: sqlite3_int64  = 0;
          let ptr= sqlite3_serialize(self.connection, c"main".as_ptr(), &mut sz, 0);
          if ptr.is_null() {
              return Err(SQLiteError::from_latest(self.connection, sqlite3_errcode(self.connection)));
          }
          let slice = std::slice::from_raw_parts(ptr as *const u8, sz as usize);
          let vec = slice.to_vec();
          sqlite3_free(ptr.cast());
          Ok(vec)
        }
    }

    pub fn prepare(&self, sql: &str) -> Result<(Option<usize>, Option<Statement>), SQLiteError> {
        let z_sql = CString::new(sql).unwrap();
        let mut stmt: *mut sqlite3_stmt = ptr::null_mut();
        let head = z_sql.as_ptr();
        let mut tail: *const c_char = ptr::null_mut();
        let rc = unsafe { sqlite3_prepare_v2(self.connection, head, -1, &mut stmt, &mut tail) };
        if rc == SQLITE_OK {
            let rest = if tail.is_null() {
                None
            } else {
                let nused = (tail as usize) - (head as usize);
                if nused == sql.len() {
                    None
                } else {
                    match sql.get(nused..) {
                        Some(rest) => {
                            if !rest.trim().is_empty() {
                                Some(nused)
                            } else {
                                None
                            }
                        }
                        None => None,
                    }
                }
            };
            if stmt.is_null() {
                Ok((rest, None))
            } else {
                Ok((rest, Some(Statement { statement: stmt })))
            }
        } else {
            Err(SQLiteError::from_latest(self.connection, rc))
        }
    }

    /// https://www.sqlite.org/c3ref/interrupt.html
    pub fn is_interrupted(&self) -> bool {
        unsafe { sqlite3_is_interrupted(self.connection) != 0 }
    }
    /// https://www.sqlite.org/c3ref/interrupt.html
    pub fn interrupt(&self) {
        unsafe { sqlite3_interrupt(self.connection) };
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // https://www.sqlite.org/c3ref/close.html
        unsafe { sqlite3_close(self.connection) };
    }
}

/// https://www.sqlite.org/c3ref/complete.html
pub fn complete(sql: &str) -> bool {
    let sql = CString::new(sql).unwrap();
    unsafe { sqlite3_complete(sql.as_ptr()) != 0 }
}

pub fn sqlite_version() -> Cow<'static, str> {
    unsafe { CStr::from_ptr(sqlite3_libversion()).to_string_lossy() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TODO: why does tihs panic?
    #[ignore]
    #[test]
    fn test_connnection() {
        let connection = Connection::open_in_memory().unwrap();
        assert!(!connection.is_interrupted());

        let (rest, stmt) = connection.prepare("select :p").unwrap();
        assert_eq!(rest, None);
        let stmt = stmt.unwrap();
        assert_eq!(stmt.expanded_sql().unwrap(), "select NULL");

        /*assert!(matches!(
            stmt.next().unwrap().unwrap().get(0).unwrap(),
            ValueRefXValue::Null
        ));*/

        stmt.reset();
        stmt.bind_int64(1, 100);
        assert_eq!(stmt.expanded_sql().unwrap(), "select 100");
        /*assert!(matches!(
            stmt.next().unwrap().unwrap().get(0).unwrap(),
            ValueRefXValue::Int(_)
        ));*/
    }
    #[test]
    fn test_multiple_statements() {
        let connection = Connection::open_in_memory().unwrap();
        assert_eq!(connection.prepare("select 1;").unwrap().0, None);
        assert_eq!(connection.prepare("select 1;    ").unwrap().0, None);
        let sql = "select 1; select 2;";
        let rest = connection.prepare(sql).unwrap().0.unwrap();
        assert_eq!(rest, "select 1;".len());
        assert_eq!(sql.get(rest..).unwrap(), " select 2;");
        assert_eq!(
            connection
                .prepare(
                    "select value, value, value
            from json_each('[99,88,77,66,55]');
        "
                )
                .unwrap()
                .0,
            None
        );
    }

    #[test]
    fn test_complete() {
        assert!(complete("select 1;"));
        assert!(!complete("select 1"));
        // TODO handle
        //assert!(!complete("select '\0'"));
    }

    #[test]
    fn test_escape_identifier() {
        assert_eq!(escape_identifier("alex"), "alex".to_string());
        assert_eq!(
            escape_identifier("alex \"garcia\""),
            "alex \"\"garcia\"\"".to_string()
        );
    }
}
