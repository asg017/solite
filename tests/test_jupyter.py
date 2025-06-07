from conftest import escape_ansi_codes


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



# TODO
# - test JSON, numbers, null, etc.
# - error messages
# - test is_complete() messages
# - test banner?
