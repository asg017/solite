import re


def redact_banner(text, replacement):
    # Normalize both the Solite and SQLite versions in the REPL banner so
    # version bumps don't churn snapshots.
    text = re.sub(r"Solite \d+\.\d+\.\d+(-[a-z]+\.\d+)?", f"Solite {replacement}", text)
    return re.sub(r"SQLite \d+\.\d+\.\d+", f"SQLite {replacement}", text)


def repl(solite_cli, commands):
    msg = "\n".join(commands) + "\n"
    result = solite_cli([], communicate=[msg.encode()], kill=True)
    stdout = redact_banner(result.stdout, "REDACTED")
    stderr = result.stderr
    return {"stdout": stdout, "stderr": stderr}


def test_repl(solite_cli, snapshot):
    output = solite_cli(
        [], communicate=[b".timer off\nselect 1 + 1;\n"], kill=True
    ).stdout
    output = redact_banner(output, "VERSION")
    print(output)
    assert output == snapshot


def test_err(solite_cli, snapshot):
    assert repl(solite_cli, ["select xxx();"]) == snapshot
