def test_run_basic(solite_cli, tmp_path):
    (tmp_path / "helper.sql").write_text("select 42;\n")
    (tmp_path / "main.sql").write_text(".timer off\n.run helper.sql\n")
    result = solite_cli(["run", "main.sql"], cwd=tmp_path)
    assert result.success
    assert "42" in result.stdout


def test_run_with_procedure(solite_cli, tmp_path):
    (tmp_path / "procs.sql").write_text(
        "-- name: greet :row\nselect 'hello ' || :name as greeting;\n"
    )
    (tmp_path / "main.sql").write_text(
        ".timer off\n.param set name world\n.run procs.sql greet\n"
    )
    result = solite_cli(["run", "main.sql"], cwd=tmp_path)
    assert result.success
    assert "hello world" in result.stdout


def test_run_with_parameters(solite_cli, tmp_path):
    (tmp_path / "query.sql").write_text("select :name;\n")
    (tmp_path / "main.sql").write_text(".timer off\n.run query.sql --name=alex\n")
    result = solite_cli(["run", "main.sql"], cwd=tmp_path)
    assert result.success
    assert "alex" in result.stdout


def test_run_step_ordering(solite_cli, tmp_path):
    (tmp_path / "helper.sql").write_text("select 'middle';\n")
    (tmp_path / "main.sql").write_text(
        ".timer off\nselect 'before';\n.run helper.sql\nselect 'after';\n"
    )
    result = solite_cli(["run", "main.sql"], cwd=tmp_path)
    assert result.success
    out = result.stdout
    # Verify ordering: before appears before middle, middle before after
    assert out.index("before") < out.index("middle") < out.index("after")


def test_run_dot_commands_in_file(solite_cli, tmp_path):
    (tmp_path / "helper.sql").write_text(".print hello from helper\n")
    (tmp_path / "main.sql").write_text(".timer off\n.run helper.sql\n")
    result = solite_cli(["run", "main.sql"], cwd=tmp_path)
    assert result.success
    assert "hello from helper" in result.stdout


def test_run_nested_files(solite_cli, tmp_path):
    (tmp_path / "c.sql").write_text("select 'c_val';\n")
    (tmp_path / "b.sql").write_text("select 'b_val';\n.run c.sql\n")
    (tmp_path / "main.sql").write_text(
        ".timer off\nselect 'a_val';\n.run b.sql\nselect 'd_val';\n"
    )
    result = solite_cli(["run", "main.sql"], cwd=tmp_path)
    assert result.success
    out = result.stdout
    assert out.index("a_val") < out.index("b_val") < out.index("c_val") < out.index("d_val")


def test_run_recursive_cycle(solite_cli, tmp_path):
    (tmp_path / "a.sql").write_text(".run b.sql\n")
    (tmp_path / "b.sql").write_text(".run a.sql\n")
    (tmp_path / "main.sql").write_text(".timer off\n.run a.sql\n")
    result = solite_cli(["run", "main.sql"], cwd=tmp_path)
    assert "cycle" in result.stderr.lower()


def test_run_self_reference(solite_cli, tmp_path):
    (tmp_path / "self.sql").write_text(".run self.sql\n")
    (tmp_path / "main.sql").write_text(".timer off\n.run self.sql\n")
    result = solite_cli(["run", "main.sql"], cwd=tmp_path)
    assert "cycle" in result.stderr.lower()


def test_run_deep_cycle(solite_cli, tmp_path):
    (tmp_path / "a.sql").write_text(".run b.sql\n")
    (tmp_path / "b.sql").write_text(".run c.sql\n")
    (tmp_path / "c.sql").write_text(".run a.sql\n")
    (tmp_path / "main.sql").write_text(".timer off\n.run a.sql\n")
    result = solite_cli(["run", "main.sql"], cwd=tmp_path)
    assert "cycle" in result.stderr.lower()


def test_run_file_not_found(solite_cli, tmp_path):
    (tmp_path / "main.sql").write_text(".timer off\n.run nonexistent.sql\n")
    result = solite_cli(["run", "main.sql"], cwd=tmp_path)
    assert "Failed to read" in result.stderr


def test_run_params_scoped(solite_cli, tmp_path):
    (tmp_path / "helper.sql").write_text("select :name;\n")
    (tmp_path / "main.sql").write_text(
        ".timer off\n.run helper.sql --name=scoped\nselect :name;\n"
    )
    result = solite_cli(["run", "main.sql"], cwd=tmp_path)
    assert result.success
    assert "scoped" in result.stdout


def test_run_procedure_with_params(solite_cli, tmp_path):
    (tmp_path / "procs.sql").write_text(
        "-- name: greet :row\nselect 'hi ' || :name as msg;\n"
    )
    (tmp_path / "main.sql").write_text(
        ".timer off\n.param set name world\n.run procs.sql greet\n"
    )
    result = solite_cli(["run", "main.sql"], cwd=tmp_path)
    assert result.success
    assert "hi world" in result.stdout
