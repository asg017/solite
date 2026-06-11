use core::fmt;
use libsqlite3_sys::*;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cell::RefCell;
use std::sync::{Arc, Mutex as StdMutex};
use std::ffi::{c_int, c_void, CStr};
use std::ptr::{self};
use std::str::Utf8Error;
use std::{ffi::CString, os::raw::c_char};

pub use libsqlite3_sys::{sqlite3_sql, sqlite3_stmt};
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
    /// # Safety
    /// `db` must be a valid, non-null pointer to an open sqlite3 database.
    pub unsafe fn from_latest(db: *mut sqlite3, result_code: i32) -> Self {
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

impl std::error::Error for SQLiteError {}

// Use the SQLite printf '%Q' conversion to escape a string as a SQL string literal, surrounded by single quotes.
pub fn escape_string(value: &str) -> String {
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
    /// Stored subtype for owned/buffered values (where raw pointer is null).
    /// For raw pointer values, this is None and subtype() uses the C API.
    stored_subtype: Option<u32>,
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
            stored_subtype: None,
        }
    }
    pub fn as_str(&self) -> &str {
        if self.raw.is_null() {
            match &self.value {
                ValueRefXValue::Text(bytes) => std::str::from_utf8(bytes).unwrap_or(""),
                _ => "",
            }
        } else {
            unsafe {
                let n = sqlite3_value_bytes(self.raw);
                let s = sqlite3_value_text(self.raw);
                if n == 0 {
                    ""
                } else {
                    std::str::from_utf8(std::slice::from_raw_parts(s, n as usize)).unwrap_or("")
                }
            }
        }
    }
    pub fn as_int64(&self) -> i64 {
        if self.raw.is_null() {
            match &self.value {
                ValueRefXValue::Int(v) => *v,
                _ => 0,
            }
        } else {
            unsafe { sqlite3_value_int64(self.raw) }
        }
    }
}

impl ValueRefX<'_> {
    pub fn subtype(&self) -> Option<u32> {
        if let Some(st) = self.stored_subtype {
            if st == 0 { None } else { Some(st) }
        } else if self.raw.is_null() {
            None
        } else {
            let subtype = unsafe { sqlite3_value_subtype(self.raw) };
            if subtype == 0 { None } else { Some(subtype) }
        }
    }

    /// Create a ValueRefX from owned data (for buffered/remote statements).
    pub fn from_owned(value: ValueRefXValue<'_>, subtype: Option<u32>) -> ValueRefX<'_> {
        ValueRefX {
            raw: ptr::null_mut(),
            value,
            stored_subtype: subtype,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ColumnMeta {
    pub name: String,
    pub origin_database: Option<String>,
    pub origin_table: Option<String>,
    pub origin_column: Option<String>,
    pub decltype: Option<String>,
    /// `Some(true)` if the origin column permits NULL, `Some(false)` if it has a
    /// NOT NULL constraint, or `None` when the column is not a direct reference
    /// to a base-table column (e.g. expressions, aggregates, unions).
    #[serde(default)]
    pub nullable: Option<bool>,
}

pub enum IsExplain{
  Explain,
  ExplainQueryPlan,
}
/// https://www.sqlite.org/c3ref/stmt.html
#[derive(Serialize, Debug)]
pub struct Statement {
    #[serde(skip)]
    inner: StatementInner,
}

enum StatementInner {
    Local {
        statement: *mut sqlite3_stmt,
    },
    /// A statement whose results have been fully materialized (from remote execution).
    Buffered {
        result: crate::rpc::QueryResult,
        cursor: std::cell::Cell<usize>,
    },
}

impl std::fmt::Debug for StatementInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StatementInner::Local { statement } => {
                f.debug_struct("Local").field("statement", statement).finish()
            }
            StatementInner::Buffered { result, cursor } => f
                .debug_struct("Buffered")
                .field("sql", &result.sql)
                .field("rows", &result.rows.len())
                .field("cursor", &cursor.get())
                .finish(),
        }
    }
}

impl Statement {
    /// Create a buffered statement from a remote QueryResult.
    pub fn from_query_result(result: crate::rpc::QueryResult) -> Self {
        Statement {
            inner: StatementInner::Buffered {
                result,
                cursor: std::cell::Cell::new(0),
            },
        }
    }

