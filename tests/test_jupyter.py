import time

from conftest import ensure_sync, escape_ansi_codes


def test_hello(solite_kernel, snapshot):
    reply, output_msgs = solite_kernel.execute("select 1 + 1")
    assert (
        escape_ansi_codes(output_msgs[0]["content"]["data"]["text/plain"]) == snapshot()
    )
    assert output_msgs[0]["content"]["data"]["text/html"] == snapshot()

    reply, output_msgs = solite_kernel.execute("select 1 + ")
    assert output_msgs[0]["content"] == snapshot(name="error contents: incomplete")

    reply, output_msgs = solite_kernel.execute("select substr();")
    assert output_msgs[0]["content"] == snapshot(name="error contents: wrong num args")



def test_is_complete(solite_kernel):
    client = solite_kernel.client

    def status_of(code):
        client.is_complete(code)
        reply = solite_kernel.get_non_kernel_info_reply()
        assert reply["header"]["msg_type"] == "is_complete_reply"
        return reply["content"]["status"]

    assert status_of("select 1;") == "complete"
    assert status_of("select 1 +") == "incomplete"
    assert status_of("select 'unterminated") == "incomplete"
    assert status_of("") == "complete"
    assert status_of(".tables") == "complete"
    assert status_of("!ls") == "complete"
    assert status_of(".export out.csv") == "incomplete"
    assert status_of(".export out.csv\nselect 1;") == "complete"


def test_complete(solite_kernel):
    client = solite_kernel.client

    def complete(code, cursor_pos=None):
        client.complete(code, cursor_pos if cursor_pos is not None else len(code))
        reply = solite_kernel.get_non_kernel_info_reply()
        assert reply["header"]["msg_type"] == "complete_reply"
        return reply["content"]

    # keyword completion at statement start
    content = complete("sel")
    assert "select" in content["matches"]
    assert content["cursor_start"] == 0
    assert content["cursor_end"] == 3

    # table-name completion after FROM, against the live schema
    solite_kernel.execute("create table students(name text, age int);")
    content = complete("select * from stu")
    assert "students" in content["matches"]
    assert content["cursor_start"] == len("select * from ")

    # column completion in a WHERE clause
    content = complete("select * from students where ")
    assert "name" in content["matches"]
    assert "age" in content["matches"]

    # dot command name completion
    content = complete(".ta")
    assert content["matches"] == ["tables"]
    assert content["cursor_start"] == 1

    # nothing sensible to complete still gets a reply
    content = complete("select 1;")
    assert content["status"] == "ok"


def test_execute_input_broadcast(solite_kernel):
    client = solite_kernel.client
    code = "select 42"
    client.execute(code=code, silent=False, store_history=False, stop_on_error=False)
    reply = solite_kernel.get_non_kernel_info_reply()
    assert reply is not None

    saw_execute_input = False
    while True:
        msg = ensure_sync(client.iopub_channel.get_msg)(timeout=5)
        if msg["msg_type"] == "execute_input":
            assert msg["content"]["code"] == code
            assert msg["content"]["execution_count"] >= 1
            saw_execute_input = True
        if msg["msg_type"] == "status" and msg["content"]["execution_state"] == "idle":
            break
    assert saw_execute_input


def test_param_dot_commands(solite_kernel):
    k = solite_kernel

    reply, msgs = k.execute(".param set name alex")
    assert reply["content"]["status"] == "ok"

    # the parameter binds in later cells
    reply, msgs = k.execute("select :name as n")
    assert "alex" in msgs[0]["content"]["data"]["text/html"]

    # .param list renders a table of key/value pairs
    reply, msgs = k.execute(".param list")
    assert reply["content"]["status"] == "ok"
    html = msgs[0]["content"]["data"]["text/html"]
    assert "name" in html and "alex" in html

    # .param unset removes it
    reply, msgs = k.execute(".param unset name")
    assert reply["content"]["status"] == "ok"
    reply, msgs = k.execute(".param list")
    assert "alex" not in str(msgs[0]["content"]["data"])

    # .param clear deletes everything
    k.execute(".param set a 1")
    k.execute(".param set b 2")
    reply, msgs = k.execute(".param clear")
    assert "Cleared 2 parameter(s)" in msgs[0]["content"]["data"]["text/plain"]


def test_timer_dot_command(solite_kernel):
    k = solite_kernel

    def plain_outputs(msgs):
        return [
            m["content"]["data"].get("text/plain", "")
            for m in msgs
            if m["msg_type"] == "display_data"
        ]

    reply, msgs = k.execute(".timer on")
    assert reply["content"]["status"] == "ok"
    reply, msgs = k.execute("select 1")
    assert any(p.startswith("Took ") for p in plain_outputs(msgs))

    reply, msgs = k.execute(".timer off")
    reply, msgs = k.execute("select 1")
    assert not any(p.startswith("Took ") for p in plain_outputs(msgs))


