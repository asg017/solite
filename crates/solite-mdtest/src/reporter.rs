//! Test result reporting for mdtest

use std::fmt;

/// A test failure
#[derive(Debug)]
pub enum TestFailure {
    /// Marker not found in SQL
    MissingMarker { marker_id: u32, kind: String },

    /// Expected completion items not found
    AutocompleteMissing {
        marker_id: u32,
        missing: Vec<String>,
        got: Vec<String>,
    },

    /// Extra completion items found (strict mode)
    AutocompleteExtra { marker_id: u32, extra: Vec<String> },

    /// Hover failed to resolve
    HoverFailed { marker_id: u32, reason: String },

    /// Expected hover content not found
    HoverMissing {
        marker_id: u32,
        expected: String,
        got: String,
    },

    /// Expected diagnostic not found
    MissingDiagnostic {
        line: u32,
        rule: Option<String>,
        message: Option<String>,
    },

    /// Unexpected diagnostic found
    UnexpectedDiagnostic { line: u32, message: String },
}

impl fmt::Display for TestFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TestFailure::MissingMarker { marker_id, kind } => {
                write!(f, "Marker <{}{}> not found in SQL", kind, marker_id)
            }

            TestFailure::AutocompleteMissing {
                marker_id,
                missing,
                got,
            } => {
                write!(
                    f,
                    "Autocomplete <ac{}> missing: [{}]\n  Got: [{}]",
                    marker_id,
                    missing.join(", "),
                    got.join(", ")
                )
            }

            TestFailure::AutocompleteExtra { marker_id, extra } => {
                write!(
                    f,
                    "Autocomplete <ac{}> has extra items (strict mode): [{}]",
                    marker_id,
                    extra.join(", ")
                )
            }

            TestFailure::HoverFailed { marker_id, reason } => {
                write!(f, "Hover <hv{}> failed: {}", marker_id, reason)
            }

            TestFailure::HoverMissing {
                marker_id,
                expected,
                got,
            } => {
                write!(
                    f,
                    "Hover <hv{}> missing expected content:\n  Expected: \"{}\"\n  Got: \"{}\"",
                    marker_id, expected, got
                )
            }

            TestFailure::MissingDiagnostic {
                line,
                rule,
                message,
            } => {
                let mut desc = format!("line {}", line + 1);
                if let Some(r) = rule {
                    desc.push_str(&format!(" [{}]", r));
                }
                if let Some(m) = message {
                    desc.push_str(&format!(" \"{}\"", m));
                }
                write!(f, "Expected diagnostic not found: {}", desc)
            }

            TestFailure::UnexpectedDiagnostic { line, message } => {
                write!(
                    f,
                    "Unexpected diagnostic on line {}: {}",
                    line + 1,
                    message
                )
            }
        }
    }
}
