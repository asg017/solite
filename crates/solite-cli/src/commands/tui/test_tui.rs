#[cfg(test)]
mod tests {
    use crate::commands::tui::{App, Clipboard, ListingPage, Page, SharedClipboard};
    use crossterm::event::{KeyCode, KeyEvent};
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

        // move selection off the first row, then sort: selection resets to row 1
        app.send_char('j');
        app.send_char('[');
        app.draw_and_snapshot("sorted ascending");

        app.send_char(']');
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
