import json
import sqlite3

QUERIES = """\
create table users(id integer primary key, name text not null);

-- name: getUserById :row
select id, name from users where id = $id::int;
"""


def test_codegen_stdout_json(solite_cli, tmp_path):
    (tmp_path / "queries.sql").write_text(QUERIES)

    result = solite_cli(["codegen", "queries.sql"], cwd=tmp_path)
    assert result.success

    report = json.loads(result.stdout)
    assert report["setup"] == [
        "create table users(id integer primary key, name text not null);"
    ]

    assert len(report["exports"]) == 1
    export = report["exports"][0]
    assert export["name"] == "getUserById"
    assert export["result_type"] == "Row"
    assert export["sql"] == "select id, name from users where id = $id::int;"

    assert [p["name"] for p in export["parameters"]] == ["id"]
    assert export["parameters"][0]["full_name"] == "$id::int"
    assert export["parameters"][0]["annotated_type"] == "int"
    assert export["parameters"][0]["nullable"] is False

    assert [c["name"] for c in export["columns"]] == ["id", "name"]


def test_codegen_output_flag(solite_cli, tmp_path):
    (tmp_path / "queries.sql").write_text(QUERIES)

    result = solite_cli(
        ["codegen", "queries.sql", "--output", "report.json"], cwd=tmp_path
    )
    assert result.success
    assert result.stdout == ""

    report = json.loads((tmp_path / "report.json").read_text())
    assert [e["name"] for e in report["exports"]] == ["getUserById"]


def test_codegen_schema_db(solite_cli, tmp_path):
    db_path = tmp_path / "schema.db"
    with sqlite3.connect(db_path) as db:
        db.execute("create table users(id integer primary key, name text not null)")

    (tmp_path / "queries.sql").write_text(
        "-- name: getUserById :row\n"
        "select id, name from users where id = $id::int;\n"
    )

    result = solite_cli(
        ["codegen", "queries.sql", "--schema", "schema.db"], cwd=tmp_path
    )
    assert result.success

    report = json.loads(result.stdout)
    # The external schema validates queries but stays out of `setup`.
    assert report["setup"] == []
    export = report["exports"][0]
    columns = {c["name"]: c for c in export["columns"]}
    assert columns["name"]["decltype"] == "TEXT"
    assert columns["name"]["nullable"] is False


def test_codegen_schema_sql_not_in_setup(solite_cli, tmp_path):
    (tmp_path / "schema.sql").write_text(
        "create table users(id integer primary key, name text not null);\n"
    )
    (tmp_path / "queries.sql").write_text(
        "create index users_name on users(name);\n"
        "\n"
        "-- name: getUserById :row\n"
        "select id, name from users where id = $id::int;\n"
    )

    result = solite_cli(
        ["codegen", "queries.sql", "--schema", "schema.sql"], cwd=tmp_path
    )
    assert result.success

    report = json.loads(result.stdout)
    assert len(report["setup"]) == 1
    assert "users_name" in report["setup"][0]
    assert [e["name"] for e in report["exports"]] == ["getUserById"]


def test_codegen_schema_unsupported_extension(solite_cli, tmp_path):
    (tmp_path / "queries.sql").write_text(QUERIES)
    (tmp_path / "schema.txt").write_text("create table t(a);\n")

    result = solite_cli(
        ["codegen", "queries.sql", "--schema", "schema.txt"], cwd=tmp_path
    )
    assert not result.success
    assert ".db, .sqlite, .sqlite3, or .sql" in result.stderr


def test_codegen_prepare_error(solite_cli, tmp_path):
    (tmp_path / "queries.sql").write_text(
        "-- name: broken :row\nselect * from missing;\n"
    )

    result = solite_cli(["codegen", "queries.sql"], cwd=tmp_path)
    assert not result.success
    assert result.stdout == ""
    assert "queries.sql:2:1" in result.stderr
    assert "no such table: missing" in result.stderr
    assert "select * from missing;" in result.stderr


def test_codegen_malformed_name_annotation(solite_cli, tmp_path):
    (tmp_path / "queries.sql").write_text(
        "create table t(a int);\n\n--name: getA :row\nselect a from t;\n"
    )

    result = solite_cli(["codegen", "queries.sql"], cwd=tmp_path)
    assert not result.success
    assert "--name: getA :row" in result.stderr
    assert "queries.sql:4" in result.stderr


def test_codegen_duplicate_export_names(solite_cli, tmp_path):
    (tmp_path / "queries.sql").write_text(
        "create table t(a int);\n"
        "\n"
        "-- name: getThing :value\n"
        "select a from t;\n"
        "\n"
        "-- name: getThing :value\n"
        "select count(*) from t;\n"
    )

    result = solite_cli(["codegen", "queries.sql"], cwd=tmp_path)
    assert not result.success
    assert "Duplicate export name `getThing`" in result.stderr
