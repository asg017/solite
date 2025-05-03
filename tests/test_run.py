def test_run_basic(solite_cli, snapshot, tmp_path):
    (tmp_path / "a.sql").write_text(
        """
.timer off

select 1;

select 2;

select 3
""",
        newline="\n",
    )
    assert solite_cli(["run", "a.sql"], cwd=tmp_path).stdout == snapshot


def test_run_basic_dots(solite_cli, snapshot, tmp_path):
    (tmp_path / "a.sql").write_text(
        """
.timer off

select 1;

.print yo

.print yo2
""",
        newline="\n",
    )
    assert solite_cli(["run", "a.sql"], cwd=tmp_path).stdout == snapshot


def test_run_error(solite_cli, snapshot, tmp_path):
    (tmp_path / "a.sql").write_text(
        """
.timer off

select 'hello' as world;
select 1 + 1 as result;

select
  1,
  2,
  3,
  substr(),
  4,
  5,
  6;

""",
        newline="\n",
    )
    assert solite_cli(["run", "a.sql"], cwd=tmp_path).stdout == snapshot(name="stdout")
    assert solite_cli(["run", "a.sql"], cwd=tmp_path).stderr == snapshot(name="stderr")
