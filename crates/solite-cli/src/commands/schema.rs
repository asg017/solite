use std::path::PathBuf;

use solite_core::{dot::SchemaCommand, Runtime};

pub fn schema(database: PathBuf) -> Result<(), ()> {
    let runtime = Runtime::new(Some(database.to_string_lossy().to_string()));
    let cmd = SchemaCommand {};
    let schemas = cmd.execute(&runtime).map_err(|_| ())?;
    for schema in schemas {
        println!("{};", schema);
    }
    Ok(())
}
