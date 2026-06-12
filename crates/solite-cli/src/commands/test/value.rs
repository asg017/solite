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
        ValueRefXValue::Double(d) => format_double(*d),
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

/// Format a double the way SQLite renders REAL values (`%!.15g`):
/// 15 significant digits, a guaranteed decimal point (`1.0`, not `1`),
/// and exponent form for very large/small magnitudes (`1.0e+20`).
///
/// Shared by the inline-assertion renderer and the snapshot renderer so
/// the two can't drift.
pub fn format_double(value: f64) -> String {
    if value == f64::INFINITY {
        return "Inf".to_string();
    }
    if value == f64::NEG_INFINITY {
        return "-Inf".to_string();
    }
    if value.is_nan() {
        return "NaN".to_string();
    }

    // 15 significant digits in scientific form, e.g. "1.23456789012346e17";
    // the exponent decides between fixed and exponential rendering, exactly
    // like C's %g.
    let sci = format!("{:.14e}", value);
    let e_idx = sci.find('e').expect("{:e} always contains an exponent");
    let exp: i32 = sci[e_idx + 1..].parse().expect("exponent is an integer");

    if !(-4..15).contains(&exp) {
        // Exponential form, SQLite-style: "1.0e+20", "1.0e-05"
        let mantissa = ensure_decimal(trim_trailing_zeros(&sci[..e_idx]));
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{}{:02}", mantissa, sign, exp.abs())
    } else {
        // Fixed form rounded to 15 significant digits: "3.14", "1.0", "100.0"
        let decimals = (15 - 1 - exp).max(0) as usize;
        let fixed = format!("{:.*}", decimals, value);
        ensure_decimal(trim_trailing_zeros(&fixed))
    }
}

/// Strip trailing zeros after a decimal point (`"3.140000"` → `"3.14"`,
/// `"1.000"` → `"1."`). Leaves strings without a `.` untouched.
fn trim_trailing_zeros(s: &str) -> &str {
    if s.contains('.') {
        s.trim_end_matches('0')
    } else {
        s
    }
}

/// Guarantee the rendering reads as a float: append `0` after a bare
/// trailing `.`, or `.0` when no decimal point survived.
fn ensure_decimal(s: &str) -> String {
    if s.ends_with('.') {
        format!("{}0", s)
    } else if s.contains('.') {
        s.to_string()
    } else {
        format!("{}.0", s)
    }
}

/// Compare two rendered values numerically: true when both parse as `f64`
/// and are exactly equal (no epsilon). Lets `-- 1.0`, `-- 1`, and
/// `-- 1.0e+20` all match regardless of formatting.
pub fn values_numerically_equal(expected: &str, actual: &str) -> bool {
    match (expected.parse::<f64>(), actual.parse::<f64>()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // value_to_string requires constructing ValueRefX instances (core
    // internals); it's covered by the test_impl integration tests in
    // mod.rs. The pure helpers are tested directly here.

    #[test]
    fn test_format_double_integral_values_keep_a_decimal() {
        assert_eq!(format_double(1.0), "1.0");
        assert_eq!(format_double(-1.0), "-1.0");
        assert_eq!(format_double(0.0), "0.0");
        assert_eq!(format_double(100.0), "100.0");
    }

    #[test]
    fn test_format_double_fractions() {
        assert_eq!(format_double(3.14), "3.14");
        assert_eq!(format_double(-0.5), "-0.5");
        assert_eq!(format_double(0.0001), "0.0001");
        assert_eq!(format_double(0.1), "0.1");
    }

    #[test]
    fn test_format_double_exponent_form() {
        assert_eq!(format_double(1e20), "1.0e+20");
        assert_eq!(format_double(1e-5), "1.0e-05");
        assert_eq!(format_double(-1e20), "-1.0e+20");
        assert_eq!(format_double(1.5e100), "1.5e+100");
    }

    #[test]
    fn test_format_double_15_significant_digits() {
        assert_eq!(format_double(123456789012345678.0), "1.23456789012346e+17");
        assert_eq!(format_double(0.3333333333333333), "0.333333333333333");
    }

    #[test]
    fn test_format_double_non_finite() {
        assert_eq!(format_double(f64::INFINITY), "Inf");
        assert_eq!(format_double(f64::NEG_INFINITY), "-Inf");
        assert_eq!(format_double(f64::NAN), "NaN");
    }

    #[test]
    fn test_values_numerically_equal() {
        assert!(values_numerically_equal("1.0", "1"));
        assert!(values_numerically_equal("1.0e+20", "100000000000000000000"));
        assert!(values_numerically_equal("3.14", "3.14"));
        assert!(!values_numerically_equal("1.0", "1.1"));
        assert!(!values_numerically_equal("'1.0'", "1.0"));
        assert!(!values_numerically_equal("abc", "abc"));
        // NaN never equals itself
        assert!(!values_numerically_equal("NaN", "NaN"));
    }
}
