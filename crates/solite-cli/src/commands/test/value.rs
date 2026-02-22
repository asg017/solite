//! Value conversion utilities for test assertions.

use solite_core::sqlite::{ValueRefX, ValueRefXValue};

/// Convert a SQLite value reference to a string for comparison.
///
/// # Formats
///
/// - NULL: `"NULL"`
/// - Integer: decimal representation
/// - Double: decimal representation
/// - Text: SQL single-quoted literal with escaped quotes
/// - Blob: hex representation with `X'...'` prefix
///
/// # Examples
///
/// ```ignore
/// value_to_string(&null_value)  // "NULL"
/// value_to_string(&int_42)      // "42"
/// value_to_string(&text_hello)  // "'hello'"
/// ```
pub fn value_to_string(v: &ValueRefX) -> String {
    match &v.value {
        ValueRefXValue::Null => "NULL".to_string(),
        ValueRefXValue::Int(i) => format!("{}", i),
        ValueRefXValue::Double(d) => format!("{}", d),
        ValueRefXValue::Text(b) => {
            let mut s = String::from("'");
            let inner = String::from_utf8_lossy(b).replace('\'', "''");
            s.push_str(&inner);
            s.push('\'');
            s
        }
        ValueRefXValue::Blob(b) => {
            // Format as hex literal
            format!("X'{}'", hex::encode(b))
        }
    }
}

#[cfg(test)]
mod tests {
    // Note: These tests would require constructing ValueRefX instances,
    // which depends on the solite_core internals. For now, we test
    // the logic indirectly through integration tests.
}
