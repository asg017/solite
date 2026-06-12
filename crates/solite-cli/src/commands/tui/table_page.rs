use std::fmt::Write;

use crate::commands::tui::copy_popup::{CopyOption, CopyPopup};
use crate::commands::tui::help_popup::{help_bar_from, HelpPopup, TABLE_KEYS};
use crate::commands::tui::row_page::{get_primary_keys, PrimaryKeyInfo};
use crate::commands::tui::utils::render_value_for_display_capped;
use crate::commands::tui::tui_theme::TuiTheme;
use crate::commands::tui::{
    value_to_string, Frame, HandleKeyResult, NavigateToPage, RowPageData, SharedClipboard,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, HorizontalAlignment, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Text;
use ratatui::widgets::{Cell, Row, Table, TableState};
use solite_core::sqlite::{escape_string, quote_identifier, OwnedValue};
use solite_core::Runtime;

#[derive(Debug)]
pub struct Data {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<OwnedValue>>,
}

impl Data {
    fn empty() -> Self {
        Self {
            columns: vec![],
            rows: vec![],
        }
    }
}

#[derive(Clone)]
pub struct Order {
    column_idx: usize,
    direction: SortDirection,
}

/// Result of loading table data
pub struct LoadResult {
    pub data: Data,
    pub error: Option<String>,
}

/// Configuration for windowed data loading
pub const WINDOW_SIZE: usize = 200;
const PREFETCH_THRESHOLD: usize = 50;

/// Maximum number of rows a full-table copy will put on the clipboard.
/// Larger tables are truncated (with an honest footer message) — the
/// clipboard is the wrong channel for huge tables; `.export` exists for that.
const COPY_ROW_LIMIT: usize = 100_000;

/// Maximum size of a single cell fetched into the window: characters for
/// text, bytes for blobs (SQL `substr`/`length` semantics). Larger values
/// are truncated at the SQL layer so a window crossing huge cells never
/// materializes them; cell-copy and the row page fetch full values on
/// demand. Comfortably larger than MAX_CELL_DISPLAY_LEN.
const MAX_CELL_FETCH_LEN: usize = 1024;

/// Rows to count per incremental batch
const COUNT_BATCH_SIZE: usize = 60493;

/// Spinner characters for counting animation
const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Where an in-progress count is coming from.
enum CountSource {
    /// `SELECT COUNT(*)` running on its own read-only connection in a
    /// background thread; the result arrives on this channel.
    Background(std::sync::mpsc::Receiver<Result<usize, String>>),
    /// Incremental OFFSET probing on the UI connection. Fallback for
    /// databases a second connection can't reach (in-memory, remote).
    Probe,
}

/// Tracks row count with incremental discovery
pub struct RowCount {
    /// Minimum known row count (from loaded data)
    known: usize,
    /// Whether we've found the actual end
    pub is_complete: bool,
    /// Next offset to probe when counting
    probe_offset: usize,
    /// Spinner frame for animation
    spinner_frame: usize,
    /// Active counting strategy
    source: CountSource,
}

impl RowCount {
    /// Probe-based counter (OFFSET batches on the UI connection).
    pub fn new(initial_known: usize) -> Self {
        Self {
            known: initial_known,
            is_complete: initial_known == 0, // Empty table is complete
            probe_offset: initial_known,
            spinner_frame: 0,
            source: CountSource::Probe,
        }
    }

    /// Counter for `table` in the database at `runtime`'s connection.
    ///
    /// When the database is a local file, spawns a background thread that
    /// runs `SELECT COUNT(*)` on its own read-only connection — a single
    /// optimized scan, off the UI thread. In-memory and remote databases
    /// can't be reopened by a second connection, so those keep the
    /// incremental OFFSET probe.
    fn start(initial_known: usize, runtime: &Runtime, table: &str) -> Self {
        let mut row_count = Self::new(initial_known);
        if row_count.is_complete {
            return row_count;
        }
        let Some(path) = background_countable_path(runtime) else {
            return row_count;
        };

        // The thread is detached and never cancelled: if the page closes
        // mid-count the send simply fails and the thread exits. On a huge
        // rollback-journal database the read-only scan's SHARED lock could
        // briefly make concurrent writes return SQLITE_BUSY; add a
        // cancellation flag here if that ever bites.
        let (tx, rx) = std::sync::mpsc::channel();
        let table = table.to_owned();
        std::thread::spawn(move || {
            let result = count_rows(&path, &table);
            // Receiver may be gone (page closed); nothing to do then.
            let _ = tx.send(result);
        });
        row_count.source = CountSource::Background(rx);
        row_count
    }

    /// Drive counting forward from the render loop. Non-blocking for the
    /// background source; one OFFSET batch for the probe source.
    fn tick(&mut self, runtime: &Runtime, table: &str) {
        if self.is_complete {
            return;
        }
        match &self.source {
            CountSource::Background(rx) => {
                use std::sync::mpsc::TryRecvError;
                self.spinner_frame = (self.spinner_frame + 1) % SPINNER_CHARS.len();
                match rx.try_recv() {
                    Ok(Ok(total)) => {
                        // Loads may have seen more rows than the count if the
                        // table grew meanwhile; keep the larger value.
                        self.known = self.known.max(total);
                        self.is_complete = true;
                    }
                    // Count failed or the thread died: fall back to probing.
                    Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                        self.source = CountSource::Probe;
                        self.probe_offset = self.known;
                    }
                    Err(TryRecvError::Empty) => {}
                }
            }
            CountSource::Probe => {
                self.count_batch(runtime, table);
            }
        }
    }

    /// Update known count from loaded data
    fn update_from_load(&mut self, window_start: usize, loaded_count: usize) {
        let new_known = window_start + loaded_count;
        if new_known > self.known {
            self.known = new_known;
            // If we loaded less than a full window, we've found the end
            if loaded_count < WINDOW_SIZE {
                self.is_complete = true;
            }
        }
    }

    /// Count a batch of rows to discover more. Returns true if still counting.
    pub fn count_batch(&mut self, runtime: &Runtime, table: &str) -> bool {
        if self.is_complete {
            return false;
        }

        let sql = format!(
            "SELECT 1 FROM {} LIMIT {} OFFSET {}",
            quote_identifier(table),
            COUNT_BATCH_SIZE,
            self.probe_offset
        );

        let mut stmt = match runtime.connection.prepare(&sql) {
            Ok((_, Some(stmt))) => stmt,
            _ => {
                self.is_complete = true;
                return false;
            }
        };

        let mut batch_count = 0;
        while let Ok(Some(_)) = stmt.next() {
            batch_count += 1;
        }

        self.probe_offset += batch_count;
        if self.probe_offset > self.known {
            self.known = self.probe_offset;
        }
        // Advance spinner
        self.spinner_frame = (self.spinner_frame + 1) % SPINNER_CHARS.len();

        if batch_count < COUNT_BATCH_SIZE {
            self.is_complete = true;
            false
        } else {
            true // More to count
        }
    }

    /// Get display string for row count with formatting
    fn display(&self) -> String {
        use super::format_number;
        let formatted = format_number(self.known);
        if self.is_complete {
            formatted
        } else {
            let spinner = SPINNER_CHARS[self.spinner_frame];
            format!("{}+ {}", formatted, spinner)
        }
    }
}

