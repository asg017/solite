use crate::cli::{McpCommand, McpNamespace};


pub(crate) fn mcp(cmd: McpNamespace) -> Result<(), ()> {
  match cmd.command {
    McpCommand::Up(_) => solite_mcp::upp().map_err(|_| ()),
    McpCommand::Install(_) => {
      todo!("MCP install is not implemented yet.")
    }
  }
}