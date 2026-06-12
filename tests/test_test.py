# CLI-level tests for `solite test`.


def write(path, text):
    path.write_text(text)
    return path


def test_multi_file_all_passing(solite_cli, tmp_path):
    a = write(tmp_path / "a.sql", "SELECT 1; -- 1\n")
    b = write(tmp_path / "b.sql", "SELECT 2; -- 2\n")
    result = solite_cli(["test", str(a), str(b)], cwd=tmp_path)
    assert result.success
    assert "2 successes" in result.stdout
    # per-file attribution
    assert "a.sql" in result.stdout
    assert "b.sql" in result.stdout


def test_multi_file_one_failing_exits_nonzero(solite_cli, tmp_path):
    a = write(tmp_path / "a.sql", "SELECT 1; -- 1\n")
    b = write(tmp_path / "b.sql", "SELECT 2; -- 999\n")
    result = solite_cli(["test", str(a), str(b)], cwd=tmp_path)
    assert not result.success
    assert "1 failures" in result.stdout


def test_directory_argument_runs_all_sql_files(solite_cli, tmp_path):
    d = tmp_path / "suite"
    d.mkdir()
    write(d / "a.sql", "SELECT 1; -- 1\n")
    write(d / "b.sql", "SELECT 2; -- 2\n")
    (d / "notes.txt").write_text("not sql")
    result = solite_cli(["test", str(d)], cwd=tmp_path)
    assert result.success
    assert "2 successes" in result.stdout


def test_directory_with_failure_exits_nonzero(solite_cli, tmp_path):
    d = tmp_path / "suite"
    d.mkdir()
    write(d / "a.sql", "SELECT 1; -- 1\n")
    write(d / "b.sql", "SELECT 2; -- 999\n")
    result = solite_cli(["test", str(d)], cwd=tmp_path)
    assert not result.success


def test_no_files_is_a_usage_error(solite_cli):
    result = solite_cli(["test"])
    assert not result.success
    assert result.stderr.strip() != ""
