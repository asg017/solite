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


# TODO
# - test JSON, numbers, null, etc.
# - error messages
# - test is_complete() messages
# - test banner?
