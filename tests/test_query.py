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
    assert solite_cli(["q", "select * from does_not_exist"]).stderr == snapshot(name="table DNE")
    assert solite_cli(["q", "select dne from pragma_function_list"]).stderr == snapshot(name="column DNE")


def test_query_value(solite_cli):
    assert solite_cli(["q", "select 1", "-f", "value"]).stdout == "1"
    assert solite_cli(["q", "select 'alex'", "-f", "value"]).stdout == "alex"
    assert (
        solite_cli(["q", "select zeroblob(5)", "-f", "value"]).stdout
        == "\x00\x00\x00\x00\x00"
    )

    assert (
        solite_cli(["q", "select 1 limit 0", "-f", "value"]).stderr
        == "No rows returned in query.\n"
    )
    assert (
        solite_cli(
            ["q", "select column1 from (values (1), (2));", "-f", "value"]
        ).stderr
        == "More than 1 query returned, exepcted a single row. Try a `LIMIT 1`\n"
    )


def test_query_parameters(solite_cli):
    def add(a, b):
        return solite_cli(["q", "select :a + :b", "-p", "a", str(a), "-p", "b", str(b)])

    assert add(1, 2).stdout == '[{":a + :b":3}]\n'
    assert add(1, 1).stdout == '[{":a + :b":2}]\n'
