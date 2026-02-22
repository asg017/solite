//! Layout computation including column collapsing.

use crate::types::{ColumnInfo, TableLayout};

/// Minimum width for the ellipsis column ("…").
const ELLIPSIS_WIDTH: usize = 3;

/// Width for border characters (│ on each side plus spaces).
const BORDER_OVERHEAD: usize = 4; // "│ " on left, " │" on right

/// Width per column separator (" │ ").
const SEPARATOR_WIDTH: usize = 3;

/// Minimum width for any column (must fit at least a few chars).
const MIN_COLUMN_WIDTH: usize = 3;

/// Maximum reasonable width for a single column when filling available space.
/// Content longer than this will be truncated.
const MAX_REASONABLE_COLUMN_WIDTH: usize = 40;

/// Compute the layout for rendering, potentially collapsing columns.
///
/// Strategy:
/// 1. Try to fit all columns, shrinking them if needed
/// 2. Only collapse columns if they can't fit even at minimum width
/// 3. Fill available width by expanding columns up to their natural width
pub fn compute_layout(
    columns: &[ColumnInfo],
    max_width: usize,
    max_cell_width: usize,
) -> TableLayout {
    if columns.is_empty() {
        return TableLayout::all_visible(vec![]);
    }

    let n = columns.len();

    // Calculate natural (ideal) and minimum widths for each column
    // Cap natural width at a reasonable maximum to prevent one column from dominating
    let natural_widths: Vec<usize> = columns
        .iter()
        .map(|c| c.display_width().min(max_cell_width).min(MAX_REASONABLE_COLUMN_WIDTH))
        .collect();

    let min_widths: Vec<usize> = columns
        .iter()
        .map(|c| c.header_width.max(MIN_COLUMN_WIDTH))
        .collect();

    // Calculate total width if all columns shown at minimum width
    let min_total = calculate_total_width(&min_widths);

    // If all columns fit at minimum width, show all and distribute extra space
    if min_total <= max_width {
        let widths = distribute_width(&min_widths, &natural_widths, max_width);
        return TableLayout::all_visible(widths);
    }

    // Need to collapse columns - try to fit as many as possible at minimum width
    collapse_columns_min_width(n, &min_widths, &natural_widths, max_width)
}

/// Distribute available width among columns, expanding from min to natural width.
fn distribute_width(
    min_widths: &[usize],
    natural_widths: &[usize],
    max_width: usize,
) -> Vec<usize> {
    let n = min_widths.len();
    let mut widths = min_widths.to_vec();

    let min_total = calculate_total_width(min_widths);
    let mut available = max_width.saturating_sub(min_total);

    if available == 0 {
        return widths;
    }

    // Calculate how much each column wants to grow
    let mut wants: Vec<usize> = (0..n)
        .map(|i| natural_widths[i].saturating_sub(min_widths[i]))
        .collect();

    // Distribute available space proportionally
    while available > 0 {
        let total_want: usize = wants.iter().sum();
        if total_want == 0 {
            break;
        }

        let mut distributed = 0;
        for i in 0..n {
            if wants[i] > 0 {
                // Give at least 1 char to columns that want more
                let give = ((wants[i] as f64 / total_want as f64) * available as f64)
                    .ceil()
                    .min(wants[i] as f64) as usize;
                let give = give.max(1).min(wants[i]).min(available - distributed);

                widths[i] += give;
                wants[i] -= give;
                distributed += give;

                if distributed >= available {
                    break;
                }
            }
        }

        available = available.saturating_sub(distributed);
        if distributed == 0 {
            break;
        }
    }

    widths
}

/// Calculate total table width for given column widths.
fn calculate_total_width(widths: &[usize]) -> usize {
    if widths.is_empty() {
        return BORDER_OVERHEAD;
    }

    // Border + columns + separators between columns
    BORDER_OVERHEAD + widths.iter().sum::<usize>() + SEPARATOR_WIDTH * (widths.len() - 1)
}

