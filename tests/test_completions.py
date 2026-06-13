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
    # ValueHint annotations make the engine offer filesystem paths for the
    # `run` positional. (Ticket 03 narrows these to .sql/.ipynb/.db.)
    (tmp_path / "a.sql").write_text("SELECT 1;")
    (tmp_path / "b.db").write_text("")
    candidates = _complete(solite_cli, ["solite", "run", ""], 2, cwd=tmp_path)
    assert "a.sql" in candidates, candidates
    assert "b.db" in candidates, candidates
