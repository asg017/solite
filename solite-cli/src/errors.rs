use std::ops::Range;

use codespan_reporting::diagnostic::{Diagnostic, Label};
use codespan_reporting::files::SimpleFiles;
use codespan_reporting::term;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
use solite_core::sqlite::SQLiteError;
use termcolor::Buffer;

fn error_diagnostic(
    file_name: &str,
    sql: &str,
    error: &SQLiteError,
    additional_offset: Option<usize>,
) -> (SimpleFiles<String, String>, Diagnostic<usize>) {
    let mut files = SimpleFiles::new();
    let source = sql.to_owned();
    let id = files.add(file_name.to_owned(), source.clone());
    let labels = match err_diagnostic_range(source.as_str(), error, additional_offset) {
        Some(range) => vec![Label::primary(id, range).with_message(&error.message)],
        None => vec![],
    };
    let diagnostic = Diagnostic::error()
        .with_message(if labels.is_empty() {
            error.message.clone()
        } else {
            error.code_description.clone()
        })
        .with_code(error.result_code.to_string())
        .with_labels(labels);

    (files, diagnostic)
}
pub(crate) fn report_error(
    file_name: &str,
    sql: &str,
    error: &SQLiteError,
    additional_offset: Option<usize>,
) {
    let (files, diagnostic) = error_diagnostic(file_name, sql, error, additional_offset);
    let writer = StandardStream::stderr(ColorChoice::Auto);

    let config = term::Config::default();
    term::emit(&mut writer.lock(), &config, &files, &diagnostic).unwrap();
}

pub(crate) fn report_error_string(
    file_name: &str,
    sql: &str,
    error: &SQLiteError,
    additional_offset: Option<usize>,
) -> String {
    let (files, diagnostic) = error_diagnostic(file_name, sql, error, additional_offset);
    let config = term::Config::default();
    let mut b = Buffer::no_color();
    term::emit(&mut b, &config, &files, &diagnostic).unwrap();
    String::from_utf8(b.into_inner()).unwrap()
}

fn err_diagnostic_range(
    sql: &str,
    error: &SQLiteError,
    additional_offset: Option<usize>,
) -> Option<Range<usize>> {
    match error.offset {
        Some(offset) => {
            let start = offset + additional_offset.unwrap_or(0);
            let mut idx = start + 1;
            while let Some(c) = sql.as_bytes().get(idx) {
                if c.is_ascii_alphanumeric() || *c == b'_' {
                    idx += 1;
                } else {
                    break;
                }
            }
            Some(start..idx)
        }
        None => None,
    }
}
