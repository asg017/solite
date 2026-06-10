import re


def redact_version(s):
    return re.sub(r"solite \d+\.\d+\.\d+\S*", "solite <VERSION>", s)


def test_help(solite_cli, snapshot):
    result = solite_cli(["--help"])
    assert result.success
    assert redact_version(result.stdout) == snapshot(name="--help")

    for command in ["run", "query", "repl", "jupyter"]:
        result = solite_cli([command, "--help"])
        assert result.success
        assert result.stdout == snapshot(name=f"{command} --help")


def test_version(solite_cli):
    result = solite_cli(["--version"])
    assert result.success
    assert result.stdout.startswith("solite ")


def test_usage_errors_exit_nonzero(solite_cli):
    assert not solite_cli(["definitely-not-a-command"]).success
    assert not solite_cli(["query", "--not-a-flag"]).success
