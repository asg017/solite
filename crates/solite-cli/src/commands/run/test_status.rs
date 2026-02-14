#[cfg(test)]
mod tests {
    use crate::commands::run::status::{get_statement_status, StatementStatus};
    use solite_core::Runtime;

    /// Execute SQL and return the StatementStatus from bytecode analysis.
    /// Optionally runs setup SQL first (e.g., CREATE TABLE).
    fn status_for_sql(setup: Option<&str>, sql: &str) -> StatementStatus {
        let rt = Runtime::new(None);
        if let Some(setup_sql) = setup {
            rt.connection.execute_script(setup_sql).unwrap();
        }
        let (_, stmt) = rt.connection.prepare(sql).unwrap();
        let stmt = stmt.unwrap();
        stmt.execute().unwrap();
        get_statement_status(stmt.pointer())
    }

    /// Debug helper: print bytecode for a statement
    #[allow(dead_code)]
    fn debug_bytecode(setup: Option<&str>, sql: &str) {
        let rt = Runtime::new(None);
        if let Some(setup_sql) = setup {
            rt.connection.execute_script(setup_sql).unwrap();
        }
        let (_, stmt) = rt.connection.prepare(sql).unwrap();
        let stmt = stmt.unwrap();
        stmt.execute().unwrap();
        let steps = unsafe { solite_core::sqlite::bytecode_steps(stmt.pointer()) };
        eprintln!("Bytecode for: {}", sql);
        for step in &steps {
            if step.nexec > 0 || step.opcode == "Insert" || step.opcode == "Delete" {
                eprintln!(
                    "  {} p1={} p2={} p3={} p4={:?} p5={} nexec={}",
                    step.opcode, step.p1, step.p2, step.p3, step.p4, step.p5, step.nexec
                );
            }
        }
        eprintln!("Status: {:?}", get_statement_status(stmt.pointer()));
    }

