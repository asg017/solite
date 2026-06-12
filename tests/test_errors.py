import pytest


@pytest.mark.parametrize(
    "args",
    [
        ["test", "/nonexistent-dir/nope.sql"],
        ["repl", "/nonexistent-dir/nope.db"],
        ["run", "/nonexistent-dir/nope.sql"],
        ["codegen", "/nonexistent-dir/nope.sql"],
        ["vacuum", "/nonexistent-dir/nope.db"],
        ["backup", "/nonexistent-dir/nope.db", "/nonexistent-dir/out.db"],
        ["bench"],
    ],
    ids=lambda args: args[0],
)
def test_failures_print_a_diagnostic(solite_cli, args):
    """Every command failure must write something to stderr, never exit silently."""
    result = solite_cli(args)
    assert not result.success
    assert result.stderr.strip() != ""


def test_setup_statement_failure_fails_test_run(solite_cli, tmp_path):
    """A failing setup statement (no assertion comment) must fail the run
    and point at the statement, not be silently swallowed."""
    test_file = tmp_path / "t.sql"
    test_file.write_text(
        "CREATE TABLE t(id INTEGER UNIQUE);\n"
        "INSERT INTO t VALUES (1);\n"
        "INSERT INTO t VALUES (1);\n"
        "SELECT COUNT(*) FROM t; -- 2\n"
    )
    result = solite_cli(["test", str(test_file)], cwd=tmp_path)
    assert not result.success
    assert "UNIQUE constraint failed" in result.stderr
    assert "t.sql:3" in result.stderr


def test_query_sql_error_printed_once(solite_cli):
    result = solite_cli(["query", "SELECT * FROM nope_table"])
    assert not result.success
    assert result.stderr.count("no such table") == 1


def test_prepare_error_prints_message_and_location(solite_cli, tmp_path):
    """A statement that fails to *prepare* (no such table) must report the
    real SQLite message with a file:line diagnostic, not a bare
    'Error preparing step' string, and must abort the rest of the file."""
    test_file = tmp_path / "p.sql"
    test_file.write_text("SELECT 1; -- 1\nSELECT * FROM nope;\nSELECT 2; -- 2\n")
    result = solite_cli(["test", str(test_file)], cwd=tmp_path)
    assert not result.success
    assert "no such table: nope" in result.stderr
    assert "p.sql:2" in result.stderr
    assert "aborting test file" in result.stderr


def test_prepare_error_assertion_passes(solite_cli, tmp_path):
    """An `-- error:` assertion on a prepare-time failure passes, and the
    rest of the file still runs."""
    test_file = tmp_path / "p.sql"
    test_file.write_text(
        "SELECT * FROM nope; -- error: no such table: nope\nSELECT 1; -- 1\n"
    )
    result = solite_cli(["test", str(test_file)], cwd=tmp_path)
    assert result.success
    assert "2 successes" in result.stdout


def test_dot_run_missing_file_fails_test_run(solite_cli, tmp_path):
    """A `.run` pointing at a missing file is broken setup: the run must
    exit nonzero and name the file, not warn and pass."""
    test_file = tmp_path / "t.sql"
    test_file.write_text(".run does-not-exist.sql\nSELECT 1; -- 1\n")
    result = solite_cli(["test", str(test_file)], cwd=tmp_path)
    assert not result.success
    assert "does-not-exist.sql" in result.stderr
    assert "aborting test file" in result.stderr


def test_no_rows_assertion_failure_prints_diagnostic(solite_cli, tmp_path):
    """An assertion failing because the statement returned zero rows must
    print a located expected/actual diagnostic, not just a red x."""
    test_file = tmp_path / "t.sql"
    test_file.write_text("CREATE TABLE t(x);\nSELECT * FROM t; -- 5\n")
    result = solite_cli(["test", str(test_file)], cwd=tmp_path)
    assert not result.success
    assert "t.sql:2" in result.stderr
    assert "expected: 5" in result.stderr
    assert "[no results]" in result.stderr
