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
