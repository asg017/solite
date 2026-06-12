use std::path::PathBuf;

use anyhow::bail;
use solite_core::{dot::SchemaCommand, sqlite, Runtime};

use crate::cli::SchemaFormat;

pub fn schema(
    database: PathBuf,
    pattern: Option<String>,
    format: SchemaFormat,
    allow_ssh: bool,
) -> Result<(), ()> {
    schema_impl(database, pattern, format, allow_ssh).map_err(|e| eprintln!("Error: {e}"))
}

fn schema_impl(
    database: PathBuf,
    pattern: Option<String>,
    format: SchemaFormat,
    allow_ssh: bool,
) -> anyhow::Result<()> {
    match format {
        SchemaFormat::Sql => schema_sql(database, pattern, allow_ssh),
        SchemaFormat::Json => schema_json(database, pattern),
    }
}

fn schema_sql(database: PathBuf, pattern: Option<String>, allow_ssh: bool) -> anyhow::Result<()> {
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
    let cmd = SchemaCommand { pattern };
    let schemas = cmd.execute(&runtime)?;
    for schema in schemas {
        println!("{}", schema);
    }
    Ok(())
}

fn schema_json(database: PathBuf, pattern: Option<String>) -> anyhow::Result<()> {
    use solite_schema::introspect::introspect_sqlite_db;
    use solite_schema::json::JsonSchema;

    if pattern.is_some() {
        bail!("pattern filtering is not supported with --format json");
    }
    let path = database.to_string_lossy().to_string();
    if sqlite::is_remote_path(&path) {
        bail!("--format json is not supported for remote databases");
    }
    if !database.exists() {
        bail!("no such file: {}", database.display());
    }

    // introspect_sqlite_db opens the database read-only
    let introspected = introspect_sqlite_db(&database)?;
    let json = JsonSchema::from(&introspected);
    println!("{}", json.to_json()?);
    Ok(())
}
