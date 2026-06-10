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


def test_help_lists_dot_commands(solite_cli):
    output = solite_cli([], communicate=[b".help\n"], kill=True).stdout
    assert "Unknown command" not in output
    for needle in [".tables", ".export <path>", ".param set", "!<command>", "?<question>"]:
        assert needle in output


def test_help_topic(solite_cli):
    output = solite_cli([], communicate=[b".help export\n"], kill=True).stdout
    assert ".export <path>" in output


def test_db_file_opens_repl(solite_cli, tmp_path):
    # `solite <file>` opens a REPL for any recognized database extension
    import shutil
    from pathlib import Path

    src = Path(__file__).parent / "legislators.db"
    for ext in ["db", "sqlite", "sqlite3"]:
        db = tmp_path / f"data.{ext}"
        shutil.copy(src, db)
        result = solite_cli([str(db)], communicate=[b".tables\n"], kill=True)
        assert f'Connected to "{db}"' in result.stdout, ext


def test_non_db_file_is_usage_error(solite_cli):
    result = solite_cli(["not-a-db.txt"])
    assert not result.success
    assert result.stderr != ""
