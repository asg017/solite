from pathlib import Path

import pytest


def test_docs_inline_without_extension(solite_cli, tmp_path):
    """docs inline must work without --extension (regression: it used to fail
    with "no such table: solite_docs.solite_docs_loaded_functions")."""
    doc = tmp_path / "doc.md"
    doc.write_text("# Demo\n\n```sql\nSELECT 1 + 1;\n```\n")

    result = solite_cli(["docs", "inline", str(doc)], cwd=tmp_path)
    assert result.success, result.stderr
    assert "no such table" not in result.stderr
    assert "-- 2" in result.stdout


def test_docs_inline_output_file(solite_cli, tmp_path):
    doc = tmp_path / "doc.md"
    doc.write_text("# Demo\n\n```sql\nSELECT 'hello' AS greeting;\n```\n")

    result = solite_cli(
        ["docs", "inline", str(doc), "--output", "out.md"], cwd=tmp_path
    )
    assert result.success, result.stderr
    assert "'hello'" in (tmp_path / "out.md").read_text()


def test_docs_inline_skips_non_sql_blocks(solite_cli, tmp_path):
    """Only ```sql blocks are executed; other languages and untagged blocks
    are left untouched (regression: every fenced block used to run as SQL)."""
    doc = tmp_path / "doc.md"
    doc.write_text(
        "# Demo\n"
        "\n"
        "```python\n"
        'print("hello")\n'
        "```\n"
        "\n"
        "```\n"
        "plain text, not sql\n"
        "```\n"
        "\n"
        "```sql\n"
        "SELECT 1 + 1;\n"
        "```\n"
    )

    result = solite_cli(["docs", "inline", str(doc)], cwd=tmp_path)
    assert result.success, result.stderr
    assert 'print("hello")' in result.stdout
    assert "plain text, not sql" in result.stdout
    assert "-- 2" in result.stdout


def test_docs_inline_sameline_statements(solite_cli, tmp_path):
    """Two statements on one line both survive with their own result
    comments (regression: the second statement used to be swallowed into
    the first statement's `--` result comment)."""
    doc = tmp_path / "doc.md"
    doc.write_text("# Demo\n\n```sql\nSELECT 1; SELECT 2;\n```\n")

    result = solite_cli(
        ["docs", "inline", str(doc), "--output", "out1.md"], cwd=tmp_path
    )
    assert result.success, result.stderr
    out1 = (tmp_path / "out1.md").read_text()
    assert "SELECT 1;\n-- 1\nSELECT 2;\n-- 2" in out1

    # Re-running on the output is stable (same statements, same results)
    result = solite_cli(
        ["docs", "inline", str(tmp_path / "out1.md"), "--output", "out2.md"],
        cwd=tmp_path,
    )
    assert result.success, result.stderr
    assert (tmp_path / "out2.md").read_text() == out1


def test_docs_inline_adjacent_statements_no_blank_line(solite_cli, tmp_path):
    """Statements adjacent in the source stay adjacent in the output (no
    gratuitous blank-line churn between them)."""
    doc = tmp_path / "doc.md"
    doc.write_text("# Demo\n\n```sql\nSELECT 1;\nSELECT 2;\n```\n")

    result = solite_cli(["docs", "inline", str(doc)], cwd=tmp_path)
    assert result.success, result.stderr
    assert "SELECT 1;\n-- 1\nSELECT 2;\n-- 2" in result.stdout


def test_docs_inline_comment_breakout(solite_cli, tmp_path):
    """A result value containing `*/` cannot terminate the generated block
    comment early; the table falls back to `-- ` line comments."""
    doc = tmp_path / "doc.md"
    doc.write_text(
        "# Demo\n\n```sql\nSELECT '*/ hello' AS a UNION ALL SELECT 'b';\n```\n"
    )

    result = solite_cli(["docs", "inline", str(doc)], cwd=tmp_path)
    assert result.success, result.stderr
    # No block comment is opened, and every table line is a line comment
    assert "/*" not in result.stdout
    assert "-- │ '*/ hello' │" in result.stdout


