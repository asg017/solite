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
