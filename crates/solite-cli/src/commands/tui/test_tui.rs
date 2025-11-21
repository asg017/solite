#[cfg(test)]
mod tests {
    use crate::commands::tui::{App, ListingPage, Page};
    use crossterm::event::{KeyCode, KeyEvent};
    use insta::assert_snapshot;
    use ratatui::{backend::TestBackend, Terminal};
    use solite_core::Runtime;

    struct TestApp<'a> {
        terminal: Terminal<TestBackend>,
        app: App<'a>,
    }
    impl<'a> TestApp<'a> {
        fn new(runtime: &'a mut Runtime, width: u16, height: u16) -> Self {
            let terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let theme = crate::commands::tui::tui_theme::CTP_MOCHA_THEME.clone();
            let page = Page::Listing(ListingPage::new(&runtime, &theme.clone()));
            let app = App { runtime, page, theme };
            Self { terminal, app }
        }

        fn draw_and_snapshot(&mut self, name: &str) {
            self.terminal.draw(|frame| self.app.render(frame)).unwrap();
            assert_snapshot!(name, self.terminal.backend());
        }
        fn send_key(&mut self, key: KeyEvent) {
            self.app.handle_key(key);
        }
    }

    #[test]
    fn test_render_app() {
        let mut runtime = Runtime::new(None);
        runtime
            .connection
            .execute_script(
                r#"
              create table t as select 1 as a, 'asdf' as b, sqlite_version() as c;

              create table t2(c1,c2,c3,c4,c5,c6,c7,c8,c9,c10);
              insert into t2 select 1,2,3,4,5,6,7,8,9,10;
              insert into t2 select 'a','b','c','d','e','f','g','h','i','j';

              
              "#,
            )
            .unwrap();
        let mut app = TestApp::new(&mut runtime, 80, 20);
        app.draw_and_snapshot("listing page");

        app.send_key(KeyCode::Enter.into());
        app.draw_and_snapshot("enter -> table view");

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
}
