//! Value display utilities for documentation.

use crate::commands::test::snap::{ValueCopy, ValueCopyValue};
use solite_core::sqlite::escape_string;

/// Display a copied SQLite value as a string for documentation.
///
/// # Formats
///
/// - NULL/Pointer: `"NULL"`
/// - Integer: decimal representation
/// - Double: decimal representation
/// - Text: escaped SQL string
/// - Blob: uppercase hex with `X'...'` prefix
pub fn display_value(v: &ValueCopy) -> String {
    match &v.value {
        ValueCopyValue::Null | ValueCopyValue::Pointer => "NULL".to_string(),
        ValueCopyValue::Int(value) => value.to_string(),
        ValueCopyValue::Double(value) => value.to_string(),
        ValueCopyValue::Text(value) => {
            escape_string(&String::from_utf8_lossy(value))
        }
        ValueCopyValue::Blob(value) => {
            format!("X'{}'", hex::encode(value).to_uppercase())
        }
    }
}

#[cfg(test)]
mod tests {
    // Note: Testing requires constructing ValueCopy instances.
    // The logic is tested indirectly through integration tests.
}
