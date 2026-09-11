"""e2e coverage for `--theme` / $SOLITE_THEME: built-ins, user TOML files,
and the truecolor downgrade.

Every test forces `--color always` (stdout is a pipe here, never a pty) and
pins `COLORTERM` explicitly, since the depth downgrade reads it and the
inherited value differs between a developer's terminal and CI.
"""

TRUECOLOR = {"COLORTERM": "truecolor"}
NO_TRUECOLOR = {"COLORTERM": ""}

# Catppuccin Mocha peach (#fab387), the `integer` role.
MOCHA_PEACH = "\x1b[38;2;250;179;135m"


def write_theme(tmp_path, name, body):
    """Write `body` to <tmp_path>/config/solite/themes/<name>.toml."""
    themes = tmp_path / "config" / "solite" / "themes"
    themes.mkdir(parents=True, exist_ok=True)
    path = themes / f"{name}.toml"
    path.write_text(body)
    return path


def test_builtin_mocha_emits_truecolor_escapes(solite_cli):
    result = solite_cli(
        ["--theme", "catppuccin-mocha", "--color", "always", "run", "-c", "select 7;"],
        escape_ansi=False,
        env=TRUECOLOR,
    )
    assert result.success
    assert MOCHA_PEACH in result.stdout


def test_builtin_mocha_aliases(solite_cli):
    for alias in ["mocha", "catppuccin_mocha", "Catppuccin-Mocha"]:
        result = solite_cli(
            ["--theme", alias, "--color", "always", "run", "-c", "select 7;"],
            escape_ansi=False,
            env=TRUECOLOR,
        )
        assert result.success, alias
        assert MOCHA_PEACH in result.stdout, alias


def test_default_theme_is_ansi16_only(solite_cli):
    """No --theme means the terminal theme: named ANSI codes, no truecolor."""
    result = solite_cli(
        ["--color", "always", "run", "-c", "select 7;"],
        escape_ansi=False,
        env=TRUECOLOR,
    )
    assert result.success
    assert "38;2;" not in result.stdout
    assert "\x1b[33m" in result.stdout  # integer role = ANSI yellow


def test_missing_theme_errors_with_available_themes(solite_cli):
    result = solite_cli(
        ["--theme", "nonexistent", "run", "-c", "select 1;"], escape_ansi=False
    )
    assert not result.success
    assert "no theme named `nonexistent`" in result.stderr
    assert "terminal" in result.stderr
    assert "catppuccin-mocha" in result.stderr
    assert "nonexistent.toml" in result.stderr  # the searched path


def test_user_theme_file_by_path_via_env(solite_cli, tmp_path):
    path = write_theme(
        tmp_path,
        "mine",
        'inherits = "terminal"\n'
        "[palette]\n"
        'brand = "#00ff00"\n'
        "[roles]\n"
        'integer = { fg = "brand", bold = true }\n',
    )
    result = solite_cli(
        ["--color", "always", "run", "-c", "select 7;"],
        escape_ansi=False,
        env={**TRUECOLOR, "SOLITE_THEME": str(path)},
    )
    assert result.success
    assert "\x1b[1;38;2;0;255;0m7\x1b[0m" in result.stdout


def test_user_theme_file_by_name_in_config_dir(solite_cli, tmp_path):
    write_theme(
        tmp_path,
        "mine",
        "[roles]\ninteger = \"#00ff00\"\n",
    )
    result = solite_cli(
        ["--theme", "mine", "--color", "always", "run", "-c", "select 7;"],
        escape_ansi=False,
        env={**TRUECOLOR, "XDG_CONFIG_HOME": str(tmp_path / "config")},
    )
    assert result.success
    assert "\x1b[38;2;0;255;0m" in result.stdout


def test_partial_theme_inherits_the_rest(solite_cli, tmp_path):
    """A file that only overrides `integer` keeps mocha everywhere else."""
    write_theme(
        tmp_path,
        "tweaked",
        'inherits = "catppuccin-mocha"\n[roles]\ninteger = "#00ff00"\n',
    )
    result = solite_cli(
        [
            "--theme",
            "tweaked",
            "--color",
            "always",
            "run",
            "-c",
            "select 7 as a, 'x' as b;",
        ],
        escape_ansi=False,
        env={**TRUECOLOR, "XDG_CONFIG_HOME": str(tmp_path / "config")},
    )
    assert result.success
    assert "\x1b[38;2;0;255;0m" in result.stdout  # the override
    assert "\x1b[38;2;205;214;244m" in result.stdout  # mocha `text`, inherited


def test_theme_flag_beats_env(solite_cli, tmp_path):
    path = write_theme(tmp_path, "mine", '[roles]\ninteger = "#00ff00"\n')
    result = solite_cli(
        ["--theme", "catppuccin-mocha", "--color", "always", "run", "-c", "select 7;"],
        escape_ansi=False,
        env={**TRUECOLOR, "SOLITE_THEME": str(path)},
    )
    assert result.success
    assert MOCHA_PEACH in result.stdout
    assert "\x1b[38;2;0;255;0m" not in result.stdout


def test_bad_theme_file_names_the_key(solite_cli, tmp_path):
    write_theme(tmp_path, "broken", '[roles]\nkeyword = "chartreuse"\n')
    result = solite_cli(
        ["--theme", "broken", "run", "-c", "select 1;"],
        escape_ansi=False,
        env={"XDG_CONFIG_HOME": str(tmp_path / "config")},
    )
    assert not result.success
    assert "role `keyword`" in result.stderr
    assert "unknown color `chartreuse`" in result.stderr


def test_truecolor_downgraded_without_colorterm(solite_cli):
    """Without COLORTERM=truecolor, Rgb roles emit 256-color indexes."""
    result = solite_cli(
        ["--theme", "catppuccin-mocha", "--color", "always", "run", "-c", "select 7;"],
        escape_ansi=False,
        env=NO_TRUECOLOR,
    )
    assert result.success
    assert "38;2;" not in result.stdout
    assert "38;5;" in result.stdout


def test_theme_does_not_reintroduce_color_when_gated_off(solite_cli):
    """--theme is orthogonal to --color: a piped run stays escape-free."""
    result = solite_cli(
        ["--theme", "catppuccin-mocha", "run", "-c", "select 7;"],
        escape_ansi=False,
        env=TRUECOLOR,
    )
    assert result.success
    assert "\x1b[" not in result.stdout
