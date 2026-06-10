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
    ],
    ids=lambda args: args[0],
)
def test_failures_print_a_diagnostic(solite_cli, args):
    """Every command failure must write something to stderr, never exit silently."""
    result = solite_cli(args)
    assert not result.success
    assert result.stderr.strip() != ""


def test_query_sql_error_printed_once(solite_cli):
    result = solite_cli(["query", "SELECT * FROM nope_table"])
    assert not result.success
    assert result.stderr.count("no such table") == 1
