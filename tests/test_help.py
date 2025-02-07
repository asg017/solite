def test_help(solite_cli, snapshot):
    assert solite_cli(["--help"]).stdout == snapshot(name="--help")
    assert solite_cli(["run", "--help"]).stdout == snapshot(name="run --help")
    assert solite_cli(["query", "--help"]).stdout == snapshot(name="query --help")
    assert solite_cli(["repl", "--help"]).stdout == snapshot(name="repl --help")
    assert solite_cli(["jupyter", "--help"]).stdout == snapshot(name="jupyter --help")
