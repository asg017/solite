use std::path::PathBuf;

use solite_core::{dot::SchemaCommand, Runtime};

pub fn schema(database: PathBuf) -> Result<(), ()> {
    schema_impl(database).map_err(|e| eprintln!("Error: {e:?}"))
}

fn schema_impl(database: PathBuf) -> anyhow::Result<()> {
    let runtime = Runtime::new(Some(database.to_string_lossy().to_string()))?;
    let cmd = SchemaCommand {};
    let schemas = cmd.execute(&runtime)?;
    for schema in schemas {
        println!("{};", schema);
    }
    Ok(())
}