def test_clear_dot_command(solite_kernel):
    reply, msgs = solite_kernel.execute(".clear")
    assert reply["content"]["status"] == "ok"
    assert any(m["msg_type"] == "clear_output" for m in msgs)


def test_dot_command_errors_fail_cell(solite_kernel):
    # failing .open: parent directory doesn't exist
    reply, output_msgs = solite_kernel.execute(".open /nonexistent/dir/db.db")
    assert reply["content"]["status"] == "error"
    errors = [m for m in output_msgs if m["msg_type"] == "error"]
    assert len(errors) == 1
    assert errors[0]["content"]["ename"] == "OpenError"

    # failing .load: not a loadable extension
    reply, output_msgs = solite_kernel.execute(".load not-an-extension")
    assert reply["content"]["status"] == "error"
    errors = [m for m in output_msgs if m["msg_type"] == "error"]
    assert len(errors) == 1
    assert errors[0]["content"]["ename"] == "LoadError"

    # successful dot commands still report ok
    reply, output_msgs = solite_kernel.execute(".tables")
    assert reply["content"]["status"] == "ok"


def test_run_dot_command(solite_kernel, tmp_path):
    sql_file = tmp_path / "queries.sql"
    sql_file.write_text(
        "create table nums(n int);\n"
        "insert into nums values (1), (2);\n"
        "select sum(n) as total from nums;\n"
    )
    reply, output_msgs = solite_kernel.execute(f".run {sql_file}")
    assert reply["content"]["status"] == "ok"
    htmls = [
        m["content"]["data"]["text/html"]
        for m in output_msgs
        if m["msg_type"] == "display_data" and "text/html" in m["content"]["data"]
    ]
    assert any(">3<" in html for html in htmls)

    # a prepare error inside the run file errors the cell, with source context
    bad_file = tmp_path / "bad.sql"
    bad_file.write_text("select * from no_such_table;\n")
    reply, output_msgs = solite_kernel.execute(f".run {bad_file}")
    assert reply["content"]["status"] == "error"
    errors = [m for m in output_msgs if m["msg_type"] == "error"]
    assert len(errors) == 1
    evalue = escape_ansi_codes(errors[0]["content"]["evalue"])
    assert "no such table: no_such_table" in evalue
    assert "bad.sql" in evalue


def test_inspect(solite_kernel):
    client = solite_kernel.client

    def inspect(code, cursor_pos):
        client.inspect(code, cursor_pos)
        reply = solite_kernel.get_non_kernel_info_reply()
        assert reply["header"]["msg_type"] == "inspect_reply"
        return reply["content"]

    solite_kernel.execute("create table users(id integer primary key, name text);")

    # cursor inside 'users' in the FROM clause
    code = "select name from users"
    content = inspect(code, code.index("users") + 2)
    assert content["found"] is True
    assert "users" in content["data"]["text/markdown"]

    # cursor on a column name
    content = inspect(code, code.index("name") + 2)
    assert content["found"] is True
    assert "name" in content["data"]["text/markdown"]

    # unknown symbol still replies, with found=false
    content = inspect("select 1", 7)
    assert content["found"] is False


def test_shutdown(solite_kernel):
    """shutdown_request on the control channel gets a reply and the kernel exits."""
    client = solite_kernel.client
    manager = solite_kernel.manager

    client.shutdown()
    reply = client.get_control_msg(timeout=5)
    assert reply["header"]["msg_type"] == "shutdown_reply"
    assert reply["content"]["status"] == "ok"
    assert reply["content"]["restart"] is False

    # The kernel should exit on its own, without the manager force-killing it.
    deadline = time.time() + 5
    while time.time() < deadline and manager.is_alive():
        time.sleep(0.1)
    assert not manager.is_alive()


def test_interrupt(solite_kernel):
    """Interrupting a long-running query errors the cell but keeps the kernel alive."""
    client = solite_kernel.client
    manager = solite_kernel.manager

    infinite_query = (
        "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c) "
        "SELECT count(*) FROM c;"
    )
    client.execute(
        code=infinite_query, silent=False, store_history=False, stop_on_error=False
    )
    busy_msg = ensure_sync(client.iopub_channel.get_msg)(timeout=5)
    assert busy_msg["content"]["execution_state"] == "busy"

    # Give the statement a moment to start executing before interrupting.
    time.sleep(0.3)
    manager.interrupt_kernel()

    solite_kernel.timeout = 10
    reply = solite_kernel.get_non_kernel_info_reply()
    assert reply["content"]["status"] == "error"

    # Drain iopub until idle so the next execute starts clean.
    while True:
        msg = ensure_sync(client.iopub_channel.get_msg)(timeout=5)
        if msg["msg_type"] == "status" and msg["content"]["execution_state"] == "idle":
            break

    # The kernel must still be usable after the interrupt.
    reply, output_msgs = solite_kernel.execute("select 1 + 1")
    assert reply["content"]["status"] == "ok"
    assert "2" in output_msgs[0]["content"]["data"]["text/html"]


