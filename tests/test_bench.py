"""Integration tests for `solite bench` (and the `.bench` dot command)."""


def test_bench_missing_sql_file_errors(solite_cli, tmp_path):
    """A .sql argument that doesn't exist is a file error naming the path,
    not benched as literal SQL."""
    missing = tmp_path / "queyr.sql"
    result = solite_cli(["bench", str(missing)])
    assert not result.success
    assert "queyr.sql" in result.stderr
    # no benchmark output was produced
    assert "Time" not in result.stdout


def test_bench_single_database_broadcasts_to_all_queries(solite_cli, tmp_path):
    """One --database with several queries benches all of them against it."""
    db = tmp_path / "app.db"
    setup = tmp_path / "setup.sql"
    setup.write_text("CREATE TABLE t(a integer); INSERT INTO t VALUES (1);")
    assert solite_cli(["run", str(setup), str(db)]).success

    result = solite_cli(
        [
            "bench",
            "--database",
            str(db),
            "SELECT count(*) FROM t;",
            "SELECT max(a) FROM t;",
        ]
    )
    assert result.success, result.stderr
    assert "SELECT count(*) FROM t;" in result.stdout
    assert "SELECT max(a) FROM t;" in result.stdout


def test_bench_database_arity_mismatch_fails_before_benchmarking(
    solite_cli, tmp_path
):
    """2 databases for 3 queries errors up front: no benchmark output."""
    db_a = tmp_path / "a.db"
    db_b = tmp_path / "b.db"
    result = solite_cli(
        [
            "bench",
            "--database",
            str(db_a),
            "--database",
            str(db_b),
            "SELECT 1;",
            "SELECT 2;",
            "SELECT 3;",
        ]
    )
    assert not result.success
    assert "got 2 databases for 3 queries" in result.stderr
    assert "Time" not in result.stdout


def test_bench_multi_statement_file_runs_setup_and_benches_last(
    solite_cli, tmp_path
):
    """Leading statements run once as untimed setup; the last statement is
    the one benched."""
    sql_file = tmp_path / "bench.sql"
    sql_file.write_text(
        "CREATE TABLE t(a integer);\n"
        "INSERT INTO t VALUES (1), (2), (3);\n"
        "SELECT count(*) FROM t;\n"
    )
    result = solite_cli(["bench", str(sql_file)])
    assert result.success, result.stderr
    assert "ran 2 setup statements" in result.stdout
    # the benched statement labels the results
    assert "SELECT count(*) FROM t;" in result.stdout
    assert "Time" in result.stdout


def test_bench_trailing_comment_benches_last_statement(solite_cli, tmp_path):
    """A comment after the final statement must not turn that statement into
    setup: setup statements run once, the LAST real statement is benched, and
    side effects hit the database exactly once."""
    import sqlite3

    db_path = tmp_path / "data.db"
    sql_file = tmp_path / "q.sql"
    sql_file.write_text(
        "CREATE TABLE t(a integer);\n"
        "INSERT INTO t VALUES (1), (2), (3);\n"
        "SELECT count(*) FROM t;\n"
        "-- done\n"
    )
    result = solite_cli(["bench", "--database", str(db_path), str(sql_file)])
    assert result.success, result.stderr
    assert "no SQL statement to benchmark" not in result.stderr
    assert "ran 2 setup statements" in result.stdout
    # the last real statement is the one benched
    assert "SELECT count(*) FROM t;" in result.stdout
    assert "Time" in result.stdout
    # setup ran exactly once: the INSERT was not re-executed per iteration
    with sqlite3.connect(db_path) as db:
        assert db.execute("SELECT count(*) FROM t").fetchone()[0] == 3


def test_bench_single_statement_trailing_comment_no_setup(solite_cli, tmp_path):
    """A single statement followed by a comment is benched directly — it is
    never executed as setup."""
    sql_file = tmp_path / "q.sql"
    sql_file.write_text("SELECT 1 + 1;\n-- done\n")
    result = solite_cli(["bench", str(sql_file)])
    assert result.success, result.stderr
    assert "setup statement" not in result.stdout
    assert "SELECT 1 + 1;" in result.stdout
    assert "Time" in result.stdout


def test_bench_comments_only_file_errors(solite_cli, tmp_path):
    """A file with no real statement (comments/whitespace only) is a clear
    error, not a benchmark."""
    sql_file = tmp_path / "empty.sql"
    sql_file.write_text("-- just a comment\n\n-- another\n")
    result = solite_cli(["bench", str(sql_file)])
    assert not result.success
    assert "no SQL statement to benchmark" in result.stderr
    assert "Time" not in result.stdout
