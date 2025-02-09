use core::fmt;
use libsqlite3_sys::*;
use std::borrow::Cow;
use std::ffi::CStr;
use std::ptr;
use std::str::Utf8Error;
use std::{ffi::CString, os::raw::c_char};

// https://github.com/sqlite/sqlite/blob/853fb5e723a284347051756157a42bd65b53ebc4/src/json.c#L126
pub const JSON_SUBTYPE: u32 = 74;

// https://github.com/sqlite/sqlite/blob/853fb5e723a284347051756157a42bd65b53ebc4/src/vdbeapi.c#L212
pub const POINTER_SUBTYPE: u32 = 112;

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
            std::str::from_utf8(std::slice::from_raw_parts(s, n as usize)).unwrap()
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

/// https://www.sqlite.org/c3ref/stmt.html
pub struct Statement {
    statement: *mut sqlite3_stmt,
}
impl Statement {
    pub fn sql(&self) -> String {
        unsafe {
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
            rc => Err(latest_error(
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
            rc => Err(latest_error(
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
                    return Err(latest_error(
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
    pub fn reset(&self) {
        unsafe { sqlite3_reset(self.statement) };
    }
    pub fn readonly(&self) -> bool {
        unsafe { sqlite3_stmt_readonly(self.statement) != 0 }
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

/// https://www.sqlite.org/c3ref/sqlite3.html
pub struct Connection {
    connection: *mut sqlite3,
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
            Ok(Connection { connection })
        } else {
            let err = latest_error(connection, rc);
            unsafe {
                sqlite3_close(connection);
            }
            Err(err)
        }
    }
    pub fn open_in_memory() -> Result<Self, SQLiteError> {
        let mut connection: *mut sqlite3 = ptr::null_mut();
        let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_FULLMUTEX;
        let filename = CString::new(":memory:").unwrap();
        let rc =
            unsafe { sqlite3_open_v2(filename.as_ptr(), &mut connection, flags, ptr::null_mut()) };
        if rc == SQLITE_OK {
            unsafe {
                let v = 1;
                let x = sqlite3_db_config(connection, SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION, 1, &v);
                //let x = sqlite3_enable_load_extension(connection, 1);
                //sqlite3_db_config(connection, SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION);
            }
            Ok(Connection { connection })
        } else {
            let err = latest_error(connection, rc);
            unsafe {
                sqlite3_close(connection);
            }
            Err(err)
        }
    }
    pub(crate) unsafe fn db(&self) -> *mut sqlite3 {
        self.connection
    }

    pub fn in_transaction(&self) -> bool {
        unsafe { sqlite3_get_autocommit(self.connection) == 0 }
    }

    pub fn load_extension(&self, path: &str, entrypoint: &Option<String>) {
        let p = CString::new(path).unwrap();
        let entrypoint = entrypoint
            .as_ref()
            .map(|entrypoint| CString::new(entrypoint.clone()).unwrap());

        let entrypoint_cstr = entrypoint
            .as_ref()
            .map(|e| CString::new(e.clone()).unwrap());
        let entrypoint_ptr = entrypoint_cstr.as_ref().map_or(ptr::null(), |e| e.as_ptr());
        let mut pz_err_msg: *mut c_char = ptr::null_mut();
        let rc = unsafe {
            sqlite3_load_extension(self.db(), p.as_ptr(), entrypoint_ptr, &mut pz_err_msg)
        };
        if rc != SQLITE_OK {
            let s = unsafe { CStr::from_ptr(pz_err_msg).to_string_lossy() };
            println!("Loading extension failed: {s}");
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
            Err(latest_error(self.connection, rc))
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

#[derive(Debug)]
pub struct SQLiteError {
    pub result_code: i32,
    pub code_description: String,
    pub message: String,
    pub offset: Option<usize>,
}
impl fmt::Display for SQLiteError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} ({}) : {}",
            self.code_description, self.result_code, self.message
        )
    }
}
fn latest_error(db: *mut sqlite3, result_code: i32) -> SQLiteError {
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
    SQLiteError {
        result_code,
        code_description,
        message,
        offset,
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
