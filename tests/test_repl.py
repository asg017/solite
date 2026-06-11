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


def test_param_set_binds_all_placeholder_prefixes(solite_cli):
    out = repl(
        solite_cli,
        [
            ".timer off",
            ".param set x 1",
            "select $x as dollar, :x as colon, @x as at;",
        ],
    )["stdout"]
    # All three placeholder styles bind the bare key
    row = [line for line in out.splitlines() if "│ 1" in line]
    assert row, out
    assert row[0].count("1") == 3, out


def test_param_set_prefixed_key_still_binds(solite_cli):
    out = repl(
        solite_cli,
        [".timer off", ".param set $x prefixed", "select $x as v;"],
    )["stdout"]
    assert "prefixed" in out


def test_param_list_empty(solite_cli):
    # `.param list` before any `.param set`: the temp table doesn't exist yet
    out = repl(solite_cli, [".timer off", ".param list"])
    assert "No parameters set" in out["stdout"]
    assert "not yet implemented" not in out["stdout"] + out["stderr"]


def test_param_set_list_unset_clear(solite_cli):
    out = repl(
        solite_cli,
        [
            ".timer off",
            ".param set a apple",
            ".param set b banana",
            ".param list",
            ".param unset a",
            "select :a is null as a_gone, :b as b_kept;",
            ".param clear",
            "select :b is null as b_gone;",
        ],
    )["stdout"]
    assert "✓ set 'a' parameter" in out
    assert "✓ set 'b' parameter" in out
    # .param list shows both keys and values
    for needle in ["a", "apple", "b", "banana"]:
        assert needle in out
    assert "✓ unset 'a' parameter" in out
    assert "✓ cleared 1 parameter(s)" in out


def test_run_mode_param_subcommands(solite_cli, tmp_path):
    (tmp_path / "main.sql").write_text(
        ".timer off\n"
        ".param set x 1\n"
        ".param list\n"
        ".param unset x\n"
        ".param clear\n"
    )
    result = solite_cli(["run", "main.sql"], cwd=tmp_path)
    assert result.success
    assert "not yet implemented" not in result.stdout + result.stderr
    assert "parameter x unset" in result.stdout
    assert "cleared 0 parameter(s)" in result.stdout


def test_repl_run_procedure_params_scoped(solite_cli, tmp_path):
    # `.run file proc --k=v` in the REPL must not leak parameters
    (tmp_path / "procs.sql").write_text(
        "-- name: greet :row\nselect upper(:x) as msg;\n"
    )
    out = solite_cli(
        [],
        communicate=[
            b".timer off\n.run procs.sql greet --x=world\nselect :x as leaked;\n"
        ],
        kill=True,
        cwd=tmp_path,
    ).stdout
    assert "WORLD" in out
    assert "world" not in out.replace("WORLD", "")


def test_sigint_interrupts_query_without_exiting():
    """SIGINT (Ctrl-C) during a long-running query aborts the statement but
    keeps the REPL alive; a subsequent statement still executes."""
    import signal
    import subprocess
    import time
    from pathlib import Path

    cli = Path(__file__).parent.parent / "target" / "debug" / "solite"
    p = subprocess.Popen(
        [str(cli)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    try:
        p.stdin.write(
            b".timer off\n"
            b"WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c)"
            b" SELECT count(*) FROM c;\n"
        )
        p.stdin.flush()
        time.sleep(1.0)  # let the query start spinning
        p.send_signal(signal.SIGINT)
        time.sleep(0.5)
        assert p.poll() is None, "REPL exited after SIGINT"
        p.stdin.write(b"select 'still-alive';\n")
        p.stdin.flush()
        out, _ = p.communicate(timeout=10)
        text = out.decode("utf8", "replace").lower()
        assert "interrupt" in text
        assert "still-alive" in text
    finally:
        p.kill()