/// Collapse columns using minimum widths, fitting as many as possible.
fn collapse_columns_min_width(
    total_columns: usize,
    min_widths: &[usize],
    natural_widths: &[usize],
    max_width: usize,
) -> TableLayout {
    if total_columns == 0 {
        return TableLayout::all_visible(vec![]);
    }

    // Greedily add columns from front and back, alternating
    let mut front_indices: Vec<usize> = vec![];
    let mut back_indices: Vec<usize> = vec![];

    let mut front_idx = 0;
    let mut back_idx = total_columns - 1;

    // Reserve space for ellipsis column
    let mut current_width = BORDER_OVERHEAD + ELLIPSIS_WIDTH + SEPARATOR_WIDTH;

    let mut add_from_front = true;

    while front_idx <= back_idx {
        let idx = if add_from_front { front_idx } else { back_idx };
        let col_width = min_widths[idx];
        let additional = col_width + SEPARATOR_WIDTH;

        if current_width + additional <= max_width {
            current_width += additional;
            if add_from_front {
                front_indices.push(idx);
                front_idx += 1;
            } else {
                back_indices.push(idx);
                back_idx -= 1;
            }
            add_from_front = !add_from_front;
        } else {
            break;
        }
    }

    // Build visible columns list
    let mut visible_columns = front_indices.clone();
    let ellipsis_position = if front_idx <= back_idx {
        Some(visible_columns.len())
    } else {
        None
    };

    back_indices.reverse();
    visible_columns.extend(back_indices);

    // If all columns fit, return without ellipsis
    if visible_columns.len() == total_columns {
        let min_for_visible: Vec<usize> = visible_columns.iter().map(|&i| min_widths[i]).collect();
        let nat_for_visible: Vec<usize> = visible_columns
            .iter()
            .map(|&i| natural_widths[i])
            .collect();
        let widths = distribute_width(&min_for_visible, &nat_for_visible, max_width);
        return TableLayout::all_visible(widths);
    }

    // Build width vector for visible columns and distribute remaining space
    let min_for_visible: Vec<usize> = visible_columns.iter().map(|&i| min_widths[i]).collect();
    let nat_for_visible: Vec<usize> = visible_columns
        .iter()
        .map(|&i| natural_widths[i])
        .collect();

    // Account for ellipsis in available width
    let available_for_cols = max_width - ELLIPSIS_WIDTH - SEPARATOR_WIDTH;
    let column_widths = distribute_width(&min_for_visible, &nat_for_visible, available_for_cols);

    TableLayout {
        visible_columns,
        ellipsis_position,
        column_widths,
        total_columns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_columns(widths: &[usize]) -> Vec<ColumnInfo> {
        widths
            .iter()
            .enumerate()
            .map(|(i, &w)| {
                let mut col = ColumnInfo::new(format!("c{}", i));
                col.max_content_width = w;
                col
            })
            .collect()
    }

    #[test]
    fn test_all_fit() {
        let columns = make_columns(&[10, 10, 10]);
        let layout = compute_layout(&columns, 80, 100);

        assert_eq!(layout.visible_columns, vec![0, 1, 2]);
        assert!(layout.ellipsis_position.is_none());
        assert_eq!(layout.total_columns, 3);
    }

    #[test]
    fn test_shrink_to_fit() {
        // 3 columns with wide content but narrow headers
        // Should shrink to fit all rather than collapse
        let columns = make_columns(&[30, 30, 30]);
        let layout = compute_layout(&columns, 50, 100);

        // Should show all 3 columns, just narrower
        assert_eq!(layout.visible_columns.len(), 3);
        assert!(layout.ellipsis_position.is_none());
    }

    #[test]
    fn test_collapse_when_necessary() {
        // Many columns that can't fit even at minimum width
        let columns = make_columns(&[20; 20]);
        let layout = compute_layout(&columns, 80, 100);

        assert!(layout.visible_columns.len() < 20);
        assert!(layout.ellipsis_position.is_some());
    }

    #[test]
    fn test_front_back_alternation() {
        let columns = make_columns(&[10; 10]);
        let layout = compute_layout(&columns, 50, 100);

        // Should include first and last columns
        assert!(layout.visible_columns.contains(&0));
        if layout.visible_columns.len() > 1 {
            assert!(layout.visible_columns.contains(&9));
        }
    }

    #[test]
    fn test_empty_columns() {
        let columns: Vec<ColumnInfo> = vec![];
        let layout = compute_layout(&columns, 80, 100);

        assert!(layout.visible_columns.is_empty());
        assert!(layout.ellipsis_position.is_none());
    }

    #[test]
    fn test_single_column() {
        let columns = make_columns(&[10]);
        let layout = compute_layout(&columns, 80, 100);

        assert_eq!(layout.visible_columns, vec![0]);
        assert!(layout.ellipsis_position.is_none());
    }

    #[test]
    fn test_fills_width() {
        // Columns should expand to fill available width
        let columns = make_columns(&[5, 5, 5]);
        let layout = compute_layout(&columns, 80, 100);

        let total = calculate_total_width(&layout.column_widths);
        // Should use close to the available width (expanding columns)
        // With 3 columns and 80 max, should use most of it
        assert!(total >= 20); // At least use natural widths
        assert!(total <= 80); // But not exceed max
    }

    #[test]
    fn test_calculate_total_width() {
        assert_eq!(calculate_total_width(&[]), 4); // Just borders
        assert_eq!(calculate_total_width(&[10]), 14); // 4 + 10
        assert_eq!(calculate_total_width(&[10, 10]), 27); // 4 + 20 + 3
        assert_eq!(calculate_total_width(&[10, 10, 10]), 40); // 4 + 30 + 6
    }
}
