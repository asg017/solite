//! Duration formatting utilities.

use std::time::Duration;

/// Format a duration for human-readable display.
///
/// - Under 5ms: shows with 3 decimal places (e.g., "1.234ms")
/// - Under 1s: shows integer milliseconds (e.g., "150ms")
/// - 1s and above: shows with 2 decimal places (e.g., "2.50s")
pub fn format_duration(duration: Duration) -> String {
    if duration < Duration::from_millis(5) {
        format!("{:.3}ms", duration.as_secs_f32() / 0.001)
    } else if duration < Duration::from_secs(1) {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{:.2}s", duration.as_secs_f32())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_microseconds() {
        let d = Duration::from_micros(500);
        assert_eq!(format_duration(d), "0.500ms");
    }

    #[test]
    fn test_format_duration_few_milliseconds() {
        let d = Duration::from_micros(2500);
        assert_eq!(format_duration(d), "2.500ms");
    }

    #[test]
    fn test_format_duration_milliseconds() {
        let d = Duration::from_millis(150);
        assert_eq!(format_duration(d), "150ms");
    }

    #[test]
    fn test_format_duration_seconds() {
        let d = Duration::from_millis(2500);
        assert_eq!(format_duration(d), "2.50s");
    }
}