def test_docs_inline_multiline_value_stays_commented(solite_cli, tmp_path):
    """A single-value result containing a newline cannot break out of the
    `-- ` comment: every line of the value gets its own `-- ` prefix
    (regression: `SELECT char(10) || ...` used to emit an unprefixed raw
    line, making the block invalid SQL and a rerun fail)."""
    doc = tmp_path / "doc.md"
    doc.write_text(
        "# Demo\n\n```sql\nSELECT char(10) || char(39) || char(120);\n```\n"
    )

    result = solite_cli(
        ["docs", "inline", str(doc), "--output", "out1.md"], cwd=tmp_path
    )
    assert result.success, result.stderr
    out1 = (tmp_path / "out1.md").read_text()
    # The value renders as a quoted literal split over two lines; both
    # lines carry the comment prefix
    assert "-- '\n-- ''x'" in out1
    # No line between the fences escapes the comment prefix
    block = out1.split("```sql\n")[1].split("```")[0]
    for line in block.splitlines():
        assert line.startswith(("SELECT", "--")), line

    # Re-running on the output is byte-stable and exits 0
    result = solite_cli(
        ["docs", "inline", str(tmp_path / "out1.md"), "--output", "out2.md"],
        cwd=tmp_path,
    )
    assert result.success, result.stderr
    assert (tmp_path / "out2.md").read_text() == out1


def test_docs_inline_error_report(solite_cli, tmp_path):
    """A failing statement cites the markdown file (not the literal `TODO`)
    and the error message appears exactly once, with no Debug dump."""
    doc = tmp_path / "err.md"
    doc.write_text("# Demo\n\n```sql\nSELECT * FROM no_such_table;\n```\n")

    result = solite_cli(["docs", "inline", str(doc)], cwd=tmp_path)
    assert not result.success
    assert "TODO" not in result.stderr
    assert str(doc) in result.stderr
    assert result.stderr.count("no such table: no_such_table") == 1
    assert "SQLiteError {" not in result.stderr


def test_docs_inline_anchor_idempotent(solite_cli, tmp_path):
    """Re-running docs inline over its own output keeps exactly one
    `{#anchor}` per heading, with underscores intact (regression: a second
    pass used to append another anchor and escape the first)."""
    doc = tmp_path / "doc.md"
    doc.write_text("### `my_func(a, b)`\n\nbody\n")

    result = solite_cli(
        ["docs", "inline", str(doc), "--output", "out1.md"], cwd=tmp_path
    )
    assert result.success, result.stderr
    out1 = (tmp_path / "out1.md").read_text()
    assert out1.count("{#my_func}") == 1
    assert r"\_" not in out1

    result = solite_cli(
        ["docs", "inline", str(tmp_path / "out1.md"), "--output", "out2.md"],
        cwd=tmp_path,
    )
    assert result.success, result.stderr
    out2 = (tmp_path / "out2.md").read_text()
    assert out2 == out1
    assert out2.count("{#my_func}") == 1


def test_docs_inline_anchor_self_heals(solite_cli, tmp_path):
    """A stale anchor from a renamed heading is replaced, not accumulated."""
    doc = tmp_path / "doc.md"
    doc.write_text("### `renamed_fn(x)` {#old_name}\n\nbody\n")

    result = solite_cli(["docs", "inline", str(doc)], cwd=tmp_path)
    assert result.success, result.stderr
    assert "{#renamed_fn}" in result.stdout
    assert "{#old_name}" not in result.stdout