    pub fn sql(&self) -> String {
        match &self.inner {
            StatementInner::Local { statement } => unsafe {
                let z = sqlite3_sql(*statement);
                let s = CStr::from_ptr(z);
                s.to_str().unwrap().to_string()
            },
            StatementInner::Buffered { result, .. } => result.sql.clone(),
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn expanded_sql(&self) -> Result<String, ()> {
        match &self.inner {
            StatementInner::Local { statement } => {
                let result = unsafe { sqlite3_expanded_sql(*statement) };
                if result.is_null() {
                    Err(())
                } else {
                    unsafe {
                        let s = CStr::from_ptr(result);
                        let expanded = s.to_str().unwrap().to_string();
                        sqlite3_free(result.cast());
                        Ok(expanded)
                    }
                }
            }
            StatementInner::Buffered { result, .. } => Ok(result.sql.clone()),
        }
    }

    pub fn is_explain(&self) -> Option<IsExplain> {
        match &self.inner {
            StatementInner::Local { statement } => {
                let result = unsafe { sqlite3_stmt_isexplain(*statement) };
                match result {
                    0 => None,
                    1 => Some(IsExplain::Explain),
                    2 => Some(IsExplain::ExplainQueryPlan),
                    _ => None,
                }
            }
            StatementInner::Buffered { result, .. } => match result.is_explain {
                Some(1) => Some(IsExplain::Explain),
                Some(2) => Some(IsExplain::ExplainQueryPlan),
                _ => None,
            },
        }
    }

    pub fn explain(&self, _emode: i32) {
        // TODO: sqlite3_stmt_explain(self.statement, emode);
    }

    pub fn column_names(&self) -> Result<Vec<String>, Utf8Error> {
        match &self.inner {
            StatementInner::Local { statement } => unsafe {
                let mut columns = vec![];
                let n = sqlite3_column_count(*statement);
                for i in 0..n {
                    let s = CStr::from_ptr(sqlite3_column_name(*statement, i));
                    columns.push(s.to_str()?.to_string());
                }
                Ok(columns)
            },
            StatementInner::Buffered { result, .. } => {
                Ok(result.columns.iter().map(|c| c.name.clone()).collect())
            }
        }
    }

    pub fn column_meta(&self) -> Vec<ColumnMeta> {
        match &self.inner {
            StatementInner::Local { statement } => unsafe {
                let mut columns = vec![];
                let db = sqlite3_db_handle(*statement);
                let n = sqlite3_column_count(*statement);
                for i in 0..n {
                    let name = CStr::from_ptr(sqlite3_column_name(*statement, i));
                    let origin_database = sqlite3_column_database_name(*statement, i);
                    let origin_table = sqlite3_column_table_name(*statement, i);
                    let origin_column = sqlite3_column_origin_name(*statement, i);
                    let decltype = sqlite3_column_decltype(*statement, i);

                    let nullable = if !origin_table.is_null() && !origin_column.is_null() {
                        let mut not_null: c_int = 0;
                        let rc = sqlite3_table_column_metadata(
                            db,
                            origin_database,
                            origin_table,
                            origin_column,
                            ptr::null_mut(),
                            ptr::null_mut(),
                            &mut not_null,
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );
                        if rc == SQLITE_OK {
                            Some(not_null == 0)
                        } else {
                            None
                        }
                    } else {
                        None
                    };

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
                        nullable,
                    });
                }
                columns
            },
            StatementInner::Buffered { result, .. } => result.columns.clone(),
        }
    }

