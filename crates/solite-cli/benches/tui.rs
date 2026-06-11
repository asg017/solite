//! Criterion benchmarks for the TUI data and render hot paths.
//!
//! Run with `make bench` (or `cargo bench -p solite-cli`). These are
//! dev-tooling for measuring table-page changes (window loading, row
//! counting, full-frame rendering, copy serialization) — they are
//! intentionally NOT part of CI or `make test`; don't wire them in.
//!
//! Internals are reached through `solite_cli::tui_bench_support`, a hidden
//! re-export that exists only for this file.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ratatui::{backend::TestBackend, Terminal};
use solite_cli::tui_bench_support as tui;
use solite_core::Runtime;
use std::cell::RefCell;
use std::rc::Rc;

/// Rows in the "big" fixture. Large enough that O(n) vs O(n²) counting and
/// shallow vs deep OFFSET loads are clearly distinguishable.
const BIG_ROWS: usize = 100_000;
/// Columns in the "wide" fixture.
const WIDE_COLS: usize = 100;

struct NoopClipboard;
impl tui::Clipboard for NoopClipboard {
    fn set_text(&mut self, _text: String) -> Result<(), String> {
        Ok(())
    }
}

fn noop_clipboard() -> tui::SharedClipboard {
    Rc::new(RefCell::new(NoopClipboard))
}

/// In-memory db: `big` with 100k rows × 10 mixed columns.
fn big_runtime() -> Runtime {
    let runtime = Runtime::new(None).unwrap();
    runtime
        .connection
        .execute_script(&format!(
            "CREATE TABLE big AS WITH RECURSIVE c(n) AS \
             (SELECT 1 UNION ALL SELECT n+1 FROM c LIMIT {BIG_ROWS}) \
             SELECT n AS id, n * 0.5 AS half, 'row number ' || n AS label, \
                    n % 2 AS parity, randomblob(8) AS blob8, n * n AS sq, \
                    'bucket ' || (n % 100) AS bucket, NULL AS empty_col, \
                    1.5 AS f, 'constant' AS k FROM c"
        ))
        .unwrap();
    runtime
}

/// In-memory db: `wide` with 1k rows × 100 integer columns.
fn wide_runtime() -> Runtime {
    let runtime = Runtime::new(None).unwrap();
    let cols: Vec<String> = (0..WIDE_COLS).map(|i| format!("n + {i} AS c{i}")).collect();
    runtime
        .connection
        .execute_script(&format!(
            "CREATE TABLE wide AS WITH RECURSIVE c(n) AS \
             (SELECT 1 UNION ALL SELECT n+1 FROM c LIMIT 1000) \
             SELECT {} FROM c",
            cols.join(", ")
        ))
        .unwrap();
    runtime
}

/// In-memory db: `chunky` with 1k rows carrying ~1KB text values.
fn big_values_runtime() -> Runtime {
    let runtime = Runtime::new(None).unwrap();
    runtime
        .connection
        .execute_script(
            "CREATE TABLE chunky AS WITH RECURSIVE c(n) AS \
             (SELECT 1 UNION ALL SELECT n+1 FROM c LIMIT 1000) \
             SELECT n AS id, hex(randomblob(512)) AS kb_text FROM c",
        )
        .unwrap();
    runtime
}

/// 1. Window load at OFFSET 0 vs a deep OFFSET (quantifies the OFFSET
///    pagination cost ticket 06 targets), plus a window of 1KB values.
fn bench_window_load(c: &mut Criterion) {
    let big = big_runtime();
    let chunky = big_values_runtime();
    let mut group = c.benchmark_group("load_table_data");
    group.sample_size(30);

    group.bench_function("offset 0", |b| {
        b.iter(|| {
            let result = tui::load_table_data(&big, "big", None, 0, tui::WINDOW_SIZE);
            black_box(result.data.rows.len())
        })
    });
    group.bench_function(format!("offset {}", BIG_ROWS - tui::WINDOW_SIZE), |b| {
        b.iter(|| {
            let result = tui::load_table_data(
                &big,
                "big",
                None,
                BIG_ROWS - tui::WINDOW_SIZE,
                tui::WINDOW_SIZE,
            );
            black_box(result.data.rows.len())
        })
    });
    group.bench_function("1KB text values", |b| {
        b.iter(|| {
            let result = tui::load_table_data(&chunky, "chunky", None, 0, tui::WINDOW_SIZE);
            black_box(result.data.rows.len())
        })
    });
    group.finish();
}

/// 2. Full row count: the incremental OFFSET-probe state machine vs a plain
///    `SELECT COUNT(*)` (ticket 06's replacement candidate).
fn bench_row_count(c: &mut Criterion) {
    let big = big_runtime();
    let mut group = c.benchmark_group("row_count");
    group.sample_size(10);

    group.bench_function("count_batch probe to completion", |b| {
        b.iter(|| {
            let mut row_count = tui::RowCount::new(tui::WINDOW_SIZE);
            while row_count.count_batch(&big, "big") {}
            black_box(row_count.is_complete)
        })
    });
    group.bench_function("select count(*)", |b| {
        b.iter(|| {
            let (_, stmt) = big.connection.prepare("SELECT COUNT(*) FROM big").unwrap();
            let mut stmt = stmt.unwrap();
            let row = stmt.next().unwrap().unwrap();
            black_box(row[0].as_int64())
        })
    });
    group.finish();
}

/// 3. Full-frame render of a populated TablePage on a TestBackend, at a
///    small and a large terminal size, plus a 100-column table (catches
///    per-frame allocation regressions).
fn bench_render(c: &mut Criterion) {
    let big = big_runtime();
    let wide = wide_runtime();
    let theme = tui::CTP_MOCHA_THEME.clone();
    let mut group = c.benchmark_group("render_table_page");

    for (width, height) in [(80u16, 24u16), (250u16, 60u16)] {
        let mut page = tui::TablePage::new("big", &big, theme.clone(), noop_clipboard());
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        // Let the incremental row counter finish so frames are steady-state.
        for _ in 0..4 {
            terminal.draw(|f| page.render(f, f.area())).unwrap();
        }
        group.bench_function(format!("big {width}x{height}"), |b| {
            b.iter(|| {
                terminal.draw(|f| page.render(f, f.area())).unwrap();
            })
        });
    }

    let mut page = tui::TablePage::new("wide", &wide, theme.clone(), noop_clipboard());
    let mut terminal = Terminal::new(TestBackend::new(250, 60)).unwrap();
    for _ in 0..4 {
        terminal.draw(|f| page.render(f, f.area())).unwrap();
    }
    group.bench_function("wide 250x60", |b| {
        b.iter(|| {
            terminal.draw(|f| page.render(f, f.area())).unwrap();
        })
    });
    group.finish();
}

/// 4. Copy serialization over one loaded window.
fn bench_copy_serialization(c: &mut Criterion) {
    let big = big_runtime();
    let data = tui::load_table_data(&big, "big", None, 0, tui::WINDOW_SIZE).data;
    let mut group = c.benchmark_group("copy_serialization");

    group.bench_function("table_to_tsv window", |b| {
        b.iter(|| black_box(tui::data_to_tsv(&data)).len())
    });
    group.bench_function("generate_inserts window", |b| {
        b.iter(|| black_box(tui::data_to_inserts("big", &data)).len())
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_window_load,
    bench_row_count,
    bench_render,
    bench_copy_serialization
);
criterion_main!(benches);
