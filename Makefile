
# Run all sub-suites even if one fails, so a single failure (e.g. a stale
# snapshot) doesn't hide failures in the later suites. Fails at the end if
# any sub-suite failed. Kept sequential on purpose: test-pytest and
# test-snap both need the built binary / cargo target-dir lock.
test:
	@rc=0; \
	$(MAKE) test-cargo  || rc=1; \
	$(MAKE) test-pytest || rc=1; \
	$(MAKE) test-snap   || rc=1; \
	exit $$rc

test-cargo:
	cargo test

test-pytest:
	cargo build
	uv run --project tests pytest -vv

# Default mode so snapshot drift fails CI; snapshots are pinned to the
# vendored SQLite, so they're stable per checkout. Use test-snap-update to
# intentionally re-record (e.g. in the bump-sqlite flow).
test-snap:
	cargo run --bin solite -- test tests/snaps/

test-snap-update:
	cargo run --bin solite -- test --update tests/snaps/

# Bump the vendored SQLite submodule to a release tag, regenerate the
# amalgamation and version-bearing snapshots, and verify with the full suite.
# Snapshot updates are blind accepts — review `git diff` before committing;
# only the canonical sqlite_version snapshot should change. Never auto-commits.
bump-sqlite:
ifndef VERSION
	$(error Usage: make bump-sqlite VERSION=3.54.0)
endif
	@command -v cargo-insta >/dev/null || { echo "cargo-insta not found; install with: cargo install cargo-insta"; exit 1; }
	cd vendor/sqlite && git fetch --depth 1 origin tag version-$(VERSION) && git checkout version-$(VERSION)
	cargo build
	cargo insta test --accept
	uv run --project tests pytest --snapshot-update
	$(MAKE) test-snap-update
	make test
	@echo "SQLite bumped to $(VERSION). Review 'git diff' and 'git -C vendor/sqlite log -1', then commit."

# Criterion benchmarks for the sqlite binding and TUI hot paths
# (dev-tooling, not CI).
bench:
	cargo bench -p solite-core -p solite-cli

.PHONY: test test-cargo test-pytest test-snap test-snap-update bump-sqlite bench

docs-dev:
	npm -C site run dev

stdlib-loadable:
	mkdir -p dist/debug
	cargo build --package solite-stdlib --no-default-features
	mv target/debug/libsolite_stdlib.dylib dist/debug/

stdlib-static:
	mkdir -p dist/debug
	cargo build --package solite-stdlib
	mv target/debug/libsolite_stdlib.a dist/debug/

stdlib-loadable-release:
	mkdir -p dist/release
	cargo build --package solite-stdlib --no-default-features --release
	cp target/release/libsolite_stdlib.dylib dist/release/

stdlib-static-release:
	mkdir -p dist/release
	cargo build --package solite-stdlib --release
	cp target/release/libsolite_stdlib.a dist/release/


stdlib:
	make stdlib-loadable
	make stdlib-static

stdlib-release:
	make stdlib-loadable-release
	make stdlib-static-release

.PHONY: stdlib stdlib-loadable stdlib-static \
	stdlib-release stdlib-loadable-release stdlib-static-release
# $ sqlite3 :memory: '.load target/debug/libsolite_stdlib.dylib solite_stdlib_init' 'select 1'
