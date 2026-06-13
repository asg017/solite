"""Tests for `solite completions` and the dynamic shell-completion hook.

The dynamic engine re-invokes `solite` with `COMPLETE=<shell>` set and the
command line passed after `--`, using `_CLAP_COMPLETE_INDEX` to mark which word
is being completed. These tests drive that protocol directly so they don't need
a real shell present.
"""


def _complete(solite_cli, words, index, cwd=None, shell="bash"):
    """Invoke the completion hook for `words` at `index`, return candidate lines."""
    result = solite_cli(
        ["--", *words],
        env={"COMPLETE": shell, "_CLAP_COMPLETE_INDEX": str(index)},
        cwd=cwd,
    )
    return [line for line in result.stdout.splitlines() if line]


def test_completions_registration_scripts(solite_cli):
    for shell in ["bash", "zsh", "fish"]:
        result = solite_cli(["completions", shell])
        assert result.success, f"completions {shell} failed: {result.stderr}"
        assert "solite" in result.stdout


def test_completions_unknown_shell_errors(solite_cli):
    result = solite_cli(["completions", "notashell"])
    assert not result.success
    assert "notashell" in result.stderr


def test_completion_hook_lists_subcommands(solite_cli):
    candidates = _complete(solite_cli, ["solite", ""], 1)
    for expected in ["run", "repl", "test", "completions"]:
        assert expected in candidates, f"{expected} not in {candidates}"


def test_path_args_complete_filenames(solite_cli, tmp_path):
    # The `run` positional is a union completer: it offers both scripts and
    # databases, but not unrelated files.
    (tmp_path / "a.sql").write_text("SELECT 1;")
    (tmp_path / "b.db").write_text("")
    (tmp_path / "c.txt").write_text("")
    candidates = _complete(solite_cli, ["solite", "run", ""], 2, cwd=tmp_path)
    assert "a.sql" in candidates, candidates
    assert "b.db" in candidates, candidates
    assert "c.txt" not in candidates, candidates


def test_database_arg_completes_only_databases(solite_cli, tmp_path):
    # `query`'s database positional offers only db-like files (+ :memory:),
    # never .sql/.txt.
    (tmp_path / "a.sql").write_text("SELECT 1;")
    (tmp_path / "c.db").write_text("")
    (tmp_path / "d.txt").write_text("")
    candidates = _complete(solite_cli, ["solite", "query", "SELECT 1", ""], 3, cwd=tmp_path)
    assert "c.db" in candidates, candidates
    assert ":memory:" in candidates, candidates
    assert "a.sql" not in candidates, candidates
    assert "d.txt" not in candidates, candidates


def test_run_completes_procedure_names(solite_cli, tmp_path):
    # `solite run <db> queries.sql <TAB>` offers procedures defined in the
    # referenced file (recovered from argv, since per-arg completers can't see
    # sibling args).
    (tmp_path / "queries.sql").write_text(
        "-- name: getUser :row\n"
        "SELECT * FROM users WHERE id = $id;\n"
        "-- name: listUsers :rows\n"
        "SELECT * FROM users;\n"
    )
    (tmp_path / "app.db").write_text("")
    candidates = _complete(
        solite_cli, ["solite", "run", "app.db", "queries.sql", ""], 4, cwd=tmp_path
    )
    assert "getUser" in candidates, candidates
    assert "listUsers" in candidates, candidates


def test_query_sql_arg_completes_tables_with_sibling_db(solite_cli, tmp_path):
    # `solite query "SELECT * FROM us" app.db <TAB>` (cursor at end of the SQL
    # arg) offers the `users` table from the sibling db, as a whole
    # reconstructed string so the shell replaces the entire quoted word.
    db = tmp_path / "app.db"
    res = solite_cli(["execute", str(db), "CREATE TABLE users(id, name)"])
    assert res.success, res.stderr
    candidates = _complete(
        solite_cli, ["solite", "query", "SELECT * FROM us", "app.db"], 2, cwd=tmp_path
    )
    assert "SELECT * FROM users" in candidates, candidates


def test_query_sql_arg_completes_keywords_without_db(solite_cli, tmp_path):
    candidates = _complete(solite_cli, ["solite", "query", "SEL"], 2, cwd=tmp_path)
    assert any(c.lower() == "select" for c in candidates), candidates


def test_codegen_file_completes_only_scripts(solite_cli, tmp_path):
    (tmp_path / "a.sql").write_text("SELECT 1;")
    (tmp_path / "b.ipynb").write_text("{}")
    (tmp_path / "c.db").write_text("")
    candidates = _complete(solite_cli, ["solite", "codegen", ""], 2, cwd=tmp_path)
    assert "a.sql" in candidates, candidates
    assert "b.ipynb" in candidates, candidates
    assert "c.db" not in candidates, candidates
