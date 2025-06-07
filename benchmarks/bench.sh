#!/bin/bash

hyperfine --warmup=3 \
	'sqlite3 :memory: "select value + 1 from generate_series(1, 100)"' \
	'target/release/solite query "select value + 1 from generate_series(1, 100)"'
