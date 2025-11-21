use crate::cli::RpcNamespace;

mod jsonrpc;
mod client;
mod server;

pub fn rpc(cmd: RpcNamespace) -> std::result::Result<(), ()> {
    let result = match cmd.command {
        crate::cli::RpcCommand::ClientDebug(args) => {
            client::run(args.executable)
        }
        crate::cli::RpcCommand::Server(_args) => {
            server::run()
        }
    };

    result.map_err(|e| {
        eprintln!("RPC command failed: {}", e);
    })
}