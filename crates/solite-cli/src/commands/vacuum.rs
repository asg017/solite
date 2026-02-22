use std::time::Instant;

use console::style;
use indicatif::HumanBytes;
use solite_core::sqlite::Connection;

use crate::cli::VacuumArgs;

pub fn vacuum(args: VacuumArgs) -> Result<(), ()> {
    let db_path = args.database.to_string_lossy();
    let conn = Connection::open(&db_path).map_err(|e| {
        eprintln!("Error opening database: {}", e.message);
    })?;

    let start = Instant::now();

    let target = match args.into_path() {
        Some(into) => {
            let (_, stmt) = conn.prepare("VACUUM INTO ?").map_err(|e| {
                eprintln!("Vacuum failed: {}", e.message);
            })?;
            let stmt = stmt.unwrap();
            stmt.bind_text(1, into.to_string_lossy());
            stmt.execute().map_err(|e| {
                eprintln!("Vacuum failed: {}", e.message);
            })?;
            into.clone()
        }
        None => {
            conn.execute_script("VACUUM").map_err(|e| {
                eprintln!("Vacuum failed: {}", e.message);
            })?;
            args.database.clone()
        }
    };

    let elapsed = start.elapsed();
    let size = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);

    println!(
        "{} Vacuumed {} ({}, {:.2?})",
        style("\u{2714}").green(),
        target.display(),
        HumanBytes(size),
        elapsed,
    );

    Ok(())
}
