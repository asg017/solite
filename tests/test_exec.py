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
