#[cfg(test)]
mod tests {
    use crate::commands::tui::{App, Clipboard, ListingPage, Page, SharedClipboard};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use insta::assert_snapshot;
    use ratatui::{backend::TestBackend, Terminal};
    use solite_core::Runtime;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Records copied text instead of touching the real system clipboard,
    /// so copy paths are testable headless (CI has no clipboard).
    #[derive(Default)]
    struct FakeClipboard {
        copied: Vec<String>,
    }

    impl Clipboard for FakeClipboard {
        fn set_text(&mut self, text: String) -> Result<(), String> {
            self.copied.push(text);
            Ok(())
        }
    }

    struct TestApp<'a> {
        terminal: Terminal<TestBackend>,
        app: App<'a>,
        clipboard: Rc<RefCell<FakeClipboard>>,
    }
    impl<'a> TestApp<'a> {
        fn new(runtime: &'a mut Runtime, width: u16, height: u16) -> Self {
            let terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let theme = crate::commands::tui::tui_theme::CTP_MOCHA_THEME.clone();
            let clipboard = Rc::new(RefCell::new(FakeClipboard::default()));
            let shared: SharedClipboard = clipboard.clone();
            let page = Page::Listing(ListingPage::new(runtime, &theme));
            let app = App {
                runtime,
                page,
                theme,
                clipboard: shared,
            };
            Self {
                terminal,
                app,
                clipboard,
            }
        }

        /// Render a frame without snapshotting (drives render-side work like
        /// incremental row counting).
        fn draw(&mut self) {
            self.terminal.draw(|frame| self.app.render(frame)).unwrap();
        }

        fn draw_and_snapshot(&mut self, name: &str) {
            self.terminal.draw(|frame| self.app.render(frame)).unwrap();
            assert_snapshot!(name, self.terminal.backend());
        }
        fn send_key(&mut self, key: KeyEvent) {
            self.app.handle_key(key);
        }
        fn send_char(&mut self, c: char) {
            self.send_key(KeyCode::Char(c).into());
        }
        fn copied(&self) -> Vec<String> {
            self.clipboard.borrow().copied.clone()
        }
        fn last_copied(&self) -> Option<String> {
            self.clipboard.borrow().copied.last().cloned()
        }
    }

    #[test]
    fn test_render_app() {
        let mut runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(
                r#"
              -- fixed literal, not sqlite_version(): keeps snapshots stable across SQLite bumps
              create table t as select 1 as a, 'asdf' as b, '9.9.9' as c;

              create table t2(c1,c2,c3,c4,c5,c6,c7,c8,c9,c10);
              insert into t2 select 1,2,3,4,5,6,7,8,9,10;
              insert into t2 select 'a','b','c','d','e','f','g','h','i','j';


              "#,
            )
            .unwrap();
        let mut app = TestApp::new(&mut runtime, 80, 20);
        app.draw_and_snapshot("listing page");

        app.send_key(KeyCode::Enter.into());
        app.draw_and_snapshot("enter - table view");

        app.send_key(KeyCode::Esc.into());
        app.draw_and_snapshot("Back to listing");

        app.send_key(KeyCode::Down.into());
        app.send_key(KeyCode::Enter.into());
        app.draw_and_snapshot("selected t2");

        app.send_key(KeyCode::Char('L').into());
        app.draw_and_snapshot("move to last column");

        for i in 0..5 {
            app.send_key(KeyCode::Left.into());
            app.draw_and_snapshot(format!("shifted left {i}").as_str());
        }

        app.send_key(KeyCode::Char('H').into());
        app.draw_and_snapshot("move to first column");
    }

    #[test]
    fn test_row_page() {
        let mut runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(
                r#"
              create table people(id integer primary key, name text, score real);
              insert into people values (1, 'alice', 90.5), (2, 'bob', 80.25), (3, 'carol', 70.0);
              "#,
            )
            .unwrap();
        let mut app = TestApp::new(&mut runtime, 80, 20);

        // listing -> table -> row detail
        app.send_key(KeyCode::Enter.into());
        app.send_key(KeyCode::Enter.into());
        app.draw_and_snapshot("row page");

        // k from the first item wraps to the last (pinned behavior)
        app.send_char('k');
        app.draw_and_snapshot("row page wrapped to last");
        // j from the last item wraps back to the first
        app.send_char('j');

        // y copies the selected (first) value
        app.send_char('y');
        assert_eq!(app.last_copied().unwrap(), "1");

        // p copies the primary key
        app.send_char('p');
        assert_eq!(app.last_copied().unwrap(), "1");

        // Y copies the whole row as valid JSON
        app.send_char('Y');
        let json = app.last_copied().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["id"], 1);
        assert_eq!(parsed["name"], "alice");
        assert_eq!(parsed["score"], 90.5);

        // G jumps to the last item, g back to the first
        app.send_char('G');
        app.send_char('y');
        assert_eq!(app.last_copied().unwrap(), "90.5");
        app.send_char('g');
        app.send_char('y');
        assert_eq!(app.last_copied().unwrap(), "1");

        // q goes back to the table page for the same table
        app.send_char('q');
        app.draw_and_snapshot("row page back to table");
    }

    #[test]
    fn test_copy_popup() {
        let mut runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(
                r#"
              create table t(a, b);
              insert into t values (1, 'x'), (2, 'y');
              "#,
            )
            .unwrap();
        let mut app = TestApp::new(&mut runtime, 80, 20);
        app.send_key(KeyCode::Enter.into());

        // y opens the popup
        app.send_char('y');
        app.draw_and_snapshot("copy popup open");

        // Esc dismisses without copying
        app.send_key(KeyCode::Esc.into());
        assert!(app.copied().is_empty());
        app.draw_and_snapshot("copy popup dismissed");

        // number key 1 copies the selected cell directly
        app.send_char('y');
        app.send_char('1');
        assert_eq!(app.last_copied().unwrap(), "1");

        // j + Enter selects the second option (copy row as TSV)
        app.send_char('y');
        app.send_char('j');
        app.send_key(KeyCode::Enter.into());
        assert_eq!(app.last_copied().unwrap(), "1\tx");

        // 3 copies the whole table as TSV (header + all rows)
        app.send_char('y');
        app.send_char('3');
        assert_eq!(app.last_copied().unwrap(), "a\tb\n1\tx\n2\ty");

        // 4 copies a SELECT statement
        app.send_char('y');
        app.send_char('4');
        assert_eq!(app.last_copied().unwrap(), "SELECT * FROM \"t\";");

        // 5 copies INSERT statements for every row
        app.send_char('y');
        app.send_char('5');
        assert_eq!(
            app.last_copied().unwrap(),
            "INSERT INTO \"t\" (\"a\", \"b\") VALUES (1, 'x');\n\
             INSERT INTO \"t\" (\"a\", \"b\") VALUES (2, 'y');"
        );
    }

    #[test]
    fn test_sorting() {
        let mut runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(
                r#"
              create table nums(n, label);
              insert into nums values (3, 'three'), (1, 'one'), (2, 'two');
              "#,
            )
            .unwrap();
        let mut app = TestApp::new(&mut runtime, 80, 20);
        app.send_key(KeyCode::Enter.into());

        // move selection off the first row, then sort: selection resets to
        // row 1. The sort is deferred one frame so a "Sorting…" message is
        // on screen while the blocking ORDER BY runs.
        app.send_char('j');
        app.send_char('[');
        app.draw_and_snapshot("sorting feedback");
        app.draw(); // executes the deferred sort
        app.draw_and_snapshot("sorted ascending");

        app.send_char(']');
        app.draw();
        app.draw();
        app.draw_and_snapshot("sorted descending");

        // the sorted order is what gets copied
        app.send_char('y');
        app.send_char('3');
        assert_eq!(
            app.last_copied().unwrap(),
            "n\tlabel\n3\tthree\n2\ttwo\n1\tone"
        );
    }

    #[test]
    fn test_windowing_large_table() {
        let mut runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(
                "create table big as with recursive c(n) as \
                 (select 1 union all select n+1 from c limit 500) select n from c",
            )
            .unwrap();
        let mut app = TestApp::new(&mut runtime, 80, 20);
        app.send_key(KeyCode::Enter.into());

        // first draw drives the incremental count to completion (500 < batch size)
        app.draw();

        // G jumps to the absolute last row, past the initial 200-row window
        app.send_char('G');
        app.draw_and_snapshot("large table last row");

        // g jumps back to the first row
        app.send_char('g');

        // j past the window edge keeps the absolute position consistent
        for _ in 0..220 {
            app.send_char('j');
        }
        app.draw_and_snapshot("large table row 221");

        // PageDown moves by a page
        app.send_key(KeyCode::PageDown.into());
        app.send_key(KeyCode::PageUp.into());
        app.draw_and_snapshot("large table back to row 221");

        // copy row at a deep position copies the right row
        app.send_char('y');
        app.send_char('2');
        assert_eq!(app.last_copied().unwrap(), "221");

        // Ctrl+d / Ctrl+u page like PageDown / PageUp
        app.send_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        app.send_char('y');
        app.send_char('2');
        assert_eq!(app.last_copied().unwrap(), "241");
        app.send_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        app.send_char('y');
        app.send_char('2');
        assert_eq!(app.last_copied().unwrap(), "221");
    }

    #[test]
    fn test_help_overlay() {
        let mut runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(
                r#"
              create table t(a, b);
              insert into t values (1, 'x');
              "#,
            )
            .unwrap();
        let mut app = TestApp::new(&mut runtime, 80, 24);

        // ? opens the overlay on the listing page; ? again dismisses it
        app.send_char('?');
        app.draw_and_snapshot("help overlay listing");
        app.send_char('?');
        app.draw_and_snapshot("help overlay dismissed");

        // table page overlay
        app.send_key(KeyCode::Enter.into());
        app.send_char('?');
        app.draw_and_snapshot("help overlay table");
        // while the overlay is open, other keys are consumed (no navigation)
        app.send_char('j');
        app.send_char('y');
        assert!(app.copied().is_empty());
        // Esc dismisses the overlay without leaving the page
        app.send_key(KeyCode::Esc.into());

        // row page overlay
        app.send_key(KeyCode::Enter.into());
        app.send_char('?');
        app.draw_and_snapshot("help overlay row");
        // q dismisses the overlay (does not navigate back)
        app.send_char('q');
        app.draw_and_snapshot("help overlay row dismissed");
    }

    #[test]
    fn test_pagination_consistency_sorted_vs_unsorted() {
        let mut runtime = Runtime::new(None).unwrap();
        // Values descend as rowids ascend, so the keyset (rowid) order and
        // the sorted order are different and mixing them up would show.
        runtime
            .connection
            .execute_script(
                "create table ks as with recursive c(n) as \
                 (select 1 union all select n+1 from c limit 500) \
                 select 501 - n as v from c",
            )
            .unwrap();
        let mut app = TestApp::new(&mut runtime, 80, 20);
        app.send_key(KeyCode::Enter.into());
        app.draw(); // completes the row count

        // Unsorted (keyset path): last row is the last inserted, v = 1
        app.send_char('G');
        app.send_char('y');
        app.send_char('2');
        assert_eq!(app.last_copied().unwrap(), "1");

        // Forward hops past the window edge stay consistent: row 221 = 280
        app.send_char('g');
        for _ in 0..220 {
            app.send_char('j');
        }
        app.send_char('y');
        app.send_char('2');
        assert_eq!(app.last_copied().unwrap(), "280");

        // Backward (splice) path: G then PageUp lands on row 480, v = 21
        app.send_char('G');
        app.send_key(KeyCode::PageUp.into());
        app.send_char('y');
        app.send_char('2');
        assert_eq!(app.last_copied().unwrap(), "21");

        // Sorted ascending (OFFSET path): same positions, sorted values
        app.send_char('[');
        app.draw();
        app.draw();
        app.send_char('G');
        app.send_char('y');
        app.send_char('2');
        assert_eq!(app.last_copied().unwrap(), "500");
        app.send_char('g');
        app.send_char('y');
        app.send_char('2');
        assert_eq!(app.last_copied().unwrap(), "1");
    }

    #[test]
    fn test_background_count_on_file_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("counted.db");
        let mut runtime = Runtime::new(Some(path.to_str().unwrap().to_owned())).unwrap();
        runtime
            .connection
            .execute_script(
                "create table big as with recursive c(n) as \
                 (select 1 union all select n+1 from c limit 500) select n from c",
            )
            .unwrap();
        let mut app = TestApp::new(&mut runtime, 80, 20);
        app.send_key(KeyCode::Enter.into());

        // File-backed db: the count arrives from the background COUNT(*)
        // thread; poll renders until it lands.
        let mut complete = false;
        for _ in 0..200 {
            app.draw();
            if let Page::Table(table_page) = &app.app.page {
                if table_page.row_count.is_complete {
                    complete = true;
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(complete, "background count never completed");
        match &app.app.page {
            Page::Table(table_page) => assert_eq!(table_page.total_rows(), 500),
            _ => panic!("expected table page"),
        }

        // Sorting must not discard the completed count
        app.send_char('[');
        app.draw();
        app.draw();
        match &app.app.page {
            Page::Table(table_page) => {
                assert!(table_page.row_count.is_complete);
                assert_eq!(table_page.total_rows(), 500);
            }
            _ => panic!("expected table page"),
        }
    }

    #[test]
    fn test_empty_table() {
        let mut runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script("create table empty(a, b)")
            .unwrap();
        let mut app = TestApp::new(&mut runtime, 80, 20);
        app.send_key(KeyCode::Enter.into());
        app.draw_and_snapshot("empty table");

        // navigation on an empty table must not panic
        app.send_char('j');
        app.send_char('k');
        app.send_char('G');
        app.send_char('g');
        app.draw();

        // cell/row copies have nothing to copy; popup just closes
        app.send_char('y');
        app.send_char('1');
        app.send_char('y');
        app.send_char('2');
        assert!(app.copied().is_empty());

        // whole-table copy still yields the header
        app.send_char('y');
        app.send_char('3');
        assert_eq!(app.last_copied().unwrap(), "a\tb\n");
    }

    #[test]
    fn test_wide_table_column_fit() {
        // 100 columns: the number of visible columns adapts to the width
        let columns: Vec<String> = (0..100).map(|i| format!("{} as col{i}", i * 11)).collect();
        let script = format!("create table wide as select {}", columns.join(", "));

        let mut runtime = Runtime::new(None).unwrap();
        runtime.connection.execute_script(&script).unwrap();
        {
            let mut app = TestApp::new(&mut runtime, 80, 16);
            app.send_key(KeyCode::Enter.into());
            app.draw_and_snapshot("wide 100col at 80");
            // L jumps toward the last column; the grid stays intact
            app.send_char('L');
            app.draw_and_snapshot("wide 100col at 80 last column");
        }

        let mut app = TestApp::new(&mut runtime, 200, 16);
        app.send_key(KeyCode::Enter.into());
        app.draw_and_snapshot("wide 100col at 200");
    }

    #[test]
    fn test_newlines_do_not_break_the_grid() {
        let mut runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(
                "create table notes(id, note);\n\
                 insert into notes values (1, 'line one' || char(10) || 'line two'), \
                 (2, 'tabbed' || char(9) || 'value');",
            )
            .unwrap();
        let mut app = TestApp::new(&mut runtime, 80, 12);
        app.send_key(KeyCode::Enter.into());
        // Each row stays one line high; \n and \t show as visible escapes
        app.draw_and_snapshot("newline value grid");
    }

    #[test]
    fn test_huge_values_truncated_in_window_but_copied_in_full() {
        const HUGE_LEN: usize = 100_000;
        let mut runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(&format!(
                "create table blobs as \
                 select 1 as id, hex(zeroblob({})) as big_text, zeroblob({}) as big_blob",
                HUGE_LEN / 2,
                HUGE_LEN
            ))
            .unwrap();
        let mut app = TestApp::new(&mut runtime, 80, 12);
        app.send_key(KeyCode::Enter.into());

        // The window never materializes the full values…
        if let Page::Table(table_page) = &app.app.page {
            for value in &table_page.data.rows[0] {
                let size = match value {
                    solite_core::sqlite::OwnedValue::Text(s) => s.len(),
                    solite_core::sqlite::OwnedValue::Blob(b) => b.len(),
                    _ => 0,
                };
                assert!(size <= 1024, "window cell holds {} bytes", size);
            }
        } else {
            panic!("expected table page");
        }

        // …but a cell copy fetches the full value on demand
        app.send_char('l'); // select the big_text column
        app.send_char('y');
        app.send_char('1');
        assert_eq!(app.last_copied().unwrap().len(), HUGE_LEN);

        // and the row page receives full values (its display truncates)
        app.send_key(KeyCode::Enter.into());
        app.send_char('j'); // big_text
        app.send_char('y');
        assert_eq!(app.last_copied().unwrap().len(), HUGE_LEN);
    }

    #[test]
    fn test_quoted_table_name() {
        let mut runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(
                r#"
              create table "my ""table""" (x);
              insert into "my ""table""" values (42);
              "#,
            )
            .unwrap();
        let mut app = TestApp::new(&mut runtime, 80, 20);
        app.send_key(KeyCode::Enter.into());
        app.draw_and_snapshot("quoted table name");

        // full-table copy quotes the table name correctly
        app.send_char('y');
        app.send_char('3');
        assert_eq!(app.last_copied().unwrap(), "x\n42");
        app.send_char('y');
        app.send_char('5');
        assert_eq!(
            app.last_copied().unwrap(),
            "INSERT INTO \"my \"\"table\"\"\" (\"x\") VALUES (42);"
        );
    }
}
