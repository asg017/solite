# Upgrading SQLite

Steps to update the vendored SQLite version.

## 1. Update the submodule

```sh
cd vendor/sqlite
git fetch origin --tags
git checkout version-X.YY.Z
```

## 2. Remove the old amalgamation

The build script (`build.rs`) skips `./configure && make` if `vendor/sqlite/sqlite3.c` already exists. Delete the stale files to force regeneration:

```sh
rm vendor/sqlite/sqlite3.c vendor/sqlite/sqlite3.h
```

## 3. Build and fix compile errors

```sh
cargo build
```

Common breakage:

- **Renamed extension init functions.** SQLite occasionally renames the `sqlite3_*_init` entry point for bundled extensions (e.g. 3.53 renamed `sqlite3_base_init` to `sqlite3_base64_init`). Fix in two places:
  - `src/lib.rs` — the `extern "C"` declaration and the `init_arg0(...)` call.
  - `build.rs` — the shell.c symbol renames (`.define("sqlite3_old_init", Some("_shell_old_init"))`). Any extension we compile separately *and* that shell.c also calls needs a rename to avoid duplicate symbols. To check which extensions shell.c embeds, grep for `sqlite3_*_init` calls in `vendor/sqlite/src/shell.c.in`.

- **Removed or added extensions.** If an extension in `vendor/sqlite/ext/misc/` is removed, delete it from the `extensions` vec in `build.rs` and remove the corresponding `extern "C"` block and `init_arg0` call in `src/lib.rs`. New extensions require the reverse.

## 4. Run the full test suite and update snapshots

Run the **entire** workspace test suite, not just solite-stdlib:

```sh
cargo test
```

Several insta snapshots embed the SQLite version and will need updating:

- `crates/solite-core/src/snapshots/` — `version_functions_of()` snapshot includes `sqlite_version`
- `crates/solite-cli/src/commands/tui/snapshots/` — TUI test renders `sqlite_version()` in table data

Accept the updated snapshots:

```sh
cargo insta accept
```

Then rerun `cargo test` to confirm everything passes. Snapshot failures can cascade (insta stops on the first mismatch per test), so you may need to accept and rerun more than once.

The Python integration tests (`tests/`) also have snapshots that embed the SQLite version (REPL banner, help output). Update those with:

```sh
uv run --project tests pytest --snapshot-update
```

Run the full suite with `make test` to verify everything passes end-to-end.

## 5. Update the standalone Makefile (if needed)

`sqlite3/Makefile` has `SQLITE_VERSION` and `SQLITE_YEAR` variables used to download the official amalgamation zip for standalone `sqlite3` builds. Update these if you use that build path.
