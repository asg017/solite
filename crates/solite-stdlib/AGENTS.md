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

`solite_stdlib_init` (in `src/lib.rs`, exported as `#[no_mangle] extern "C"`) is the single entry point. It receives the standard SQLite extension arguments `(db, pzErrMsg, pApi)` and calls each extension's init function in sequence:

1. **Rust extensions** that match the `(db, pzErrMsg, pApi)` signature are called directly: `sqlite3_ulid_init`, `sqlite3_regex_init`, `sqlite3_http_init`, `sqlite3_path_init`, `sqlite3_url_init`, `sqlite3_xsv_init`, `sqlite3_str_init`, and `sqlite3_solite_stdlib_init`.
2. **C extensions and sqlite3_vec** use the `init_arg0` helper, which transmutes a zero-arg function pointer to the standard three-arg signature via `std::mem::transmute`. This covers: base64, decimal, ieee754, fileio, uint, series, sha1, shathree, spellfix, uuid, usleep, completion, and vec.

## C bindings

Each C extension is declared in `lib.rs` with `#[link(name = "...")]` and `extern "C"`. The actual `.c` files live in `vendor/sqlite/ext/misc/` and are compiled by `build.rs` using the `cc` crate with `SQLITE_CORE` defined (for static builds).

The SQLite amalgamation is compiled with many `SQLITE_ENABLE_*` flags (FTS5, rtree, geopoly, math functions, dbstat vtab, column metadata, update/delete limit, dbpage vtab, etc.).

Shell.c is compiled with duplicate extension symbols renamed (e.g., `sqlite3_shathree_init` -> `_shell_shathree_init`) to avoid linker conflicts since those extensions are also compiled standalone.

## Bundled extensions

**Rust (via crate dependencies):** sqlite-ulid, sqlite-regex, sqlite-http, sqlite-path, sqlite-xsv, sqlite-url, sqlite-str, sqlite-vec.

**C (from SQLite source tree):** base64, decimal, fileio, ieee754, sha1, shathree, spellfix, series, uuid, completion, uint.

**Local C:** usleep.
