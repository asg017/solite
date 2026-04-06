use solite_core::rpc::{read_frame, write_frame, Request, Response};
use std::io::{BufReader, BufWriter};
use std::process::{Command, Stdio};

fn solite_binary() -> String {
    let mut path = std::env::current_exe().unwrap();
    // Navigate from test binary to the cargo target dir
    path.pop(); // remove test binary name
    path.pop(); // remove "deps"
    path.push("solite");
    path.to_string_lossy().to_string()
}

#[test]
fn test_serve_query_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let mut child = Command::new(solite_binary())
        .arg("serve")
        .arg(db_path.to_str().unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn solite serve");

    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut writer = BufWriter::new(stdin);
    let mut reader = BufReader::new(stdout);

    // Create a table via ExecuteScript
    let req = Request::ExecuteScript {
        sql: "CREATE TABLE users (id INTEGER, name TEXT); INSERT INTO users VALUES (1, 'Alice'), (2, 'Bob');".to_string(),
    };
    write_frame(&mut writer, &req).unwrap();
    let resp: Response = read_frame(&mut reader).unwrap();
    assert!(matches!(resp, Response::ScriptOk));

    // Query the table
    let req = Request::Query {
        sql: "SELECT id, name FROM users ORDER BY id".to_string(),
        params: vec![],
    };
    write_frame(&mut writer, &req).unwrap();
    let resp: Response = read_frame(&mut reader).unwrap();

    match resp {
        Response::Query(result) => {
            assert_eq!(result.columns.len(), 2);
            assert_eq!(result.columns[0].name, "id");
            assert_eq!(result.columns[1].name, "name");
            assert_eq!(result.rows.len(), 2);

            // Check first row
            match &result.rows[0][0].value {
                solite_core::sqlite::OwnedValue::Integer(v) => assert_eq!(*v, 1),
                other => panic!("Expected Integer, got {:?}", other),
            }
            match &result.rows[0][1].value {
                solite_core::sqlite::OwnedValue::Text(v) => {
                    assert_eq!(std::str::from_utf8(v).unwrap(), "Alice")
                }
                other => panic!("Expected Text, got {:?}", other),
            }

            // Check second row
            match &result.rows[1][0].value {
                solite_core::sqlite::OwnedValue::Integer(v) => assert_eq!(*v, 2),
                other => panic!("Expected Integer, got {:?}", other),
            }
        }
        other => panic!("Expected Query response, got {:?}", other),
    }

    // Test InTransaction
    let req = Request::InTransaction;
    write_frame(&mut writer, &req).unwrap();
    let resp: Response = read_frame(&mut reader).unwrap();
    assert!(matches!(resp, Response::InTransaction { value: false }));

    // Close
    let req = Request::Close;
    write_frame(&mut writer, &req).unwrap();
    let resp: Response = read_frame(&mut reader).unwrap();
    assert!(matches!(resp, Response::Closed));

    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn test_serve_error_handling() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");

    let mut child = Command::new(solite_binary())
        .arg("serve")
        .arg(db_path.to_str().unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn solite serve");

    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut writer = BufWriter::new(stdin);
    let mut reader = BufReader::new(stdout);

    // Query a table that doesn't exist
    let req = Request::Query {
        sql: "SELECT * FROM nonexistent".to_string(),
        params: vec![],
    };
    write_frame(&mut writer, &req).unwrap();
    let resp: Response = read_frame(&mut reader).unwrap();

    match resp {
        Response::Error(e) => {
            assert!(e.message.contains("no such table"), "Error was: {}", e.message);
        }
        other => panic!("Expected Error response, got {:?}", other),
    }

    // Close
    let req = Request::Close;
    write_frame(&mut writer, &req).unwrap();
    let _ = read_frame::<_, Response>(&mut reader).unwrap();
    let _ = child.wait();
}
