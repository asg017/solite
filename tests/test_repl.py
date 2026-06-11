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


def test_dot_tables_and_schema(solite_cli):
    out = repl(
        solite_cli,
        [
            ".timer off",
            "create table users(id integer, name text);",
            ".tables",
            ".schema",
        ],
    )["stdout"]
    assert "users" in out
    # .schema echoes the stored CREATE statement
    assert "create table users" in out.lower()


def test_dot_print(solite_cli):
    out = repl(solite_cli, [".timer off", ".print hello world"])["stdout"]
    assert "hello world" in out


def test_dot_env_set_unset(solite_cli):
    out = repl(
        solite_cli,
        [".timer off", ".env set MY_TEST_VAR myvalue", ".env unset MY_TEST_VAR"],
    )["stdout"]
    assert "✓ set environment variable 'MY_TEST_VAR'" in out
    assert "✓ unset environment variable 'MY_TEST_VAR'" in out


def test_dot_open_creates_and_uses_db(solite_cli, tmp_path):
    out = solite_cli(
        [],
        communicate=[
            b".timer off\n"
            b".open mydb.db\n"
            b"create table t(x);\n"
            b"insert into t values (1), (2);\n"
            b"select count(*) as n from t;\n"
        ],
        kill=True,
        cwd=tmp_path,
    ).stdout
    assert "✓ opened database" in out
    assert (tmp_path / "mydb.db").exists()
    assert "n" in out and "2" in out


def test_timer_defaults_on_and_toggles(solite_cli):
    # The timer defaults to on: each statement is followed by a duration line
    timer_re = re.compile(r"\d+(\.\d+)?(ms|s)")
    on = repl(solite_cli, ["select 1;"])["stdout"]
    assert timer_re.search(on), on
    off = repl(solite_cli, [".timer off", "select 1;"])["stdout"]
    assert not timer_re.search(off), off


def test_multiline_sql_executes_once(solite_cli):
    # A statement split over lines is buffered until input_complete
    out = repl(solite_cli, [".timer off", "select", "1 + 1", "as answer;"])["stdout"]
    assert "answer" in out
    assert out.count("answer") == 1


def test_multiline_export(solite_cli, tmp_path):
    out = solite_cli(
        [],
        communicate=[b".timer off\n.export out.csv\nselect 1 as a, 2 as b;\n"],
        kill=True,
        cwd=tmp_path,
    ).stdout
    assert "exported to out.csv" in out
    lines = (tmp_path / "out.csv").read_text().strip().splitlines()
    assert lines[0] == "a,b"
    assert lines[1] == "1,2"


def test_procedure_definition_and_call(solite_cli):
    out = repl(
        solite_cli,
        [
            ".timer off",
            "-- name: answer :value",
            "select 42 as answer;",
            ".call answer",
        ],
    )["stdout"]
    assert "Registered procedure: answer" in out
    assert "42" in out


def test_error_then_recover(solite_cli):
    # A prepare error doesn't wedge the REPL; the next statement runs
    out = repl(solite_cli, [".timer off", "select nope();", "select 'recovered';"])
    assert "no such function" in out["stdout"] + out["stderr"]
    assert "recovered" in out["stdout"]


def test_transaction_sequence_executes(solite_cli):
    # The transaction prompt (`❱•`) needs a PTY to observe; this only checks
    # that a begin/commit sequence executes cleanly through the REPL.
    out = repl(
        solite_cli,
        [
            ".timer off",
            "create table t(x);",
            "begin;",
            "insert into t values (1);",
            "commit;",
            "select count(*) as n from t;",
        ],
    )
    assert "1" in out["stdout"]
    assert "error" not in out["stderr"].lower()


def test_unknown_dot_command_reports_error(solite_cli):
    out = repl(solite_cli, [".timer off", ".notacommand"])
    assert "Unknown command" in out["stdout"] + out["stderr"]


def test_pasted_prompts_are_stripped(solite_cli):
    # Lines copied from another REPL session (prompts included) execute
    # cleanly; every line of a multi-line paste is stripped.
    out = repl(
        solite_cli,
        [".timer off", "❱ select", "❱ 40 + 2", "❱ as pasted;"],
    )["stdout"]
    assert "pasted" in out
    assert "42" in out


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


