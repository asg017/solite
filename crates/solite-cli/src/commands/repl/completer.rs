

use super::highlighter::CTP_MOCHA_THEME;
use rustyline::completion::{Completer, Pair};
use rustyline::Result;
use solite_core::Runtime;
use std::cell::RefCell;
use std::rc::Rc;

// https://github.com/sqlite/sqlite/blob/cd889c7a88b2bd23ac71a897c54c43c84eee972d/ext/misc/completion.c#L74-L85
pub(crate) struct ReplCompleter {
    runtime: Rc<RefCell<Runtime>>,
}

impl ReplCompleter {
    pub fn new(runtime: Rc<RefCell<Runtime>>) -> Self {
        Self { runtime }
    }

    fn complete_dot(
        &self,
        line: &str,
        _pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> Result<(usize, Vec<Pair>)> {
        let dots = ["load", "tables", "open", "schema", "timer", "help"];
        if line.contains(' ') || !line.starts_with('.') {
            return Ok((0, vec![]));
        }
        let prefix = &line[1..];

        let candidates = dots
            .iter()
            .filter_map(|v| {
                if v.starts_with(prefix) {
                    Some(Pair {
                        display: CTP_MOCHA_THEME.style_dot(v),
                        replacement: format!("{v} "),
                    })
                } else {
                    None
                }
            })
            .collect();
        Ok((1, candidates))
    }
    fn complete_sql(
        &self,
        line: &str,
        _pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> Result<(usize, Vec<Pair>)> {
        let rt = self.runtime.borrow();
        let (last_word, last_word_idx) = line
            .trim_end()
            .rfind(|c: char| c.is_whitespace())
            .map(|idx| (&line[(idx + 1)..], idx + 1))
            .unwrap_or((line, 0));

        let stmt = match rt.connection.prepare(
            r#"
              select
                case
                  when phase == 1 then lower(candidate)
                  else candidate
                end as candidate,
                phase,
                case
                  when phase == 8 then 1 /* tables */
                  when phase == 9 then 2 /* columns */
                  when phase == 3 then 3 /* functions */
                  when phase == 1 then 4 /* keywords */
                  when phase == 10 then 5 /* modules */
                  when phase == 7 then 6 /* databases */
                  when phase == 2 then 7 /* pragmas */
                  when phase == 4 then 8 /* collations */
                  when phase == 5 then 9 /* indexes */
                  when phase == 6 then 10 /* triggers */
                  else 100
                end as rank
              from completion(?, ?)
              group by 1
              order by rank, candidate
              limit 20
            "#,
        ) {
            Ok((_, Some(stmt))) => stmt,
            _ => return Ok((0, vec![])),
        };

        stmt.bind_text(1, last_word);
        stmt.bind_text(2, line);

        let mut candidates: Vec<Pair> = vec![];
        while let Ok(Some(row)) = stmt.next() {
            let Some(first) = row.first() else { continue };
            let candidate = first.as_str().to_string();
            let phase = row.get(1).map(|v| v.as_int64()).unwrap_or(0);
            let display = match phase {
                9 => format!("ᶜ {}", candidate),      // columns
                1 => CTP_MOCHA_THEME.style_keyword(&candidate), // keywords
                _ => format!("ᵗ {}", candidate),      // tables/other
            };
            candidates.push(Pair {
                display,
                replacement: candidate,
            });
        }
        Ok((last_word_idx, candidates))
    }
}

impl Completer for ReplCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &rustyline::Context<'_>,
    ) -> Result<(usize, Vec<Self::Candidate>)> {
        if line.starts_with('.') {
            self.complete_dot(line, pos, ctx)
        } else {
            self.complete_sql(line, pos, ctx)
        }
    }
}
