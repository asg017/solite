import pytest
from mcp import McpError
from fastmcp import Client

class TestMcp:
    def __init__(self, mcp_client: Client, snapshot):
        self.mcp_client = mcp_client
    
    async def execute_sql(self, sql: str, snapshot=None, name: str | None = None):
        result = await self.mcp_client.call_tool("execute_sql", {"sql": sql})
        if snapshot is not None:
            text_content = "\n".join([x.text for x in result.content])
            assert text_content == snapshot(name=name)
        return result

@pytest.mark.asyncio
async def test_mcp(mcp_client: Client, snapshot):
    tools = await mcp_client.list_tools()
    assert sorted([tool.name for tool in tools]) == snapshot(name="list_tools")
    resources = await mcp_client.list_resources()
    assert resources == snapshot(name="list_resources")
    prompts = await mcp_client.list_prompts()
    assert prompts == snapshot(name="list_prompts")

    t = TestMcp(mcp_client, snapshot)
    await t.execute_sql("select 1 + 1", snapshot=snapshot, name="select 1 + 1")
    await t.execute_sql("create table t as select value from json_each('[1, 2, 3]');")
    await t.execute_sql("select * from t", snapshot=snapshot, name="select * from t")
    
    with pytest.raises(McpError):
        await t.execute_sql("select error;")