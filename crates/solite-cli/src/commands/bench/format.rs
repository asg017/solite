//! Formatting utilities for benchmark output.

use jiff::{Span, SpanRound, Unit};

/// Format a time span for human-readable display.
///
/// Durations under 50ms are shown with decimal precision (e.g., "4.5 ms").
/// Longer durations are rounded and shown in appropriate units.
///
/// # Examples
///
/// ```ignore
/// format_runtime(Span::new().microseconds(1000)) // "1.0 ms"
/// format_runtime(Span::new().seconds(61))        // "1m 1s"
/// ```
pub fn format_runtime(span: Span) -> String {
    let threshold = Span::new().milliseconds(50);

    // Check if under threshold
    let is_under_threshold = span
        .compare(threshold)
        .map(|ord| ord.is_lt())
        .unwrap_or(false);

    if is_under_threshold {
        match span.total(Unit::Millisecond) {
            Ok(total) => format!("{total:.1} ms"),
            Err(_) => "? ms".to_string(),
        }
    } else {
        match span.round(
            SpanRound::new()
                .largest(Unit::Minute)
                .smallest(Unit::Millisecond),
        ) {
            Ok(rounded) => format!("{rounded:?}"),
            Err(_) => "?".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_runtime_microseconds() {
        insta::assert_snapshot!(format_runtime(Span::new().microseconds(1000)), @"1.0 ms");
    }

    #[test]
    fn test_format_runtime_few_milliseconds() {
        insta::assert_snapshot!(format_runtime("4ms 4us".parse().unwrap()), @"4.0 ms");
    }

    #[test]
    fn test_format_runtime_near_threshold() {
        insta::assert_snapshot!(format_runtime("49ms 999us".parse().unwrap()), @"50.0 ms");
    }

    #[test]
    fn test_format_runtime_at_threshold() {
        insta::assert_snapshot!(format_runtime("50ms 999us".parse().unwrap()), @"51ms");
    }

    #[test]
    fn test_format_runtime_sub_second() {
        insta::assert_snapshot!(format_runtime("989ms 999us".parse().unwrap()), @"990ms");
    }

    #[test]
    fn test_format_runtime_seconds() {
        insta::assert_snapshot!(format_runtime("1s 1ms 999us".parse().unwrap()), @"1s 2ms");
        insta::assert_snapshot!(format_runtime("2s 1ms 999us".parse().unwrap()), @"2s 2ms");
    }

    #[test]
    fn test_format_runtime_minutes() {
        insta::assert_snapshot!(format_runtime("61s".parse().unwrap()), @"1m 1s");
    }
}
