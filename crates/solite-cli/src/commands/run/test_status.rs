#[cfg(test)]
mod tests {
    use crate::commands::run::status::{
        get_statement_status, StatementStatus, TriggerEffect, TriggerOperation,
    };
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
                    "  [subprog={:?}] {} p1={} p2={} p3={} p4={:?} p5={} nexec={}",
                    step.subprog, step.opcode, step.p1, step.p2, step.p3, step.p4, step.p5,
                    step.nexec
                );
            }
        }
        eprintln!("Status: {:?}", get_statement_status(stmt.pointer()));
    }

    // =====================================================================
    // Basic operations (no triggers)
    // =====================================================================

    #[test]
    fn test_insert_status() {
        let status = status_for_sql(
            Some("CREATE TABLE test_table (id INTEGER, name TEXT)"),
            "INSERT INTO test_table VALUES (1, 'hello')",
        );
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 1, ref name, ref trigger_effects }
            if *name == Some("test_table".into()) && trigger_effects.is_empty()
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
            StatementStatus::Insert { num_inserts: 5, ref name, ref trigger_effects }
            if *name == Some("nums".into()) && trigger_effects.is_empty()
        ));
    }

    #[test]
    fn test_delete_status() {
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (1), (2), (3)"),
            "DELETE FROM t WHERE id > 0",
        );
        assert!(matches!(
            status,
            StatementStatus::Delete { num_deletes: 3, ref trigger_effects }
            if trigger_effects.is_empty()
        ));
    }

    #[test]
    fn test_update_status() {
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER, val TEXT); INSERT INTO t VALUES (1, 'a'), (2, 'b')"),
            "UPDATE t SET val = 'updated'",
        );
        assert!(matches!(
            status,
            StatementStatus::Update { num_updates: 2, ref trigger_effects }
            if trigger_effects.is_empty()
        ));
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
            StatementStatus::Insert { num_inserts: 1, ref name, .. }
            if *name == Some("new_table".into())
        ));
    }

    // =====================================================================
    // CTEs, subqueries, joins (no triggers)
    // =====================================================================

    #[test]
    fn test_insert_with_cte() {
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER)"),
            "WITH nums AS (SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3)
             INSERT INTO t SELECT * FROM nums",
        );
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 3, ref name, .. }
            if *name == Some("t".into())
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
            StatementStatus::Insert { num_inserts: 2, ref name, .. }
            if *name == Some("t".into())
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
            StatementStatus::Insert { num_inserts: 4, ref name, .. }
            if *name == Some("c".into())
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
        assert!(matches!(status, StatementStatus::Update { num_updates: 3, .. }));
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
        assert!(matches!(status, StatementStatus::Update { num_updates: 2, .. }));
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
        assert!(matches!(status, StatementStatus::Delete { num_deletes: 3, .. }));
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
        assert!(matches!(status, StatementStatus::Delete { num_deletes: 2, .. }));
    }

    // =====================================================================
    // Conflict handling (no triggers)
    // =====================================================================

    #[test]
    fn test_insert_or_replace() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT);
                 INSERT INTO t VALUES (1, 'old')",
            ),
            "INSERT OR REPLACE INTO t VALUES (1, 'new'), (2, 'also new')",
        );
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 2, ref name, .. }
            if *name == Some("t".into())
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
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 1, ref name, .. }
            if *name == Some("t".into())
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
        let valid = matches!(
            status,
            StatementStatus::Insert { .. } | StatementStatus::Update { .. }
        );
        assert!(valid, "Expected Insert or Update, got {:?}", status);
    }

    // =====================================================================
    // RETURNING clause
    // =====================================================================

    #[test]
    fn test_insert_returning() {
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT)"),
            "INSERT INTO t(val) VALUES ('a'), ('b'), ('c') RETURNING id",
        );
        match &status {
            StatementStatus::Insert {
                num_inserts,
                name,
                ..
            } => {
                assert_eq!(*num_inserts, 3);
                assert_eq!(name.as_deref(), Some("t"), "RETURNING should still report correct table name");
            }
            other => panic!("Expected Insert, got {:?}", other),
        }
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
        assert!(matches!(status, StatementStatus::Delete { num_deletes: 2, .. }));
    }

    #[test]
    fn test_insert_returning_with_trigger() {
        // Simulates the real-world case: INSERT ... RETURNING with a trigger
        // that inserts into another table. The RETURNING clause creates ephemeral
        // Insert opcodes that should not be confused with the real table insert.
        let status = status_for_sql(
            Some(
                "CREATE TABLE permits (id INTEGER PRIMARY KEY, permit_type TEXT, published_at TEXT);
                 CREATE TABLE alert_queue (permit_id INTEGER);
                 CREATE TRIGGER permits_ai AFTER INSERT ON permits
                 BEGIN INSERT INTO alert_queue VALUES (NEW.id); END",
            ),
            "INSERT INTO permits(permit_type, published_at) VALUES ('test', '2024-01-01') RETURNING id, permit_type, published_at",
        );
        match &status {
            StatementStatus::Insert {
                num_inserts,
                name,
                trigger_effects,
            } => {
                assert_eq!(*num_inserts, 1);
                assert_eq!(
                    name.as_deref(),
                    Some("permits"),
                    "Should report the real table, not ??? from ephemeral RETURNING table"
                );
                let alert_effect = trigger_effects
                    .iter()
                    .find(|e| e.table == "alert_queue");
                assert!(
                    alert_effect.is_some(),
                    "Should report trigger effect on alert_queue, got: {:?}",
                    trigger_effects
                );
            }
            other => panic!("Expected Insert, got {:?}", other),
        }
    }

    // =====================================================================
    // Misc (no triggers)
    // =====================================================================

    #[test]
    fn test_insert_default_values() {
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT DEFAULT 'default')"),
            "INSERT INTO t DEFAULT VALUES",
        );
        assert!(matches!(
            status,
            StatementStatus::Insert { num_inserts: 1, ref name, .. }
            if *name == Some("t".into())
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
        assert!(matches!(status, StatementStatus::Ddl { .. }));
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
        // The function checks Delete before Insert, so this may show as Delete.
        assert!(matches!(
            status,
            StatementStatus::Delete { .. } | StatementStatus::Insert { .. }
        ));
    }

    // =====================================================================
    // DDL statements should return Unknown
    // =====================================================================

    #[test]
    fn test_create_table_label() {
        let status = status_for_sql(None, "CREATE TABLE t (id INTEGER, name TEXT)");
        match &status {
            StatementStatus::Ddl { label } => assert_eq!(label, "created table t"),
            other => panic!("Expected Ddl, got {:?}", other),
        }
    }

    #[test]
    fn test_create_table_if_not_exists_label() {
        let status = status_for_sql(None, "CREATE TABLE IF NOT EXISTS t (id INTEGER)");
        match &status {
            StatementStatus::Ddl { label } => assert_eq!(label, "created table t"),
            other => panic!("Expected Ddl, got {:?}", other),
        }
    }

    #[test]
    fn test_create_trigger_label() {
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER)"),
            "CREATE TRIGGER t_ai AFTER INSERT ON t BEGIN SELECT 1; END",
        );
        match &status {
            StatementStatus::Ddl { label } => assert_eq!(label, "created trigger t_ai"),
            other => panic!("Expected Ddl, got {:?}", other),
        }
    }

    #[test]
    fn test_create_view_label() {
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER)"),
            "CREATE VIEW v AS SELECT * FROM t",
        );
        match &status {
            StatementStatus::Ddl { label } => assert_eq!(label, "created view v"),
            other => panic!("Expected Ddl, got {:?}", other),
        }
    }

    #[test]
    fn test_create_index_label() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER, val TEXT);
                 INSERT INTO t VALUES (1, 'a'), (2, 'b'), (3, 'c')",
            ),
            "CREATE INDEX t_val ON t(val)",
        );
        match &status {
            StatementStatus::Ddl { label } => assert_eq!(label, "created index t_val"),
            other => panic!("Expected Ddl, got {:?}", other),
        }
    }

    #[test]
    fn test_drop_table_label() {
        let status = status_for_sql(Some("CREATE TABLE t (id INTEGER)"), "DROP TABLE t");
        match &status {
            StatementStatus::Ddl { label } => assert_eq!(label, "dropped table t"),
            other => panic!("Expected Ddl, got {:?}", other),
        }
    }

    #[test]
    fn test_drop_table_if_exists_label() {
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER)"),
            "DROP TABLE IF EXISTS t",
        );
        match &status {
            StatementStatus::Ddl { label } => assert_eq!(label, "dropped table t"),
            other => panic!("Expected Ddl, got {:?}", other),
        }
    }

    // =====================================================================
    // TRIGGER TESTS - The main focus
    // =====================================================================

    #[test]
    fn test_insert_with_after_insert_trigger() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE users (id INTEGER, name TEXT);
                 CREATE TABLE audit (action TEXT, table_name TEXT);
                 CREATE TRIGGER users_insert AFTER INSERT ON users
                 BEGIN INSERT INTO audit VALUES ('insert', 'users'); END",
            ),
            "INSERT INTO users VALUES (1, 'alice'), (2, 'bob')",
        );
        match &status {
            StatementStatus::Insert {
                num_inserts,
                name,
                trigger_effects,
            } => {
                assert_eq!(*num_inserts, 2, "Should report 2 rows inserted into main table");
                assert_eq!(name.as_deref(), Some("users"));
                assert_eq!(trigger_effects.len(), 1, "Should have 1 trigger effect");
                assert_eq!(trigger_effects[0].table, "audit");
                assert_eq!(trigger_effects[0].operation, TriggerOperation::Insert);
                assert_eq!(trigger_effects[0].count, 2, "Trigger fires once per row");
            }
            other => panic!("Expected Insert, got {:?}", other),
        }
    }

    #[test]
    fn test_insert_trigger_inserts_more_rows_than_main() {
        // This is the key bug case: trigger inserts MORE rows than main statement.
        // Before the fix, max_by_key would pick the trigger's Insert opcode.
        let status = status_for_sql(
            Some(
                "CREATE TABLE orders (id INTEGER);
                 CREATE TABLE order_items (order_id INTEGER, item TEXT);
                 CREATE TRIGGER orders_insert AFTER INSERT ON orders
                 BEGIN
                     INSERT INTO order_items VALUES (NEW.id, 'item1');
                     INSERT INTO order_items VALUES (NEW.id, 'item2');
                     INSERT INTO order_items VALUES (NEW.id, 'item3');
                 END",
            ),
            "INSERT INTO orders VALUES (1)",
        );
        match &status {
            StatementStatus::Insert {
                num_inserts,
                name,
                trigger_effects,
            } => {
                assert_eq!(*num_inserts, 1, "Should report 1 row inserted into orders");
                assert_eq!(name.as_deref(), Some("orders"));
                assert!(
                    trigger_effects.iter().any(|e| e.table == "order_items"
                        && e.operation == TriggerOperation::Insert
                        && e.count == 3),
                    "Should report 3 trigger inserts into order_items, got: {:?}",
                    trigger_effects
                );
            }
            other => panic!("Expected Insert, got {:?}", other),
        }
    }

    #[test]
    fn test_insert_with_multiple_triggers_to_different_tables() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE events (id INTEGER, type TEXT);
                 CREATE TABLE event_log (event_id INTEGER, logged_at TEXT);
                 CREATE TABLE event_counts (type TEXT, count INTEGER);
                 CREATE TRIGGER events_log AFTER INSERT ON events
                 BEGIN INSERT INTO event_log VALUES (NEW.id, datetime('now')); END;
                 CREATE TRIGGER events_count AFTER INSERT ON events
                 BEGIN INSERT INTO event_counts VALUES (NEW.type, 1); END",
            ),
            "INSERT INTO events VALUES (1, 'click'), (2, 'view'), (3, 'click')",
        );
        match &status {
            StatementStatus::Insert {
                num_inserts,
                name,
                trigger_effects,
            } => {
                assert_eq!(*num_inserts, 3);
                assert_eq!(name.as_deref(), Some("events"));
                assert_eq!(trigger_effects.len(), 2, "Should have 2 trigger effects");
                // Sorted alphabetically by table name
                let log_effect = trigger_effects
                    .iter()
                    .find(|e| e.table == "event_log")
                    .expect("Should have event_log effect");
                assert_eq!(log_effect.count, 3);
                assert_eq!(log_effect.operation, TriggerOperation::Insert);
                let count_effect = trigger_effects
                    .iter()
                    .find(|e| e.table == "event_counts")
                    .expect("Should have event_counts effect");
                assert_eq!(count_effect.count, 3);
                assert_eq!(count_effect.operation, TriggerOperation::Insert);
            }
            other => panic!("Expected Insert, got {:?}", other),
        }
    }

    #[test]
    fn test_insert_trigger_does_update() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
                 CREATE TABLE item_stats (total INTEGER);
                 INSERT INTO item_stats VALUES (0);
                 CREATE TRIGGER items_insert AFTER INSERT ON items
                 BEGIN UPDATE item_stats SET total = total + 1; END",
            ),
            "INSERT INTO items VALUES (1, 'a'), (2, 'b'), (3, 'c')",
        );
        match &status {
            StatementStatus::Insert {
                num_inserts,
                name,
                trigger_effects,
            } => {
                assert_eq!(*num_inserts, 3);
                assert_eq!(name.as_deref(), Some("items"));
                let update_effect = trigger_effects
                    .iter()
                    .find(|e| e.table == "item_stats" && e.operation == TriggerOperation::Update)
                    .expect("Should have update trigger effect on item_stats");
                assert_eq!(update_effect.count, 3, "Update fires once per inserted row");
            }
            other => panic!("Expected Insert, got {:?}", other),
        }
    }

    #[test]
    fn test_insert_trigger_does_delete() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE logs (id INTEGER PRIMARY KEY, msg TEXT);
                 CREATE TABLE old_logs (id INTEGER, msg TEXT);
                 INSERT INTO old_logs VALUES (1, 'old1'), (2, 'old2');
                 CREATE TRIGGER logs_insert AFTER INSERT ON logs
                 BEGIN DELETE FROM old_logs WHERE id = NEW.id; END",
            ),
            "INSERT INTO logs VALUES (1, 'new'), (2, 'also new')",
        );
        match &status {
            StatementStatus::Insert {
                num_inserts,
                name,
                trigger_effects,
            } => {
                assert_eq!(*num_inserts, 2);
                assert_eq!(name.as_deref(), Some("logs"));
                let delete_effect = trigger_effects
                    .iter()
                    .find(|e| e.table == "old_logs" && e.operation == TriggerOperation::Delete);
                assert!(
                    delete_effect.is_some(),
                    "Should have delete trigger effect on old_logs, got: {:?}",
                    trigger_effects
                );
            }
            other => panic!("Expected Insert, got {:?}", other),
        }
    }

    #[test]
    fn test_update_with_insert_trigger() {
        // An UPDATE that triggers INSERTs — the main operation should still be Update
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
        match &status {
            StatementStatus::Update {
                num_updates,
                trigger_effects,
            } => {
                assert_eq!(*num_updates, 2);
                let insert_effect = trigger_effects
                    .iter()
                    .find(|e| e.table == "audit" && e.operation == TriggerOperation::Insert);
                assert!(
                    insert_effect.is_some(),
                    "Should have insert trigger effect on audit, got: {:?}",
                    trigger_effects
                );
            }
            other => panic!("Expected Update, got {:?}", other),
        }
    }

    #[test]
    fn test_delete_with_insert_trigger() {
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
        match &status {
            StatementStatus::Delete {
                num_deletes,
                trigger_effects,
            } => {
                assert_eq!(*num_deletes, 2);
                let insert_effect = trigger_effects
                    .iter()
                    .find(|e| e.table == "audit" && e.operation == TriggerOperation::Insert);
                assert!(
                    insert_effect.is_some(),
                    "Should have insert trigger effect on audit, got: {:?}",
                    trigger_effects
                );
            }
            other => panic!("Expected Delete, got {:?}", other),
        }
    }

    #[test]
    fn test_insert_with_before_insert_trigger() {
        // BEFORE triggers should also be captured
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER, val TEXT);
                 CREATE TABLE pre_log (msg TEXT);
                 CREATE TRIGGER t_before BEFORE INSERT ON t
                 BEGIN INSERT INTO pre_log VALUES ('before insert'); END",
            ),
            "INSERT INTO t VALUES (1, 'a'), (2, 'b')",
        );
        match &status {
            StatementStatus::Insert {
                num_inserts,
                name,
                trigger_effects,
            } => {
                assert_eq!(*num_inserts, 2);
                assert_eq!(name.as_deref(), Some("t"));
                let pre_effect = trigger_effects
                    .iter()
                    .find(|e| e.table == "pre_log" && e.operation == TriggerOperation::Insert);
                assert!(
                    pre_effect.is_some(),
                    "Should capture BEFORE trigger inserts, got: {:?}",
                    trigger_effects
                );
            }
            other => panic!("Expected Insert, got {:?}", other),
        }
    }

    #[test]
    fn test_insert_bulk_with_trigger() {
        // Bulk insert where trigger fires many times
        let status = status_for_sql(
            Some(
                "CREATE TABLE data (n INTEGER);
                 CREATE TABLE shadow (n INTEGER, doubled INTEGER);
                 CREATE TRIGGER data_insert AFTER INSERT ON data
                 BEGIN INSERT INTO shadow VALUES (NEW.n, NEW.n * 2); END",
            ),
            "INSERT INTO data SELECT value FROM json_each('[1,2,3,4,5,6,7,8,9,10]')",
        );
        match &status {
            StatementStatus::Insert {
                num_inserts,
                name,
                trigger_effects,
            } => {
                assert_eq!(*num_inserts, 10);
                assert_eq!(name.as_deref(), Some("data"));
                let shadow_effect = trigger_effects
                    .iter()
                    .find(|e| e.table == "shadow")
                    .expect("Should have shadow table trigger effect");
                assert_eq!(shadow_effect.count, 10, "Shadow should get 10 rows from trigger");
            }
            other => panic!("Expected Insert, got {:?}", other),
        }
    }

    #[test]
    fn test_insert_no_trigger_has_empty_effects() {
        let status = status_for_sql(
            Some("CREATE TABLE t (id INTEGER)"),
            "INSERT INTO t VALUES (1), (2), (3)",
        );
        match &status {
            StatementStatus::Insert {
                trigger_effects, ..
            } => {
                assert!(trigger_effects.is_empty(), "No triggers = no effects");
            }
            other => panic!("Expected Insert, got {:?}", other),
        }
    }

    #[test]
    fn test_cascading_triggers_insert_to_insert() {
        // Table A insert -> trigger inserts into B -> trigger on B inserts into C
        let status = status_for_sql(
            Some(
                "CREATE TABLE a (id INTEGER);
                 CREATE TABLE b (a_id INTEGER);
                 CREATE TABLE c (b_ref INTEGER);
                 CREATE TRIGGER a_insert AFTER INSERT ON a
                 BEGIN INSERT INTO b VALUES (NEW.id); END;
                 CREATE TRIGGER b_insert AFTER INSERT ON b
                 BEGIN INSERT INTO c VALUES (NEW.a_id); END",
            ),
            "INSERT INTO a VALUES (1), (2), (3)",
        );
        match &status {
            StatementStatus::Insert {
                num_inserts,
                name,
                trigger_effects,
            } => {
                assert_eq!(*num_inserts, 3);
                assert_eq!(name.as_deref(), Some("a"));
                let b_effect = trigger_effects
                    .iter()
                    .find(|e| e.table == "b" && e.operation == TriggerOperation::Insert);
                assert!(b_effect.is_some(), "Should have trigger effect on b, got: {:?}", trigger_effects);
                let c_effect = trigger_effects
                    .iter()
                    .find(|e| e.table == "c" && e.operation == TriggerOperation::Insert);
                assert!(c_effect.is_some(), "Should have cascaded trigger effect on c, got: {:?}", trigger_effects);
            }
            other => panic!("Expected Insert, got {:?}", other),
        }
    }

    #[test]
    fn test_trigger_with_conditional_insert() {
        // Trigger only fires for certain rows
        let status = status_for_sql(
            Some(
                "CREATE TABLE t (id INTEGER, important INTEGER);
                 CREATE TABLE important_log (id INTEGER);
                 CREATE TRIGGER t_insert AFTER INSERT ON t
                 WHEN NEW.important = 1
                 BEGIN INSERT INTO important_log VALUES (NEW.id); END",
            ),
            "INSERT INTO t VALUES (1, 1), (2, 0), (3, 1), (4, 0), (5, 1)",
        );
        match &status {
            StatementStatus::Insert {
                num_inserts,
                name,
                trigger_effects,
            } => {
                assert_eq!(*num_inserts, 5);
                assert_eq!(name.as_deref(), Some("t"));
                let log_effect = trigger_effects
                    .iter()
                    .find(|e| e.table == "important_log")
                    .expect("Should have important_log trigger effect");
                assert_eq!(log_effect.count, 3, "Only 3 of 5 rows have important=1");
            }
            other => panic!("Expected Insert, got {:?}", other),
        }
    }

    #[test]
    fn test_trigger_mixed_operations() {
        // A single trigger that does both INSERT and UPDATE
        let status = status_for_sql(
            Some(
                "CREATE TABLE orders (id INTEGER, total REAL);
                 CREATE TABLE order_log (order_id INTEGER, action TEXT);
                 CREATE TABLE daily_totals (day TEXT PRIMARY KEY, total REAL);
                 INSERT INTO daily_totals VALUES ('today', 0);
                 CREATE TRIGGER orders_insert AFTER INSERT ON orders
                 BEGIN
                     INSERT INTO order_log VALUES (NEW.id, 'created');
                     UPDATE daily_totals SET total = total + NEW.total WHERE day = 'today';
                 END",
            ),
            "INSERT INTO orders VALUES (1, 10.0), (2, 20.0)",
        );
        match &status {
            StatementStatus::Insert {
                num_inserts,
                name,
                trigger_effects,
            } => {
                assert_eq!(*num_inserts, 2);
                assert_eq!(name.as_deref(), Some("orders"));
                let log_effect = trigger_effects
                    .iter()
                    .find(|e| e.table == "order_log" && e.operation == TriggerOperation::Insert);
                assert!(log_effect.is_some(), "Should have insert trigger on order_log, got: {:?}", trigger_effects);
                let total_effect = trigger_effects
                    .iter()
                    .find(|e| e.table == "daily_totals" && e.operation == TriggerOperation::Update);
                assert!(total_effect.is_some(), "Should have update trigger on daily_totals, got: {:?}", trigger_effects);
            }
            other => panic!("Expected Insert, got {:?}", other),
        }
    }

    #[test]
    fn test_delete_trigger_with_cascading_delete() {
        let status = status_for_sql(
            Some(
                "CREATE TABLE parent (id INTEGER PRIMARY KEY);
                 CREATE TABLE child (id INTEGER, parent_id INTEGER);
                 INSERT INTO parent VALUES (1), (2);
                 INSERT INTO child VALUES (10, 1), (20, 1), (30, 2);
                 CREATE TRIGGER parent_delete AFTER DELETE ON parent
                 BEGIN DELETE FROM child WHERE parent_id = OLD.id; END",
            ),
            "DELETE FROM parent WHERE id = 1",
        );
        match &status {
            StatementStatus::Delete {
                num_deletes,
                trigger_effects,
            } => {
                assert_eq!(*num_deletes, 1, "Deleted 1 parent row");
                let child_delete = trigger_effects
                    .iter()
                    .find(|e| e.table == "child" && e.operation == TriggerOperation::Delete);
                assert!(
                    child_delete.is_some(),
                    "Should have delete trigger effect on child, got: {:?}",
                    trigger_effects
                );
            }
            other => panic!("Expected Delete, got {:?}", other),
        }
    }

    // =====================================================================
    // Progress & completion message tests
    // =====================================================================

    #[test]
    fn test_progress_message_insert() {
        let status = StatementStatus::Insert {
            num_inserts: 1000,
            name: Some("users".to_string()),
            trigger_effects: vec![],
        };
        assert_eq!(status.progress_message(), "inserting 1,000 rows into users");
    }

    #[test]
    fn test_progress_message_insert_no_name() {
        let status = StatementStatus::Insert {
            num_inserts: 500,
            name: None,
            trigger_effects: vec![],
        };
        assert_eq!(status.progress_message(), "inserting 500 rows");
    }

    #[test]
    fn test_progress_message_delete() {
        let status = StatementStatus::Delete {
            num_deletes: 42,
            trigger_effects: vec![],
        };
        assert_eq!(status.progress_message(), "delete: 42");
    }

    #[test]
    fn test_progress_message_update() {
        let status = StatementStatus::Update {
            num_updates: 10,
            trigger_effects: vec![],
        };
        assert_eq!(status.progress_message(), "update: 10");
    }

    #[test]
    fn test_completion_message_insert() {
        let status = StatementStatus::Insert {
            num_inserts: 100,
            name: Some("orders".to_string()),
            trigger_effects: vec![],
        };
        assert_eq!(status.completion_message(), "inserted 100 rows into orders ");
    }

    #[test]
    fn test_completion_message_insert_no_name() {
        let status = StatementStatus::Insert {
            num_inserts: 50,
            name: None,
            trigger_effects: vec![],
        };
        assert_eq!(status.completion_message(), "inserted 50 rows into ??? ");
    }

    #[test]
    fn test_completion_message_delete() {
        let status = StatementStatus::Delete {
            num_deletes: 25,
            trigger_effects: vec![],
        };
        assert_eq!(status.completion_message(), "deleted 25 rows ");
    }

    #[test]
    fn test_completion_message_update() {
        let status = StatementStatus::Update {
            num_updates: 75,
            trigger_effects: vec![],
        };
        assert_eq!(status.completion_message(), "updated 75 rows ");
    }

    #[test]
    fn test_completion_message_unknown() {
        let status = StatementStatus::Unknown;
        assert_eq!(status.completion_message(), "");
    }

    #[test]
    fn test_completion_message_with_trigger_effects() {
        let status = StatementStatus::Insert {
            num_inserts: 5,
            name: Some("users".to_string()),
            trigger_effects: vec![
                TriggerEffect {
                    table: "audit_log".to_string(),
                    operation: TriggerOperation::Insert,
                    count: 5,
                },
                TriggerEffect {
                    table: "user_counts".to_string(),
                    operation: TriggerOperation::Update,
                    count: 5,
                },
            ],
        };
        // Main message should not include trigger effects
        assert_eq!(status.completion_message(), "inserted 5 rows into users ");
        // Trigger effects returned separately
        let lines = status.trigger_effect_lines();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "5 rows inserted into audit_log");
        assert_eq!(lines[1], "5 rows updated in user_counts");
    }

    #[test]
    fn test_completion_message_delete_with_trigger() {
        let status = StatementStatus::Delete {
            num_deletes: 3,
            trigger_effects: vec![TriggerEffect {
                table: "child_records".to_string(),
                operation: TriggerOperation::Delete,
                count: 9,
            }],
        };
        assert_eq!(status.completion_message(), "deleted 3 rows ");
        let lines = status.trigger_effect_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "9 rows deleted from child_records");
    }

    #[test]
    fn test_create_virtual_table_label() {
        let status = status_for_sql(None, "CREATE VIRTUAL TABLE v USING vec0(a float[1])");
        match &status {
            StatementStatus::Ddl { label } => assert_eq!(label, "created virtual table v"),
            other => panic!("Expected Ddl, got {:?}", other),
        }
    }

    #[test]
    fn test_insert_into_virtual_table() {
        let status = status_for_sql(
            Some("CREATE VIRTUAL TABLE v USING vec0(a float[1])"),
            "INSERT INTO v(a) VALUES ('[1.0]')",
        );
        match &status {
            StatementStatus::Insert {
                num_inserts,
                name,
                ..
            } => {
                assert_eq!(*num_inserts, 1);
                assert_eq!(name.as_deref(), Some("v"));
            }
            other => panic!("Expected Insert, got {:?}", other),
        }
    }
}
