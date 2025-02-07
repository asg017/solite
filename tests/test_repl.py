def test_repl(solite_cli, snapshot):
    assert (
        solite_cli([], communicate=[b".timer off\nselect 1 + 1;\n"], kill=True).stdout
        == snapshot
    )
