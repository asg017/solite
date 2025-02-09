use arboard::Clipboard;
use sqlite_loadable::prelude::*;
use sqlite_loadable::{api, define_scalar_function, FunctionFlags, Result};

pub fn solite_stdlib_version(
    context: *mut sqlite3_context,
    _values: &[*mut sqlite3_value],
) -> Result<()> {
    api::result_text(context, format!("v{}", env!("CARGO_PKG_VERSION")))?;
    Ok(())
}

pub fn clipboard_set(context: *mut sqlite3_context, values: &[*mut sqlite3_value]) -> Result<()> {
    let contents = api::value_text(&values[0])?;
    let mut cb = Clipboard::new().unwrap();
    cb.set_text(contents).unwrap();
    api::result_bool(context, true);
    Ok(())
}

#[sqlite_entrypoint]
pub fn sqlite3_solite_stdlib_init(db: *mut sqlite3) -> Result<()> {
    define_scalar_function(
        db,
        "solite_stdlib_version",
        0,
        solite_stdlib_version,
        FunctionFlags::UTF8 | FunctionFlags::DETERMINISTIC,
    )?;
    define_scalar_function(db, "clipboard_set", 1, clipboard_set, FunctionFlags::UTF8)?;
    Ok(())
}
