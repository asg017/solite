"""e2e coverage for `--theme` / $SOLITE_THEME: built-ins, user TOML files,
and the truecolor downgrade.

Every test forces `--color always` (stdout is a pipe here, never a pty) and
pins `COLORTERM` explicitly, since the depth downgrade reads it and the
inherited value differs between a developer's terminal and CI.

Also covers `--theme-dark`/`--theme-light` (`$SOLITE_THEME_DARK`/
`$SOLITE_THEME_LIGHT`) resolution. `solite_cli` always runs the binary with
stdin/stdout as pipes (see conftest.py), never a pty, so every OSC 11
background query in these tests is short-circuited before it's ever sent
(see `colors::detect_background_mode`) and the pair always resolves via the
"detection unavailable" -> dark path. That's deliberate: it's the one path
these piped, deterministic tests *can* exercise. The real
detected-light/detected-dark branches need a real terminal and are covered
by the manual matrix in todos/theming/08-dark-light-adaptive.md, not here.
"""

import time

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


# --- --theme-dark / --theme-light ------------------------------------------


def test_theme_dark_only_piped_falls_back_to_dark(solite_cli):
    """Piped -> detection unavailable -> treated as dark -> `theme_dark`."""
    result = solite_cli(
        [
            "--theme-dark",
            "catppuccin-mocha",
            "--color",
            "always",
            "run",
            "-c",
            "select 7;",
        ],
        escape_ansi=False,
        env=TRUECOLOR,
    )
    assert result.success
    assert MOCHA_PEACH in result.stdout


def test_theme_light_only_piped_falls_back_to_terminal(solite_cli):
    """Piped -> detection unavailable -> treated as dark, but only `light`
    is configured, so there's no dark theme to fall back to: plain
    `terminal` wins instead of either half of the pair."""
    result = solite_cli(
        [
            "--theme-light",
            "catppuccin-mocha",
            "--color",
            "always",
            "run",
            "-c",
            "select 7;",
        ],
        escape_ansi=False,
        env=TRUECOLOR,
    )
    assert result.success
    assert MOCHA_PEACH not in result.stdout
    assert "38;2;" not in result.stdout
    assert "\x1b[33m" in result.stdout  # terminal default: integer = ANSI yellow


def test_theme_flag_beats_dark_light_pair(solite_cli):
    """A plain --theme bypasses the pair (and detection) entirely, even
    when both are passed on the same invocation."""
    result = solite_cli(
        [
            "--theme",
            "terminal",
            "--theme-dark",
            "catppuccin-mocha",
            "--color",
            "always",
            "run",
            "-c",
            "select 7;",
        ],
        escape_ansi=False,
        env=TRUECOLOR,
    )
    assert result.success
    assert MOCHA_PEACH not in result.stdout


def test_theme_env_beats_dark_light_pair(solite_cli):
    """$SOLITE_THEME wins over $SOLITE_THEME_DARK too, mirroring --theme."""
    result = solite_cli(
        ["--color", "always", "run", "-c", "select 7;"],
        escape_ansi=False,
        env={
            **TRUECOLOR,
            "SOLITE_THEME": "terminal",
            "SOLITE_THEME_DARK": "catppuccin-mocha",
        },
    )
    assert result.success
    assert MOCHA_PEACH not in result.stdout


def test_theme_dark_env(solite_cli):
    result = solite_cli(
        ["--color", "always", "run", "-c", "select 7;"],
        escape_ansi=False,
        env={**TRUECOLOR, "SOLITE_THEME_DARK": "catppuccin-mocha"},
    )
    assert result.success
    assert MOCHA_PEACH in result.stdout


def test_theme_dark_flag_beats_env(solite_cli):
    result = solite_cli(
        [
            "--theme-dark",
            "catppuccin-mocha",
            "--color",
            "always",
            "run",
            "-c",
            "select 7;",
        ],
        escape_ansi=False,
        env={**TRUECOLOR, "SOLITE_THEME_DARK": "terminal"},
    )
    assert result.success
    assert MOCHA_PEACH in result.stdout


def test_theme_dark_pair_no_stray_bytes_when_piped(solite_cli):
    """No OSC 11 query bytes (or anything else unexpected) should ever
    reach stdout/stderr when piped -- only the table's own ANSI escapes."""
    result = solite_cli(
        [
            "--theme-dark",
            "catppuccin-mocha",
            "--theme-light",
            "terminal",
            "--color",
            "always",
            "run",
            "-c",
            "select 7;",
        ],
        escape_ansi=False,
        env=TRUECOLOR,
    )
    assert result.success
    assert result.stderr == ""
    assert "\x1b]11" not in result.stdout  # OSC 11 query/response prefix
    assert "\x1b]11" not in result.stderr


def test_dark_light_pair_detection_is_instant_when_piped(solite_cli):
    """Detection must be lazy and must never block: piped stdin/stdout is
    short-circuited before any OSC 11 query is sent, so this returns almost
    immediately even though the underlying crate's own query has a 1s
    timeout for terminals that don't reply."""
    start = time.monotonic()
    result = solite_cli(
        [
            "--theme-dark",
            "catppuccin-mocha",
            "--color",
            "always",
            "run",
            "-c",
            "select 1;",
        ],
        escape_ansi=False,
        env=TRUECOLOR,
    )
    elapsed = time.monotonic() - start
    assert result.success
    assert elapsed < 1.0


def test_no_pair_configured_is_instant_when_piped(solite_cli):
    """With neither half of the pair set, detection must not run at all."""
    start = time.monotonic()
    result = solite_cli(["run", "-c", "select 1;"], escape_ansi=False)
    elapsed = time.monotonic() - start
    assert result.success
    assert elapsed < 1.0