def test_docs_inline_unicode_table_alignment(solite_cli, tmp_path):
    """Multibyte text pads by display width, so table borders stay aligned
    (regression: widths were computed from byte lengths)."""
    doc = tmp_path / "doc.md"
    doc.write_text(
        "# Demo\n\n```sql\n"
        "SELECT 'héllo wörld' AS a UNION ALL SELECT 'plain ascii x';\n"
        "```\n"
    )

    result = solite_cli(["docs", "inline", str(doc)], cwd=tmp_path)
    assert result.success, result.stderr
    table_lines = [l for l in result.stdout.splitlines() if "│" in l or "─" in l]
    assert table_lines, result.stdout
    assert len({len(l) for l in table_lines}) == 1, result.stdout


def test_docs_inline_no_json_prefix(solite_cli, tmp_path):
    """Single JSON values render as plain SQL strings, without the snapshot
    harness's `(json)` prefix."""
    doc = tmp_path / "doc.md"
    doc.write_text("# Demo\n\n```sql\nSELECT json_object('a', 1);\n```\n")

    result = solite_cli(["docs", "inline", str(doc)], cwd=tmp_path)
    assert result.success, result.stderr
    assert "(json)" not in result.stdout
    assert "-- '{\"a\":1}'" in result.stdout


def test_docs_inline_gfm_table_and_strikethrough(solite_cli, tmp_path):
    """GFM tables and strikethrough no longer crash serialization and are
    preserved byte-for-byte (regression: the old AST re-serializer had no
    GFM handlers and hard-failed)."""
    doc = tmp_path / "doc.md"
    table = "| col | desc |\n|-----|------|\n| a   | ~~old~~ new |\n"
    doc.write_text("# Demo\n\n" + table + "\n```sql\nSELECT 1 + 1;\n```\n")

    result = solite_cli(["docs", "inline", str(doc)], cwd=tmp_path)
    assert result.success, result.stderr
    assert table in result.stdout
    assert "-- 2" in result.stdout


def test_docs_inline_frontmatter(solite_cli, tmp_path):
    """YAML frontmatter is preserved and does not crash serialization."""
    doc = tmp_path / "doc.md"
    fm = "---\ntitle: My Extension\n---\n"
    doc.write_text(fm + "\n# Demo\n\n```sql\nSELECT 1 + 1;\n```\n")

    result = solite_cli(["docs", "inline", str(doc)], cwd=tmp_path)
    assert result.success, result.stderr
    assert fm in result.stdout
    assert "-- 2" in result.stdout


def test_docs_inline_no_reformatting_churn(solite_cli, tmp_path):
    """Content outside code blocks comes back byte-for-byte: no `-` → `*`
    bullet rewrites and no `\\_` escaping of prose underscores."""
    doc = tmp_path / "doc.md"
    prose = (
        "# Demo\n\n"
        "- bullet one\n"
        "- bullet two\n\n"
        "prose with an_underscore and snake_case words\n\n"
        "[^1]: a footnote\n\n"
    )
    doc.write_text(prose + "```sql\nSELECT 1 + 1;\n```\n")

    result = solite_cli(
        ["docs", "inline", str(doc), "--output", "out.md"], cwd=tmp_path
    )
    assert result.success, result.stderr
    out = (tmp_path / "out.md").read_text()
    assert out.startswith(prose)
    assert "\\_" not in out
    assert "* bullet" not in out


def test_docs_inline_nested_code_block(solite_cli, tmp_path):
    """```sql blocks nested in containers (list items) are processed too,
    preserving their indentation."""
    doc = tmp_path / "doc.md"
    doc.write_text(
        "# Demo\n\n- a list item\n\n  ```sql\n  SELECT 'in-list';\n  ```\n"
    )

    result = solite_cli(["docs", "inline", str(doc)], cwd=tmp_path)
    assert result.success, result.stderr
    assert "  -- 'in-list'" in result.stdout


