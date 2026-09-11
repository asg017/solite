"""e2e coverage for --color / NO_COLOR / CLICOLOR / TERM=dumb gating.

`solite_cli` always pipes stdout/stderr through a plain OS pipe (never a
pty), so by default `is_terminal()` is false and no color escapes should
ever appear; `--color always` is the only way to force them back on.
"""

ESC = "\x1b["


def test_run_piped_has_no_escapes_by_default(solite_cli):
    result = solite_cli(["run", "-c", "select 2000 as y;"], escape_ansi=False)
    assert result.success
    assert ESC not in result.stdout
    assert ESC not in result.stderr
    # Sanity: the table itself still renders, just without color.
    assert "2000" in result.stdout


def test_run_no_color_env_is_clean(solite_cli):
    result = solite_cli(
        ["run", "-c", "select 2000 as y;"],
        escape_ansi=False,
        env={"NO_COLOR": "1"},
    )
    assert result.success
    assert ESC not in result.stdout
    assert ESC not in result.stderr


def test_run_term_dumb_is_clean(solite_cli):
    result = solite_cli(
        ["run", "-c", "select 2000 as y;"],
        escape_ansi=False,
        env={"TERM": "dumb"},
    )
    assert result.success
    assert ESC not in result.stdout


def test_run_clicolor_zero_is_clean(solite_cli):
    result = solite_cli(
        ["run", "-c", "select 2000 as y;"],
        escape_ansi=False,
        env={"CLICOLOR": "0"},
    )
    assert result.success
    assert ESC not in result.stdout


def test_run_color_always_keeps_escapes_when_piped(solite_cli):
    result = solite_cli(
        ["--color", "always", "run", "-c", "select 2000 as y;"], escape_ansi=False
    )
    assert result.success
    assert ESC in result.stdout


def test_run_color_never_overrides_clicolor_force(solite_cli):
    """--color never wins over every env override, per the documented
    precedence (--color > NO_COLOR > CLICOLOR_FORCE > CLICOLOR > TERM)."""
    result = solite_cli(
        ["--color", "never", "run", "-c", "select 2000 as y;"],
        escape_ansi=False,
        env={"CLICOLOR_FORCE": "1"},
    )
    assert result.success
    assert ESC not in result.stdout


def test_run_clicolor_force_forces_color_when_piped(solite_cli):
    result = solite_cli(
        ["run", "-c", "select 2000 as y;"],
        escape_ansi=False,
        env={"CLICOLOR_FORCE": "1"},
    )
    assert result.success
    assert ESC in result.stdout


def test_run_completion_status_checkmark_respects_gate(solite_cli, tmp_path):
    """The `✓ ...` completion status line (run/sql.rs) is also gated, not
    just table output."""
    script = tmp_path / "main.sql"
    script.write_text("create table t(a); insert into t values (1);\n")

    plain = solite_cli(["run", str(script)], escape_ansi=False)
    assert plain.success
    assert ESC not in plain.stdout
    assert "✓" in plain.stdout  # the checkmark glyph itself still prints

    colored = solite_cli(
        ["--color", "always", "run", str(script)], escape_ansi=False
    )
    assert colored.success
    assert ESC in colored.stdout


def test_backup_and_vacuum_checkmarks_respect_gate(solite_cli, tmp_path):
    db = tmp_path / "data.db"
    assert solite_cli(["exec", str(db), "create table t(a)"]).success

    backup_plain = solite_cli(
        ["backup", str(db), str(tmp_path / "b1.db")], escape_ansi=False
    )
    assert backup_plain.success
    assert ESC not in backup_plain.stdout

    backup_colored = solite_cli(
        ["--color", "always", "backup", str(db), str(tmp_path / "b2.db")],
        escape_ansi=False,
    )
    assert backup_colored.success
    assert ESC in backup_colored.stdout

    vacuum_plain = solite_cli(
        ["vacuum", str(db), "-o", str(tmp_path / "v1.db")], escape_ansi=False
    )
    assert vacuum_plain.success
    assert ESC not in vacuum_plain.stdout

    vacuum_colored = solite_cli(
        ["--color", "always", "vacuum", str(db), "-o", str(tmp_path / "v2.db")],
        escape_ansi=False,
    )
    assert vacuum_colored.success
    assert ESC in vacuum_colored.stdout


def test_diagnostics_respect_color_gate(solite_cli):
    """codespan-reporting error diagnostics (errors.rs) are also gated."""
    plain = solite_cli(["q", "select * from does_not_exist"], escape_ansi=False)
    assert not plain.success
    assert ESC not in plain.stderr

    colored = solite_cli(
        ["--color", "always", "q", "select * from does_not_exist"],
        escape_ansi=False,
    )
    assert not colored.success
    assert ESC in colored.stderr


def test_color_flag_shows_up_in_help(solite_cli):
    result = solite_cli(["--help"])
    assert result.success
    assert "--color" in result.stdout