    pub fn next(&self) -> Result<Option<Vec<ValueRefX<'_>>>, SQLiteError> {
        match &self.inner {
            StatementInner::Local { statement } => {
                let rc = unsafe { sqlite3_step(*statement) };
                match rc {
                    SQLITE_DONE => Ok(None),
                    SQLITE_ROW => {
                        let n = unsafe { sqlite3_column_count(*statement) };
                        let mut row = Vec::with_capacity(n as usize);
                        for i in 0..n {
                            row.push(unsafe {
                                ValueRefX::from_value(sqlite3_column_value(*statement, i))
                            });
                        }
                        Ok(Some(row))
                    }
                    rc => Err(unsafe {
                        SQLiteError::from_latest(sqlite3_db_handle(*statement), rc)
                    }),
                }
            }
            StatementInner::Buffered { result, cursor } => {
                let idx = cursor.get();
                if idx >= result.rows.len() {
                    return Ok(None);
                }
                cursor.set(idx + 1);
                let wire_row = &result.rows[idx];
                let row: Vec<ValueRefX<'_>> = wire_row
                    .iter()
                    .map(|wv| {
                        let value = match &wv.value {
                            OwnedValue::Null => ValueRefXValue::Null,
                            OwnedValue::Integer(v) => ValueRefXValue::Int(*v),
                            OwnedValue::Double(v) => ValueRefXValue::Double(*v),
                            OwnedValue::Text(v) => ValueRefXValue::Text(v.as_slice()),
                            OwnedValue::Blob(v) => ValueRefXValue::Blob(v.as_slice()),
                        };
                        ValueRefX::from_owned(value, wv.subtype)
                    })
                    .collect();
                Ok(Some(row))
            }
        }
    }

    #[inline(always)]
    pub fn nextx<'a>(&self) -> Result<Option<Row<'a>>, SQLiteError> {
        match &self.inner {
            StatementInner::Local { statement } => {
                let rc = unsafe { sqlite3_step(*statement) };
                match rc {
                    SQLITE_DONE => Ok(None),
                    SQLITE_ROW => Ok(Some(Row {
                        statement: *statement,
                        phantom: std::marker::PhantomData,
                    })),
                    rc => Err(unsafe {
                        SQLiteError::from_latest(sqlite3_db_handle(*statement), rc)
                    }),
                }
            }
            StatementInner::Buffered { .. } => {
                // nextx returns a Row that wraps a raw pointer — not compatible with buffered.
                // Callers should use next() for buffered statements.
                Err(SQLiteError {
                    result_code: -1,
                    code_description: "UNSUPPORTED".to_string(),
                    message: "nextx() is not supported on buffered statements. Use next() instead."
                        .to_string(),
                    offset: None,
                })
            }
        }
    }

    /// Execute statement until completion, ie SQLITE_DONE.
    /// Returns the number of SQLITE_ROW results yielded by sqlite3_step(),
    /// or an error.
    pub fn execute(&self) -> Result<usize, SQLiteError> {
        match &self.inner {
            StatementInner::Local { statement } => {
                let mut n = 0;
                loop {
                    let rc = unsafe { sqlite3_step(*statement) };
                    n += 1;
                    match rc {
                        SQLITE_DONE => break,
                        SQLITE_ROW => continue,
                        _ => {
                            return Err(unsafe {
                                SQLiteError::from_latest(sqlite3_db_handle(*statement), rc)
                            })
                        }
                    }
                }
                Ok(n)
            }
            StatementInner::Buffered { result, .. } => Ok(result.rows.len()),
        }
    }

    pub fn bind_int64(&self, i: i32, value: i64) {
        if let StatementInner::Local { statement } = &self.inner {
            unsafe { sqlite3_bind_int64(*statement, i, value) };
        }
        // Buffered statements have params already bound on the server
    }
    pub fn bind_double(&self, i: i32, value: f64) {
        if let StatementInner::Local { statement } = &self.inner {
            unsafe { sqlite3_bind_double(*statement, i, value) };
        }
    }
    pub fn bind_null(&self, i: i32) {
        if let StatementInner::Local { statement } = &self.inner {
            unsafe { sqlite3_bind_null(*statement, i) };
        }
    }
    pub fn bind_blob(&self, i: i32, value: &[u8]) {
        if let StatementInner::Local { statement } = &self.inner {
            unsafe {
                sqlite3_bind_blob(
                    *statement,
                    i,
                    value.as_ptr().cast(),
                    value.len().try_into().unwrap(),
                    SQLITE_TRANSIENT(),
                )
            };
        }
    }

    /// # Safety
    /// `p` must be a valid pointer for the given pointer type `name`.
    pub unsafe fn bind_pointer(&self, i: i32, p: *mut c_void, name: &CStr) {
        if let StatementInner::Local { statement } = &self.inner {
            unsafe { sqlite3_bind_pointer(*statement, i, p, name.as_ptr(), None) };
        }
    }

    pub fn bind_text<S: AsRef<str>>(&self, i: i32, value: S) {
        if let StatementInner::Local { statement } = &self.inner {
            let n = value.as_ref().len();
            let s = CString::new(value.as_ref()).unwrap();
            let _rc = unsafe {
                sqlite3_bind_text(*statement, i, s.as_ptr(), n as i32, SQLITE_TRANSIENT())
            };
        }
    }

    pub fn bind_parameters(&self) -> Vec<String> {
        match &self.inner {
            StatementInner::Local { statement } => unsafe {
                let n = sqlite3_bind_parameter_count(*statement);
                let mut bind_parameters = vec![];
                for i in 0..n {
                    let name = sqlite3_bind_parameter_name(*statement, i + 1);
                    let name = CStr::from_ptr(name).to_string_lossy().to_string();
                    bind_parameters.push(name);
                }
                bind_parameters
            },
            StatementInner::Buffered { .. } => {
                // Buffered statements already have params bound
                vec![]
            }
        }
    }

    pub fn parameter_info(&self) -> Vec<String> {
        self.bind_parameters()
    }

    pub fn reset(&self) {
        match &self.inner {
            StatementInner::Local { statement } => {
                unsafe { sqlite3_reset(*statement) };
            }
            StatementInner::Buffered { cursor, .. } => {
                cursor.set(0);
            }
        }
    }

    pub fn readonly(&self) -> bool {
        match &self.inner {
            StatementInner::Local { statement } => {
                unsafe { sqlite3_stmt_readonly(*statement) != 0 }
            }
            StatementInner::Buffered { result, .. } => result.readonly,
        }
    }

    /// Get the raw statement pointer. Panics on buffered statements.
    pub fn pointer(&self) -> *mut sqlite3_stmt {
        match &self.inner {
            StatementInner::Local { statement } => *statement,
            StatementInner::Buffered { .. } => {
                panic!("Cannot access raw sqlite3_stmt pointer on a buffered statement")
            }
        }
    }
}