/// The database file path, if a background thread could open a second
/// read-only connection to it. None for in-memory and remote databases.
fn background_countable_path(runtime: &Runtime) -> Option<String> {
    if runtime.connection.is_remote() {
        return None;
    }
    let path = runtime.connection.db_name()?;
    if path.is_empty() || !std::path::Path::new(&path).exists() {
        return None;
    }
    Some(path)
}

/// Open `path` read-only and `SELECT COUNT(*)` from `table`.
/// Runs on the background counting thread.
fn count_rows(path: &str, table: &str) -> Result<usize, String> {
    let connection = solite_core::sqlite::Connection::open_readonly(path)
        .map_err(|e| format!("Failed to open database for counting: {}", e))?;
    let sql = format!("SELECT COUNT(*) FROM {}", quote_identifier(table));
    let (_, stmt) = connection
        .prepare(&sql)
        .map_err(|e| format!("Count query error: {}", e))?;
    let mut stmt = stmt.ok_or("Failed to prepare count query")?;
    let row = stmt
        .next()
        .map_err(|e| format!("Count error: {}", e))?
        .ok_or("Count query returned no rows")?;
    Ok(row[0].as_int64().max(0) as usize)
}

/// Load rows with full, untruncated values (`SELECT *`).
pub fn load_table_data(
    runtime: &Runtime,
    table: &str,
    order: Option<Order>,
    offset: usize,
    limit: usize,
) -> LoadResult {
    load_table_data_with_select(runtime, table, "*", order, offset, limit)
}

/// Load rows with an explicit SELECT list (used by window loads to truncate
/// oversized values at the SQL layer; see [`window_select_list`]).
fn load_table_data_with_select(
    runtime: &Runtime,
    table: &str,
    select_list: &str,
    order: Option<Order>,
    offset: usize,
    limit: usize,
) -> LoadResult {
    let mut sql: String = String::new();
    // Use quoted identifier to handle special table names
    let _ = writeln!(
        &mut sql,
        "SELECT {} FROM {}",
        select_list,
        quote_identifier(table)
    );
    if let Some(order) = order {
        let _ = writeln!(
            &mut sql,
            "ORDER BY {} {}",
            order.column_idx + 1,
            match order.direction {
                SortDirection::Ascending => "ASC",
                SortDirection::Descending => "DESC",
            }
        );
    }
    let _ = writeln!(&mut sql, "LIMIT {} OFFSET {}", limit, offset);

    let mut stmt = match runtime.connection.prepare(&sql) {
        Ok((_, Some(stmt))) => stmt,
        Ok((_, None)) => {
            return LoadResult {
                data: Data::empty(),
                error: Some("Failed to prepare query".to_owned()),
            }
        }
        Err(e) => {
            return LoadResult {
                data: Data::empty(),
                error: Some(format!("Query error: {}", e)),
            }
        }
    };

    let columns = stmt.column_names().unwrap_or_default();
    let mut rows = vec![];
    let mut error = None;

    loop {
        match stmt.next() {
            Ok(None) => break,
            Ok(Some(row)) => {
                let row_values: Vec<OwnedValue> = row
                    .iter()
                    .map(|v| OwnedValue::from_value_ref(v))
                    .collect();
                rows.push(row_values);
            }
            Err(e) => {
                error = Some(format!("Error reading row: {}", e));
                break;
            }
        }
    }

    LoadResult {
        data: Data { columns, rows },
        error,
    }
}

/// The column names exactly as `SELECT *` produces them, via a prepared
/// `LIMIT 0` probe. Crucially this includes generated columns, which
/// `pragma_table_info` omits — the truncating window SELECT list, the
/// displayed columns, and the on-demand `SELECT *` full-row fetch must all
/// agree on the same column set. Empty on failure.
fn table_column_names(runtime: &Runtime, table: &str) -> Vec<String> {
    let sql = format!("SELECT * FROM {} LIMIT 0", quote_identifier(table));
    match runtime.connection.prepare(&sql) {
        Ok((_, Some(stmt))) => stmt.column_names().unwrap_or_default(),
        _ => vec![],
    }
}

/// Per-column SELECT expression that truncates oversized text/blob values
/// at the SQL layer, so the window never materializes a huge cell.
fn truncating_select_expr(column: &str) -> String {
    let quoted = quote_identifier(column);
    format!(
        "CASE WHEN typeof({q}) IN ('text','blob') AND length({q}) > {n} \
         THEN substr({q}, 1, {n}) ELSE {q} END AS {q}",
        q = quoted,
        n = MAX_CELL_FETCH_LEN
    )
}

/// How many columns (with 1-cell spacing) fit in `available` width, walking
/// `widths` in order. At least 1 when `widths` is non-empty.
fn fit_column_count<'w>(widths: impl Iterator<Item = &'w u16>, available: u16) -> usize {
    let mut used: u16 = 0;
    let mut count = 0usize;
    for width in widths {
        let needed = *width + if count == 0 { 0 } else { 1 };
        if used.saturating_add(needed) > available && count > 0 {
            break;
        }
        used = used.saturating_add(needed);
        count += 1;
    }
    count
}

