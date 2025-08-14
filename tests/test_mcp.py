import pytest
from mcp import McpError
from mcp.types import TextContent, EmbeddedResource, BlobResourceContents
from fastmcp import Client
import base64
import sqlite3

class TestMcp:
    def __init__(self, mcp_client: Client, snapshot):
        self.mcp_client = mcp_client
    
    async def execute_sql(self, sql: str, snapshot=None, name: str | None = None):
        result = await self.mcp_client.call_tool("execute_sql", {"sql": sql})
        if snapshot is not None:
            text_content = ""
            for x in result.content:
                assert isinstance(x, TextContent)
                text_content += x.text + "\n"
            assert text_content == snapshot(name=name)
        return result

@pytest.mark.asyncio
async def test_mcp_meta(mcp_client: Client, snapshot):
    tools = await mcp_client.list_tools()
    assert sorted([tool.name for tool in tools]) == snapshot(name="list_tools")
    resources = await mcp_client.list_resources()
    assert resources == snapshot(name="list_resources")
    prompts = await mcp_client.list_prompts()
    assert prompts == snapshot(name="list_prompts")

@pytest.mark.asyncio
async def test_mcp_export_database(mcp_client: Client, snapshot):
    result = await mcp_client.call_tool("execute_sql", {"sql": "create table t as select 1 as a;"})
    result = await mcp_client.call_tool("export_database")
    #print(result)
    assert len(result.content) == 1

    resource = result.content[0]
    assert isinstance(resource, EmbeddedResource)
    
    contents = resource.resource
    assert isinstance(contents, BlobResourceContents)
    assert contents.uri.encoded_string() == "solite://aaa"
    assert contents.mimeType == "application/vnd.sqlite3"
    body = base64.b64decode(contents.blob.encode())
    assert len(body) == 8192
    db = sqlite3.connect(":memory:")
    db.deserialize(body)
    assert db.execute("SELECT name FROM sqlite_master;").fetchall() == [('t',)]
    

@pytest.mark.asyncio
async def test_mcp_sql_basic(mcp_client: Client, snapshot):
    t = TestMcp(mcp_client, snapshot)
    await t.execute_sql("select 1 + 1", snapshot=snapshot, name="select 1 + 1")
    await t.execute_sql("create table t as select value from json_each('[1, 2, 3]');")
    await t.execute_sql("select * from t", snapshot=snapshot, name="select * from t")
    
    with pytest.raises(McpError):
        await t.execute_sql("select error;")