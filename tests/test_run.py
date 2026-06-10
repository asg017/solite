def test_run_basic(solite_cli, snapshot, tmp_path):
    (tmp_path / "a.sql").write_text(
        """
.timer off

select 1;

select 2;

select 3
""",
        newline="\n",
    )
    assert solite_cli(["run", "a.sql"], cwd=tmp_path).stdout == snapshot


def test_run_basic_dots(solite_cli, snapshot, tmp_path):
    (tmp_path / "a.sql").write_text(
        """
.timer off

select 1;

.print yo

.print yo2
""",
        newline="\n",
    )
    assert solite_cli(["run", "a.sql"], cwd=tmp_path).stdout == snapshot


def test_run_param_file_blob(solite_cli, snapshot, tmp_path):
    (tmp_path / "blob.bin").write_bytes(b"\xde\xad\xbe\xef")
    result = solite_cli(
        [
            "run",
            "-p",
            "data",
            "@blob.bin",
            "-c",
            ".timer off\nselect typeof(:data) as t, length(:data) as len, hex(:data) as h;",
        ],
        cwd=tmp_path,
    )
    assert result.stdout == snapshot
    assert result.success


def test_run_param_file_blob_with_script(solite_cli, snapshot, tmp_path):
    (tmp_path / "blob.bin").write_bytes(b"\x00\x01\x02\x03\x04")
    (tmp_path / "a.sql").write_text(
        """
.timer off

select typeof(:data) as t, length(:data) as len, hex(:data) as h;
""",
        newline="\n",
    )
    result = solite_cli(
        ["run", "a.sql", "-p", "data", "@blob.bin"],
        cwd=tmp_path,
    )
    assert result.stdout == snapshot
    assert result.success


def test_run_param_file_blob_mixed(solite_cli, snapshot, tmp_path):
    (tmp_path / "blob.bin").write_bytes(b"\xff\xfe")
    result = solite_cli(
        [
            "run",
            "-p",
            "name",
            "alice",
            "-p",
            "data",
            "@blob.bin",
            "-c",
            ".timer off\nselect typeof(:name) as nt, :name as nv, typeof(:data) as dt, hex(:data) as dh;",
        ],
        cwd=tmp_path,
    )
    assert result.stdout == snapshot
    assert result.success


def test_run_param_file_missing(solite_cli, snapshot, tmp_path):
    result = solite_cli(
        [
            "run",
            "-p",
            "data",
            "@does-not-exist.bin",
            "-c",
            "select 1;",
        ],
        cwd=tmp_path,
    )
    assert not result.success
    assert result.stderr == snapshot


def test_run_error(solite_cli, snapshot, tmp_path):
    (tmp_path / "a.sql").write_text(
        """
.timer off

select 'hello' as world;
select 1 + 1 as result;

select
  1,
  2,
  3,
  substr(),
  4,
  5,
  6;

""",
        newline="\n",
    )
    assert solite_cli(["run", "a.sql"], cwd=tmp_path).stdout == snapshot(name="stdout")
    assert solite_cli(["run", "a.sql"], cwd=tmp_path).stderr == snapshot(name="stderr")