/// SELECT list for window loads: truncating expressions when the column
/// names are known, `*` otherwise (e.g. the column probe failed).
fn window_select_list(columns: &[String]) -> String {
    if columns.is_empty() {
        "*".to_owned()
    } else {
        columns
            .iter()
            .map(|c| truncating_select_expr(c))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Whether keyset pagination on `rowid` is usable for this table: a real
/// rowid table (not WITHOUT ROWID, not a view) with no user-defined column
/// shadowing `rowid`.
fn rowid_keyset_usable(runtime: &Runtime, table: &str) -> bool {
    let escaped = table.replace('\'', "''");

    let sql = format!("SELECT wr, type FROM pragma_table_list('{}')", escaped);
    let Ok((_, Some(mut stmt))) = runtime.connection.prepare(&sql) else {
        return false;
    };
    match stmt.next() {
        Ok(Some(row)) => {
            // wr = 1 means WITHOUT ROWID
            if row[0].as_int64() != 0 || row[1].as_str() != "table" {
                return false;
            }
        }
        _ => return false,
    }

    // A column literally named "rowid" shadows the real rowid; keyset
    // anchors on it would be wrong (it may not be unique or ordered).
    let sql = format!(
        "SELECT count(*) FROM pragma_table_info('{}') WHERE lower(name) = 'rowid'",
        escaped
    );
    let Ok((_, Some(mut stmt))) = runtime.connection.prepare(&sql) else {
        return false;
    };
    matches!(stmt.next(), Ok(Some(row)) if row[0].as_int64() == 0)
}

/// Load a window of `SELECT rowid, {select_list} FROM "table" {suffix}`,
/// returning the rowids alongside the rows (rowid stripped from
/// columns/rows). `reverse` flips the fetched order, for
/// `ORDER BY rowid DESC` reads.
#[allow(clippy::type_complexity)]
fn load_rowid_window(
    runtime: &Runtime,
    table: &str,
    select_list: &str,
    suffix: &str,
    reverse: bool,
) -> Result<(Vec<String>, Vec<Vec<OwnedValue>>, Vec<i64>), String> {
    let sql = format!(
        "SELECT rowid, {} FROM {} {}",
        select_list,
        quote_identifier(table),
        suffix
    );
    let mut stmt = match runtime.connection.prepare(&sql) {
        Ok((_, Some(stmt))) => stmt,
        Ok((_, None)) => return Err("Failed to prepare query".to_owned()),
        Err(e) => return Err(format!("Query error: {}", e)),
    };

    let mut columns = stmt.column_names().unwrap_or_default();
    if !columns.is_empty() {
        columns.remove(0); // the rowid column
    }
    let mut rows = vec![];
    let mut rowids = vec![];
    loop {
        match stmt.next() {
            Ok(None) => break,
            Ok(Some(row)) => {
                let mut values = row.iter();
                let rowid = values.next().map(|v| v.as_int64()).unwrap_or(0);
                rowids.push(rowid);
                rows.push(values.map(OwnedValue::from_value_ref).collect());
            }
            Err(e) => return Err(format!("Error reading row: {}", e)),
        }
    }
    if reverse {
        rows.reverse();
        rowids.reverse();
    }
    Ok((columns, rows, rowids))
}

/// Load up to `cap` rows of the whole table (respecting the active sort
/// order) for a full-table copy. Returns the data plus whether the table was
/// truncated at `cap`.
fn load_table_for_copy(
    runtime: &Runtime,
    table: &str,
    order: Option<Order>,
    cap: usize,
) -> Result<(Data, bool), String> {
    // Fetch one extra row so truncation can be detected without a count.
    let result = load_table_data(runtime, table, order, 0, cap + 1);
    if let Some(err) = result.error {
        return Err(err);
    }
    let mut data = result.data;
    let truncated = data.rows.len() > cap;
    if truncated {
        data.rows.truncate(cap);
    }
    Ok((data, truncated))
}

/// Escape a string for use as a TSV field: embedded tabs and newlines would
/// silently shift columns/rows in the pasted output, so render them as
/// visible `\t`/`\n`/`\r` escapes instead.
fn tsv_escape(s: String) -> String {
    if s.contains(['\t', '\n', '\r']) {
        s.replace('\t', "\\t").replace('\n', "\\n").replace('\r', "\\r")
    } else {
        s
    }
}

/// Generate TSV (header + rows) for the given data.
pub fn data_to_tsv(data: &Data) -> String {
    let header = data
        .columns
        .iter()
        .map(|c| tsv_escape(c.clone()))
        .collect::<Vec<_>>()
        .join("\t");
    let rows: Vec<String> = data
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|v| tsv_escape(value_to_string(v)))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect();
    format!("{}\n{}", header, rows.join("\n"))
}

/// Generate INSERT statements for the given data.
pub fn data_to_inserts(table_name: &str, data: &Data) -> String {
    if data.rows.is_empty() {
        return format!("-- No data in table \"{}\"", table_name);
    }

    let cols = data
        .columns
        .iter()
        .map(|c| quote_identifier(c))
        .collect::<Vec<_>>()
        .join(", ");
    data.rows
        .iter()
        .map(|row| {
            let values: Vec<String> = row
                .iter()
                .map(|v| match v {
                    OwnedValue::Null => "NULL".to_owned(),
                    OwnedValue::Integer(i) => i.to_string(),
                    OwnedValue::Double(f) => f.to_string(),
                    OwnedValue::Text(s) => {
                        // Invalid UTF-8 in TEXT values is silently replaced by
                        // from_utf8_lossy; BLOBs round-trip exactly via hex.
                        // escape_string (%Q) does the single-quote escaping.
                        escape_string(&String::from_utf8_lossy(s))
                    }
                    OwnedValue::Blob(b) => format!("X'{}'", hex::encode(b)),
                })
                .collect();
            format!(
                "INSERT INTO {} ({}) VALUES ({});",
                quote_identifier(table_name),
                cols,
                values.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub struct TablePage<'a> {
    runtime: &'a Runtime,
    pub(crate) theme: TuiTheme,
    pub(crate) state: TableState,
    pub(crate) table_name: String,
    pub(crate) data: Data,
    pub(crate) column_idx_offset: usize,
    footer_message: Option<String>,
    n_columns_show: usize,
    error: Option<String>,
    copy_popup: CopyPopup,
    help_popup: HelpPopup,
    primary_keys: Vec<PrimaryKeyInfo>,
    /// Row count tracker (streams count incrementally)
    pub(crate) row_count: RowCount,
    /// Starting row index of the current window
    window_start: usize,
    /// Current sort order (if any)
    current_order: Option<Order>,
    /// Sort requested but not yet executed (a "Sorting…" frame is shown
    /// before the blocking ORDER BY query runs)
    pending_sort: Option<PendingSort>,
    /// Whether keyset pagination on rowid is usable for this table
    use_rowid: bool,
    /// rowids for the loaded window (when loaded via the rowid path),
    /// used as keyset anchors
    rowids: Option<Vec<i64>>,
    /// SELECT list for window loads (truncates oversized values)
    select_list: String,
    /// Measured display width per column for the loaded window
    col_widths: Vec<u16>,
    /// Width of the table area at the last render (used by `L` to compute
    /// how many tail columns fit)
    last_table_width: u16,
    /// Destination for copy operations
    clipboard: SharedClipboard,
}

/// A sort waiting for its feedback frame before the blocking query runs.
struct PendingSort {
    order: Order,
    /// The "Sorting…" footer has been drawn; the query may run now.
    message_rendered: bool,
}

impl<'a> TablePage<'a> {
    pub fn new(
        table_name: &str,
        runtime: &'a Runtime,
        theme: TuiTheme,
        clipboard: SharedClipboard,
    ) -> Self {
        let use_rowid = rowid_keyset_usable(runtime, table_name);
        let select_list = window_select_list(&table_column_names(runtime, table_name));
        let (data, rowids, error) = if use_rowid {
            match load_rowid_window(
                runtime,
                table_name,
                &select_list,
                &format!("ORDER BY rowid LIMIT {}", WINDOW_SIZE),
                false,
            ) {
                Ok((columns, rows, rowids)) => (Data { columns, rows }, Some(rowids), None),
                Err(e) => (Data::empty(), None, Some(e)),
            }
        } else {
            let result =
                load_table_data_with_select(runtime, table_name, &select_list, None, 0, WINDOW_SIZE);
            (result.data, None, result.error)
        };
        let primary_keys = get_primary_keys(runtime, table_name);
        let mut state = TableState::default();
        if !data.rows.is_empty() {
            state.select_first();
            state.select_first_column();
        }
        let row_count = RowCount::start(data.rows.len(), runtime, table_name);
        let mut page = Self {
            runtime,
            theme,
            state,
            table_name: table_name.to_owned(),
            data,
            n_columns_show: 5,
            column_idx_offset: 0,
            footer_message: None,
            error,
            copy_popup: CopyPopup::new(),
            help_popup: HelpPopup::new(" Help — Table ", TABLE_KEYS),
            primary_keys,
            row_count,
            window_start: 0,
            current_order: None,
            pending_sort: None,
            use_rowid,
            rowids,
            select_list,
            col_widths: vec![],
            last_table_width: 80,
            clipboard,
        };
        page.recompute_col_widths();
        page
    }

    /// Measure per-column display widths for the loaded window: header width
    /// vs the widest sampled cell, clamped to a sane range.
    fn recompute_col_widths(&mut self) {
        const MIN_COL_WIDTH: usize = 3;
        const MAX_COL_WIDTH: usize = 40;
        const SAMPLE_ROWS: usize = 50;
        self.col_widths = self
            .data
            .columns
            .iter()
            .enumerate()
            .map(|(col_idx, name)| {
                let mut width = name.chars().count();
                for row in self.data.rows.iter().take(SAMPLE_ROWS) {
                    if let Some(value) = row.get(col_idx) {
                        let display =
                            render_value_for_display_capped(value, Some(MAX_CELL_FETCH_LEN));
                        width = width.max(display.chars().count());
                    }
                }
                width.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH) as u16
            })
            .collect();
    }

    /// Get the known row count (may be incomplete)
    pub(crate) fn total_rows(&self) -> usize {
        self.row_count.known
    }

    /// Ensure the given absolute row index is loaded in the current window.
    /// If not, reload a window centered around that row.
    fn ensure_row_loaded(&mut self, absolute_row: usize) {
        let window_end = self.window_start + self.data.rows.len();

        // Check if row is already in window with enough buffer
        let near_start = absolute_row < self.window_start + PREFETCH_THRESHOLD;
        let near_end = absolute_row + PREFETCH_THRESHOLD >= window_end;

        // Use row_count.known as estimate, but may load beyond if count is incomplete
        let should_reload = absolute_row < self.window_start
            || absolute_row >= window_end
            || (near_start && self.window_start > 0)
            || (near_end && !self.row_count.is_complete);

        if should_reload {
            // Center the window around the target row
            let new_start = absolute_row.saturating_sub(WINDOW_SIZE / 2);
            self.load_window(new_start);
        }
    }

    /// Load the window starting at `new_start`, preferring keyset (rowid
    /// anchored, O(window)) reads over OFFSET (O(offset)) ones.
    fn load_window(&mut self, new_start: usize) {
        let keyset_eligible = self.use_rowid && self.current_order.is_none();
        if keyset_eligible && self.load_window_keyset(new_start) {
            return;
        }

        if keyset_eligible {
            // OFFSET fallback that still fetches rowids (with an explicit
            // ORDER BY rowid, matching the keyset reads) so later keyset
            // hops have anchors again.
            match load_rowid_window(
                self.runtime,
                &self.table_name,
                &self.select_list,
                &format!("ORDER BY rowid LIMIT {} OFFSET {}", WINDOW_SIZE, new_start),
                false,
            ) {
                Ok((columns, rows, rowids)) => {
                    self.apply_window(new_start, Data { columns, rows }, Some(rowids));
                }
                Err(e) => self.error = Some(e),
            }
            return;
        }

        let result = load_table_data_with_select(
            self.runtime,
            &self.table_name,
            &self.select_list,
            self.current_order.clone(),
            new_start,
            WINDOW_SIZE,
        );
        if result.error.is_none() {
            self.apply_window(new_start, result.data, None);
        } else {
            self.error = result.error;
        }
    }

    /// Install a freshly loaded window and update the row count from it.
    fn apply_window(&mut self, new_start: usize, data: Data, rowids: Option<Vec<i64>>) {
        self.window_start = new_start;
        self.row_count.update_from_load(new_start, data.rows.len());
        self.data = data;
        self.rowids = rowids;
        self.recompute_col_widths();
    }

    /// Try a keyset (rowid-anchored) load of the window at `new_start`.
    /// Returns false when no anchor applies — the caller falls back to
    /// OFFSET. On query errors, sets `self.error` and reports handled.
    fn load_window_keyset(&mut self, new_start: usize) -> bool {
        let Some(rowids) = self.rowids.clone() else {
            return false;
        };
        if rowids.is_empty() {
            return false;
        }
        let window_end = self.window_start + rowids.len();

        if new_start >= self.window_start && new_start < window_end {
            // Forward: anchor on a row inside the current window.
            let anchor = rowids[new_start - self.window_start];
            match load_rowid_window(
                self.runtime,
                &self.table_name,
                &self.select_list,
                &format!("WHERE rowid >= {} ORDER BY rowid LIMIT {}", anchor, WINDOW_SIZE),
                false,
            ) {
                Ok((columns, rows, new_rowids)) => {
                    self.apply_window(new_start, Data { columns, rows }, Some(new_rowids));
                }
                Err(e) => self.error = Some(e),
            }
            true
        } else if new_start < self.window_start && new_start + WINDOW_SIZE > self.window_start {
            // Backward with overlap: fetch only the gap before the current
            // window (reading backwards from its first rowid) and splice it
            // with the front of the rows we already have.
            let gap = self.window_start - new_start;
            let anchor = rowids[0];
            match load_rowid_window(
                self.runtime,
                &self.table_name,
                &self.select_list,
                &format!("WHERE rowid < {} ORDER BY rowid DESC LIMIT {}", anchor, gap),
                true,
            ) {
                Ok((columns, mut rows, mut new_rowids)) => {
                    if rows.len() != gap {
                        // The table changed underneath us; let OFFSET resolve.
                        return false;
                    }
                    let keep = WINDOW_SIZE - gap;
                    rows.extend(self.data.rows.iter().take(keep).cloned());
                    new_rowids.extend(rowids.iter().take(keep).copied());
                    self.apply_window(new_start, Data { columns, rows }, Some(new_rowids));
                    true
                }
                Err(e) => {
                    self.error = Some(e);
                    true
                }
            }
        } else if self.row_count.is_complete && new_start + WINDOW_SIZE >= self.row_count.known {
            // Tail jump (e.g. `G` deep into the table): read the last rows
            // in reverse — O(window) instead of a near-full OFFSET scan.
            let n = self.row_count.known.saturating_sub(new_start);
            if n == 0 {
                return false;
            }
            match load_rowid_window(
                self.runtime,
                &self.table_name,
                &self.select_list,
                &format!("ORDER BY rowid DESC LIMIT {}", n),
                true,
            ) {
                Ok((columns, rows, new_rowids)) => {
                    let start = self.row_count.known.saturating_sub(rows.len());
                    self.apply_window(start, Data { columns, rows }, Some(new_rowids));
                }
                Err(e) => self.error = Some(e),
            }
            true
        } else {
            false
        }
    }

    /// Convert absolute row index to window-relative index
    fn absolute_to_window(&self, absolute: usize) -> Option<usize> {
        if absolute >= self.window_start && absolute < self.window_start + self.data.rows.len() {
            Some(absolute - self.window_start)
        } else {
            None
        }
    }

    /// Convert window-relative index to absolute row index
    fn window_to_absolute(&self, window_idx: usize) -> usize {
        self.window_start + window_idx
    }

    /// Get the currently selected absolute row index
    fn selected_absolute_row(&self) -> Option<usize> {
        self.state.selected().map(|window_idx| self.window_to_absolute(window_idx))
    }

    /// Request a sort. The blocking ORDER BY query is deferred until after
    /// the next frame so a "Sorting…" message is on screen while it runs
    /// (see the end of `render`).
    fn sort(&mut self, direction: SortDirection) {
        let col_idx = self
            .state
            .selected_column()
            .unwrap_or(0)
            .saturating_add(self.column_idx_offset);
        self.pending_sort = Some(PendingSort {
            order: Order {
                column_idx: col_idx,
                direction,
            },
            message_rendered: false,
        });
    }

    /// Run the (blocking) sorted reload for `order`.
    fn apply_sort(&mut self, order: Order) {
        let result = load_table_data_with_select(
            self.runtime,
            &self.table_name,
            &self.select_list,
            Some(order.clone()),
            0,
            WINDOW_SIZE,
        );
        if let Some(err) = result.error {
            // Keep the previous view; just report the failure.
            self.footer_message = Some(format!("Sort error: {}", err));
            return;
        }
        self.window_start = 0;
        self.current_order = Some(order);
        // A sort doesn't change cardinality: keep the row count, only
        // extending it if this load saw more rows.
        self.row_count.update_from_load(0, result.data.rows.len());
        self.data = result.data;
        // Sorted windows are loaded by OFFSET; rowid anchors no longer apply.
        self.rowids = None;
        self.recompute_col_widths();
        // Reset selection to first row after sort
        self.state.select_first();
    }

    /// Fetch the full (untruncated) values of one row. Window values are
    /// truncated at MAX_CELL_FETCH_LEN; copies and the row page need the
    /// real thing.
    fn fetch_full_row(&self, absolute_row: usize) -> Result<Vec<OwnedValue>, String> {
        // Prefer the rowid anchor: an O(1) lookup.
        if self.current_order.is_none() {
            if let (Some(rowids), Some(window_idx)) =
                (&self.rowids, self.absolute_to_window(absolute_row))
            {
                if let Some(rowid) = rowids.get(window_idx) {
                    let sql = format!(
                        "SELECT * FROM {} WHERE rowid = {}",
                        quote_identifier(&self.table_name),
                        rowid
                    );
                    let mut stmt = match self.runtime.connection.prepare(&sql) {
                        Ok((_, Some(stmt))) => stmt,
                        Ok((_, None)) => return Err("Failed to prepare query".to_owned()),
                        Err(e) => return Err(format!("Query error: {}", e)),
                    };
                    return match stmt.next() {
                        Ok(Some(row)) => {
                            Ok(row.iter().map(OwnedValue::from_value_ref).collect())
                        }
                        Ok(None) => Err("Row not found".to_owned()),
                        Err(e) => Err(format!("Error reading row: {}", e)),
                    };
                }
            }
        }

        // OFFSET fallback, respecting the active sort. Caveat: SQLite gives
        // no cross-query ordering guarantee under sort ties (or for
        // unordered scans of WITHOUT ROWID tables), so under a non-unique
        // sort this can in principle land on a different row than the one
        // displayed. In practice the same plan re-runs; acceptable for the
        // copy fallback.
        let result = load_table_data(
            self.runtime,
            &self.table_name,
            self.current_order.clone(),
            absolute_row,
            1,
        );
        if let Some(err) = result.error {
            return Err(err);
        }
        result
            .data
            .rows
            .into_iter()
            .next()
            .ok_or_else(|| "Row not found".to_owned())
    }

    /// Full values for the window row at `window_idx`, for the row detail
    /// page: falls back to the (possibly truncated) window values on error —
    /// a degraded view beats failing to open the page.
    fn full_row_or_window(&self, window_idx: usize) -> Vec<OwnedValue> {
        self.fetch_full_row(self.window_to_absolute(window_idx))
            .unwrap_or_else(|_| self.data.rows[window_idx].clone())
    }

    /// Generate TSV for one row's values
    fn values_to_tsv(values: &[OwnedValue]) -> String {
        values
            .iter()
            .map(|v| tsv_escape(value_to_string(v)))
            .collect::<Vec<_>>()
            .join("\t")
    }

    /// Generate a SELECT statement for this table
    fn generate_select(&self) -> String {
        format!("SELECT * FROM {};", quote_identifier(&self.table_name))
    }

    /// Load the full table (up to COPY_ROW_LIMIT rows, respecting the active
    /// sort) for a whole-table copy. Returns the data plus the success
    /// message describing what was copied.
    fn load_for_full_copy(&self, what: &str) -> Result<(Data, String), String> {
        let (data, truncated) = load_table_for_copy(
            self.runtime,
            &self.table_name,
            self.current_order.clone(),
            COPY_ROW_LIMIT,
        )?;
        let message = if truncated {
            format!(
                "Copied first {} rows as {} (table larger than copy limit)",
                super::format_number(COPY_ROW_LIMIT),
                what
            )
        } else {
            format!("Copied table as {} to clipboard", what)
        };
        Ok((data, message))
    }

    /// Execute a copy operation based on the selected option
    fn execute_copy(&mut self, option: CopyOption) {
        let (content, message) = match option {
            CopyOption::Cell => {
                if let Some((row, col)) = self.state.selected_cell() {
                    let actual_col = col.saturating_add(self.column_idx_offset);
                    if row < self.data.rows.len() && actual_col < self.data.rows[row].len() {
                        // Full value, fetched on demand: the window copy may
                        // be truncated at MAX_CELL_FETCH_LEN. Fail honestly
                        // rather than silently copy a truncated value.
                        match self.fetch_full_row(self.window_to_absolute(row)) {
                            Ok(values) => match values.get(actual_col) {
                                Some(value) => (
                                    value_to_string(value),
                                    "Copied cell to clipboard".to_owned(),
                                ),
                                None => return,
                            },
                            Err(e) => {
                                self.footer_message = Some(format!("Copy failed: {}", e));
                                return;
                            }
                        }
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
            CopyOption::Row => {
                if let Some((row, _)) = self.state.selected_cell() {
                    if row < self.data.rows.len() {
                        match self.fetch_full_row(self.window_to_absolute(row)) {
                            Ok(values) => (
                                Self::values_to_tsv(&values),
                                "Copied row to clipboard".to_owned(),
                            ),
                            Err(e) => {
                                self.footer_message = Some(format!("Copy failed: {}", e));
                                return;
                            }
                        }
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
            CopyOption::Table => match self.load_for_full_copy("TSV") {
                Ok((data, message)) => (data_to_tsv(&data), message),
                Err(e) => {
                    self.footer_message = Some(format!("Copy failed: {}", e));
                    return;
                }
            },
            CopyOption::SqlSelect => (
                self.generate_select(),
                "Copied SELECT to clipboard".to_owned(),
            ),
            CopyOption::SqlInsert => match self.load_for_full_copy("INSERT statements") {
                Ok((data, message)) => (data_to_inserts(&self.table_name, &data), message),
                Err(e) => {
                    self.footer_message = Some(format!("Copy failed: {}", e));
                    return;
                }
            },
        };

        match self.clipboard.borrow_mut().set_text(content) {
            Ok(()) => {
                self.footer_message = Some(message);
            }
            Err(e) => {
                self.footer_message = Some(e);
            }
        }
    }
}

#[derive(Clone, Copy)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl TablePage<'_> {
    /// Move the selection down one page (PageDown / Ctrl+d).
    fn page_down(&mut self) {
        let page_size = 20; // Approximate visible rows
        if let Some(current) = self.state.selected() {
            let absolute = self.window_to_absolute(current);
            let target = absolute
                .saturating_add(page_size)
                .min(self.row_count.known.saturating_sub(1));
            self.ensure_row_loaded(target);
            if let Some(window_idx) = self.absolute_to_window(target) {
                self.state.select(Some(window_idx));
            }
        }
    }

    /// Move the selection up one page (PageUp / Ctrl+u).
    fn page_up(&mut self) {
        let page_size = 20; // Approximate visible rows
        if let Some(current) = self.state.selected() {
            let absolute = self.window_to_absolute(current);
            let target = absolute.saturating_sub(page_size);
            self.ensure_row_loaded(target);
            if let Some(window_idx) = self.absolute_to_window(target) {
                self.state.select(Some(window_idx));
            }
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> HandleKeyResult {
        // Popups consume keys while visible
        if self.help_popup.visible {
            self.help_popup.handle_key(key);
            return HandleKeyResult::None;
        }
        if self.copy_popup.visible {
            if let Some(option) = self.copy_popup.handle_key(key) {
                self.execute_copy(option);
            }
            return HandleKeyResult::None;
        }

        // Clear footer message on any key press
        self.footer_message = None;

        // Ctrl+d / Ctrl+u page like PageDown / PageUp
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('d') => {
                    self.page_down();
                    return HandleKeyResult::None;
                }
                KeyCode::Char('u') => {
                    self.page_up();
                    return HandleKeyResult::None;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('?') => {
                self.help_popup.show();
                HandleKeyResult::None
            }
            KeyCode::Char('q') | KeyCode::Esc => HandleKeyResult::Navigate(NavigateToPage::Listing),
            KeyCode::Char('Q') => HandleKeyResult::Quit,
            KeyCode::Char('[') => {
                self.sort(SortDirection::Ascending);
                HandleKeyResult::None
            }
            KeyCode::Char(']') => {
                self.sort(SortDirection::Descending);
                HandleKeyResult::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(current) = self.state.selected() {
                    let absolute = self.window_to_absolute(current);
                    if absolute + 1 < self.row_count.known {
                        let next_absolute = absolute + 1;
                        // Check if we need to load a new window
                        if next_absolute >= self.window_start + self.data.rows.len() {
                            self.ensure_row_loaded(next_absolute);
                        }
                        // Update selection to new window-relative position
                        if let Some(new_window_idx) = self.absolute_to_window(next_absolute) {
                            self.state.select(Some(new_window_idx));
                        }
                    }
                } else {
                    self.state.select_first();
                }
                HandleKeyResult::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(current) = self.state.selected() {
                    let absolute = self.window_to_absolute(current);
                    if absolute > 0 {
                        let prev_absolute = absolute - 1;
                        // Check if we need to load a new window
                        if prev_absolute < self.window_start {
                            self.ensure_row_loaded(prev_absolute);
                        }
                        // Update selection to new window-relative position
                        if let Some(new_window_idx) = self.absolute_to_window(prev_absolute) {
                            self.state.select(Some(new_window_idx));
                        }
                    }
                } else {
                    self.state.select_first();
                }
                HandleKeyResult::None
            }
            KeyCode::PageDown => {
                self.page_down();
                HandleKeyResult::None
            }
            KeyCode::PageUp => {
                self.page_up();
                HandleKeyResult::None
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if let Some(idx) = self.state.selected_column() {
                    if idx >= (self.n_columns_show - 1)
                        && self.column_idx_offset + self.n_columns_show < self.data.columns.len()
                    {
                        self.column_idx_offset += 1;
                    } else {
                        self.state.select_next_column();
                    }
                } else {
                    self.state.select_next_column();
                }
                HandleKeyResult::None
            }
            KeyCode::Char('h') | KeyCode::Left => {
                if let Some(idx) = self.state.selected_column() {
                    if idx == 0 && self.column_idx_offset > 0 {
                        self.column_idx_offset -= 1;
                    } else {
                        self.state.select_previous_column();
                    }
                } else {
                    self.state.select_previous_column();
                }
                HandleKeyResult::None
            }
            KeyCode::Char('g') => {
                // Jump to first row
                if self.row_count.known > 0 {
                    self.ensure_row_loaded(0);
                    self.state.select(Some(0));
                }
                HandleKeyResult::None
            }
            KeyCode::Char('G') => {
                // Jump to last row
                if self.row_count.known > 0 {
                    let last_row = self.row_count.known - 1;
                    self.ensure_row_loaded(last_row);
                    if let Some(window_idx) = self.absolute_to_window(last_row) {
                        self.state.select(Some(window_idx));
                    }
                }
                HandleKeyResult::None
            }
            KeyCode::Char('L') => {
                // Jump to the last column: with variable widths, the number
                // of columns that fit at the tail differs from the current
                // fit, so measure backwards from the last column.
                if !self.data.columns.is_empty() {
                    let count = fit_column_count(
                        self.col_widths.iter().rev(),
                        self.last_table_width.max(1),
                    )
                    .max(1);
                    self.column_idx_offset = self.data.columns.len().saturating_sub(count);
                    self.state.select_last_column();
                }
                HandleKeyResult::None
            }
            KeyCode::Char('H') => {
                self.state.select_first_column();
                self.column_idx_offset = 0;
                HandleKeyResult::None
            }
            // Open copy popup
            KeyCode::Char('y') | KeyCode::Char('c') => {
                self.copy_popup.show();
                HandleKeyResult::None
            }
            // Navigate to row detail view
            KeyCode::Enter => {
                if let Some((window_idx, _)) = self.state.selected_cell() {
                    if window_idx < self.data.rows.len() {
                        let absolute_row = self.window_to_absolute(window_idx);
                        let data = RowPageData {
                            table_name: self.table_name.clone(),
                            row_index: absolute_row,
                            columns: self.data.columns.clone(),
                            // Full values: the window copies may be truncated
                            values: self.full_row_or_window(window_idx),
                            primary_keys: self.primary_keys.clone(),
                        };
                        return HandleKeyResult::Navigate(NavigateToPage::Row(data));
                    }
                }
                HandleKeyResult::None
            }
            _ => HandleKeyResult::None,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let layout = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(2),
        ]);
        let [table_rect, message_rect, help_rect] = area.layout(&layout);

        // Fit as many columns as the area allows, using the measured widths
        // (an id column no longer gets the same slot as a description one,
        // and a 100-column table shows as many columns as actually fit).
        let column_spacing: u16 = 1;
        self.last_table_width = table_rect.width;
        let count = fit_column_count(
            self.col_widths.iter().skip(self.column_idx_offset),
            table_rect.width,
        );
        let mut visible_widths: Vec<u16> = self
            .col_widths
            .iter()
            .skip(self.column_idx_offset)
            .take(count)
            .copied()
            .collect();
        if visible_widths.is_empty() {
            // No measured widths (e.g. load error): one full-width column
            visible_widths.push(table_rect.width.max(1));
        }
        // Key handling (h/l/H/L) uses the latest fit
        self.n_columns_show = visible_widths.len();
        let widths: Vec<Constraint> = visible_widths
            .iter()
            .map(|width| Constraint::Length(*width))
            .collect();

        let selected_header_idx = self
            .state
            .selected_column()
            .unwrap_or(0)
            .saturating_add(self.column_idx_offset);

        let n_columns_show = self.n_columns_show;
        let header = Row::new(self.data.columns.iter().skip(self.column_idx_offset).take(n_columns_show).enumerate().map(
            |(idx, c)| {
                Cell::from(Text::from(c.as_str())).style(
                    Style::new()
                        .bold()
                        .fg(self.theme.header_fg.clone().into())
                        .bg(
                            if selected_header_idx == idx.saturating_add(self.column_idx_offset) {
                                self.theme.header_selected_bg.clone().into()
                            } else {
                                self.theme.header_bg.clone().into()
                            },
                        ),
                )
            },
        ))
        .style(
            Style::new()
                .bold()
                .fg(self.theme.header_style_fg.clone().into()),
        );

        let rows = self.data.rows.iter().map(|r| {
            Row::new(r.iter().skip(self.column_idx_offset).take(n_columns_show).map(|value| {
                let display_text =
                    render_value_for_display_capped(value, Some(MAX_CELL_FETCH_LEN));
                let text = match value {
                    OwnedValue::Integer(_) | OwnedValue::Double(_) => {
                        Text::from(display_text).alignment(HorizontalAlignment::Right)
                    }
                    _ => Text::from(display_text),
                };
                Cell::default()
                    .content(text)
                    .style(match value {
                        OwnedValue::Null => Style::new().fg(self.theme.null.clone().into()),
                        OwnedValue::Integer(_) => {
                            Style::new().fg(self.theme.integer.clone().into())
                        }
                        OwnedValue::Double(_) => Style::new().fg(self.theme.double.clone().into()),
                        OwnedValue::Text(_) => Style::new().fg(self.theme.text.clone().into()),
                        OwnedValue::Blob(_) => Style::new().fg(self.theme.blob.clone().into()),
                    })
            }))
        });

        let table = Table::new(rows, widths)
            .header(header)
            .column_spacing(column_spacing)
            .style(Style::new().fg(self.theme.table_fg.clone().into()))
            .row_highlight_style(Style::new().bold().bg(self.theme.row_hl_bg.clone().into()))
            .cell_highlight_style(
                Style::new()
                    .bold()
                    .fg(self.theme.cell_hl_fg.clone().into())
                    .bg(self.theme.cell_hl_bg.clone().into()),
            );

        frame.render_stateful_widget(table, table_rect, &mut self.state);

        // Footer message (copy confirmation, errors, position indicator)
        if self.pending_sort.is_some() {
            use ratatui::style::Color;
            frame.render_widget(
                Text::from("Sorting…")
                    .style(Style::new().fg(Color::Yellow))
                    .centered(),
                message_rect,
            );
        } else if let Some(msg) = &self.footer_message {
            use ratatui::style::Color;
            let style = if msg.starts_with("Copied") || msg.starts_with("✓") {
                Style::new().fg(Color::Green)
            } else {
                Style::new().fg(Color::Red)
            };
            frame.render_widget(
                Text::from(msg.as_str()).style(style).centered(),
                message_rect,
            );
        } else if let Some(ref error) = self.error {
            use ratatui::style::Color;
            frame.render_widget(
                Text::from(format!("Error: {}", error))
                    .style(Style::new().fg(Color::Red))
                    .centered(),
                message_rect,
            );
        } else if self.row_count.known > 0 || !self.row_count.is_complete {
            // Show position indicator with streaming count
            use super::format_number;
            use ratatui::style::Color;
            let current_row = self.selected_absolute_row().map(|r| r + 1).unwrap_or(0);
            let current_row_display = format_number(current_row);
            let count_display = self.row_count.display();
            let position_text = format!("Row {} of {}", current_row_display, count_display);
            frame.render_widget(
                Text::from(position_text)
                    .style(Style::new().fg(Color::DarkGray))
                    .centered(),
                message_rect,
            );

            // Continue counting if not complete (poll the background
            // COUNT(*), or advance the OFFSET probe one batch)
            if !self.row_count.is_complete {
                self.row_count.tick(self.runtime, &self.table_name);
            }
        }

        // Help bar
        help_bar_from(TABLE_KEYS).render(frame, help_rect);

        // Popups (render on top)
        self.copy_popup.render(frame, area);
        self.help_popup.render(frame, area);

        // Run a pending sort only after its "Sorting…" frame has been
        // composed: the blocking ORDER BY query executes between frames,
        // with honest feedback on screen instead of a silent freeze.
        if let Some(pending) = &mut self.pending_sort {
            if !pending.message_rendered {
                pending.message_rendered = true;
            } else {
                let order = pending.order.clone();
                self.pending_sort = None;
                self.apply_sort(order);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory runtime with a `nums(n)` table of `count` rows (1..=count).
    fn runtime_with_rows(count: usize) -> Runtime {
        let runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(&format!(
                "CREATE TABLE nums AS WITH RECURSIVE c(n) AS \
                 (SELECT 1 UNION ALL SELECT n+1 FROM c LIMIT {}) SELECT n FROM c",
                count
            ))
            .unwrap();
        runtime
    }

    #[test]
    fn test_full_table_copy_covers_more_than_one_window() {
        let count = WINDOW_SIZE * 2 + 50;
        let runtime = runtime_with_rows(count);
        let (data, truncated) =
            load_table_for_copy(&runtime, "nums", None, COPY_ROW_LIMIT).unwrap();
        assert!(!truncated);
        assert_eq!(data.rows.len(), count);

        // TSV: header + every row, not just the 200-row window
        let tsv = data_to_tsv(&data);
        assert_eq!(tsv.lines().count(), count + 1);
        assert_eq!(tsv.lines().next().unwrap(), "n");
        assert_eq!(tsv.lines().last().unwrap(), count.to_string());

        // INSERT statements: one per row
        let inserts = data_to_inserts("nums", &data);
        assert_eq!(inserts.lines().count(), count);
        assert!(inserts
            .lines()
            .last()
            .unwrap()
            .contains(&format!("VALUES ({})", count)));
    }

    #[test]
    fn test_full_table_copy_respects_sort_order() {
        let count = WINDOW_SIZE + 10;
        let runtime = runtime_with_rows(count);
        let order = Order {
            column_idx: 0,
            direction: SortDirection::Descending,
        };
        let (data, truncated) =
            load_table_for_copy(&runtime, "nums", Some(order), COPY_ROW_LIMIT).unwrap();
        assert!(!truncated);
        assert!(matches!(data.rows[0][0], OwnedValue::Integer(i) if i == count as i64));
        assert!(matches!(data.rows[count - 1][0], OwnedValue::Integer(1)));
    }

    #[test]
    fn test_full_table_copy_truncates_at_cap() {
        let runtime = runtime_with_rows(50);
        let (data, truncated) = load_table_for_copy(&runtime, "nums", None, 30).unwrap();
        assert!(truncated);
        assert_eq!(data.rows.len(), 30);
    }

    #[test]
    fn test_load_table_with_embedded_quote_name() {
        // A table (and column) whose name contains a double quote must load
        // through the count/window/full-row paths, all of which interpolate
        // the identifier. Without escaping the generated SQL is malformed.
        let runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(
                "CREATE TABLE \"we\"\"ird\" (\"c\"\"ol\" INTEGER); \
                 INSERT INTO \"we\"\"ird\" VALUES (1), (2), (3);",
            )
            .unwrap();

        let result = load_table_data(&runtime, "we\"ird", None, 0, 10);
        assert!(result.error.is_none(), "load error: {:?}", result.error);
        assert_eq!(result.data.rows.len(), 3);
        assert_eq!(result.data.columns, vec!["c\"ol".to_string()]);

        // Generated INSERTs round-trip the escaped identifiers.
        let inserts = data_to_inserts("we\"ird", &result.data);
        assert!(
            inserts.starts_with("INSERT INTO \"we\"\"ird\" (\"c\"\"ol\") VALUES (1);"),
            "got: {inserts}"
        );
    }

    #[test]
    fn test_inserts_escape_quoted_column_names() {
        let data = Data {
            columns: vec!["a\"b".to_owned(), "plain".to_owned()],
            rows: vec![vec![OwnedValue::Integer(1), OwnedValue::Text(b"x".to_vec())]],
        };
        let inserts = data_to_inserts("t", &data);
        assert_eq!(inserts, "INSERT INTO \"t\" (\"a\"\"b\", \"plain\") VALUES (1, 'x');");
    }

    #[test]
    fn test_tsv_escapes_tabs_and_newlines() {
        let data = Data {
            columns: vec!["col\ta".to_owned(), "b".to_owned()],
            rows: vec![vec![
                OwnedValue::Text(b"has\ttab".to_vec()),
                OwnedValue::Text(b"has\nnewline\rcr".to_vec()),
            ]],
        };
        let tsv = data_to_tsv(&data);
        let lines: Vec<&str> = tsv.lines().collect();
        // One header line + one row line: embedded newlines never split rows
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "col\\ta\tb");
        // Each line has exactly one (separator) tab: embedded tabs are escaped
        assert_eq!(lines[1], "has\\ttab\thas\\nnewline\\rcr");
    }

    #[test]
    fn test_full_table_copy_reports_query_errors() {
        let runtime = Runtime::new(None).unwrap();
        let err = load_table_for_copy(&runtime, "no_such_table", None, 10).unwrap_err();
        assert!(err.contains("no_such_table"));
    }

    #[test]
    fn test_rowid_keyset_usable_classes() {
        let runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script(
                "create table plain(a, b);\n\
                 create table no_rowid(id integer primary key, v) without rowid;\n\
                 create table shadowed(a, rowid text);\n\
                 create view v_plain as select * from plain;",
            )
            .unwrap();
        assert!(rowid_keyset_usable(&runtime, "plain"));
        assert!(!rowid_keyset_usable(&runtime, "no_rowid"));
        assert!(!rowid_keyset_usable(&runtime, "shadowed"));
        assert!(!rowid_keyset_usable(&runtime, "v_plain"));
        assert!(!rowid_keyset_usable(&runtime, "missing_table"));
    }

    #[test]
    fn test_table_column_names_include_generated_columns() {
        // pragma_table_info omits generated columns; the SELECT * probe
        // must not, or the window/copy/row-page column sets disagree
        let runtime = Runtime::new(None).unwrap();
        runtime
            .connection
            .execute_script("create table gen(a integer, b as (a * 2), c text)")
            .unwrap();
        assert_eq!(
            table_column_names(&runtime, "gen"),
            vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]
        );
    }

    #[test]
    fn test_row_count_new() {
        // an empty initial load means the table is empty: counting is done
        let empty = RowCount::new(0);
        assert!(empty.is_complete);
        assert_eq!(empty.known, 0);

        // a full initial window means there may be more rows
        let partial = RowCount::new(WINDOW_SIZE);
        assert!(!partial.is_complete);
        assert_eq!(partial.known, WINDOW_SIZE);
    }

    #[test]
    fn test_row_count_update_from_load() {
        let mut rc = RowCount::new(WINDOW_SIZE);

        // a full window deeper in the table extends the known count
        rc.update_from_load(WINDOW_SIZE, WINDOW_SIZE);
        assert_eq!(rc.known, 2 * WINDOW_SIZE);
        assert!(!rc.is_complete);

        // a short window means the end was found
        rc.update_from_load(2 * WINDOW_SIZE, 50);
        assert_eq!(rc.known, 2 * WINDOW_SIZE + 50);
        assert!(rc.is_complete);

        // stale loads never shrink the known count
        rc.update_from_load(0, WINDOW_SIZE);
        assert_eq!(rc.known, 2 * WINDOW_SIZE + 50);
    }

    #[test]
    fn test_row_count_count_batch_discovers_total() {
        let runtime = runtime_with_rows(500);
        let mut rc = RowCount::new(WINDOW_SIZE);
        let more = rc.count_batch(&runtime, "nums");
        // 500 rows fit in one batch: counting finished in a single call
        assert!(!more);
        assert!(rc.is_complete);
        assert_eq!(rc.known, 500);
    }

    #[test]
    fn test_row_count_count_batch_handles_missing_table() {
        let runtime = Runtime::new(None).unwrap();
        let mut rc = RowCount::new(WINDOW_SIZE);
        let more = rc.count_batch(&runtime, "no_such_table");
        assert!(!more);
        assert!(rc.is_complete);
    }
}
