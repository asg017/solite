mod html;
mod server;

use crate::cli::{JupyterCommand, JupyterInstallArgs, JupyterNamespace, JupyterUpArgs};

use serde_json::json;
use server::start_kernel;
use std::env::current_exe;

fn install(args: JupyterInstallArgs) -> anyhow::Result<()> {
    let user_data_dir = runtimelib::dirs::user_data_dir()?;
    let kernelspec_path = user_data_dir
        .join("kernels")
        .join(args.name.unwrap_or("solite".to_string()))
        .join("kernel.json");
    std::fs::create_dir_all(kernelspec_path.parent().unwrap())?;

    if kernelspec_path.exists() && !args.force {
        return Err(anyhow::anyhow!(
            "Kernel spec already exists at {}. Use --force to overwrite.",
            kernelspec_path.display()
        ));
    }

    let kernel_json = json!({
      "argv": [
        current_exe()?.to_string_lossy().to_string(),
        "jupyter",
        "up",
        "--connection",
        "{connection_file}"
      ],
      "env": {},
      "display_name": args.display.unwrap_or("Solite".to_string()),
      "language": "sql",
      "interrupt_mode": "signal",
      "metadata": {}
    });

    let f = std::fs::File::create(&kernelspec_path).unwrap();
    serde_json::to_writer(f, &kernel_json).unwrap();
    println!(
        "Successfully installed Solite Jupyter kernel at {}",
        kernelspec_path.display()
    );
    Ok(())
}

fn up(args: JupyterUpArgs) -> Result<(), ()> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        start_kernel(args.connection).await.unwrap();
    });
    Ok(())
}
pub(crate) fn jupyter(cmd: JupyterNamespace) -> Result<(), ()> {
    match cmd.command {
        JupyterCommand::Install(args) => match install(args) {
            Ok(_) => Ok(()),
            Err(error) => {
                eprintln!("{error}");
                Err(())
            }
        },
        JupyterCommand::Up(args) => up(args),
    }
}
