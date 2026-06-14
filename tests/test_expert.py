# CLI-level tests for `solite sqlite3-expert` (vendored ext/expert).
#
# sqlite3_expert analyzes a SQL workload against a schema and proposes
# CREATE INDEX statements that would speed it up, plus the resulting plan.
#
# Invocation note: the DATABASE is the LAST argument
# (`sqlite3_expert ?OPTIONS? DATABASE`), so `-sql "..."` comes before the path.

import sqlite3


def _make_db(path, n=300):
    conn = sqlite3.connect(path)
    conn.execute("CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT, age INT)")
    conn.executemany(
        "INSERT INTO users(name, age) VALUES (?, ?)",
        [(f"u{i}", i % 50) for i in range(n)],
    )
    conn.commit()
    conn.close()


def test_recommends_index_for_filtered_column(solite_cli, tmp_path):
    db = tmp_path / "app.db"
    _make_db(db)

    result = solite_cli(
        ["sqlite3-expert", "-sql", "SELECT * FROM users WHERE age = 42", str(db)],
        cwd=tmp_path,
    )
    assert result.success
    out = result.stdout
    # Proposes an index on the filtered column...
    assert "CREATE INDEX" in out
    assert "users(age)" in out
    # ...and shows the plan now searches via that index.
    assert "SEARCH users USING INDEX" in out


def test_no_args_is_a_usage_error(solite_cli):
    # The database arg is required; expert prints usage to stderr, exits nonzero.
    result = solite_cli(["sqlite3-expert"])
    assert not result.success
    assert "Usage sqlite3_expert" in result.stderr
