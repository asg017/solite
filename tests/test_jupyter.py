from conftest import escape_ansi_codes


def test_hello(solite_kernel, snapshot):
    reply, output_msgs = solite_kernel.execute("select 1 + 1")
    assert output_msgs[0]["header"]["msg_type"] == "execute_result"
    assert (
        escape_ansi_codes(output_msgs[0]["content"]["data"]["text/plain"]) == snapshot()
    )
    assert output_msgs[0]["content"]["data"]["text/html"] == snapshot()

    reply, output_msgs = solite_kernel.execute("select 1 + ")
    assert output_msgs[0]["header"]["msg_type"] == "error"
    assert output_msgs[0]["content"] == snapshot(name="error contents: incomplete")

    reply, output_msgs = solite_kernel.execute("select substr();")
    assert output_msgs[0]["header"]["msg_type"] == "error"
    assert output_msgs[0]["content"] == snapshot(name="error contents: wrong num args")


def test_multiple(solite_kernel, snapshot):
    reply, output_msgs = solite_kernel.execute(
        """
        drop table if exists t;
        create table t(a,b,c);
        insert into t
          select value, value, value
          from json_each('[99,88,77,66,55]')
          returning *;
      """
    )
    assert len(output_msgs) == 1
    assert output_msgs[0]["header"]["msg_type"] == "execute_result"
    assert (
        escape_ansi_codes(output_msgs[0]["content"]["data"]["text/plain"]) == snapshot()
    )

    reply, output_msgs = solite_kernel.execute("select * from t")
    assert output_msgs[0]["header"]["msg_type"] == "execute_result"
    assert (
        escape_ansi_codes(output_msgs[0]["content"]["data"]["text/plain"]) == snapshot()
    )


# TODO
# - test JSON, numbers, null, etc.
# - error messages
# - test is_complete() messages
# - test banner?