def html_of(msgs):
    return msgs[0]["content"]["data"]["text/html"]


def plain_of(msgs):
    return escape_ansi_codes(msgs[0]["content"]["data"]["text/plain"])


def test_value_rendering(solite_kernel, snapshot):
    k = solite_kernel

    for name, code in [
        ("null", "select null"),
        ("integer", "select 1"),
        ("float", "select 1.5"),
        ("blob", "select x'01ff'"),
        ("multi-row", "select * from (values (1), (2), (3))"),
        ("zero-rows", "select 1 as x where 0"),
    ]:
        reply, msgs = k.execute(code)
        assert reply["content"]["status"] == "ok", code
        assert plain_of(msgs) == snapshot(name=f"{name} plain")

    # HTML special characters are escaped end-to-end
    reply, msgs = k.execute("select 'a < b & c' as v")
    assert "a &lt; b &amp; c" in html_of(msgs)
    assert "a < b & c" in plain_of(msgs)


def test_reply_semantics(solite_kernel):
    k = solite_kernel

    reply, _ = k.execute("select 1")
    first_count = reply["content"]["execution_count"]
    assert reply["content"]["status"] == "ok"

    reply, _ = k.execute("select 2")
    assert reply["content"]["execution_count"] == first_count + 1

    # SQL errors carry the SQLError ename and error status
    reply, msgs = k.execute("select substr();")
    assert reply["content"]["status"] == "error"
    errors = [m for m in msgs if m["msg_type"] == "error"]
    assert errors[0]["content"]["ename"] == "SQLError"


def test_simple_dot_commands(solite_kernel, tmp_path):
    k = solite_kernel

    reply, msgs = k.execute(".print hello world")
    assert plain_of(msgs) == "hello world"

    reply, msgs = k.execute(".help")
    assert reply["content"]["status"] == "ok"
    assert ".tables" in plain_of(msgs)

    reply, msgs = k.execute(".env set SOLITE_TEST_VAR 1")
    assert plain_of(msgs) == "Set environment variable: SOLITE_TEST_VAR"

    reply, msgs = k.execute(f".open {tmp_path}/fresh.db")
    assert reply["content"]["status"] == "ok"
    assert plain_of(msgs).startswith("Opened database at ")


def test_shell_dot_command(solite_kernel):
    reply, msgs = solite_kernel.execute(".sh echo hi")
    assert reply["content"]["status"] == "ok"
    streams = [m for m in msgs if m["msg_type"] == "stream"]
    assert any("hi" in m["content"]["text"] for m in streams)


def test_schema_dot_commands(solite_kernel):
    k = solite_kernel
    k.execute("create table pets(name text, owner_id int);")

    # cross-cell state: the table is visible to later cells
    reply, msgs = k.execute(".tables")
    assert "pets" in plain_of(msgs)

    # .schema output is syntax-highlighted HTML, one span per token
    reply, msgs = k.execute(".schema")
    schema_html = html_of(msgs)
    assert "CREATE" in schema_html and "pets" in schema_html

    reply, msgs = k.execute(".graphviz")
    assert "digraph" in plain_of(msgs)


def test_export_dot_command(solite_kernel, tmp_path):
    out = tmp_path / "out.csv"
    reply, msgs = solite_kernel.execute(f".export {out}\nselect 1 as a;")
    assert reply["content"]["status"] == "ok"
    assert plain_of(msgs).startswith("Export successfully to ")
    assert out.read_text().strip().splitlines() == ["a", "1"]


def test_vegalite_dot_command(solite_kernel):
    reply, msgs = solite_kernel.execute(".vegalite bar\nselect 'a' as x, 1 as y;")
    assert reply["content"]["status"] == "ok"
    datas = [
        m["content"]["data"] for m in msgs if m["msg_type"] == "display_data"
    ]
    assert any(
        any(mime.startswith("application/vnd.vegalite") for mime in data)
        for data in datas
    )


def test_bench_dot_command(solite_kernel):
    reply, msgs = solite_kernel.execute(".bench\nselect 1;")
    assert reply["content"]["status"] == "ok"


def test_procedures(solite_kernel):
    k = solite_kernel

    # defining a procedure produces no output but registers it
    reply, msgs = k.execute("-- name: double :value\nselect 2 * 21 as answer;")
    assert reply["content"]["status"] == "ok"

    # .call in a later cell runs it
    reply, msgs = k.execute(".call double")
    assert reply["content"]["status"] == "ok"
    assert ">42<" in html_of(msgs)


def test_kernel_info(solite_kernel):
    client = solite_kernel.client
    client.kernel_info()
    reply = client.get_shell_msg(timeout=5)
    assert reply["header"]["msg_type"] == "kernel_info_reply"
    content = reply["content"]
    assert content["banner"] == "Solite Kernel"
    assert content["language_info"]["name"] == "sqlite"
    assert content["language_info"]["file_extension"] == ".sql"
