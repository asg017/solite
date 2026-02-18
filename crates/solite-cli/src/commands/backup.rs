use std::ffi::CString;
use std::ptr;
use std::time::Instant;

use console::style;
use indicatif::{HumanBytes, ProgressBar, ProgressStyle};
use libsqlite3_sys::*;
use solite_core::sqlite::Connection;

use crate::cli::BackupArgs;

fn get_page_size(conn: &Connection) -> u64 {
    let (_, stmt) = conn.prepare("PRAGMA page_size").unwrap();
    let stmt = stmt.unwrap();
    if let Ok(Some(row)) = stmt.next() {
        if let solite_core::sqlite::ValueRefXValue::Int(v) = row[0].value {
            return v as u64;
        }
    }
    4096
}

// TODO: make a safe(r) Rust wrapper around raw C API
pub fn backup(args: BackupArgs) -> Result<(), ()> {
    let source_path = args.database.to_string_lossy();
    let source = Connection::open(&source_path).map_err(|e| {
        eprintln!("Error opening source database: {}", e.message);
    })?;

    let page_size = get_page_size(&source);

    let dest_path = CString::new(args.destination.to_string_lossy().as_ref()).map_err(|_| {
        eprintln!("Invalid destination path");
    })?;
    let mut dest_db: *mut sqlite3 = ptr::null_mut();
    let rc = unsafe {
        sqlite3_open_v2(
            dest_path.as_ptr(),
            &mut dest_db,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            ptr::null(),
        )
    };
    if rc != SQLITE_OK {
        let msg = unsafe {
            let p = sqlite3_errmsg(dest_db);
            if p.is_null() {
                "unknown error".to_string()
            } else {
                std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        };
        eprintln!("Error opening destination database: {msg}");
        unsafe { sqlite3_close(dest_db) };
        return Err(());
    }

    let db_name = CString::new(args.db.as_str()).map_err(|_| {
        eprintln!("Invalid database name");
    })?;
    let dest_name = CString::new("main").unwrap();

    let backup = unsafe {
        sqlite3_backup_init(dest_db, dest_name.as_ptr(), source.db(), db_name.as_ptr())
    };
    if backup.is_null() {
        let msg = unsafe {
            let p = sqlite3_errmsg(dest_db);
            if p.is_null() {
                "unknown error".to_string()
            } else {
                std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        };
        eprintln!("Error initializing backup: {msg}");
        unsafe { sqlite3_close(dest_db) };
        return Err(());
    }

    let src_display = args.database.display();
    let dest_display = args.destination.display();

    let start = Instant::now();
    let pb = ProgressBar::new(0);
    if let Ok(style) = ProgressStyle::with_template(
        &format!("{src_display} -> {dest_display}\n[{{bar:60}}] {{msg}} ({{elapsed}} / ETA {{eta}})")
    ) {
        pb.set_style(style);
    }

    loop {
        let rc = unsafe { sqlite3_backup_step(backup, 100) };
        let remaining = unsafe { sqlite3_backup_remaining(backup) } as u64;
        let pagecount = unsafe { sqlite3_backup_pagecount(backup) } as u64;
        let total_bytes = pagecount * page_size;
        let done_bytes = (pagecount - remaining) * page_size;

        pb.set_length(total_bytes);
        pb.set_position(done_bytes);
        pb.set_message(format!("{} / {}", HumanBytes(done_bytes), HumanBytes(total_bytes)));

        if rc == SQLITE_OK {
            continue;
        }
        if rc == SQLITE_DONE {
            break;
        }
        // Error during backup
        pb.finish_and_clear();
        let finish_rc = unsafe { sqlite3_backup_finish(backup) };
        let msg = unsafe {
            let p = sqlite3_errmsg(dest_db);
            if p.is_null() {
                "unknown error".to_string()
            } else {
                std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        };
        eprintln!("Backup failed (rc={finish_rc}): {msg}");
        unsafe { sqlite3_close(dest_db) };
        return Err(());
    }

    pb.finish_and_clear();

    let rc = unsafe { sqlite3_backup_finish(backup) };
    if rc != SQLITE_OK {
        let msg = unsafe {
            let p = sqlite3_errmsg(dest_db);
            if p.is_null() {
                "unknown error".to_string()
            } else {
                std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        };
        eprintln!("Backup finish failed: {msg}");
        unsafe { sqlite3_close(dest_db) };
        return Err(());
    }

    unsafe { sqlite3_close(dest_db) };

    let elapsed = start.elapsed();
    let size = std::fs::metadata(&args.destination).map(|m| m.len()).unwrap_or(0);

    println!(
        "{} Backed up to {} ({}, {:.2?})",
        style("\u{2714}").green(),
        dest_display,
        HumanBytes(size),
        elapsed,
    );
    Ok(())
}