def test_repl_bench(solite_cli):
    out = repl(solite_cli, [".timer off", ".bench", "select 1;"])
    combined = out["stdout"] + out["stderr"]
    assert "not supported" not in combined
    assert "Benchmark" in out["stdout"]
    assert "iterations" in out["stdout"]
    assert "Range (min ... max)" in out["stdout"]


def test_repl_vegalite_writes_spec(solite_cli):
    out = repl(
        solite_cli,
        [".timer off", ".vegalite bar", "select 1 as x, 2 as y;"],
    )
    combined = out["stdout"] + out["stderr"]
    assert "not supported" not in combined
    assert "Vega-Lite spec" in out["stdout"]
    # The printed path points at a real JSON spec
    m = re.search(r"wrote Vega-Lite spec to (\S+\.vl\.json)", out["stdout"])
    assert m, out["stdout"]
    import json
    from pathlib import Path

    spec_path = Path(m.group(1))
    spec = json.loads(spec_path.read_text())
    assert "mark" in spec
    spec_path.unlink()


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


def _editor_script(tmp_path, body):
    script = tmp_path / "editor.sh"
    script.write_text(f"#!/bin/sh\n{body}\n")
    script.chmod(0o755)
    return script


def test_editor_command_executes_buffer(solite_cli, tmp_path):
    # \e runs $EDITOR on a scratch file; whatever it writes is executed
    script = _editor_script(
        tmp_path, 'printf "select 41+1 as forty_two;" > "$1"'
    )
    out = solite_cli(
        [],
        communicate=[b".timer off\n\\e\n"],
        kill=True,
        env={"EDITOR": str(script)},
    ).stdout
    assert "forty_two" in out


def test_editor_command_preloads_last_input(solite_cli, tmp_path):
    # The scratch buffer is seeded with the most recently executed input
    side = tmp_path / "buffer-contents.txt"
    script = _editor_script(
        tmp_path, f'cat "$1" > {side}; printf "select 2;" > "$1"'
    )
    solite_cli(
        [],
        communicate=[b".timer off\nselect 'seeded-sql';\n\\e\n"],
        kill=True,
        env={"EDITOR": str(script)},
    )
    assert "seeded-sql" in side.read_text()


def test_editor_command_records_sql_in_history(solite_cli, tmp_path):
    # History records the SQL that ran, not the literal \e
    script = _editor_script(
        tmp_path, 'printf "select 99 as from_editor;" > "$1"'
    )
    solite_cli(
        [],
        communicate=[b".timer off\n\\e\n"],
        kill=True,
        env={"EDITOR": str(script), "HOME": str(tmp_path)},
    )
    history = (tmp_path / ".solite_history").read_text()
    assert "from_editor" in history
    assert "\\e" not in history


def test_editor_command_abort_executes_nothing(solite_cli, tmp_path):
    # A non-zero editor exit aborts without executing
    script = _editor_script(tmp_path, 'printf "select 7 as aborted;" > "$1"; exit 1')
    result = solite_cli(
        [],
        communicate=[b".timer off\n\\e\n"],
        kill=True,
        env={"EDITOR": str(script)},
    )
    assert "aborted" not in result.stdout
    assert "editor command failed" in result.stderr


def test_history_dedups_consecutive_entries(solite_cli, tmp_path):
    solite_cli(
        [],
        communicate=[b"select 'dup-me';\nselect 'dup-me';\nselect 'dup-me';\n"],
        kill=True,
        env={"HOME": str(tmp_path)},
    )
    history = (tmp_path / ".solite_history").read_text()
    assert history.count("dup-me") == 1


def test_history_solite_history_override(solite_cli, tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    history_file = tmp_path / "custom-history"
    solite_cli(
        [],
        communicate=[b"select 'overridden';\n"],
        kill=True,
        env={"HOME": str(home), "SOLITE_HISTORY": str(history_file)},
    )
    assert "overridden" in history_file.read_text()
    # The default location is not written when the override is set
    assert not (home / ".solite_history").exists()


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
