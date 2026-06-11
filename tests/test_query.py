def test_query_output_formats(solite_cli, snapshot, tmp_path):
    sql = "select * from json_tree('[1,2,3,4]')"
    assert solite_cli(["q", sql]).stdout == snapshot(name="basic-default")
    assert solite_cli(["q", "-f", "ndjson", sql]).stdout == snapshot(
        name="basic-ndjson"
    )

    assert solite_cli(["q", sql, "-o", "a.json"], cwd=tmp_path).stdout == snapshot(
        name="output a.json"
    )
    assert (tmp_path / "a.json").read_text() == snapshot(name="a.json contents")

    assert solite_cli(["q", sql, "-o", "a.csv"], cwd=tmp_path).success
    assert (tmp_path / "a.csv").read_text() == snapshot(name="a.csv contents")

    assert solite_cli(["q", sql, "-o", "a.tsv"], cwd=tmp_path).success
    assert (tmp_path / "a.tsv").read_text() == snapshot(name="a.tsv contents")

    assert solite_cli(["q", sql, "-o", "a.csv.gz"], cwd=tmp_path).success
    assert (tmp_path / "a.csv.gz").read_bytes() == snapshot(name="a.csv.gz contents")

    assert solite_cli(["q", sql, "-o", "a.csv.zst"], cwd=tmp_path).success
    assert (tmp_path / "a.csv.zst").read_bytes() == snapshot(name="a.csv.zst contents")


def test_query_fails(solite_cli, snapshot):
    assert solite_cli(["q", "create table t(a)"]).stderr == snapshot(name="write fails")
    assert solite_cli(["q", "select xxx()"]).stderr == snapshot(name="function DNE")
    assert solite_cli(["q", "select * from does_not_exist"]).stderr == snapshot(
        name="table DNE"
    )
    assert solite_cli(["q", "select dne from pragma_function_list"]).stderr == snapshot(
        name="column DNE"
    )

    # multiple statements are rejected, pointing the user at `solite run`
    multi = solite_cli(["q", "select 1; select 2"])
    assert not multi.success
    assert multi.stderr == snapshot(name="trailing SQL")


def test_query_memory_database(solite_cli):
    result = solite_cli(["q", "select 1", ":memory:"])
    assert result.success, result.stderr
    assert result.stdout == '[{"1":1}]\n'


def test_query_missing_database_blames_db_arg(solite_cli):
    result = solite_cli(["q", "select 1", "nope.db"])
    assert not result.success
    assert "nope.db" in result.stderr
    assert "select 1" not in result.stderr


def test_query_stdin(solite_cli, tmp_path):
    # explicit `-` placeholder
    result = solite_cli(["q", "-"], communicate=[b"select 42"])
    assert result.success, result.stderr
    assert result.stdout == '[{"42":42}]\n'

    # no positional at all, piped stdin
    result = solite_cli(["q"], communicate=[b"select 42"])
    assert result.success, result.stderr
    assert result.stdout == '[{"42":42}]\n'

    # stdin combined with a format flag
    result = solite_cli(["q", "-", "-f", "csv"], communicate=[b"select 1 as a"])
    assert result.success, result.stderr
    assert result.stdout == "a\n1\n"

    # stdin combined with a database positional and -o
    db = tmp_path / "data.db"
    assert solite_cli(["exec", str(db), "create table t(a); insert into t values (3)"]).success
    result = solite_cli(["q", str(db)], communicate=[b"select a from t"])
    assert result.success, result.stderr
    assert result.stdout == '[{"a":3}]\n'

    out = tmp_path / "out.csv"
    result = solite_cli(["q", str(db), "-", "-o", str(out)], communicate=[b"select a from t"])
    assert result.success, result.stderr
    assert out.read_text() == "a\n3\n"


def test_query_stdin_never_creates_database(solite_cli, tmp_path):
    """query is read-only: stdin paths must not create database files."""
    missing = tmp_path / "nope.db"
    result = solite_cli(["q", str(missing)], communicate=[b"select 1"])
    assert not result.success
    assert "nope.db" in result.stderr
    assert not missing.exists()

    # `solite q "select 1" -` must not create a file named "select 1"
    result = solite_cli(["q", "select 1", "-"], communicate=[b"select 2"], cwd=tmp_path)
    assert not result.success
    assert not (tmp_path / "select 1").exists()


def test_query_trailing_comment_ok(solite_cli):
    result = solite_cli(["q", "select 1; -- comment"])
    assert result.success
    assert result.stdout == '[{"1":1}]\n'


def test_query_value(solite_cli):
    assert solite_cli(["q", "select 1", "-f", "value"]).stdout == "1"
    assert solite_cli(["q", "select 'alex'", "-f", "value"]).stdout == "alex"
    assert (
        solite_cli(["q", "select zeroblob(5)", "-f", "value"]).stdout
        == "\x00\x00\x00\x00\x00"
    )

    assert (
        solite_cli(["q", "select 1 limit 0", "-f", "value"]).stderr
        == "Error: Execution failed: No rows returned in query\n"
    )
    assert (
        solite_cli(
            ["q", "select column1 from (values (1), (2));", "-f", "value"]
        ).stderr
        == "Error: Execution failed: More than 1 row returned, expected a single row. Try a `LIMIT 1`\n"
    )


def test_query_parameters(solite_cli):
    def add(a, b):
        return solite_cli(["q", "select :a + :b", "-p", "a", str(a), "-p", "b", str(b)])

    assert add(1, 2).stdout == '[{":a + :b":3}]\n'
    assert add(1, 1).stdout == '[{":a + :b":2}]\n'
