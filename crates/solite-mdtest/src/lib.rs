//! Markdown-based test framework for solite SQL LSP
//!
//! Write SQL tests in Markdown files with embedded markers and assertions.
//!
//! ## Markers
//!
//! - `<acN>` - Autocomplete position markers
//! - `<hvN>` - Hover position markers
//! - `-- error: [rule-id]` - Expected diagnostic inline comment
//!
//! See docs/MDTEST_DESIGN.md for full documentation and examples.

mod assertions;
mod markers;
mod parser;
mod reporter;
mod runner;

pub use parser::{parse_markdown, MdTest, TestFile};
pub use runner::{run_test, run_tests, TestResult};

use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MdTestError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error in {file}: {message}")]
    Parse { file: String, message: String },

    #[error("Assertion error: {0}")]
    Assertion(String),

    #[error("Test failed: {name}\n{details}")]
    TestFailed { name: String, details: String },
}

/// Run all markdown tests in a directory
pub fn run_test_directory(dir: &Path) -> Result<(), MdTestError> {
    run_tests(dir)
}

/// Run a single markdown test file
pub fn run_test_file(path: &Path) -> Result<(), MdTestError> {
    let content = std::fs::read_to_string(path)?;
    let tests = parse_markdown(&content, path.to_string_lossy().as_ref())?;

    for test in tests {
        let result = run_test(&test)?;
        if !result.passed {
            return Err(MdTestError::TestFailed {
                name: test.name.clone(),
                details: result.format_failures(),
            });
        }
    }

    Ok(())
}
