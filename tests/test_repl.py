import re

def test_repl(solite_cli, snapshot):
    output = solite_cli([], communicate=[b".timer off\nselect 1 + 1;\n"], kill=True).stdout
    output = re.sub(r"Solite \d+\.\d+\.\d+(-[a-z]+\.\d+)?", "Solite VERSION", output)
    assert output == snapshot
