//! LSP server command

use crate::cli::LspArgs;

pub fn lsp(_args: LspArgs) -> Result<(), ()> {
    // Create a tokio runtime and run the LSP server
    let rt = tokio::runtime::Runtime::new().map_err(|e| {
        eprintln!("Failed to create runtime: {e}");
    })?;

    rt.block_on(async {
        solite_lsp::run_server().await;
    });

    Ok(())
}
