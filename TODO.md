- [ ] print "bail" message on run when bailing
- [ ] on run: long SQL should have progress bar

## Core

| Command          | Description                       | Repl | Query/Exec | Run | Jupyer     |
| ---------------- | --------------------------------- | ---- | ---------- | --- | ---------- |
| `.load`          | asdfasdf                          |      |            |     |            |
| `.run`           | Run another SQL file              |      |            |     |            |
| `.export`        | Export next SQL statement to file |      |            |     |            |
| `.mode`          | box,json,csv,etc.                 |      |            |     |            |
| `.parameter`     | define a parameter                |      |            |     |            |
| `.open`          | change connection to new file     |      |            |     | important! |
| -                |                                   |      |            |     |            |
| `.bail`          | On SQL/dotcmd error, exit         |      |            |     |            |
| `.echo`          | Print SQL/dotcmd before running   |      |            |     |            |
| `.print`         | print a line                      |      |            |     |            |
| `.timer`         | asdfasdf                          |      |            |     |            |
| `.tables`        | asdfasdf                          |      |            |     |            |
| `.headers`       | asdfasdf                          |      |            |     |            |
| `.changes`       | List changes                      |      |            |     |            |
| -                |                                   |      |            |     |            |
| `.quit`/ `.exit` | Exit process                      |      |            |     |            |

---

- stdlib
- [ ] sqlite builtin
  - [ ] any more?
- [ ] sqlite3 cli
  - [ ] usleep
- [ ] `sqlite-assert`
- [ ] `sqlite-docs`
- [ ] crypto stuff
- [ ] beef up http, lines, path, url

- exec contexts
- stdlib
- replacement scans
- prql
- extensions 1st class
- .export
- docs

- repl
  - syntax highlighting
  - autocomplete
  - history
  - experience
  - docs
  - tui
- run
  - shebang support
  - `--watch` support
  - `--safe`
  - `--read-only`
  - ui
  - debugger/breakpoint?
  - parameters from CLI support
- exec

runtime

- configurable dot commands
-

replacment scans

- csv, txt/ndjson, parquet? json?
- extend: xml, google sheets, notion, duckdb, pg, etc.
- compression support?
- URLs?

.import/.export

.export

- csv, json, txt
- compression
- extension configurable (s3?)

temp tables

1. solite_parameters

- future: tasks, scheduler, migrations
- future 2: compile, bundle, fmt, lint, expand replacement scans, etc.

---

notifs print to console

jupyter todos

- special handling:
  - explain/explain query plan
  - maybe some pragmas?
  - pragma_function_list
- number coloring

## Benchmarks

```
hyperfine 'sqlite3 :memory: "select value from generate_series(1,1e6)"' './target/release/solite-cli query "select value from generate_series(1,1e6)"'
```

```
hyperfine --warmup 3 \
  'sqlite3 :memory: "select count(*) from generate_series(1,1e7)"' \
  './target/release/solite-cli query "select count(*) from generate_series(1,1e7)"'
```

```
hyperfine --warmup 10 \
  'sqlite3 :memory: "select 1"' \
  './target/release/solite-cli query "select 1"'
```

```
hyperfine --warmup 3 \
  'sqlite3 :memory: "select usleep(500 * 1e3)"' \
  './target/release/solite-cli query "select usleep(500)"'
```

## Running Jupyter Notebooks

```bash
# run notebook with in-memory DB
solite run build.ipynb

# run notebook on tmp.db DB
solite run tmp.db build.ipynb

# run notebook and save outputs to another notebook
solite run tmp.db build.ipynb -o build.2023-10-27.ipynb

solite run tmp.db build.ipynb -p name alex -p age 10
```
