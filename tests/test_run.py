import json
import sqlite3


def trace_statements(trace_path):
    """All recorded sql texts from a --trace database."""
    with sqlite3.connect(trace_path) as db:
        return [row[0] for row in db.execute("select sql from statements")]


def make_notebook(cells, nbformat_minor=5):
    """Build a minimal v4 notebook dict. `cells` is a list of
    ("code"|"markdown", source-string) tuples."""
    nb_cells = []
    for i, (cell_type, source) in enumerate(cells):
        cell = {
            "cell_type": cell_type,
            "metadata": {},
            "source": source.splitlines(keepends=True),
        }
        if nbformat_minor >= 5:
            cell["id"] = f"cell-{i}"
        if cell_type == "code":
            cell["execution_count"] = None
            cell["outputs"] = []
        nb_cells.append(cell)
    return {
        "cells": nb_cells,
        "metadata": {},
        "nbformat": 4,
        "nbformat_minor": nbformat_minor,
    }


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
    result = solite_cli(["run", "a.sql"], cwd=tmp_path)
    assert result.stdout == snapshot(name="stdout")
    assert result.stderr == snapshot(name="stderr")
    # Statements after the failure still run, but the process exits non-zero.
    assert not result.success


def test_run_exit_codes(solite_cli, tmp_path):
    # All-success script exits 0
    (tmp_path / "ok.sql").write_text(".timer off\nselect 1;\n", newline="\n")
    assert solite_cli(["run", "ok.sql"], cwd=tmp_path).success

    # -c with failing SQL exits non-zero
    result = solite_cli(["run", "-c", "select * from no_such_table;"], cwd=tmp_path)
    assert not result.success

    # A failing dot command alone flips the exit code
    (tmp_path / "dot.sql").write_text(
        ".timer off\n.load ./does-not-exist\nselect 1;\n", newline="\n"
    )
    result = solite_cli(["run", "dot.sql"], cwd=tmp_path)
    assert not result.success
    assert "Error loading extension" in result.stderr


def test_run_trace_procedure(solite_cli, tmp_path):
    (tmp_path / "procs.sql").write_text(
        "-- name: getOne :value\nselect 1 + 1;\n", newline="\n"
    )
    result = solite_cli(
        ["run", "procs.sql", "getOne", "--trace", "t.db"], cwd=tmp_path
    )
    assert result.success
    statements = trace_statements(tmp_path / "t.db")
    assert any("1 + 1" in sql for sql in statements)


def test_run_trace_nested_dot_run(solite_cli, tmp_path):
    (tmp_path / "child.sql").write_text("select 'from child';\n", newline="\n")
    (tmp_path / "main.sql").write_text(
        ".timer off\nselect 'from parent';\n.run child.sql\n", newline="\n"
    )
    result = solite_cli(["run", "main.sql", "--trace", "t.db"], cwd=tmp_path)
    assert result.success
    statements = trace_statements(tmp_path / "t.db")
    assert any("from parent" in sql for sql in statements)
    assert any("from child" in sql for sql in statements)


def test_run_ipynb_cell_order(solite_cli, snapshot, tmp_path):
    nb = make_notebook(
        [
            ("code", ".timer off"),
            ("markdown", "# this cell is skipped"),
            ("code", "select 'first';"),
            ("code", "select 'second';"),
            ("code", "select 'third';"),
        ]
    )
    (tmp_path / "a.ipynb").write_text(json.dumps(nb), newline="\n")
    result = solite_cli(["run", "a.ipynb"], cwd=tmp_path)
    assert result.success
    # Cells execute top-to-bottom: .timer off in the first cell applies to all
    # later cells, and the selects print in document order.
    assert result.stdout.index("first") < result.stdout.index("second")
    assert result.stdout.index("second") < result.stdout.index("third")
    assert result.stdout == snapshot
