import json


def test_exec_multi_statement(solite_cli, tmp_path):
    """Every statement in the input runs, not just the first."""
    db = str(tmp_path / "data.db")
    (tmp_path / "data.db").touch()

    result = solite_cli(
        ["exec", db, "create table t(a); insert into t values (1); insert into t values (2)"]
    )
    assert result.success, result.stderr

    rows = solite_cli(["q", "select a from t order by a", db, "-f", "json"])
    assert rows.success
    assert json.loads(rows.stdout) == [{"a": 1}, {"a": 2}]


def test_exec_trailing_comment(solite_cli, tmp_path):
    db = str(tmp_path / "data.db")
    (tmp_path / "data.db").touch()

    result = solite_cli(["exec", db, "create table t(a); -- done"])
    assert result.success, result.stderr


def test_exec_creates_new_database(solite_cli, tmp_path):
    """A .db argument is the database even when it doesn't exist yet."""
    db_path = tmp_path / "brand_new.db"
    assert not db_path.exists()

    result = solite_cli(["exec", str(db_path), "create table t(a)"])
    assert result.success, result.stderr
    assert db_path.exists()

    rows = solite_cli(["q", "select count(*) as n from t", str(db_path), "-f", "json"])
    assert rows.success
    assert json.loads(rows.stdout) == [{"n": 0}]


def test_exec_sql_file_input(solite_cli, tmp_path):
    """A .sql argument is read as SQL, not treated as the database."""
    script = tmp_path / "script.sql"
    script.write_text("create table t(a); insert into t values (9);")
    db = tmp_path / "data.db"

    result = solite_cli(["exec", str(script), str(db)])
    assert result.success, result.stderr

    rows = solite_cli(["q", "select a from t", str(db), "-f", "json"])
    assert json.loads(rows.stdout) == [{"a": 9}]


def test_exec_stdin_input(solite_cli, tmp_path):
    """`-` reads SQL from piped stdin; the other positional is the database."""
    db = tmp_path / "new.db"
    result = solite_cli(
        ["exec", "-", str(db)],
        communicate=[b"create table t(a); insert into t values (5);"],
    )
    assert result.success, result.stderr

    rows = solite_cli(["q", "select a from t", str(db), "-f", "json"])
    assert json.loads(rows.stdout) == [{"a": 5}]


def test_exec_stdin_lone_database_arg(solite_cli, tmp_path):
    """Piped stdin with only a database positional reads SQL from stdin."""
    db = tmp_path / "new.db"
    result = solite_cli(
        ["exec", str(db)],
        communicate=[b"create table t(a)"],
    )
    assert result.success, result.stderr
    assert db.exists()


def test_exec_two_unusable_args_error(solite_cli):
    """Nothing is silently ignored when neither arg can be classified."""
    result = solite_cli(["exec", "create table t(a)", "insert into t values (1)"])
    assert not result.success
    assert "cannot tell" in result.stderr


def test_exec_reports_affected_rows(solite_cli, tmp_path):
    db = str(tmp_path / "data.db")

    result = solite_cli(["exec", db, "create table t(a)"])
    assert result.success, result.stderr
    assert result.stdout == "0 rows affected\n"

    result = solite_cli(["exec", db, "insert into t values (1),(2),(3)"])
    assert result.stdout == "3 rows affected\n"

    result = solite_cli(["exec", db, "delete from t where a = 1"])
    assert result.stdout == "1 row affected\n"

    result = solite_cli(["exec", db, "delete from t where 0"])
    assert result.stdout == "0 rows affected\n"


def test_exec_returning_rows_visible(solite_cli, tmp_path):
    db = str(tmp_path / "data.db")
    assert solite_cli(["exec", db, "create table t(a integer primary key)"]).success

    result = solite_cli(["exec", db, "insert into t values (7) returning a"])
    assert result.success, result.stderr
    assert '[{"a":7}]' in result.stdout
    assert "1 row affected" in result.stdout


def test_exec_error_output(solite_cli):
    result = solite_cli(["exec", "insert into nope values (1)"])
    assert not result.success
    # the diagnostic names the problem; no Debug-formatted anyhow dump
    assert "no such table" in result.stderr
    assert "Error: SQL error" not in result.stderr
