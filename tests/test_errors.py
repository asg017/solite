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
