//! Criterion benchmarks for the sqlite.rs binding hot paths.
//!
//! Run with `make bench` (or `cargo bench -p solite-core`). These are
//! dev-tooling for measuring binding-layer changes — they are intentionally
//! NOT part of CI or `make test`; don't wire them in.
//!
//! (`solite bench`, the CLI command, benchmarks user SQL — unrelated.)

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use solite_core::sqlite::{escape_string, Connection, OwnedValue};

const ROWS: i64 = 10_000;

/// In-memory db with a 10k-row table of mixed int/float/text/blob columns.
fn seeded_connection() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_script("CREATE TABLE t(a INTEGER, b REAL, c TEXT, d BLOB)")
        .unwrap();
    conn.execute_script("BEGIN").unwrap();
    let (_, stmt) = conn
        .prepare("INSERT INTO t VALUES (?, ?, ?, ?)")
        .unwrap();
    let stmt = stmt.unwrap();
    for i in 0..ROWS {
        stmt.bind_int64(1, i).unwrap();
        stmt.bind_double(2, i as f64 * 0.5).unwrap();
        stmt.bind_text(3, format!("row number {i}")).unwrap();
        stmt.bind_blob(4, &i.to_le_bytes()).unwrap();
        stmt.execute().unwrap();
        stmt.reset();
    }
    conn.execute_script("COMMIT").unwrap();
    conn
}

/// 1. Statement prepare+finalize cycle.
fn bench_prepare(c: &mut Criterion) {
    let conn = seeded_connection();
    c.bench_function("prepare", |b| {
        b.iter(|| {
            let (_, stmt) = conn
                .prepare(black_box("SELECT a, b, c, d FROM t WHERE a = 1"))
                .unwrap();
            black_box(stmt)
        })
    });
}

/// 2. Full-scan stepping: next() (allocates a Vec per row) vs nextx() (lazy Row).
fn bench_full_scan(c: &mut Criterion) {
    let conn = seeded_connection();
    let mut group = c.benchmark_group("full_scan");
    group.bench_function("next", |b| {
        b.iter(|| {
            let (_, stmt) = conn.prepare("SELECT * FROM t").unwrap();
            let mut stmt = stmt.unwrap();
            let mut n = 0u64;
            while let Some(row) = stmt.next().unwrap() {
                n += row.len() as u64;
            }
            black_box(n)
        })
    });
    group.bench_function("nextx", |b| {
        b.iter(|| {
            let (_, stmt) = conn.prepare("SELECT * FROM t").unwrap();
            let mut stmt = stmt.unwrap();
            let mut n = 0u64;
            while let Some(row) = stmt.nextx().unwrap() {
                n += row.count() as u64;
            }
            black_box(n)
        })
    });
    group.finish();
}

/// 3. Per-row value access through Row::value_at.
fn bench_value_access(c: &mut Criterion) {
    let conn = seeded_connection();
    c.bench_function("value_at", |b| {
        b.iter(|| {
            let (_, stmt) = conn.prepare("SELECT a, c FROM t").unwrap();
            let mut stmt = stmt.unwrap();
            let mut total = 0i64;
            let mut bytes = 0usize;
            while let Some(row) = stmt.nextx().unwrap() {
                total += row.value_at(0).as_int64();
                bytes += row.value_at(1).as_str().len();
            }
            black_box((total, bytes))
        })
    });
}

/// 4. Materializing a full row into OwnedValues.
fn bench_owned_value(c: &mut Criterion) {
    let conn = seeded_connection();
    c.bench_function("owned_value_from_value_ref", |b| {
        b.iter(|| {
            let (_, stmt) = conn.prepare("SELECT * FROM t").unwrap();
            let mut stmt = stmt.unwrap();
            let mut n = 0usize;
            while let Some(row) = stmt.next().unwrap() {
                let owned: Vec<OwnedValue> =
                    row.iter().map(OwnedValue::from_value_ref).collect();
                n += owned.len();
            }
            black_box(n)
        })
    });
}

/// 5. escape_string on short, long, and quote-heavy inputs.
fn bench_escape_string(c: &mut Criterion) {
    let short = "hello";
    let long = "lorem ipsum dolor sit amet ".repeat(64);
    let quotes = "it's a 'quote'-heavy 'string', isn't it? ".repeat(16);
    let mut group = c.benchmark_group("escape_string");
    group.bench_function("short", |b| b.iter(|| escape_string(black_box(short))));
    group.bench_function("long", |b| b.iter(|| escape_string(black_box(&long))));
    group.bench_function("quote_heavy", |b| b.iter(|| escape_string(black_box(&quotes))));
    group.finish();
}

/// 6. INSERT loop inside a transaction: bind + execute + reset per row.
fn bench_insert_loop(c: &mut Criterion) {
    c.bench_function("insert_loop_1k", |b| {
        b.iter(|| {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_script("CREATE TABLE t(a INTEGER, c TEXT)")
                .unwrap();
            conn.execute_script("BEGIN").unwrap();
            let (_, stmt) = conn.prepare("INSERT INTO t VALUES (?, ?)").unwrap();
            let stmt = stmt.unwrap();
            for i in 0..1_000i64 {
                stmt.bind_int64(1, black_box(i)).unwrap();
                stmt.bind_text(2, "some text value").unwrap();
                stmt.execute().unwrap();
                stmt.reset();
            }
            conn.execute_script("COMMIT").unwrap();
        })
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(50);
    targets = bench_prepare,
        bench_full_scan,
        bench_value_access,
        bench_owned_value,
        bench_escape_string,
        bench_insert_loop
);
criterion_main!(benches);
