//! Value display utilities for documentation.

use crate::commands::test::snap::{ValueCopy, ValueCopyValue};
use solite_core::sqlite::escape_string;

/// Display a copied SQLite value as a string for documentation.
///
/// # Formats
///
/// - NULL/Pointer: `"NULL"`
/// - Integer: decimal representation
/// - Double: decimal representation, always with a decimal point (`2.0`,
///   matching SQLite) so REAL values are distinguishable from INTEGERs
/// - Text: escaped SQL string
/// - Blob: uppercase hex with `X'...'` prefix
pub fn display_value(v: &ValueCopy) -> String {
    match &v.value {
        ValueCopyValue::Null | ValueCopyValue::Pointer => "NULL".to_string(),
        ValueCopyValue::Int(value) => value.to_string(),
        ValueCopyValue::Double(value) => {
            if value.is_finite() && value.fract() == 0.0 {
                format!("{:.1}", value)
            } else {
                value.to_string()
            }
        }
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
    use super::*;

    fn value(v: ValueCopyValue) -> ValueCopy {
        ValueCopy::new(v, None)
    }

    #[test]
    fn test_null() {
        assert_eq!(display_value(&value(ValueCopyValue::Null)), "NULL");
        assert_eq!(display_value(&value(ValueCopyValue::Pointer)), "NULL");
    }

    #[test]
    fn test_integer() {
        assert_eq!(display_value(&value(ValueCopyValue::Int(42))), "42");
        assert_eq!(display_value(&value(ValueCopyValue::Int(-1))), "-1");
    }

    #[test]
    fn test_double_distinguishable_from_integer() {
        assert_eq!(display_value(&value(ValueCopyValue::Double(2.0))), "2.0");
        assert_eq!(display_value(&value(ValueCopyValue::Double(2.5))), "2.5");
        assert_eq!(display_value(&value(ValueCopyValue::Double(-3.0))), "-3.0");
    }

    #[test]
    fn test_text_escaped() {
        assert_eq!(
            display_value(&value(ValueCopyValue::Text(b"hello".to_vec()))),
            "'hello'"
        );
    }

    #[test]
    fn test_blob_uppercase_hex() {
        assert_eq!(
            display_value(&value(ValueCopyValue::Blob(vec![0xab, 0xcd]))),
            "X'ABCD'"
        );
    }
}
