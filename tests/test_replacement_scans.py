import json


def write_csv(tmp_path, name="data.csv", text="a,b\n1,2\n"):
    p = tmp_path / name
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)
    return p


# --- missing-file handling (regression: used to panic the process) ---


def test_missing_csv_run_errors_cleanly(solite_cli, tmp_path):
    script = tmp_path / "t.sql"
    script.write_text('select * from "missing.csv";\n')
    result = solite_cli(["run", str(script)], cwd=tmp_path)
    # NOTE: `solite run` currently exits 0 on SQL errors; that is tracked
    # separately (todos/run/02-nonzero-exit-on-sql-error). Here we only pin
    # the no-panic behavior and the source-located diagnostic.
    assert "panicked" not in result.stderr
    assert "no such table: missing.csv" in result.stderr


def test_missing_csv_query_errors_cleanly(solite_cli, tmp_path):
    result = solite_cli(["q", 'select * from "missing.csv"'], cwd=tmp_path)
    assert not result.success
    assert "panicked" not in result.stderr
    assert "no such table: missing.csv" in result.stderr


def test_missing_csv_repl_errors_cleanly(solite_cli, tmp_path):
    result = solite_cli(
        [],
        communicate=[b'select * from "missing.csv";\n'],
        kill=True,
        cwd=tmp_path,
    )
    assert "panicked" not in result.stderr
    assert "no such table: missing.csv" in result.stdout + result.stderr


def test_missing_csv_test_errors_cleanly(solite_cli, tmp_path):
    script = tmp_path / "t.sql"
    script.write_text('select * from "missing.csv"; -- error: no such table\n')
    result = solite_cli(["test", str(script)], cwd=tmp_path)
    assert "panicked" not in result.stderr


# --- happy path ---


def test_csv_query_happy_path(solite_cli, tmp_path):
    write_csv(tmp_path)
    result = solite_cli(["q", 'select * from "data.csv"'], cwd=tmp_path)
    assert result.success, result.stderr
    assert json.loads(result.stdout) == [{"a": "1", "b": "2"}]


def test_csv_run_happy_path(solite_cli, tmp_path):
    write_csv(tmp_path)
    script = tmp_path / "t.sql"
    script.write_text('select count(*) as n from "data.csv";\n')
    result = solite_cli(["run", str(script)], cwd=tmp_path)
    assert result.success, result.stderr


# --- compressed file suffixes ---


def test_csv_gz_replacement_scan(solite_cli, tmp_path):
    import gzip

    (tmp_path / "data.csv.gz").write_bytes(gzip.compress(b"a,b\n1,2\n"))
    result = solite_cli(["q", 'select * from "data.csv.gz"'], cwd=tmp_path)
    assert result.success, result.stderr
    assert json.loads(result.stdout) == [{"a": "1", "b": "2"}]


def test_tsv_gz_replacement_scan(solite_cli, tmp_path):
    import gzip

    (tmp_path / "data.tsv.gz").write_bytes(gzip.compress(b"a\tb\n1\t2\n"))
    result = solite_cli(["q", 'select * from "data.tsv.gz"'], cwd=tmp_path)
    assert result.success, result.stderr
    assert json.loads(result.stdout) == [{"a": "1", "b": "2"}]


def test_csv_zst_replacement_scan(solite_cli, tmp_path):
    import pytest

    zstandard = pytest.importorskip("zstandard")
    (tmp_path / "data.csv.zst").write_bytes(
        zstandard.ZstdCompressor().compress(b"a,b\n1,2\n")
    )
    result = solite_cli(["q", 'select * from "data.csv.zst"'], cwd=tmp_path)
    assert result.success, result.stderr
    assert json.loads(result.stdout) == [{"a": "1", "b": "2"}]


def test_unsupported_suffix_still_errors(solite_cli, tmp_path):
    # An existing file with an unrecognized suffix falls through to the
    # normal no-such-table error.
    (tmp_path / "data.csv.bz2").write_bytes(b"not really bzip2")
    result = solite_cli(["q", 'select * from "data.csv.bz2"'], cwd=tmp_path)
    assert not result.success
    assert "no such table: data.csv.bz2" in result.stderr
