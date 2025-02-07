#!/bin/bash

hyperfine --warmup=3 \
  'sqlite3 :memory: "select 1 + 1"' \
  'target/release/solite-cli query "select 1 + 1"'
