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


def test_database_flag_seeds_fixture(solite_cli, tmp_path):
    import sqlite3

    fixture = tmp_path / "fixture.db"
    conn = sqlite3.connect(fixture)
    conn.execute("CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT)")
    conn.execute("INSERT INTO users VALUES (1, 'Alice'), (2, 'Bob')")
    conn.commit()
    conn.close()
    before = fixture.read_bytes()

    test_file = write(
        tmp_path / "seeded.sql",
        "INSERT INTO users VALUES (3, 'Carol');\n"
        "SELECT COUNT(*) FROM users; -- 3\n",
    )
    result = solite_cli(
        ["test", "--database", str(fixture), str(test_file)], cwd=tmp_path
    )
    assert result.success
    # copy-on-open: the fixture itself is never modified
    assert fixture.read_bytes() == before


def test_database_flag_missing_file_errors(solite_cli, tmp_path):
    test_file = write(tmp_path / "t.sql", "SELECT 1; -- 1\n")
    result = solite_cli(
        ["test", "--database", str(tmp_path / "nope.db"), str(test_file)],
        cwd=tmp_path,
    )
    assert not result.success
    assert "nope.db" in result.stderr


# ---- exit codes and diagnostics ----


def test_passing_file_exits_zero(solite_cli, tmp_path):
    f = write(
        tmp_path / "pass.sql",
        "CREATE TABLE users(id INTEGER, name TEXT);\n"
        "INSERT INTO users VALUES (1, 'Alice');\n"
        "SELECT COUNT(*) FROM users; -- 1\n"
        "SELECT name FROM users; -- 'Alice'\n",
    )
    result = solite_cli(["test", str(f)], cwd=tmp_path)
    assert result.success
    assert "2 successes" in result.stdout
    assert "0 failures" in result.stdout


def test_failing_assertion_prints_codespan_diagnostic(solite_cli, tmp_path):
    f = write(tmp_path / "fail.sql", "SELECT 1 + 1; -- 3\n")
    result = solite_cli(["test", str(f)], cwd=tmp_path)
    assert not result.success
    assert "expected: 3" in result.stderr
    assert "actual: 2" in result.stderr
    assert "fail.sql:1" in result.stderr


def test_todo_fails_run_and_lists_location(solite_cli, tmp_path):
    f = write(tmp_path / "todo.sql", "SELECT 1; -- TODO fix later\n")
    result = solite_cli(["test", str(f)], cwd=tmp_path)
    assert not result.success
    assert "1 TODO" in result.stdout
    assert "todo.sql:1:1" in result.stdout
    assert "TODO fix later" in result.stdout


def test_no_results_assertion_pass_and_fail(solite_cli, tmp_path):
    ok = write(
        tmp_path / "ok.sql",
        "CREATE TABLE t(x);\nSELECT * FROM t; -- [no results]\n",
    )
    assert solite_cli(["test", str(ok)], cwd=tmp_path).success

    bad = write(
        tmp_path / "bad.sql",
        "CREATE TABLE u(x);\nINSERT INTO u VALUES (1);\n"
        "SELECT * FROM u; -- [no results]\n",
    )
    result = solite_cli(["test", str(bad)], cwd=tmp_path)
    assert not result.success


def test_error_assertion_pass_and_wrong_message(solite_cli, tmp_path):
    ok = write(
        tmp_path / "err_ok.sql",
        "CREATE TABLE t(x UNIQUE);\nINSERT INTO t VALUES (1);\n"
        "INSERT INTO t VALUES (1); -- error: UNIQUE constraint failed: t.x\n",
    )
    assert solite_cli(["test", str(ok)], cwd=tmp_path).success

    bad = write(
        tmp_path / "err_bad.sql",
        "CREATE TABLE u(x UNIQUE);\nINSERT INTO u VALUES (1);\n"
        "INSERT INTO u VALUES (1); -- error: wrong message\n",
    )
    result = solite_cli(["test", str(bad)], cwd=tmp_path)
    assert not result.success
    assert "UNIQUE constraint failed" in result.stderr


def test_verbose_prints_expected_vs_got_for_error_mismatch(solite_cli, tmp_path):
    f = write(
        tmp_path / "v.sql",
        "CREATE TABLE t(x UNIQUE);\nINSERT INTO t VALUES (1);\n"
        "INSERT INTO t VALUES (1); -- error: wrong message\n",
    )
    result = solite_cli(["test", "--verbose", str(f)], cwd=tmp_path)
    assert not result.success
    assert "Expected error: 'wrong message'" in result.stderr


# ---- snapshot flows ----


def test_snapshot_lifecycle(solite_cli, tmp_path):
    f = write(tmp_path / "snap.sql", "SELECT 42; -- @snap answer\n")
    snap_file = tmp_path / "__snapshots__" / "snap-answer.snap"

    # default mode with no snapshot: fail and announce
    result = solite_cli(["test", str(f)], cwd=tmp_path)
    assert not result.success
    assert "New snapshot" in result.stdout

    # --update creates it
    result = solite_cli(["test", "--update", str(f)], cwd=tmp_path)
    assert result.success
    assert snap_file.exists()
    assert "42" in snap_file.read_text()

    # rerun in default mode: passes
    result = solite_cli(["test", str(f)], cwd=tmp_path)
    assert result.success
    assert "1 snapshot passed" in result.stdout

    # drop the directive: orphan warns in default mode, stays on disk
    write(tmp_path / "snap.sql", "SELECT 42; -- 42\n")
    result = solite_cli(["test", str(f)], cwd=tmp_path)
    assert result.success
    assert "orphaned snapshot" in result.stderr
    assert snap_file.exists()

    # --update removes the orphan
    result = solite_cli(["test", "--update", str(f)], cwd=tmp_path)
    assert result.success
    assert not snap_file.exists()


def test_snapshot_mismatch_fails_with_diff(solite_cli, tmp_path):
    f = write(tmp_path / "snap.sql", "SELECT 1; -- @snap val\n")
    assert solite_cli(["test", "--update", str(f)], cwd=tmp_path).success

    write(tmp_path / "snap.sql", "SELECT 2; -- @snap val\n")
    result = solite_cli(["test", str(f)], cwd=tmp_path)
    assert not result.success
    assert "Snapshot mismatch" in result.stdout


# ---- the examples printed in --help must actually work ----


def test_help_examples_actually_work(solite_cli, tmp_path):
    """Run the assertion block from `solite test --help` as a real test
    file. The TODO example fails the run by policy; everything else must
    pass exactly as documented."""
    help_out = solite_cli(["test", "--help"]).stdout
    examples = [
        line.strip()
        for line in help_out.splitlines()
        if line.strip().startswith("SELECT") and ";" in line
    ]
    assert len(examples) == 7, f"help example block changed: {examples}"

    setup = (
        "CREATE TABLE empty(x);\n"
        "CREATE TABLE users(id INTEGER, name TEXT);\n"
        "INSERT INTO users VALUES (1, 'Ada');\n"
    )
    f = write(tmp_path / "examples.sql", setup + "\n".join(examples) + "\n")
    result = solite_cli(["test", "--update", str(f)], cwd=tmp_path)

    # the TODO line fails the run by policy; nothing else may fail
    assert not result.success
    assert "5 successes" in result.stdout
    assert "0 failures" in result.stdout
    assert "1 TODO" in result.stdout
    assert (tmp_path / "__snapshots__" / "examples-all-users.snap").exists()
