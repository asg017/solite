import json
import os
import sys

import pytest


SETUP_SQL = """
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
CREATE INDEX idx_users_name ON users(name);
CREATE VIEW v_users AS SELECT * FROM users;
CREATE TRIGGER trg AFTER INSERT ON users BEGIN SELECT 1; END;
"""


@pytest.fixture
def schema_db(solite_cli, tmp_path):
    """A database with a table, index, view, and trigger."""
    script = tmp_path / "setup.sql"
    script.write_text(SETUP_SQL)
    result = solite_cli(["run", "schema.db", str(script)], cwd=tmp_path)
    assert result.success
    return tmp_path / "schema.db"


def test_schema_basic_dump(solite_cli, tmp_path, schema_db):
    result = solite_cli(["schema", str(schema_db)], cwd=tmp_path)
    assert result.success
    assert "CREATE TABLE users" in result.stdout
    assert "CREATE INDEX idx_users_name" in result.stdout
    assert "CREATE VIEW v_users" in result.stdout
    assert "CREATE TRIGGER trg" in result.stdout
    # every statement is terminated so the dump is executable
    # (each CREATE in SETUP_SQL is a single line, so line == statement)
    for line in result.stdout.strip().splitlines():
        assert line.endswith(";"), line


def test_schema_creation_order_round_trip(solite_cli, tmp_path, schema_db):
    """The dump replays into a fresh database (tables before indexes etc)."""
    dump = solite_cli(["schema", str(schema_db)], cwd=tmp_path)
    assert dump.success
    # creation order: the table precedes the index that references it
    assert dump.stdout.index("CREATE TABLE users") < dump.stdout.index(
        "CREATE INDEX idx_users_name"
    )

    replay = tmp_path / "replay.sql"
    replay.write_text(dump.stdout)
    result = solite_cli(["run", "replayed.db", str(replay)], cwd=tmp_path)
    assert result.success, result.stderr

    # the replayed database dumps the same schema
    redump = solite_cli(["schema", str(tmp_path / "replayed.db")], cwd=tmp_path)
    assert redump.success
    assert redump.stdout == dump.stdout


def test_schema_missing_file(solite_cli, tmp_path):
    result = solite_cli(["schema", "nope.db"], cwd=tmp_path)
    assert not result.success
    assert "nope.db" in result.stderr
    # a typo'd path must not create an empty database file
    assert not (tmp_path / "nope.db").exists()


def test_schema_empty_database(solite_cli, tmp_path):
    script = tmp_path / "noop.sql"
    script.write_text("SELECT 1;\n")
    setup = solite_cli(["run", "empty.db", str(script)], cwd=tmp_path)
    assert setup.success
    assert (tmp_path / "empty.db").exists()

    result = solite_cli(["schema", str(tmp_path / "empty.db")], cwd=tmp_path)
    assert result.success
    assert result.stdout.strip() == ""


def test_schema_virtual_table(solite_cli, tmp_path):
    script = tmp_path / "fts.sql"
    script.write_text("CREATE VIRTUAL TABLE notes USING fts5(body);\n")
    setup = solite_cli(["run", "fts.db", str(script)], cwd=tmp_path)
    assert setup.success

    result = solite_cli(["schema", str(tmp_path / "fts.db")], cwd=tmp_path)
    assert result.success
    assert "CREATE VIRTUAL TABLE notes USING fts5" in result.stdout
    # shadow tables are included in the dump, matching sqlite3's .schema
    assert "notes_data" in result.stdout


def test_schema_pattern_filtering(solite_cli, tmp_path, schema_db):
    """A pattern argument shows only matching objects (and their indexes/triggers)."""
    result = solite_cli(["schema", str(schema_db), "users"], cwd=tmp_path)
    assert result.success
    assert "CREATE TABLE users" in result.stdout
    # objects ON users match via tbl_name, like sqlite3
    assert "CREATE INDEX idx_users_name" in result.stdout
    assert "CREATE TRIGGER trg" in result.stdout
    # the view is not on users and does not match
    assert "CREATE VIEW v_users" not in result.stdout

    wildcard = solite_cli(["schema", str(schema_db), "idx_%"], cwd=tmp_path)
    assert wildcard.success
    assert "CREATE INDEX idx_users_name" in wildcard.stdout
    assert "CREATE TABLE users" not in wildcard.stdout

    nothing = solite_cli(["schema", str(schema_db), "zzz"], cwd=tmp_path)
    assert nothing.success
    assert nothing.stdout.strip() == ""


def test_schema_format_json(solite_cli, tmp_path, schema_db):
    result = solite_cli(["schema", str(schema_db), "--format", "json"], cwd=tmp_path)
    assert result.success
    parsed = json.loads(result.stdout)

    assert [t["name"] for t in parsed["tables"]] == ["users"]
    assert [c["name"] for c in parsed["tables"][0]["columns"]] == ["id", "name"]
    id_col, name_col = parsed["tables"][0]["columns"]
    assert id_col["type"] == "INTEGER"
    assert id_col["primary_key"] is True
    assert name_col["type"] == "TEXT"
    assert name_col["primary_key"] is False
    assert "CREATE TABLE users" in parsed["tables"][0]["sql"]
    assert [v["name"] for v in parsed["views"]] == ["v_users"]
    assert [i["name"] for i in parsed["indexes"]] == ["idx_users_name"]
    assert [t["name"] for t in parsed["triggers"]] == ["trg"]
    assert parsed["triggers"][0]["event"] == "INSERT"

    # output is deterministic across runs
    again = solite_cli(["schema", str(schema_db), "--format", "json"], cwd=tmp_path)
    assert again.success
    assert again.stdout == result.stdout


def test_schema_format_json_rejects_pattern(solite_cli, tmp_path, schema_db):
    result = solite_cli(
        ["schema", str(schema_db), "users", "--format", "json"], cwd=tmp_path
    )
    assert not result.success
    assert "pattern" in result.stderr


def test_dot_schema_in_run_mode_terminates_statements(solite_cli, tmp_path, schema_db):
    """.schema output in run mode is copy-paste executable (trailing ;)."""
    script = tmp_path / "show.sql"
    script.write_text(".schema\n")
    result = solite_cli(["run", str(schema_db), str(script)], cwd=tmp_path)
    assert result.success
    assert "CREATE TABLE users" in result.stdout
    # each CREATE in the fixture is a single-line statement
    for line in result.stdout.splitlines():
        if line.startswith("CREATE "):
            assert line.endswith(";"), line


@pytest.mark.skipif(sys.platform == "win32", reason="POSIX file permissions")
def test_schema_readonly_file(solite_cli, tmp_path, schema_db):
    os.chmod(schema_db, 0o444)
    try:
        result = solite_cli(["schema", str(schema_db)], cwd=tmp_path)
        assert result.success
        assert "CREATE TABLE users" in result.stdout
    finally:
        os.chmod(schema_db, 0o644)
