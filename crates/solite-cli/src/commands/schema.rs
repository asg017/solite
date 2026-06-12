use std::path::PathBuf;

use anyhow::bail;
use solite_core::{dot::SchemaCommand, sqlite, Runtime};

pub fn schema(database: PathBuf, allow_ssh: bool) -> Result<(), ()> {
    schema_impl(database, allow_ssh).map_err(|e| eprintln!("Error: {e}"))
}

fn schema_impl(database: PathBuf, allow_ssh: bool) -> anyhow::Result<()> {
    let path = database.to_string_lossy().to_string();
    let runtime = if sqlite::is_remote_path(&path) {
        Runtime::new_with_options(Some(path), None, None, allow_ssh)?
    } else {
        // Introspection never needs write access: open read-only so a
        // typo'd path errors out instead of creating an empty database.
        if !database.exists() {
            bail!("no such file: {}", database.display());
        }
        Runtime::new_readonly(&path)?
    };
    let cmd = SchemaCommand {};
    let schemas = cmd.execute(&runtime)?;
    for schema in schemas {
        println!("{}", schema);
    }
    Ok(())
}