def test_docs_inline_empty_sql_block(solite_cli, tmp_path):
    """An empty ```sql block keeps its closing fence and the rest of the
    document (regression: the fence was treated as the block's interior and
    deleted, silently absorbing everything after it into an unclosed code
    block)."""
    doc = tmp_path / "doc.md"
    src = "# Demo\n\n```sql\n```\n\nAfter the block.\n\n```sql\nSELECT 1;\n```\n"
    doc.write_text(src)

    result = solite_cli(
        ["docs", "inline", str(doc), "--output", "out1.md"], cwd=tmp_path
    )
    assert result.success, result.stderr
    out1 = (tmp_path / "out1.md").read_text()
    # The empty block and the document after it survive byte-for-byte
    assert "```sql\n```\n\nAfter the block.\n" in out1
    assert "SELECT 1;\n-- 1" in out1

    # Second run is idempotent
    result = solite_cli(
        ["docs", "inline", str(tmp_path / "out1.md"), "--output", "out2.md"],
        cwd=tmp_path,
    )
    assert result.success, result.stderr
    assert (tmp_path / "out2.md").read_text() == out1


def test_docs_inline_empty_sql_block_crlf(solite_cli, tmp_path):
    """Same as above for a CRLF document: the closing fence's only
    preceding newline is the opening fence's own."""
    doc = tmp_path / "doc.md"
    src = "# Demo\r\n\r\n```sql\r\n```\r\n\r\nAfter the block.\r\n"
    doc.write_bytes(src.encode("utf8"))

    result = solite_cli(
        ["docs", "inline", str(doc), "--output", "out1.md"], cwd=tmp_path
    )
    assert result.success, result.stderr
    # Nothing to inline, so the whole document is unchanged byte-for-byte
    assert (tmp_path / "out1.md").read_bytes() == src.encode("utf8")

    result = solite_cli(
        ["docs", "inline", str(tmp_path / "out1.md"), "--output", "out2.md"],
        cwd=tmp_path,
    )
    assert result.success, result.stderr
    assert (tmp_path / "out2.md").read_bytes() == src.encode("utf8")


def test_docs_inline_blockquoted_sql_block_untouched(solite_cli, tmp_path):
    """```sql blocks inside blockquotes are passed through byte-for-byte:
    the `> ` line prefix breaks fence detection and re-indentation, so they
    are not executed or edited (matching pre-splice behavior)."""
    doc = tmp_path / "doc.md"
    quoted = "> ```sql\n> SELECT 'quoted';\n> ```\n"
    src = "# Demo\n\n" + quoted + "\nAfter the quote.\n\n```sql\nSELECT 2;\n```\n"
    doc.write_text(src)

    result = solite_cli(
        ["docs", "inline", str(doc), "--output", "out1.md"], cwd=tmp_path
    )
    assert result.success, result.stderr
    out1 = (tmp_path / "out1.md").read_text()
    assert quoted in out1
    assert "'quoted'" not in out1.replace(quoted, "")  # not executed
    assert "After the quote.\n" in out1
    assert "SELECT 2;\n-- 2" in out1

    result = solite_cli(
        ["docs", "inline", str(tmp_path / "out1.md"), "--output", "out2.md"],
        cwd=tmp_path,
    )
    assert result.success, result.stderr
    assert (tmp_path / "out2.md").read_text() == out1


# --- baseline behaviors -----------------------------------------------------


def test_docs_inline_multirow_table(solite_cli, tmp_path):
    doc = tmp_path / "doc.md"
    doc.write_text("# Demo\n\n```sql\nSELECT 1 AS n UNION ALL SELECT 2;\n```\n")

    result = solite_cli(["docs", "inline", str(doc)], cwd=tmp_path)
    assert result.success, result.stderr
    assert "/*" in result.stdout and "*/" in result.stdout
    assert "┌" in result.stdout and "└" in result.stdout


def test_docs_inline_zero_rows(solite_cli, tmp_path):
    doc = tmp_path / "doc.md"
    doc.write_text("# Demo\n\n```sql\nSELECT 1 WHERE 0;\n```\n")

    result = solite_cli(["docs", "inline", str(doc)], cwd=tmp_path)
    assert result.success, result.stderr
    assert "No results" in result.stdout


