//! Formatting ignore directives
//!
//! This module handles parsing and checking for format ignore directives:
//! - `-- solite-fmt: off` / `-- solite-fmt: on` for regions
//! - `-- solite-fmt-ignore` for next statement

use std::collections::HashSet;

/// Parsed ignore directives from source
#[derive(Debug, Clone, Default)]
pub struct IgnoreDirectives {
    /// Regions that should not be formatted: (start_offset, end_offset)
    pub regions: Vec<(usize, usize)>,
    /// Line numbers where the next statement should be ignored
    pub next_statement_lines: HashSet<usize>,
}

impl IgnoreDirectives {
    /// Parse ignore directives from source code
    pub fn parse(source: &str) -> Self {
        let mut regions = Vec::new();
        let mut next_statement_lines = HashSet::new();
        let mut in_ignore_region = false;
        let mut region_start = 0;

        for (line_num, line) in source.lines().enumerate() {
            let trimmed = line.trim();

            // Check for region directives
            if let Some(rest) = trimmed.strip_prefix("--") {
                let directive = rest.trim();

                if directive.eq_ignore_ascii_case("solite-fmt: off")
                    || directive.eq_ignore_ascii_case("solite-fmt:off")
                {
                    if !in_ignore_region {
                        in_ignore_region = true;
                        // Find byte offset of this line
                        region_start = source
                            .lines()
                            .take(line_num)
                            .map(|l| l.len() + 1) // +1 for newline
                            .sum();
                    }
                } else if directive.eq_ignore_ascii_case("solite-fmt: on")
                    || directive.eq_ignore_ascii_case("solite-fmt:on")
                {
                    if in_ignore_region {
                        in_ignore_region = false;
                        // Find byte offset of end of this line
                        let region_end: usize = source
                            .lines()
                            .take(line_num + 1)
                            .map(|l| l.len() + 1)
                            .sum();
                        regions.push((region_start, region_end));
                    }
                } else if directive.eq_ignore_ascii_case("solite-fmt-ignore")
                    || directive.eq_ignore_ascii_case("solite-fmt: ignore")
                {
                    // Next statement should be ignored
                    next_statement_lines.insert(line_num);
                }
            }
        }

        // If we're still in an ignore region at EOF, close it
        if in_ignore_region {
            regions.push((region_start, source.len()));
        }

        Self {
            regions,
            next_statement_lines,
        }
    }

    /// Check if a byte offset should be ignored (is in an ignored region)
    pub fn is_in_ignored_region(&self, offset: usize) -> bool {
        self.regions.iter().any(|(start, end)| offset >= *start && offset < *end)
    }

    /// Check if a statement starting on this line should be ignored
    /// (because the previous line has a solite-fmt-ignore directive)
    pub fn should_ignore_statement_at_line(&self, line: usize) -> bool {
        if line == 0 {
            return false;
        }
        self.next_statement_lines.contains(&(line - 1))
    }

    /// Get the original source for an ignored region
    pub fn get_ignored_source<'a>(&self, source: &'a str, span_start: usize, span_end: usize) -> Option<&'a str> {
        for (region_start, region_end) in &self.regions {
            if span_start >= *region_start && span_end <= *region_end {
                return Some(&source[span_start..span_end]);
            }
        }
        None
    }

    /// Check if a span overlaps with any ignored region
    pub fn overlaps_ignored_region(&self, start: usize, end: usize) -> bool {
        self.regions.iter().any(|(rs, re)| {
            // Overlap exists if: start < region_end AND end > region_start
            start < *re && end > *rs
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_directives() {
        let source = "SELECT 1";
        let directives = IgnoreDirectives::parse(source);
        assert!(directives.regions.is_empty());
        assert!(directives.next_statement_lines.is_empty());
    }

    #[test]
    fn test_ignore_region() {
        let source = "-- solite-fmt: off\nSELECT    1\n-- solite-fmt: on\nSELECT 2";
        let directives = IgnoreDirectives::parse(source);
        assert_eq!(directives.regions.len(), 1);
        assert!(directives.is_in_ignored_region(20)); // Inside region
        assert!(!directives.is_in_ignored_region(50)); // Outside region
    }

    #[test]
    fn test_ignore_region_no_spaces() {
        let source = "-- solite-fmt:off\nSELECT    1\n-- solite-fmt:on";
        let directives = IgnoreDirectives::parse(source);
        assert_eq!(directives.regions.len(), 1);
    }

    #[test]
    fn test_ignore_next_statement() {
        let source = "-- solite-fmt-ignore\nSELECT    1";
        let directives = IgnoreDirectives::parse(source);
        assert!(directives.should_ignore_statement_at_line(1));
        assert!(!directives.should_ignore_statement_at_line(0));
        assert!(!directives.should_ignore_statement_at_line(2));
    }

    #[test]
    fn test_unclosed_region() {
        let source = "-- solite-fmt: off\nSELECT 1";
        let directives = IgnoreDirectives::parse(source);
        assert_eq!(directives.regions.len(), 1);
        // Region should extend to EOF
        assert!(directives.is_in_ignored_region(source.len() - 1));
    }

    #[test]
    fn test_multiple_regions() {
        let source = "-- solite-fmt: off\nA\n-- solite-fmt: on\nB\n-- solite-fmt: off\nC\n-- solite-fmt: on";
        let directives = IgnoreDirectives::parse(source);
        assert_eq!(directives.regions.len(), 2);
    }
}
