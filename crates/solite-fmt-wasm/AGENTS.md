# solite-fmt-wasm

WASM bindings for the `solite-fmt` SQL formatter. This is a thin wrapper that exposes `solite-fmt` to JavaScript/TypeScript via `wasm-bindgen`.

## What it does

Compiles the Rust SQL formatter (`solite-fmt`) to WebAssembly so it can run in browsers and other JS runtimes. Includes a demo `index.html` playground.

## Public API (exported via `wasm_bindgen`)

- `format(source: string, config?: object): string` — Format SQL. Throws on parse errors.
- `check(source: string, config?: object): boolean` — Returns `true` if SQL is already formatted per the config.
- `init()` — Called automatically on WASM start; sets up `console_error_panic_hook`.

### Config object (all fields optional)

| Field                        | Type     | Values                              | Default    |
|------------------------------|----------|-------------------------------------|------------|
| `keyword_case`               | `string` | `"lower"`, `"upper"`, `"preserve"` | `"lower"`  |
| `indent_style`               | `string` | `"spaces"`, `"tabs"`               | `"spaces"` |
| `indent_size`                | `number` |                                     | `2`        |
| `line_width`                 | `number` |                                     | `80`       |
| `comma_position`             | `string` | `"trailing"`, `"leading"`          | `"trailing"` |
| `logical_operator_position`  | `string` | `"before"`, `"after"`              | `"before"` |
| `statement_separator_lines`  | `number` |                                     | (default from `FormatConfig`) |

Config is deserialized from `JsValue` via `serde-wasm-bindgen`, then converted to `solite_fmt::FormatConfig`.

## How it wraps solite-fmt

`JsFormatConfig` (internal struct with `Option<String>` / `Option<usize>` fields) is deserialized from the JS config object. Its `into_format_config()` method maps string values to the corresponding `solite_fmt` enums (`KeywordCase`, `IndentStyle`, `CommaPosition`, `LogicalOperatorPosition`) and fills in defaults for omitted fields. The exported `format` and `check` functions then delegate to `solite_fmt::format_sql` and `solite_fmt::check_formatted`.

## File layout

- `src/lib.rs` — All WASM bindings: `JsFormatConfig`, `format()`, `check()`, `parse_config()`, plus native and `wasm_bindgen_test` tests.
- `Cargo.toml` — `cdylib` crate depending on `solite-fmt`, `wasm-bindgen`, `serde`, `serde-wasm-bindgen`, `console_error_panic_hook`.
- `pkg/` — `wasm-pack` build output (JS glue, `.wasm` binary, TypeScript declarations, `package.json`).
- `index.html` — Browser demo/playground with live formatting and configurable options.
- `test.ts` — Deno integration tests that load the WASM module and exercise `format`/`check`.

## Building

Built with `wasm-pack build --target web`. Output lands in `pkg/`.