def test_docs_inline_error_writes_no_output_file(solite_cli, tmp_path):
    doc = tmp_path / "doc.md"
    doc.write_text("# Demo\n\n```sql\nSELECT * FROM no_such_table;\n```\n")

    result = solite_cli(
        ["docs", "inline", str(doc), "--output", "out.md"], cwd=tmp_path
    )
    assert not result.success
    assert not (tmp_path / "out.md").exists()


def test_docs_inline_in_place_regeneration_stable(solite_cli, tmp_path):
    """`--output` pointing back at the input (the natural regeneration
    workflow) is stable across runs."""
    doc = tmp_path / "doc.md"
    doc.write_text(
        "### `my_func(a, b)`\n\n```sql\nSELECT 1; SELECT 2;\n```\n"
    )

    result = solite_cli(
        ["docs", "inline", str(doc), "--output", str(doc)], cwd=tmp_path
    )
    assert result.success, result.stderr
    first = doc.read_text()

    result = solite_cli(
        ["docs", "inline", str(doc), "--output", str(doc)], cwd=tmp_path
    )
    assert result.success, result.stderr
    assert doc.read_text() == first
    assert first.count("{#my_func}") == 1


# --- --extension paths ------------------------------------------------------


def _sqlite_include_dir():
    """Directory holding sqlite3.h + sqlite3ext.h (the amalgamation)."""
    import os

    candidates = []
    env = os.environ.get("SOLITE_AMALGAMMATION_DIR")
    if env:
        candidates.append(Path(env))
    candidates.append(Path(__file__).resolve().parent.parent / "vendor" / "sqlite")
    for candidate in candidates:
        if (candidate / "sqlite3.h").exists() and (candidate / "sqlite3ext.h").exists():
            return candidate
    return None


@pytest.fixture(scope="session")
def docs_extension(tmp_path_factory):
    """Compile tests/fixtures/docsext.c into a loadable extension, skipping
    when no C compiler or amalgamation headers are available."""
    import shutil
    import subprocess
    import sys

    cc = shutil.which("cc") or shutil.which("gcc") or shutil.which("clang")
    if cc is None:
        pytest.skip("no C compiler available")
    include_dir = _sqlite_include_dir()
    if include_dir is None:
        pytest.skip("no SQLite amalgamation headers available")

    suffix = ".dylib" if sys.platform == "darwin" else ".so"
    out = tmp_path_factory.mktemp("docsext") / f"docsext{suffix}"
    src = Path(__file__).resolve().parent / "fixtures" / "docsext.c"
    subprocess.run(
        [cc, "-fPIC", "-shared", "-I", str(include_dir), "-o", str(out), str(src)],
        check=True,
    )
    return out


def test_docs_inline_extension_documented(solite_cli, tmp_path, docs_extension):
    """--extension happy path: every registered function has a heading."""
    doc = tmp_path / "doc.md"
    doc.write_text(
        "# My Extension\n\n"
        "### `documented_func(a, b)`\n\n"
        "```sql\nSELECT documented_func(1, 2);\n```\n\n"
        "### `undocumented_func()`\n\nAlso documented after all.\n"
    )

    result = solite_cli(
        ["docs", "inline", str(doc), "--extension", str(docs_extension)],
        cwd=tmp_path,
    )
    assert result.success, result.stderr
    assert "-- 3" in result.stdout


def test_docs_inline_extension_undocumented(solite_cli, tmp_path, docs_extension):
    """--extension failure path: an undocumented function fails the run and
    is listed exactly once despite being registered with two arities."""
    doc = tmp_path / "doc.md"
    doc.write_text(
        "# My Extension\n\n"
        "### `documented_func(a, b)`\n\n"
        "```sql\nSELECT documented_func(1, 2);\n```\n"
    )

    result = solite_cli(
        ["docs", "inline", str(doc), "--extension", str(docs_extension)],
        cwd=tmp_path,
    )
    assert not result.success
    assert result.stderr.count("undocumented_func") == 1
    assert "documented_func(a, b)" not in result.stderr
