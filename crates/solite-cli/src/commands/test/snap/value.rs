//! Value copying and snapshot formatting for SQLite values.

use solite_core::sqlite::{escape_string, ValueRefX, ValueRefXValue, JSON_SUBTYPE, POINTER_SUBTYPE};

/// Owned copy of a SQLite value for snapshot purposes.
pub enum ValueCopyValue {
    Null,
    Int(i64),
    Double(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
    Pointer,
}

/// A copied SQLite value with its subtype.
pub struct ValueCopy {
    subtype: Option<u32>,
    pub value: ValueCopyValue,
}

/// Format a copied value for snapshot output.
pub fn snapshot_value(v: &ValueCopy) -> String {
    match &v.value {
        ValueCopyValue::Null => "NULL".to_string(),
        ValueCopyValue::Int(value) => value.to_string(),
        ValueCopyValue::Double(value) => value.to_string(),
        ValueCopyValue::Text(value) => {
            let text = String::from_utf8_lossy(value);
            let escaped = escape_string(&text);
            if v.subtype == Some(JSON_SUBTYPE) {
                format!("(json) {}", escaped)
            } else {
                escaped
            }
        }
        ValueCopyValue::Blob(value) => format!("X'{}'", hex::encode(value)),
        ValueCopyValue::Pointer => "pointer[]".to_string(),
    }
}

/// Copy a SQLite value reference to an owned value.
pub fn copy(value: &ValueRefX<'_>) -> ValueCopy {
    let new_value = match value.value {
        ValueRefXValue::Null => {
            if value.subtype() == Some(POINTER_SUBTYPE) {
                ValueCopyValue::Pointer
            } else {
                ValueCopyValue::Null
            }
        }
        ValueRefXValue::Int(v) => ValueCopyValue::Int(v),
        ValueRefXValue::Double(v) => ValueCopyValue::Double(v),
        ValueRefXValue::Text(v) => ValueCopyValue::Text(v.to_vec()),
        ValueRefXValue::Blob(v) => ValueCopyValue::Blob(v.to_vec()),
    };

    ValueCopy {
        subtype: value.subtype(),
        value: new_value,
    }
}
