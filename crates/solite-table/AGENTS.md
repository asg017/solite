# solite-table

Table rendering crate for displaying SQLite query results. Handles column collapsing for narrow terminals, row truncation for large result sets, and multiple output formats (terminal with ANSI, plain text, HTML for Jupyter).

## Entry Points

- `render_statement(stmt, config)` -- Main function. Streams rows from a `Statement`, buffers head/tail rows, computes layout, and renders. Returns `RenderResult`.
- `print_statement(stmt, config)` -- Convenience wrapper that renders and prints to stdout.

## Key Types

### `TableConfig` (`config.rs`)

Controls all rendering behavior. Constructed via `TableConfig::terminal()`, `::plain()`, `::html()`, or `::default()`.

| Field              | Default          | Description                                              |
|--------------------|------------------|----------------------------------------------------------|
| `output_mode`      | `Terminal`       | Which renderer to use                                    |
| `max_width`        | `None`           | Table width limit; `None` = auto-detect terminal width   |
| `head_rows`        | `20`             | Number of rows to keep from the start                    |
| `tail_rows`        | `20`             | Number of rows to keep from the end                      |
| `max_cell_width`   | `100`            | Truncate cell content beyond this width                  |
| `theme`            | Catppuccin Mocha | `Option<Theme>` for colors; `None` disables color        |
| `show_footer`      | `true`           | Show row/column count footer                             |
| `json_interactive` | `false`          | Render JSON as interactive tree viewer (HTML mode only)   |

Builder methods: `with_output_mode()`, `with_max_width()`, `with_theme()`, `with_row_limits()`, `with_footer()`.

`effective_width()` resolves `max_width`: returns `usize::MAX / 2` for HTML (no limit), auto-detects terminal via `term_size` otherwise, falls back to 120.

### `OutputMode` (`config.rs`)

- `Terminal` -- ANSI-colored output intended for direct terminal printing.
- `StringAnsi` -- Same as Terminal (delegates to `render_terminal`).
- `StringPlain` -- No ANSI codes. Uses a clone of config with `theme: None`.
- `Html` -- HTML `<table>` output for Jupyter notebooks, with inline CSS and optional interactive JSON viewer.

### `RenderResult` (`lib.rs`)

Returned by `render_statement`. Fields:

- `output: String` -- The rendered table (may contain ANSI codes or HTML).
- `total_rows` / `shown_rows` -- Total row count vs. how many are displayed.
- `total_columns` / `shown_columns` -- Total column count vs. how many are visible.

### `CellValue` (`types.rs`)

Holds a cell's `display` string, pre-computed `width` (excluding ANSI codes), `value_type` (for coloring), and `alignment`. Constructed from `ValueRefX` via `from_sqlite_value()`. Control characters (`\n`, `\r`, `\t`) are escaped on construction.

### `ValueType` (`types.rs`)

`Null`, `Integer`, `Double`, `Text`, `Blob`, `Json`. Determines color in themed output and alignment (numbers right-aligned, text left-aligned, null/blob centered).

### `ColumnInfo` (`types.rs`)

Tracks a column's `name`, `header_width`, and `max_content_width` (updated via `observe_width()` as rows stream through). `display_width()` returns the max of header and content width.

### `TableLayout` (`types.rs`)

Result of layout computation. Contains `visible_columns` (indices), `ellipsis_position` (where to insert "..." column if columns were collapsed), `column_widths`, and `total_columns`.

## How Rendering Works

### 1. Streaming and Buffering (`buffer.rs`)

`RowBuffer` stores the first `head_rows` rows in a `Vec` and the last `tail_rows` rows in a `RingBuffer`. Rows beyond `head_capacity` go into the ring buffer, which overwrites the oldest entry when full. This means memory usage is bounded to `head_rows + tail_rows` regardless of result set size. `into_parts()` returns `(head, tail)` vectors.

### 2. Layout Computation (`layout.rs`)

`compute_layout(columns, max_width, max_cell_width)`:

1. Computes natural widths (capped at `max_cell_width` and 40) and minimum widths (header width or 3, whichever is larger).
2. If all columns fit at minimum width, shows all and distributes extra space proportionally up to natural widths.
3. Otherwise, greedily adds columns alternating front and back (so first and last columns are prioritized), reserving space for an ellipsis column. Remaining width is distributed among visible columns.

Constants: border overhead = 4, separator = 3, ellipsis column = 3, min column width = 3, max reasonable column width = 40.

### 3. Rendering (`render/`)

All renderers take the same arguments: `columns`, `head_rows`, `tail_rows`, `layout`, `config`, `total_rows`.

- **`render_terminal`** (`terminal.rs`) -- Box-drawing borders, ANSI-colored cells, ellipsis rows for truncated results, footer line.
- **`render_string`** (`string.rs`) -- Delegates to `render_terminal` (identical output).
- **`render_string_plain`** (`string.rs`) -- Calls `render_terminal` with `theme: None` to suppress ANSI codes.
- **`render_html`** (`html.rs`) -- Generates `<table class="solite-table">` with inline CSS (Catppuccin Mocha colors). Supports interactive JSON tree viewer via embedded JS/CSS.

### 4. Cell Formatting (`format/`)

- `format_cell(cell, theme, max_width)` -- Truncates to `max_width`, applies ANSI color based on `ValueType`.
- `format_cell_html(cell, theme, config)` -- HTML-escaped output with `<span>` color styling.
- `format_json(contents, theme)` -- Syntax-highlights JSON using `solite-lexer` tokenizer (keys, strings, numbers, booleans get distinct colors).
- JSON interactive viewer uses embedded JS/CSS from `json_viewer.js` and `json_viewer.css`.

## File Layout

```
src/
  lib.rs            -- Public API: render_statement, print_statement, RenderResult
  config.rs         -- TableConfig, OutputMode
  types.rs          -- CellValue, ColumnInfo, TableLayout, ValueType, Alignment, display_width()
  layout.rs         -- compute_layout() with column collapsing logic
  buffer.rs         -- RowBuffer, RingBuffer for streaming head/tail retention
  theme.rs          -- Theme (Catppuccin Mocha), Color, ANSI constants
  format/
    mod.rs          -- html_escape()
    value.rs        -- format_cell(), format_cell_html(), truncation
    json.rs         -- JSON syntax highlighting (ANSI and HTML)
    json_viewer.css -- CSS for interactive JSON tree (HTML mode)
    json_viewer.js  -- JS for interactive JSON tree (HTML mode)
  render/
    mod.rs          -- RenderedRow enum, re-exports
    terminal.rs     -- Box-drawing terminal renderer
    string.rs       -- String renderers (ANSI and plain), delegate to terminal
    html.rs         -- HTML table renderer for Jupyter
    snapshots/      -- insta snapshot tests
```

## Dependencies

- `solite-core` -- `Statement`, `SQLiteError`, `ValueRefX` types
- `solite-lexer` -- JSON tokenizer for syntax highlighting
- `term_size` -- Terminal width detection
- `unicode-width` -- Correct display width for Unicode/CJK characters
