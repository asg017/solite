# CLI-level tests for `solite sqlite3-dbhash` (vendored dbhash.c).
#
# dbhash hashes the *logical content* of a database (rows + rowids), so two
# files with identical content but different physical layout hash equal, while
# any content change flips the hash.

import sqlite3


def _make_db(path, rows):
    conn = sqlite3.connect(path)
    conn.execute("CREATE TABLE t(x)")
    conn.executemany("INSERT INTO t(rowid, x) VALUES (?, ?)", rows)
    conn.commit()
    conn.close()


def test_help_routes_to_dbhash(solite_cli):
    # disable_help_flag means --help reaches dbhash itself, not clap.
    result = solite_cli(["sqlite3-dbhash", "--help"])
    assert "Compute a SHA1 hash on the content of database" in result.stdout


def test_identical_content_different_layout_hash_equal(solite_cli, tmp_path):
    a = tmp_path / "a.db"
    b = tmp_path / "b.db"
    rows = [(1, 10), (2, 20), (3, 30)]
    _make_db(a, rows)
    _make_db(b, rows)
    # Bloat then vacuum b so its bytes differ from a while content matches.
    # isolation_level=None → autocommit, so VACUUM isn't inside a transaction.
    conn = sqlite3.connect(b, isolation_level=None)
    conn.execute("INSERT INTO t(rowid, x) VALUES (99, 99)")
    conn.execute("DELETE FROM t WHERE rowid = 99")
    conn.execute("VACUUM")
    conn.close()
    assert a.read_bytes() != b.read_bytes()

    result = solite_cli(["sqlite3-dbhash", str(a), str(b)])
    assert result.success
    hashes = [line.split()[0] for line in result.stdout.splitlines() if line.strip()]
    assert len(hashes) == 2
    assert hashes[0] == hashes[1]


def test_changed_row_changes_hash(solite_cli, tmp_path):
    a = tmp_path / "a.db"
    c = tmp_path / "c.db"
    _make_db(a, [(1, 10), (2, 20), (3, 30)])
    _make_db(c, [(1, 10), (2, 20), (3, 99)])  # one value differs

    result = solite_cli(["sqlite3-dbhash", str(a), str(c)])
    assert result.success
    hashes = [line.split()[0] for line in result.stdout.splitlines() if line.strip()]
    assert len(hashes) == 2
    assert hashes[0] != hashes[1]
