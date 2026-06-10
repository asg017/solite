//! Statistical calculations for benchmark timing data.

use jiff::Span;

/// Calculate the average of a slice of time spans.
///
/// Returns a Span representing the mean duration, or None if the slice is empty.
pub fn average(times: &[Span]) -> Option<Span> {
    if times.is_empty() {
        return None;
    }

    let microseconds: Vec<f64> = times
        .iter()
        .filter_map(|span| span.total(jiff::Unit::Microsecond).ok())
        .collect();

    if microseconds.is_empty() {
        return None;
    }

    Some(Span::new().microseconds(statistical::mean(&microseconds) as i64))
}

/// Calculate the standard deviation of a slice of time spans.
///
/// This is the *sample* standard deviation (Bessel-corrected, n-1
/// denominator) — `statistical::standard_deviation` divides the sum of
/// squared deviations by `len - 1`. The right choice for benchmark runs,
/// which sample a small number of iterations from a larger population.
///
/// Returns None for fewer than two data points (the sample formula needs
/// at least two; `statistical` asserts on less).
pub fn stddev(times: &[Span]) -> Option<Span> {
    let microseconds: Vec<f64> = times
        .iter()
        .filter_map(|span| span.total(jiff::Unit::Microsecond).ok())
        .collect();

    if microseconds.len() < 2 {
        return None;
    }

    let mean = statistical::mean(&microseconds);
    let std = statistical::standard_deviation(&microseconds, Some(mean)) as i64;
    Some(Span::new().microseconds(std))
}

/// Find the minimum span in a slice.
///
/// Returns None if the slice is empty.
pub fn min(times: &[Span]) -> Option<Span> {
    times
        .iter()
        .min_by(|a, b| a.compare(*b).unwrap_or(std::cmp::Ordering::Equal))
        .cloned()
}

/// Find the maximum span in a slice.
///
/// Returns None if the slice is empty.
pub fn max(times: &[Span]) -> Option<Span> {
    times
        .iter()
        .max_by(|a, b| a.compare(*b).unwrap_or(std::cmp::Ordering::Equal))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_average_empty() {
        assert!(average(&[]).is_none());
    }

    #[test]
    fn test_average_single() {
        let times = vec![Span::new().milliseconds(100)];
        let avg = average(&times).unwrap();
        let total = avg.total(jiff::Unit::Millisecond).unwrap();
        assert!((total - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_average_multiple() {
        let times = vec![
            Span::new().milliseconds(100),
            Span::new().milliseconds(200),
            Span::new().milliseconds(300),
        ];
        let avg = average(&times).unwrap();
        let total = avg.total(jiff::Unit::Millisecond).unwrap();
        assert!((total - 200.0).abs() < 0.001);
    }

    #[test]
    fn test_stddev_empty() {
        assert!(stddev(&[]).is_none());
    }

    #[test]
    fn test_stddev_single_point_is_none() {
        // sample stddev is undefined for n=1 (and `statistical` asserts)
        let times = vec![Span::new().milliseconds(100)];
        assert!(stddev(&times).is_none());
    }

    #[test]
    fn test_stddev_is_sample_stddev() {
        // sample (n-1) stddev of {100, 200, 300}ms = sqrt(20000/2) = 100ms
        let times = vec![
            Span::new().milliseconds(100),
            Span::new().milliseconds(200),
            Span::new().milliseconds(300),
        ];
        let s = stddev(&times).unwrap();
        let total = s.total(jiff::Unit::Millisecond).unwrap();
        assert!((total - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_min_empty() {
        assert!(min(&[]).is_none());
    }

    #[test]
    fn test_min_finds_smallest() {
        let times = vec![
            Span::new().milliseconds(300),
            Span::new().milliseconds(100),
            Span::new().milliseconds(200),
        ];
        let m = min(&times).unwrap();
        let total = m.total(jiff::Unit::Millisecond).unwrap();
        assert!((total - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_max_empty() {
        assert!(max(&[]).is_none());
    }

    #[test]
    fn test_max_finds_largest() {
        let times = vec![
            Span::new().milliseconds(100),
            Span::new().milliseconds(300),
            Span::new().milliseconds(200),
        ];
        let m = max(&times).unwrap();
        let total = m.total(jiff::Unit::Millisecond).unwrap();
        assert!((total - 300.0).abs() < 0.001);
    }
}
