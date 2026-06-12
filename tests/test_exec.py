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


def test_exec_in_memory_single_arg(solite_cli):
    """A single SQL argument runs against an in-memory database."""
    result = solite_cli(["exec", "create table t(a)"])
    assert result.success, result.stderr
    assert result.stdout == "0 rows affected\n"


def test_exec_parameters(solite_cli, tmp_path):
    db = str(tmp_path / "data.db")
    assert solite_cli(
        ["exec", db, "create table t(id integer, name text)"]
    ).success

    result = solite_cli(
        ["exec", db, "insert into t values (:id, :name)", "-p", "id", "42", "-p", "name", "alex"]
    )
    assert result.success, result.stderr
    assert result.stdout == "1 row affected\n"

    rows = solite_cli(["q", "select id, name, typeof(id) as t from t", db, "-f", "json"])
    assert json.loads(rows.stdout) == [{"id": 42, "name": "alex", "t": "integer"}]

    result = solite_cli(["exec", db, "delete from t where id = :id", "-p", "id", "42"])
    assert result.success, result.stderr
    assert result.stdout == "1 row affected\n"


def test_exec_bad_sql_stderr(solite_cli, snapshot):
    result = solite_cli(["exec", "this is not sql"])
    assert not result.success
    assert result.stdout == ""
    assert result.stderr == snapshot(name="bad sql stderr")


def test_exec_no_sql_errors(solite_cli, tmp_path):
    """A lone database arg without piped SQL fails with a clear error."""
    db = tmp_path / "data.db"
    db.touch()
    # communicate=[b""] closes stdin immediately so nothing hangs
    result = solite_cli(["exec", str(db)], communicate=[b""])
    assert not result.success


def test_exec_exit_codes(solite_cli, tmp_path):
    db = str(tmp_path / "data.db")
    assert solite_cli(["exec", db, "create table t(a)"]).success
    assert not solite_cli(["exec", db, "insert into nope values (1)"]).success
    assert not solite_cli(["exec", "a", "b"]).success


def test_exec_rejects_output_and_format_flags(solite_cli, tmp_path):
    """The formerly-hidden reserved -o/-f flags are rejected, not ignored."""
    db = str(tmp_path / "data.db")
    out = tmp_path / "out.json"

    result = solite_cli(["exec", db, "create table t(a)", "-o", str(out)])
    assert not result.success
    assert "unexpected argument" in result.stderr
    assert not out.exists()

    result = solite_cli(["exec", db, "create table t(a)", "-f", "json"])
    assert not result.success
    assert "unexpected argument" in result.stderr


def test_exec_csv_replacement_scan_import(solite_cli, tmp_path):
    """`solite exec 'create table t as select * from "data.csv"'` imports the
    CSV into a real table — the main CSV-import workflow."""
    (tmp_path / "data.csv").write_text("id,name\n1,alpha\n2,beta\n")
    db = tmp_path / "data.db"

    result = solite_cli(
        ["exec", str(db), 'create table t as select * from "data.csv"'],
        cwd=tmp_path,
    )
    assert result.success, result.stderr

    # Re-open the database: the data persisted into a real table, and the
    # temp vtab did not leak into the database file.
    rows = solite_cli(["q", "select * from t order by id", str(db)], cwd=tmp_path)
    assert rows.success, rows.stderr
    assert json.loads(rows.stdout) == [
        {"id": "1", "name": "alpha"},
        {"id": "2", "name": "beta"},
    ]
    schema = solite_cli(["q", "select count(*) as n from sqlite_master", str(db)])
    assert json.loads(schema.stdout) == [{"n": 1}]


def test_exec_missing_csv_errors_cleanly(solite_cli, tmp_path):
    result = solite_cli(
        ["exec", 'select * from "missing.csv"'],
        cwd=tmp_path,
    )
    assert not result.success
    assert "panicked" not in result.stderr
    assert "no such table: missing.csv" in result.stderr
