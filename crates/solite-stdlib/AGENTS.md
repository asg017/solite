# solite-stdlib

Bundles SQLite with a collection of first-party and third-party extensions into a single static library. This crate is the source-of-truth for the SQLite build: it compiles the amalgamation, all C extensions, and all Rust extensions, then exposes a single `solite_stdlib_init` entry point that registers everything on a connection.

## File layout

- `build.rs` -- Compiles the SQLite amalgamation (from `vendor/sqlite`), C extensions from `vendor/sqlite/ext/misc/`, `usleep.c`, `shell.c` (with renamed `main`), `sqldiff.c`, and `sqlite3_rsync.c`. Also generates `builtins.rs` (a static list of built-in function names).
- `src/lib.rs` -- Declares `extern "C"` links for each C extension and the `solite_stdlib_init` entry point that calls every extension's init function.
- `src/ext.rs` -- Solite-specific scalar functions (`solite_stdlib_version`, `clipboard_set`) registered via `sqlite-loadable`.
- `usleep.c` -- Small C extension providing a `usleep()` SQL function.
- `sqlite3/core_init.c` -- Registers `solite_stdlib_init` as an auto-extension via `sqlite3_auto_extension`; used when building a standalone `sqlite3` binary.
- `sqlite3/solite-stdlib.h` -- C header declaring `solite_stdlib_init`.
- `sqlite3/Makefile` -- Builds a standalone `sqlite3` CLI that links `libsolite_stdlib.a`.

## How extension initialization works

`solite_stdlib_init` (in `src/lib.rs`, exported as `#[no_mangle] extern "C"`) is the single entry point. It receives the standard SQLite extension arguments `(db, pzErrMsg, pApi)` and calls each extension's init function in sequence via the `try_init!` macro, returning the first non-OK result code (remaining extensions are skipped). On success it returns 0 (`SQLITE_OK`). Registration order is observable (extensions can shadow each other's function names) — don't reorder.

1. **Rust extensions** (sqlite-loadable entry points returning `c_uint`) are called directly: `sqlite3_ulid_init`, `sqlite3_regex_init`, `sqlite3_http_init`, `sqlite3_path_init`, `sqlite3_url_init`, `sqlite3_xsv_init`, `sqlite3_str_init`, and `sqlite3_solite_stdlib_init`.
2. **C extensions and sqlite3_vec** are declared in `extern "C"` blocks with the true three-arg signature (`int sqlite3_X_init(sqlite3*, char**, const sqlite3_api_routines*)`, verified against the C sources) and called directly. This covers: base64, decimal, ieee754, fileio, uint, series, sha1, shathree, spellfix, uuid, usleep, completion, and vec. Note `sqlite3_vec_init` gets a local extern declaration because the sqlite-vec crate declares it zero-arg; the crate is imported as `use sqlite_vec as _;` to keep its compiled C library linked.

Calling `solite_stdlib_init` more than once on a connection is safe — functions and modules are re-registered (pinned by the `stdlib_double_init` test).

## C bindings

Each C extension is declared in `lib.rs` with `#[link(name = "...")]` and `extern "C"`. The actual `.c` files live in `vendor/sqlite/ext/misc/` and are compiled by `build.rs` using the `cc` crate with `SQLITE_CORE` defined (for static builds).

The SQLite amalgamation is compiled with many `SQLITE_ENABLE_*` flags (FTS5, rtree, geopoly, math functions, dbstat vtab, column metadata, update/delete limit, dbpage vtab, etc.).

Shell.c is compiled with duplicate extension symbols renamed (e.g., `sqlite3_shathree_init` -> `_shell_shathree_init`) to avoid linker conflicts since those extensions are also compiled standalone.

## Bundled extensions

**Rust (via crate dependencies):** sqlite-ulid, sqlite-regex, sqlite-http, sqlite-path, sqlite-xsv, sqlite-url, sqlite-str, sqlite-vec.

**C (from SQLite source tree):** base64, decimal, fileio, ieee754, sha1, shathree, spellfix, series, uuid, completion, uint.

**Local C:** usleep.
