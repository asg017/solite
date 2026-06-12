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
