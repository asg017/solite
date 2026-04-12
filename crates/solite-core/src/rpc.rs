use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

use crate::sqlite::{ColumnMeta, OwnedValue, SQLiteError};

/// A value sent over the wire, with subtype preserved (needed for JSON subtype=74).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WireValue {
    pub value: OwnedValue,
    pub subtype: Option<u32>,
}

/// Complete result of executing a single SQL statement on the server.
#[derive(Serialize, Deserialize, Debug)]
pub struct QueryResult {
    pub sql: String,
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<WireValue>>,
    pub readonly: bool,
    pub is_explain: Option<u8>,
}

/// Client → Server request.
#[derive(Serialize, Deserialize, Debug)]
pub enum Request {
    /// Prepare, bind params, execute fully, return complete result with all rows.
    Query {
        sql: String,
        params: Vec<(String, OwnedValue)>,
    },

    /// Execute a statement to completion, return remaining offset and row count.
    /// Used for write statements (INSERT/UPDATE/DELETE) and multi-statement parsing.
    Execute {
        sql: String,
        params: Vec<(String, OwnedValue)>,
    },

    /// Execute multiple statements via sqlite3_exec. For scripts.
    ExecuteScript { sql: String },

    /// Get the database filename.
    DbName,

    /// Check if the connection is in a transaction.
    InTransaction,

    /// Interrupt a long-running query.
    Interrupt,

    /// Serialize the entire database to bytes.
    Serialize,

    /// Close the connection and shut down the server.
    Close,
}

/// Server → Client response.
#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    /// Full query result with all rows materialized.
    Query(QueryResult),

    /// Result of an Execute request.
    Executed {
        count: usize,
        remaining_offset: Option<usize>,
    },

    /// ExecuteScript completed successfully.
    ScriptOk,

    /// Database filename.
    DbName { name: Option<String> },

    /// Transaction state.
    InTransaction { value: bool },

    /// Interrupt acknowledged.
    Interrupted,

    /// Serialized database bytes.
    Serialized { data: Vec<u8> },

    /// Server shutting down.
    Closed,

    /// An error occurred.
    Error(SQLiteError),
}

/// Read a length-prefixed MessagePack frame from a reader.
pub fn read_frame<R: Read, T: for<'de> Deserialize<'de>>(reader: &mut R) -> io::Result<T> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;

    rmp_serde::from_slice(&buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Write a length-prefixed MessagePack frame to a writer.
pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> io::Result<()> {
    let buf = rmp_serde::to_vec(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let len = buf.len() as u32;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(&buf)?;
    writer.flush()?;
    Ok(())
}
