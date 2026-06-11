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


def test_exec_two_unusable_args_error(solite_cli):
    """Nothing is silently ignored when neither arg can be classified."""
    result = solite_cli(["exec", "create table t(a)", "insert into t values (1)"])
    assert not result.success
    assert "cannot tell" in result.stderr
