import pytest


@pytest.fixture
def source_db(solite_cli, tmp_path):
    """A small database to back up."""
    script = tmp_path / "setup.sql"
    script.write_text("CREATE TABLE t(a); INSERT INTO t VALUES (1), (2);\n")
    result = solite_cli(["run", "src.db", str(script)], cwd=tmp_path)
    assert result.success
    return tmp_path / "src.db"


def test_backup_refuses_existing_destination(solite_cli, tmp_path, source_db):
    dest = tmp_path / "out.db"
    dest.write_bytes(b"precious")

    result = solite_cli(["backup", str(source_db), str(dest)], cwd=tmp_path)
    assert not result.success
    assert "already exists" in result.stderr
    assert dest.read_bytes() == b"precious"


def test_backup_force_overwrites_existing_destination(solite_cli, tmp_path, source_db):
    dest = tmp_path / "out.db"
    dest.write_bytes(b"precious")

    result = solite_cli(["backup", str(source_db), str(dest), "--force"], cwd=tmp_path)
    assert result.success
    assert dest.read_bytes() != b"precious"

    count = solite_cli(["q", "--format", "json", "SELECT count(*) AS n FROM t", str(dest)], cwd=tmp_path)
    assert count.success
    assert '"n":2' in count.stdout.replace(" ", "")


def test_failed_backup_leaves_no_destination_file(solite_cli, tmp_path, source_db):
    dest = tmp_path / "out.db"

    result = solite_cli(
        ["backup", str(source_db), str(dest), "--db", "nonexistent"], cwd=tmp_path
    )
    assert not result.success
    assert "nonexistent" in result.stderr
    assert not dest.exists()


def test_vacuum_into_force_overwrites_existing_destination(
    solite_cli, tmp_path, source_db
):
    dest = tmp_path / "out.db"
    dest.write_bytes(b"precious")

    # without --force, SQLite itself refuses ("output file already exists"
    # for a valid db, "file is not a database" otherwise)
    refused = solite_cli(
        ["vacuum", str(source_db), "--into", str(dest)], cwd=tmp_path
    )
    assert not refused.success
    assert dest.read_bytes() == b"precious"

    forced = solite_cli(
        ["vacuum", str(source_db), "--into", str(dest), "--force"], cwd=tmp_path
    )
    assert forced.success
    assert dest.read_bytes() != b"precious"
