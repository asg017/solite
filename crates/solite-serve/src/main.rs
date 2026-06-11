use solite_core::rpc::{read_frame, write_frame, QueryResult, Request, Response, WireValue};
use solite_core::sqlite::{Connection, OwnedValue};
use solite_stdlib::solite_stdlib_init;
use std::io::{self, BufReader, BufWriter};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: solite-serve <database-path>");
        return ExitCode::FAILURE;
    }
    let db_path = &args[1];

    let connection = match Connection::open(db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to open database: {}", e);
            return ExitCode::FAILURE;
        }
    };
    unsafe {
        solite_stdlib_init(connection.db(), std::ptr::null_mut(), std::ptr::null_mut());
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());

    loop {
        let request: Request = match read_frame(&mut reader) {
            Ok(req) => req,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                eprintln!("Failed to read request: {}", e);
                return ExitCode::FAILURE;
            }
        };

        let response = handle_request(&connection, request);
        let is_close = matches!(response, Response::Closed);

        if let Err(e) = write_frame(&mut writer, &response) {
            eprintln!("Failed to write response: {}", e);
            return ExitCode::FAILURE;
        }

        if is_close {
            break;
        }
    }

    ExitCode::SUCCESS
}

fn handle_request(connection: &Connection, request: Request) -> Response {
    match request {
        Request::Query { sql, params } => handle_query(connection, &sql, &params),
        Request::Execute { sql, params } => handle_execute(connection, &sql, &params),
        Request::ExecuteScript { sql } => match connection.execute_script(&sql) {
            Ok(()) => Response::ScriptOk,
            Err(e) => Response::Error(e),
        },
        Request::DbName => Response::DbName {
            name: connection.db_name(),
        },
        Request::InTransaction => Response::InTransaction {
            value: connection.in_transaction(),
        },
        Request::Interrupt => {
            connection.interrupt();
            Response::Interrupted
        }
        Request::Serialize => match connection.serialize() {
            Ok(data) => Response::Serialized { data },
            Err(e) => Response::Error(e),
        },
        Request::Close => Response::Closed,
    }
}

fn handle_query(connection: &Connection, sql: &str, params: &[(String, OwnedValue)]) -> Response {
    let (_, stmt) = match connection.prepare(sql) {
        Ok((remaining, Some(stmt))) => (remaining, stmt),
        Ok((_, None)) => {
            return Response::Query(QueryResult {
                sql: sql.to_string(),
                columns: vec![],
                rows: vec![],
                readonly: true,
                is_explain: None,
            });
        }
        Err(e) => return Response::Error(e),
    };

    if let Err(e) = bind_params(&stmt, params) {
        return Response::Error(e);
    }

    let columns = stmt.column_meta();
    let readonly = stmt.readonly();
    let is_explain = stmt.is_explain().map(|e| match e {
        solite_core::sqlite::IsExplain::Explain => 1,
        solite_core::sqlite::IsExplain::ExplainQueryPlan => 2,
    });
    let stmt_sql = stmt.sql();

    let mut rows = Vec::new();
    loop {
        match stmt.next() {
            Ok(Some(row)) => {
                let wire_row: Vec<WireValue> = row
                    .iter()
                    .map(|v| WireValue {
                        value: OwnedValue::from_value_ref(v),
                        subtype: v.subtype(),
                    })
                    .collect();
                rows.push(wire_row);
            }
            Ok(None) => break,
            Err(e) => return Response::Error(e),
        }
    }

    Response::Query(QueryResult {
        sql: stmt_sql,
        columns,
        rows,
        readonly,
        is_explain,
    })
}

fn handle_execute(
    connection: &Connection,
    sql: &str,
    params: &[(String, OwnedValue)],
) -> Response {
    let (remaining, stmt) = match connection.prepare(sql) {
        Ok((remaining, Some(stmt))) => (remaining, stmt),
        Ok((remaining, None)) => {
            return Response::Executed {
                count: 0,
                remaining_offset: remaining,
            };
        }
        Err(e) => return Response::Error(e),
    };

    if let Err(e) = bind_params(&stmt, params) {
        return Response::Error(e);
    }

    match stmt.execute() {
        Ok(count) => Response::Executed {
            count,
            remaining_offset: remaining,
        },
        Err(e) => Response::Error(e),
    }
}

fn bind_params(
    stmt: &solite_core::sqlite::Statement,
    params: &[(String, OwnedValue)],
) -> Result<(), solite_core::sqlite::SQLiteError> {
    for (name, value) in params {
        let bind_params = stmt.bind_parameters();
        if let Some(idx) = bind_params.iter().position(|p| {
            p.trim_start_matches([':', '$', '@', '?'])
                == name.trim_start_matches([':', '$', '@', '?'])
        }) {
            let idx = (idx + 1) as i32;
            match value {
                OwnedValue::Null => stmt.bind_null(idx)?,
                OwnedValue::Integer(v) => stmt.bind_int64(idx, *v)?,
                OwnedValue::Double(v) => stmt.bind_double(idx, *v)?,
                OwnedValue::Text(v) => {
                    let s = String::from_utf8_lossy(v);
                    stmt.bind_text(idx, s.as_ref())?;
                }
                OwnedValue::Blob(v) => stmt.bind_blob(idx, v)?,
            }
        }
    }
    Ok(())
}
