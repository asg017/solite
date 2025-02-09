# solite-stdlib

The "standard library" of the Solite runtime.

These functions, virtual table modules, and collations are provided by two different sources: from SQLite itself, or pre-installed SQLite extensions.

## SQLite-provided functions

| Name     | Description                               |
| -------- | ----------------------------------------- |
| Builtins | Builtin SQLite functions available        |
| Math     | https://www.sqlite.org/lang_mathfunc.html |
| FTS5     | https://www.sqlite.org/fts5.html          |
| Geopoly  | https://www.sqlite.org/geopoly.html       |
| RTree    | https://www.sqlite.org/rtree.html         |
| JSON     | https://www.sqlite.org/json1.html         |

| Name         | Description | Source                                                          |
| ------------ | ----------- | --------------------------------------------------------------- |
| sqlite-regex |             | https://github.com/asg017/sqlite-regex                          |
| sqlite-ulid  |             | https://github.com/asg017/sqlite-ulid                           |
| sqlite-http  |             | https://github.com/asg017/sqlite-http/tree/rust                 |
| sqlite-lines |             | https://github.com/asg017/sqlite-lines/tree/rust                |
| sqlite-path  |             | https://github.com/asg017/sqlite-path/tree/rust                 |
| sqlite-url   |             | https://github.com/asg017/sqlite-url/tree/rust                  |
| base64       |             | https://www.sqlite.org/src/file?name=ext/misc/base64.c&ci=tip   |
| decimal      |             | https://www.sqlite.org/src/file?name=ext/misc/decimal.c&ci=tip  |
| spellfix     |             | https://www.sqlite.org/src/file?name=ext/misc/spellfix.c&ci=tip |
| ieee         |             | https://www.sqlite.org/src/file?name=ext/misc/ieee754.c&ci=tip  |
| fileio       |             | https://www.sqlite.org/src/file?name=ext/misc/fileio.c&ci=tip   |
| sha1         |             | https://www.sqlite.org/src/file?name=ext/misc/sha1.c&ci=tip     |
| sha3         |             | https://www.sqlite.org/src/file?name=ext/misc/shathree.c&ci=tip |
| uuid         |             | https://www.sqlite.org/src/file?name=ext/misc/uuid.c&ci=tip     |
| series       |             | https://www.sqlite.org/src/file?name=ext/misc/series.c&ci=tip   |
| uint         |             | https://www.sqlite.org/src/file?name=ext/misc/uint.c&ci=tip     |

- usleep
- compress
- sqlar
- zipfile

TODO

- sqlite-fastrand
- sqlite-xsv
- zipfile
- sqlar
- base85
- percentile!
