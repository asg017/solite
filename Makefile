
test:
	cargo test
	cargo build
	uv run --project tests pytest
	SOLITE_SNAPSHOT_DIRECTORY=tests/__solite_snaps__ cargo run -- snap tests/snap1.sql


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
