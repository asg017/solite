# CLI-level tests for `solite sqlite3-dbtotxt` (vendored dbtotxt.c).
#
# dbtotxt renders a database file as a hex/ASCII text dump (the canonical
# copy-pasteable form for bug reports). It reads raw bytes and links no
# sqlite3 symbols.

import sqlite3


def _make_db(path):
    conn = sqlite3.connect(path)
    conn.execute("CREATE TABLE t(x)")
    conn.execute("INSERT INTO t VALUES ('hello')")
    conn.commit()
    conn.close()


def test_dump_has_header_hex_and_footer(solite_cli, tmp_path):
    db = tmp_path / "fixture.db"
    _make_db(db)

    result = solite_cli(["sqlite3-dbtotxt", str(db)], cwd=tmp_path)
    assert result.success
    out = result.stdout
    # Header line: "| size <N> pagesize <N> filename <name>"
    assert "| size " in out
    assert "pagesize " in out
    # Page 1 always begins with the SQLite header string in the ASCII column.
    assert "SQLite format 3." in out
    # Footer line: "| end <name>"
    assert "| end " in out
    # The row we inserted shows up in the ASCII gutter.
    assert "hello" in out


def test_no_input_is_a_usage_error(solite_cli):
    result = solite_cli(["sqlite3-dbtotxt"])
    assert not result.success
    assert "Usage: dbtotxt" in result.stderr