impl Drop for Statement {
    fn drop(&mut self) {
        if let StatementInner::Local { statement } = &self.inner {
            unsafe {
                sqlite3_finalize(*statement);
            }
        }
        // Buffered statements have no resources to free
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
    pub subprog: String,
    pub nexec: i64,
    pub ncycle: i64,
}
/// # Safety
/// `pstmt` must be a valid, non-null pointer to a prepared sqlite3 statement.
pub unsafe fn bytecode_steps(pstmt: *mut sqlite3_stmt) -> Vec<BytecodeStep> {
    let mut steps = vec![];
    unsafe {
        let db: *mut sqlite3 = sqlite3_db_handle(pstmt);
        let db = Connection::from_local(db, false);

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
                    let subprog = row.value_at(8).as_str().to_owned();
                    let nexec = row.value_at(9).as_int64();
                    let ncycle = row.value_at(10).as_int64();
                    steps.push(BytecodeStep {
                        addr,
                        opcode: opcode.to_string(),
                        p1,
                        p2,
                        p3,
                        p4: p4.to_string(),
                        p5,
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
/// Transport for communicating with a remote `solite serve` process over SSH.
pub struct RemoteTransport {
    child: std::process::Child,
    reader: std::io::BufReader<std::process::ChildStdout>,
    writer: std::io::BufWriter<std::process::ChildStdin>,
}

// NOT Sync, sqlite limitation
unsafe impl std::marker::Send for RemoteTransport {}

impl RemoteTransport {
    fn send_request(&mut self, request: &crate::rpc::Request) -> Result<crate::rpc::Response, SQLiteError> {
        crate::rpc::write_frame(&mut self.writer, request)
            .map_err(|e| SQLiteError {
                result_code: -1,
                code_description: "IO_ERROR".to_string(),
                message: format!("Failed to send request to remote: {}", e),
                offset: None,
            })?;
        crate::rpc::read_frame(&mut self.reader)
            .map_err(|e| SQLiteError {
                result_code: -1,
                code_description: "IO_ERROR".to_string(),
                message: format!("Failed to read response from remote: {}", e),
                offset: None,
            })
    }
}

/// https://www.sqlite.org/c3ref/sqlite3.html
pub struct Connection {
    inner: ConnectionInner,
    /// Shared with [`InterruptHandle`]s handed out by [`Connection::interrupt_handle`].
    /// Nulled (under the lock) before the database is closed so a handle can
    /// never interrupt a freed connection.
    interrupt_db: Arc<StdMutex<*mut sqlite3>>,
}

enum ConnectionInner {
    Local {
        connection: *mut sqlite3,
        _owned: bool,
    },
    Remote {
        transport: RefCell<RemoteTransport>,
    },
}

// NOT Sync, sqlite limitation
unsafe impl std::marker::Send for Connection {}
unsafe impl std::marker::Send for Statement {}

/// A handle for interrupting an in-flight statement on a [`Connection`] from
/// another thread, via `sqlite3_interrupt` (which is documented as safe to
/// call from any thread on a live connection).
///
/// The handle outliving its `Connection` is safe: dropping the connection
/// nulls the shared pointer, turning `interrupt()` into a no-op.
#[derive(Clone)]
pub struct InterruptHandle {
    db: Arc<StdMutex<*mut sqlite3>>,
}

unsafe impl std::marker::Send for InterruptHandle {}
unsafe impl std::marker::Sync for InterruptHandle {}

impl InterruptHandle {
    /// https://www.sqlite.org/c3ref/interrupt.html
    pub fn interrupt(&self) {
        let db = self.db.lock().unwrap();
        if !db.is_null() {
            unsafe { sqlite3_interrupt(*db) };
        }
    }
}

#[derive(Debug)]
pub struct PrepareError {
    pub code: i32,
    pub code_str: String,
    pub message: String,
}

impl Connection {
    fn from_local(connection: *mut sqlite3, owned: bool) -> Self {
        Connection {
            inner: ConnectionInner::Local {
                connection,
                _owned: owned,
            },
            interrupt_db: Arc::new(StdMutex::new(connection)),
        }
    }

    fn from_remote(transport: RemoteTransport) -> Self {
        Connection {
            inner: ConnectionInner::Remote {
                transport: RefCell::new(transport),
            },
            interrupt_db: Arc::new(StdMutex::new(ptr::null_mut())),
        }
    }

    /// Get a thread-safe handle that can interrupt statements running on this
    /// connection. No-op for remote connections.
    pub fn interrupt_handle(&self) -> InterruptHandle {
        InterruptHandle {
            db: Arc::clone(&self.interrupt_db),
        }
    }

    pub fn open(path: &str) -> Result<Self, SQLiteError> {
        let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_FULLMUTEX | SQLITE_OPEN_CREATE | SQLITE_OPEN_URI;
        Self::open_with_flags(path, flags)
    }

    pub fn open_readonly(path: &str) -> Result<Self, SQLiteError> {
        let flags = SQLITE_OPEN_READONLY | SQLITE_OPEN_FULLMUTEX | SQLITE_OPEN_URI;
        Self::open_with_flags(path, flags)
    }

    fn open_with_flags(path: &str, flags: i32) -> Result<Self, SQLiteError> {
        let mut connection: *mut sqlite3 = ptr::null_mut();
        let filename = CString::new(path).unwrap();
        let rc =
            unsafe { sqlite3_open_v2(filename.as_ptr(), &mut connection, flags, ptr::null_mut()) };
        if rc == SQLITE_OK {
            unsafe {
                sqlite3_enable_load_extension(connection, 1);
            }
            Ok(Connection::from_local(connection, true))
        } else {
            let err = unsafe { SQLiteError::from_latest(connection, rc) };
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
                sqlite3_db_config(connection, SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION, 1, &v);
                sqlite3_db_config(connection, 1018, 1, &v);
            }
            Ok(Connection::from_local(connection, true))
        } else {
            let err = unsafe { SQLiteError::from_latest(connection, rc) };
            unsafe {
                sqlite3_close(connection);
            }
            Err(err)
        }
    }

    /// Open a remote database over SSH.
    ///
    /// Parses an `ssh://[user@]host[:port]/path` URL and spawns an SSH process
    /// running `solite serve <path>` on the remote host.
    ///
    /// `remote_bin` optionally specifies the absolute path to the `solite` binary
    /// on the remote machine. If `None`, uses `"solite"` (must be on remote `$PATH`).
    pub fn open_remote(url: &str) -> Result<Self, SQLiteError> {
        Self::open_remote_with_bin(url, None)
    }

    pub fn open_remote_with_bin(url: &str, remote_bin: Option<&str>) -> Result<Self, SQLiteError> {
        let (user, host, port, db_path) = parse_remote_path(url).map_err(|msg| SQLiteError {
            result_code: -1,
            code_description: "SSH_ERROR".to_string(),
            message: msg,
            offset: None,
        })?;

        let mut cmd = std::process::Command::new("ssh");
        if let Some(port) = port {
            cmd.arg("-p").arg(port.to_string());
        }
        let target = match user {
            Some(u) => format!("{}@{}", u, host),
            None => host,
        };
        let bin = remote_bin.unwrap_or("solite");
        cmd.arg(&target)
            .arg(bin)
            .arg("serve")
            .arg(&db_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());

        let mut child = cmd.spawn().map_err(|e| SQLiteError {
            result_code: -1,
            code_description: "SSH_ERROR".to_string(),
            message: format!("Failed to spawn ssh: {}", e),
            offset: None,
        })?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let mut transport = RemoteTransport {
            child,
            reader: std::io::BufReader::new(stdout),
            writer: std::io::BufWriter::new(stdin),
        };

        // Verify the connection works by sending a ping request.
        // This catches SSH failures (bad hostname, auth rejected, remote binary not found).
        let response = transport.send_request(&crate::rpc::Request::InTransaction);
        match response {
            Ok(crate::rpc::Response::InTransaction { .. }) => {}
            Ok(other) => {
                let _ = transport.child.wait();
                return Err(SQLiteError {
                    result_code: -1,
                    code_description: "SSH_ERROR".to_string(),
                    message: format!("Unexpected response from remote: {:?}", other),
                    offset: None,
                });
            }
            Err(e) => {
                // Wait for the child to finish so we don't leave zombies
                let _ = transport.child.wait();
                return Err(SQLiteError {
                    result_code: -1,
                    code_description: "SSH_ERROR".to_string(),
                    message: format!("Failed to connect to remote database: {}", e.message),
                    offset: None,
                });
            }
        }

        Ok(Connection::from_remote(transport))
    }

    /// Open a remote database via a custom transport command.
    ///
    /// `transport_cmd` is a shell command prefix (e.g. `"fly ssh console -a my-app -C"`).
    /// We append `<remote_bin> serve <db_path>` and spawn it.
    pub fn open_transport(transport_cmd: &str, db_path: &str, remote_bin: Option<&str>) -> Result<Self, SQLiteError> {
        let bin = remote_bin.unwrap_or("solite");
        let full_cmd = format!("{} {} serve {}", transport_cmd, bin, db_path);

        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg(&full_cmd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .map_err(|e| SQLiteError {
                result_code: -1,
                code_description: "TRANSPORT_ERROR".to_string(),
                message: format!("Failed to spawn transport '{}': {}", full_cmd, e),
                offset: None,
            })?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let mut transport = RemoteTransport {
            child,
            reader: std::io::BufReader::new(stdout),
            writer: std::io::BufWriter::new(stdin),
        };

        let response = transport.send_request(&crate::rpc::Request::InTransaction);
        match response {
            Ok(crate::rpc::Response::InTransaction { .. }) => {}
            Ok(other) => {
                let _ = transport.child.wait();
                return Err(SQLiteError {
                    result_code: -1,
                    code_description: "TRANSPORT_ERROR".to_string(),
                    message: format!("Unexpected response from remote: {:?}", other),
                    offset: None,
                });
            }
            Err(e) => {
                let _ = transport.child.wait();
                return Err(SQLiteError {
                    result_code: -1,
                    code_description: "TRANSPORT_ERROR".to_string(),
                    message: format!("Failed to connect via transport: {}", e.message),
                    offset: None,
                });
            }
        }

        Ok(Connection::from_remote(transport))
    }

    /// Returns true if this is a remote (SSH) connection.
    pub fn is_remote(&self) -> bool {
        matches!(self.inner, ConnectionInner::Remote { .. })
    }

    /// # Safety
    /// The returned pointer must not outlive the `Connection`.
    /// Panics if called on a remote connection.
    pub unsafe fn db(&self) -> *mut sqlite3 {
        match &self.inner {
            ConnectionInner::Local { connection, .. } => *connection,
            ConnectionInner::Remote { .. } => {
                panic!("Cannot access raw sqlite3 pointer on a remote connection")
            }
        }
    }

    pub fn db_name(&self) -> Option<String> {
        match &self.inner {
            ConnectionInner::Local { connection, .. } => unsafe {
                let filename: *const sqlite3_filename =
                    sqlite3_db_filename(*connection, c"main".as_ptr()).cast();
                if !filename.is_null() {
                    let s = sqlite3_filename_database(filename.cast());
                    let s = CStr::from_ptr(s).to_string_lossy();
                    Some(s.to_string())
                } else {
                    None
                }
            },
            ConnectionInner::Remote { .. } => {
                // For remote connections, we don't have the db name locally.
                // Could be fetched via RPC if needed.
                None
            }
        }
    }

    pub fn in_transaction(&self) -> bool {
        match &self.inner {
            ConnectionInner::Local { connection, .. } => {
                unsafe { sqlite3_get_autocommit(*connection) == 0 }
            }
            ConnectionInner::Remote { transport } => {
                let request = crate::rpc::Request::InTransaction;
                match transport.borrow_mut().send_request(&request) {
                    Ok(crate::rpc::Response::InTransaction { value }) => value,
                    _ => false,
                }
            }
        }
    }

    pub fn load_extension(&self, path: &str, entrypoint: &Option<String>) -> anyhow::Result<()> {
        match &self.inner {
            ConnectionInner::Local { .. } => {
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
                    return Err(anyhow::anyhow!("Loading extension failed: {s}"));
                }
                Ok(())
            }
            ConnectionInner::Remote { .. } => {
                Err(anyhow::anyhow!("Cannot load extensions on a remote connection"))
            }
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn execute(&self, sql: &str) -> Result<usize, ()> {
        let stmt = self.prepare(sql).unwrap().1.unwrap();
        Ok(stmt.execute().unwrap())
    }

    pub fn execute_script(&self, sql: &str) -> Result<(), SQLiteError> {
        match &self.inner {
            ConnectionInner::Local { connection, .. } => {
                let z_sql = CString::new(sql).unwrap();
                let rc = unsafe {
                    sqlite3_exec(*connection, z_sql.as_ptr(), None, ptr::null_mut(), ptr::null_mut())
                };
                if rc == SQLITE_OK {
                    Ok(())
                } else {
                    Err(unsafe { SQLiteError::from_latest(*connection, rc) })
                }
            }
            ConnectionInner::Remote { transport } => {
                let request = crate::rpc::Request::ExecuteScript { sql: sql.to_string() };
                let response = transport.borrow_mut().send_request(&request)?;
                match response {
                    crate::rpc::Response::ScriptOk => Ok(()),
                    crate::rpc::Response::Error(e) => Err(e),
                    other => Err(SQLiteError {
                        result_code: -1,
                        code_description: "PROTOCOL_ERROR".to_string(),
                        message: format!("Unexpected response: {:?}", other),
                        offset: None,
                    }),
                }
            }
        }
    }

    pub fn set_progress_handler<F, T>(&self, ops: i32, handle: Option<F>, aux: T)
    where
        F: FnMut(&T) -> bool + Send + 'static,
    {
        match &self.inner {
            ConnectionInner::Local { connection, .. } => {
                unsafe extern "C" fn call_boxed_closure<F, T>(p_arg: *mut c_void) -> c_int
                where
                    F: FnMut(&T) -> bool,
                {
                    let x = p_arg.cast::<(*mut F, *mut T)>();
                    let r = (*((*x).0))(&(*(*x).1));
                    if r { 1 } else { 0 }
                }
                if let Some(handle) = handle {
                    unsafe {
                        let x: *mut F = Box::into_raw(Box::new(handle));
                        let y: *mut T = Box::into_raw(Box::new(aux));
                        let boxed_handler = Box::into_raw(Box::new((x, y)));
                        sqlite3_progress_handler(
                            *connection,
                            ops,
                            Some(call_boxed_closure::<F, T>),
                            boxed_handler.cast(),
                        );
                    }
                }
            }
            ConnectionInner::Remote { .. } => {
                // Progress handlers don't apply to remote connections.
                // The server executes fully and returns results.
            }
        }
    }

    pub fn serialize(&self) -> Result<Vec<u8>, SQLiteError> {
        match &self.inner {
            ConnectionInner::Local { connection, .. } => unsafe {
                let mut sz: sqlite3_int64 = 0;
                let ptr = sqlite3_serialize(*connection, c"main".as_ptr(), &mut sz, 0);
                if ptr.is_null() {
                    return Err(SQLiteError::from_latest(
                        *connection,
                        sqlite3_errcode(*connection),
                    ));
                }
                let slice = std::slice::from_raw_parts(ptr as *const u8, sz as usize);
                let vec = slice.to_vec();
                sqlite3_free(ptr.cast());
                Ok(vec)
            },
            ConnectionInner::Remote { transport } => {
                let request = crate::rpc::Request::Serialize;
                let response = transport.borrow_mut().send_request(&request)?;
                match response {
                    crate::rpc::Response::Serialized { data } => Ok(data),
                    crate::rpc::Response::Error(e) => Err(e),
                    other => Err(SQLiteError {
                        result_code: -1,
                        code_description: "PROTOCOL_ERROR".to_string(),
                        message: format!("Unexpected response: {:?}", other),
                        offset: None,
                    }),
                }
            }
        }
    }

    pub fn prepare(&self, sql: &str) -> Result<(Option<usize>, Option<Statement>), SQLiteError> {
        match &self.inner {
            ConnectionInner::Local { connection, .. } => {
                let z_sql = CString::new(sql).unwrap();
                let mut stmt: *mut sqlite3_stmt = ptr::null_mut();
                let head = z_sql.as_ptr();
                let mut tail: *const c_char = ptr::null_mut();
                let rc = unsafe {
                    sqlite3_prepare_v2(*connection, head, -1, &mut stmt, &mut tail)
                };
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
                        Ok((
                            rest,
                            Some(Statement {
                                inner: StatementInner::Local { statement: stmt },
                            }),
                        ))
                    }
                } else {
                    Err(unsafe { SQLiteError::from_latest(*connection, rc) })
                }
            }
            ConnectionInner::Remote { transport } => {
                let request = crate::rpc::Request::Query {
                    sql: sql.to_string(),
                    params: vec![],
                };
                let response = transport.borrow_mut().send_request(&request)?;
                match response {
                    crate::rpc::Response::Query(result) => {
                        if result.columns.is_empty() && result.rows.is_empty() {
                            Ok((None, None))
                        } else {
                            Ok((
                                None,
                                Some(Statement {
                                    inner: StatementInner::Buffered {
                                        result,
                                        cursor: std::cell::Cell::new(0),
                                    },
                                }),
                            ))
                        }
                    }
                    crate::rpc::Response::Error(e) => Err(e),
                    other => Err(SQLiteError {
                        result_code: -1,
                        code_description: "PROTOCOL_ERROR".to_string(),
                        message: format!("Unexpected response: {:?}", other),
                        offset: None,
                    }),
                }
            }
        }
    }

    /// Prepare and execute a query on a remote connection, returning a buffered statement.
    /// This is the main entry point for remote SQL execution.
    /// Prepare and execute a query on a remote connection with parameters, returning a buffered statement.
    pub fn prepare_remote(&self, sql: &str, params: Vec<(String, OwnedValue)>) -> Result<(Option<usize>, Option<Statement>), SQLiteError> {
        match &self.inner {
            ConnectionInner::Remote { transport } => {
                let request = crate::rpc::Request::Query {
                    sql: sql.to_string(),
                    params,
                };
                let response = transport.borrow_mut().send_request(&request)?;
                match response {
                    crate::rpc::Response::Query(result) => {
                        if result.columns.is_empty() && result.rows.is_empty() {
                            Ok((None, None))
                        } else {
                            Ok((
                                None,
                                Some(Statement {
                                    inner: StatementInner::Buffered {
                                        result,
                                        cursor: std::cell::Cell::new(0),
                                    },
                                }),
                            ))
                        }
                    }
                    crate::rpc::Response::Error(e) => Err(e),
                    other => Err(SQLiteError {
                        result_code: -1,
                        code_description: "PROTOCOL_ERROR".to_string(),
                        message: format!("Unexpected response: {:?}", other),
                        offset: None,
                    }),
                }
            }
            ConnectionInner::Local { .. } => {
                // For local, just delegate to prepare()
                self.prepare(sql)
            }
        }
    }

    /// https://www.sqlite.org/c3ref/interrupt.html
    pub fn is_interrupted(&self) -> bool {
        match &self.inner {
            ConnectionInner::Local { connection, .. } => {
                unsafe { sqlite3_is_interrupted(*connection) != 0 }
            }
            ConnectionInner::Remote { .. } => false,
        }
    }

    /// https://www.sqlite.org/c3ref/interrupt.html
    pub fn interrupt(&self) {
        match &self.inner {
            ConnectionInner::Local { connection, .. } => {
                unsafe { sqlite3_interrupt(*connection) };
            }
            ConnectionInner::Remote { .. } => {
                // For remote, we could kill the SSH process or send an interrupt request.
                // For now, this is a no-op.
            }
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        match &mut self.inner {
            ConnectionInner::Local { connection, _owned } => {
                // Hold the lock across close so an InterruptHandle on another
                // thread can't interrupt mid-close; afterwards handles see null.
                let mut interrupt_db = self.interrupt_db.lock().unwrap();
                *interrupt_db = ptr::null_mut();
                if *_owned {
                    unsafe { sqlite3_close(*connection) };
                }
            }
            ConnectionInner::Remote { transport } => {
                // Try to send Close request, ignore errors
                let mut t = transport.borrow_mut();
                let _ = t.send_request(&crate::rpc::Request::Close);
                let _ = t.child.wait();
            }
        }
    }
}

/// Check if a path string refers to a remote database.
///
/// Supports two formats:
/// - URL-style: `ssh://[user@]host[:port]/path`
/// - scp-style: `[user@]host:/path` (same as sqlite3_rsync)
///
/// The scp-style detection mirrors sqlite3_rsync's `hostSeparator()`:
/// a `:` with no `/` or `\` before it indicates a remote host.
pub fn is_remote_path(s: &str) -> bool {
    if s.starts_with("ssh://") {
        return true;
    }
    host_separator(s).is_some()
}

/// Find the host:path separator in an scp-style remote path.
/// Returns the byte index of the `:` if found, or None if this is a local path.
///
/// Mirrors sqlite3_rsync's `hostSeparator()` logic:
/// - Find the first `:`
/// - If any `/` or `\` appears before it, it's a local path (not a host separator)
/// - On Windows, skip drive letters like `C:\`
fn host_separator(s: &str) -> Option<usize> {
    let colon_pos = s.find(':')?;

    // Must have something before the colon (the host part)
    if colon_pos == 0 {
        return None;
    }

    // The part after colon must start with / (an absolute path on the remote)
    if !s.get(colon_pos + 1..).map_or(false, |rest| rest.starts_with('/')) {
        return None;
    }

    // Skip Windows drive letters (e.g. C:\)
    #[cfg(windows)]
    if colon_pos == 1
        && s.as_bytes()[0].is_ascii_alphabetic()
        && s.get(2..3).map_or(false, |c| c == "/" || c == "\\")
    {
        return None;
    }

    // If any / or \ appears before the colon, it's a local path
    if s[..colon_pos].contains('/') || s[..colon_pos].contains('\\') {
        return None;
    }

    Some(colon_pos)
}

/// Parse a remote path into (user, host, port, path) components.
///
/// Accepts both formats:
/// - `ssh://[user@]host[:port]/path`
/// - `[user@]host:/path` (scp-style)
fn parse_remote_path(input: &str) -> Result<(Option<String>, String, Option<u16>, String), String> {
    if let Some(rest) = input.strip_prefix("ssh://") {
        // URL-style: ssh://[user@]host[:port]/path
        let (authority, path) = rest
            .split_once('/')
            .ok_or_else(|| "URL must contain a path after host".to_string())?;

        let path = format!("/{}", path);

        let (userhost, port) = if let Some((uh, p)) = authority.rsplit_once(':') {
            match p.parse::<u16>() {
                Ok(port) => (uh, Some(port)),
                Err(_) => (authority, None),
            }
        } else {
            (authority, None)
        };

        let (user, host) = if let Some((u, h)) = userhost.split_once('@') {
            (Some(u.to_string()), h.to_string())
        } else {
            (None, userhost.to_string())
        };

        Ok((user, host, port, path))
    } else if let Some(colon_pos) = host_separator(input) {
        // scp-style: [user@]host:/path
        let userhost = &input[..colon_pos];
        let path = &input[colon_pos + 1..];

        let (user, host) = if let Some((u, h)) = userhost.split_once('@') {
            (Some(u.to_string()), h.to_string())
        } else {
            (None, userhost.to_string())
        };

        Ok((user, host, None, path.to_string()))
    } else {
        Err(format!("Not a remote path: {}", input))
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
    fn test_column_meta_nullable() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_script(
            "CREATE TABLE t(a INTEGER NOT NULL, b TEXT, c INTEGER PRIMARY KEY)",
        )
        .unwrap();

        let (_, stmt) = conn
            .prepare("SELECT a, b, c, a + 1 AS expr FROM t")
            .unwrap();
        let stmt = stmt.unwrap();
        let meta = stmt.column_meta();

        assert_eq!(meta[0].name, "a");
        assert_eq!(meta[0].nullable, Some(false));

        assert_eq!(meta[1].name, "b");
        assert_eq!(meta[1].nullable, Some(true));

        // INTEGER PRIMARY KEY is nullable at the SQL level (it's an alias for ROWID).
        assert_eq!(meta[2].name, "c");
        assert_eq!(meta[2].nullable, Some(true));

        // Expression columns have no origin column, so nullability is unknown.
        assert_eq!(meta[3].name, "expr");
        assert_eq!(meta[3].nullable, None);
    }

    #[test]
    fn test_open_readonly_blocks_writes() {
        // Create a database with a table first
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();
        {
            let conn = Connection::open(db_str).unwrap();
            conn.execute_script("CREATE TABLE t(a TEXT)").unwrap();
            conn.execute_script("INSERT INTO t VALUES ('hello')").unwrap();
        }

        // Open readonly and verify reads work
        let conn = Connection::open_readonly(db_str).unwrap();
        let (_, stmt) = conn.prepare("SELECT * FROM t").unwrap();
        let stmt = stmt.unwrap();
        assert!(stmt.readonly());
        let row = stmt.next().unwrap();
        assert!(row.is_some());

        // Verify writes are blocked
        let result = conn.execute_script("INSERT INTO t VALUES ('world')");
        assert!(result.is_err());
    }

    #[test]
    fn test_is_remote_path() {
        // ssh:// URLs
        assert!(is_remote_path("ssh://user@host/path/to/db"));
        assert!(is_remote_path("ssh://host/path"));
        assert!(is_remote_path("ssh://user@host:2222/path"));

        // scp-style
        assert!(is_remote_path("user@host:/path/to/db"));
        assert!(is_remote_path("host:/path/to/db"));
        assert!(is_remote_path("user@myserver.com:/data/app.db"));

        // Local paths (not remote)
        assert!(!is_remote_path("/path/to/db"));
        assert!(!is_remote_path("relative/path.db"));
        assert!(!is_remote_path("./db.sqlite"));
        assert!(!is_remote_path(":memory:"));
    }

    #[test]
    fn test_parse_remote_path_ssh_url() {
        let (user, host, port, path) =
            parse_remote_path("ssh://alex@myhost/data/app.db").unwrap();
        assert_eq!(user.as_deref(), Some("alex"));
        assert_eq!(host, "myhost");
        assert_eq!(port, None);
        assert_eq!(path, "/data/app.db");

        let (user, host, port, path) =
            parse_remote_path("ssh://alex@myhost:2222/data/app.db").unwrap();
        assert_eq!(user.as_deref(), Some("alex"));
        assert_eq!(host, "myhost");
        assert_eq!(port, Some(2222));
        assert_eq!(path, "/data/app.db");
    }

    #[test]
    fn test_parse_remote_path_scp_style() {
        let (user, host, port, path) =
            parse_remote_path("alex@myhost:/data/app.db").unwrap();
        assert_eq!(user.as_deref(), Some("alex"));
        assert_eq!(host, "myhost");
        assert_eq!(port, None);
        assert_eq!(path, "/data/app.db");

        let (user, host, port, path) =
            parse_remote_path("myhost:/data/app.db").unwrap();
        assert_eq!(user, None);
        assert_eq!(host, "myhost");
        assert_eq!(port, None);
        assert_eq!(path, "/data/app.db");
    }

    #[test]
    fn test_open_readonly_nonexistent_db_fails() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("does_not_exist.db");
        let result = Connection::open_readonly(db_path.to_str().unwrap());
        assert!(result.is_err());
    }
}
