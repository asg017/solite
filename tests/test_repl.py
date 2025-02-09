import re

def repl(solite_cli, commands):
    msg = "\n".join(commands) + "\n"
    print(msg.encode())
    result = solite_cli([], communicate=[msg.encode()], kill=True)
    stdout = re.sub(r"Solite \d+\.\d+\.\d+(-[a-z]+\.\d+)?", "Solite REDACTED", result.stdout)
    stderr = result.stderr
    return {"stdout": stdout, "stderr": stderr}

def test_repl(solite_cli, snapshot):
    output = solite_cli([], communicate=[b".timer off\nselect 1 + 1;\n"], kill=True).stdout
    output = re.sub(r"Solite \d+\.\d+\.\d+(-[a-z]+\.\d+)?", "Solite VERSION", output)
    assert output == snapshot

def test_err(solite_cli, snapshot):
    assert repl(solite_cli, ["select xxx();"]) == snapshot