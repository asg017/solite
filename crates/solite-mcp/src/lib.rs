use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use tracing_subscriber::{self, EnvFilter};
mod sandbox;


#[tokio::main]
async fn up() -> Result<()> {
    /*
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init(); */

    tracing::info!("Starting MCP server");
    let sandbox = sandbox::Sandbox::new();
    let service = sandbox.serve(stdio()).await.inspect_err(|e| {
        tracing::error!("serving error: {:?}", e);
    })?;

    service.waiting().await?;
    Ok(())
}


pub fn upp() -> Result<()> {
    up()
}