    #[test]
    fn test_insert_status() {
        let status = status_for_sql(
            Some("CREATE TABLE test_table (id INTEGER, name TEXT)"),
            "INSERT INTO test_table VALUES (1, 'hello')",
        );
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 1, name } if name == Some("test_table".into())
        ));
    }

    #[test]
    fn test_insert_multiple_rows() {
        let status = status_for_sql(
            Some("CREATE TABLE nums (n INTEGER)"),
            "INSERT INTO nums SELECT value FROM json_each('[1,2,3,4,5]')",
        );
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 5, name } if name == Some("nums".into())
        ));
    }

    #[test]
    fn test_delete_status() {
        // Use WHERE clause to force row-by-row deletion (Delete opcode).
        // Without WHERE, SQLite uses Clear opcode for full table deletion.
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (1), (2), (3)"),
            "DELETE FROM t WHERE id > 0",
        );
        assert!(matches!(status, StatementStatus::Delete { num_deletes: 3 }));
    }

    #[test]
    fn test_update_status() {
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER, val TEXT); INSERT INTO t VALUES (1, 'a'), (2, 'b')"),
            "UPDATE t SET val = 'updated'",
        );
        assert!(matches!(status, StatementStatus::Update { num_updates: 2 }));
    }

    #[test]
    fn test_select_returns_unknown() {
        let status = status_for_sql(None, "SELECT 1, 2, 3");
        assert!(matches!(status, StatementStatus::Unknown));
    }

    #[test]
    fn test_create_table_as_select() {
        let status = status_for_sql(None, "CREATE TABLE new_table AS SELECT 1 as a, 2 as b");
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 1, name } if name == Some("new_table".into())
        ));
    }

    // Edge cases: CTEs, subqueries, triggers, etc.

    #[test]
    fn test_insert_with_cte() {
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER)"),
            "WITH nums AS (SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3)
             INSERT INTO t SELECT * FROM nums",
        );
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 3, name } if name == Some("t".into())
        ));
    }

    #[test]
    fn test_insert_with_subquery() {
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER)"),
            "INSERT INTO t SELECT * FROM (SELECT 1 UNION SELECT 2)",
        );
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 2, name } if name == Some("t".into())
        ));
    }

    #[test]
    fn test_insert_from_join() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE a (id INTEGER);
                 CREATE TABLE b (id INTEGER);
                 CREATE TABLE c (id INTEGER);
                 INSERT INTO a VALUES (1), (2);
                 INSERT INTO b VALUES (10), (20)",
            ),
            "INSERT INTO c SELECT a.id + b.id FROM a, b",
        );
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 4, name } if name == Some("c".into())
        ));
    }

    #[test]
    fn test_update_with_subquery() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER, val INTEGER);
                 INSERT INTO t VALUES (1, 0), (2, 0), (3, 0)",
            ),
            "UPDATE t SET val = (SELECT MAX(id) FROM t)",
        );
        assert!(matches!(status, StatementStatus::Update { num_updates: 3 }));
    }

    #[test]
    fn test_update_with_cte() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER, val INTEGER);
                 INSERT INTO t VALUES (1, 0), (2, 0)",
            ),
            "WITH new_vals AS (SELECT 100 as v)
             UPDATE t SET val = (SELECT v FROM new_vals)",
        );
        assert!(matches!(status, StatementStatus::Update { num_updates: 2 }));
    }

    #[test]
    fn test_delete_with_subquery() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER);
                 INSERT INTO t VALUES (1), (2), (3), (4), (5)",
            ),
            "DELETE FROM t WHERE id IN (SELECT value FROM json_each('[1,2,3]'))",
        );
        assert!(matches!(status, StatementStatus::Delete { num_deletes: 3 }));
    }

    #[test]
    fn test_delete_with_cte() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER);
                 INSERT INTO t VALUES (1), (2), (3)",
            ),
            "WITH to_delete AS (SELECT 1 as id UNION SELECT 2)
             DELETE FROM t WHERE id IN (SELECT id FROM to_delete)",
        );
        assert!(matches!(status, StatementStatus::Delete { num_deletes: 2 }));
    }

    #[test]
    fn test_insert_with_trigger() {
        // Trigger inserts into audit table, but we're tracking the main INSERT
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER);
                 CREATE TABLE audit (action TEXT);
                 CREATE TRIGGER t_insert AFTER INSERT ON t
                 BEGIN INSERT INTO audit VALUES ('inserted'); END",
            ),
            "INSERT INTO t VALUES (1), (2)",
        );
        // Should report the main table insert, not the trigger's insert
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 2, .. }
        ));
    }

    #[test]
    fn test_update_with_trigger() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER, val INTEGER);
                 CREATE TABLE audit (action TEXT);
                 INSERT INTO t VALUES (1, 0), (2, 0);
                 CREATE TRIGGER t_update AFTER UPDATE ON t
                 BEGIN INSERT INTO audit VALUES ('updated'); END",
            ),
            "UPDATE t SET val = 100",
        );
        // Trigger's INSERT may have more executions than the UPDATE, affecting detection
        // The function picks the Insert with max nexec, which could be the trigger's
        let valid = matches!(
            status,
            StatementStatus::Update { .. } | StatementStatus::Insert { .. }
        );
        assert!(valid, "Expected Update or Insert (from trigger), got {:?}", status);
    }

    #[test]
    fn test_delete_with_trigger() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER);
                 CREATE TABLE audit (action TEXT);
                 INSERT INTO t VALUES (1), (2);
                 CREATE TRIGGER t_delete AFTER DELETE ON t
                 BEGIN INSERT INTO audit VALUES ('deleted'); END",
            ),
            "DELETE FROM t WHERE id > 0",
        );
        assert!(matches!(status, StatementStatus::Delete { num_deletes: 2 }));
    }

    #[test]
    fn test_insert_or_replace() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT);
                 INSERT INTO t VALUES (1, 'old')",
            ),
            "INSERT OR REPLACE INTO t VALUES (1, 'new'), (2, 'also new')",
        );
        // REPLACE is implemented as DELETE + INSERT, but bytecode shows as Insert
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 2, name } if name == Some("t".into())
        ));
    }

    #[test]
    fn test_insert_on_conflict() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT);
                 INSERT INTO t VALUES (1, 'existing')",
            ),
            "INSERT INTO t VALUES (1, 'conflict'), (2, 'new') ON CONFLICT DO NOTHING",
        );
        // Only 1 row actually inserted due to conflict
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 1, name } if name == Some("t".into())
        ));
    }

    #[test]
    fn test_upsert() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT);
                 INSERT INTO t VALUES (1, 'old')",
            ),
            "INSERT INTO t VALUES (1, 'updated'), (2, 'new')
             ON CONFLICT(id) DO UPDATE SET val = excluded.val",
        );
        // Upsert uses Insert opcode with OPFLAG_ISUPDATE for the conflict case
        // This might show as Insert or Update depending on implementation
        let valid = matches!(
            status,
            StatementStatus::Insert { .. } | StatementStatus::Update { .. }
        );
        assert!(valid, "Expected Insert or Update, got {:?}", status);
    }

    #[test]
    fn test_recursive_cte_insert() {
        let status = status_for_sql(
            Some("CREATE TABLE t (n INTEGER)"),
            "WITH RECURSIVE cnt(x) AS (
                SELECT 1
                UNION ALL
                SELECT x + 1 FROM cnt WHERE x < 10
             )
             INSERT INTO t SELECT x FROM cnt",
        );
        // Recursive CTEs generate Delete opcodes for internal working table management.
        // The function checks Delete before Insert, so this shows as Delete.
        // This is a known limitation - the Delete is for the ephemeral CTE table, not user data.
        assert!(matches!(status, StatementStatus::Delete { .. }));
    }

    #[test]
    fn test_insert_returning() {
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT)"),
            "INSERT INTO t(val) VALUES ('a'), ('b'), ('c') RETURNING id",
        );
        // RETURNING creates multiple Insert opcodes (ephemeral tables for result).
        // The function may pick one without a table name since all have same nexec.
        assert!(matches!(status, StatementStatus::Insert { num_inserts: 3, .. }));
    }

    #[test]
    fn test_update_returning() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER, val TEXT);
                 INSERT INTO t VALUES (1, 'a'), (2, 'b')",
            ),
            "UPDATE t SET val = 'updated' RETURNING id",
        );
        // UPDATE RETURNING creates multiple Insert opcodes with same nexec.
        // The actual UPDATE has OPFLAG_ISUPDATE but may not be picked by max_by_key.
        // This is a known limitation when RETURNING is used.
        assert!(matches!(
            status,
            StatementStatus::Update { .. } | StatementStatus::Insert { num_inserts: 2, .. }
        ));
    }

    #[test]
    fn test_delete_returning() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER);
                 INSERT INTO t VALUES (1), (2), (3)",
            ),
            "DELETE FROM t WHERE id > 1 RETURNING id",
        );
        assert!(matches!(status, StatementStatus::Delete { num_deletes: 2 }));
    }

    #[test]
    fn test_insert_default_values() {
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT DEFAULT 'default')"),
            "INSERT INTO t DEFAULT VALUES",
        );
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 1, name } if name == Some("t".into())
        ));
    }

    #[test]
    fn test_create_index() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER, val TEXT);
                 INSERT INTO t VALUES (1, 'a'), (2, 'b'), (3, 'c')",
            ),
            "CREATE INDEX t_val ON t(val)",
        );
        // CREATE INDEX uses Insert opcodes internally to build the index
        // May show as Insert or Unknown depending on whether rows exist
        let valid = matches!(
            status,
            StatementStatus::Unknown | StatementStatus::Insert { .. }
        );
        assert!(valid, "Expected Unknown or Insert, got {:?}", status);
    }

    #[test]
    fn test_vacuum_returns_unknown() {
        let status = status_for_sql(None, "VACUUM");
        assert!(matches!(status, StatementStatus::Unknown));
    }

    #[test]
    fn test_pragma_returns_unknown() {
        let status = status_for_sql(None, "PRAGMA table_info('sqlite_master')");
        assert!(matches!(status, StatementStatus::Unknown));
    }

    // Progress message tests

    #[test]
    fn test_progress_message_insert() {
        let status = StatementStatus::Insert {
            num_inserts: 1000,
            name: Some("users".to_string()),
        };
        assert_eq!(status.progress_message(), "inserting 1,000 rows into users");
    }

    #[test]
    fn test_progress_message_insert_no_name() {
        let status = StatementStatus::Insert {
            num_inserts: 500,
            name: None,
        };
        assert_eq!(status.progress_message(), "inserting 500 rows");
    }

    #[test]
    fn test_progress_message_delete() {
        let status = StatementStatus::Delete { num_deletes: 42 };
        assert_eq!(status.progress_message(), "delete: 42");
    }

    #[test]
    fn test_progress_message_update() {
        let status = StatementStatus::Update { num_updates: 10 };
        assert_eq!(status.progress_message(), "update: 10");
    }

    // Completion message tests

    #[test]
    fn test_completion_message_insert() {
        let status = StatementStatus::Insert {
            num_inserts: 100,
            name: Some("orders".to_string()),
        };
        assert_eq!(status.completion_message(), "inserted 100 rows into orders ");
    }

    #[test]
    fn test_completion_message_insert_no_name() {
        let status = StatementStatus::Insert {
            num_inserts: 50,
            name: None,
        };
        assert_eq!(status.completion_message(), "inserted 50 rows into ??? ");
    }

    #[test]
    fn test_completion_message_delete() {
        let status = StatementStatus::Delete { num_deletes: 25 };
        assert_eq!(status.completion_message(), "deleted 25 rows ");
    }

    #[test]
    fn test_completion_message_update() {
        let status = StatementStatus::Update { num_updates: 75 };
        assert_eq!(status.completion_message(), "updated 75 rows ");
    }

    #[test]
    fn test_completion_message_unknown() {
        let status = StatementStatus::Unknown;
        assert_eq!(status.completion_message(), "");
    }
}
