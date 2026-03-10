# solite-mdtest

Markdown-based test framework for the solite SQL LSP. Tests are written as Markdown files with embedded SQL code blocks containing position markers and inline assertions. The framework parses these files, runs the SQL through the LSP analyzer, and verifies completions, hover info, diagnostics, and inlay hints.

## How Markdown Tests Work

Each `.md` file is structured with Markdown headers defining test names (nested headers are joined with " - "). SQL goes in fenced `sql` code blocks. The framework supports four kinds of assertions:

**Markers** are placed inline in SQL and referenced in assertions after the code block:
- `<acN>` -- autocomplete position. Assertions list expected completions: `- \`<ac1>\`: users, tables`. Prefix with `!` for strict (exact match) or `~` for explicitly non-strict.
- `<hvN>` -- hover position. Assertions list expected substrings in hover content: `- \`<hv1>\`: "table documentation"`.

**Inline assertions** are SQL comments on the same line:
- `-- error: [rule-id] "message"` -- expect a diagnostic on this line.
- `-- ok` -- assert no diagnostic on this line.
- `-- inlay: "label"` -- expect an inlay hint with this label on this line.

**Multi-file tests** label SQL blocks with a preceding `` `filename.sql` `` inline code span. All files are concatenated; schema (CREATE TABLE) is extracted for completion/hover context.

**Configuration** uses fenced `toml` blocks with a `[lint]` section for rule overrides and `strict = true` for global strict mode.

## Running Tests

```
cargo test -p solite-mdtest
```

Filter by file or test name with `MDTEST_FILTER`:
```
MDTEST_FILTER=autocomplete cargo test -p solite-mdtest
```

The test binary uses a custom harness (`tests/mdtest.rs`) that discovers all `.md` files under `resources/mdtest/` and runs them.

## File Layout

```
src/
  lib.rs          -- Public API: run_test_file, run_test_directory, MdTestError
  parser.rs       -- Parses Markdown into MdTest/TestFile structs using pulldown-cmark
  markers.rs      -- Extracts <acN>/<hvN> markers from SQL, produces clean SQL with offset mapping
  assertions.rs   -- Parses assertion syntax (list format, code block format, inline diagnostics/inlay hints)
  runner.rs       -- Executes tests: builds schema, runs LSP completions/hover/diagnostics/inlay hints, checks assertions
  reporter.rs     -- TestFailure enum and Display formatting
tests/
  mdtest.rs       -- Custom test harness (no default harness), entry point for `cargo test`
resources/mdtest/
  autocomplete.md -- Autocomplete completion tests
  diagnostics.md  -- Parse error and semantic diagnostic tests
  hover.md        -- Hover information tests
  inlay_hints.md  -- INSERT VALUES inlay hint tests
```
