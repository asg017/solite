//! CLI handler for the `solite stream` subcommand.

use std::time::Instant;

use indicatif::{HumanBytes, ProgressBar, ProgressStyle};

use crate::cli::{StreamCommand, StreamNamespace};
use crate::colors;

fn sync_impl(database: std::path::PathBuf, url: String) -> anyhow::Result<()> {
    let pb = ProgressBar::new_spinner();
    if let Ok(style) =
        ProgressStyle::with_template("{spinner:.cyan} {elapsed} {wide_msg}")
    {
        pb.set_style(style);
    }
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    let start = Instant::now();

    let result = ritestream_api::sync_with_progress(&url, &database, |event| {
        use ritestream_api::SyncProgress::*;
        match event {
            Generating => {
                pb.set_message("generating LTX…");
            }
            Uploading { bytes } => {
                pb.set_message(format!("uploading {}…", HumanBytes(bytes)));
            }
            Uploaded { .. } => {}
        }
    })?;

    pb.finish_and_clear();

    match result {
        Some(r) => {
            println!(
                "{} synced to {} (txid={}, {} pages, {:.2?})",
                colors::green("✓"),
                url,
                r.txid,
                r.page_count,
                start.elapsed(),
            );
        }
        None => {
            println!(
                "{} nothing to sync (database empty or missing)",
                colors::yellow("⚠"),
            );
        }
    }
    Ok(())
}

fn restore_impl(url: String, database: std::path::PathBuf) -> anyhow::Result<()> {
    let pb = ProgressBar::new(0);
    if let Ok(style) = ProgressStyle::with_template(
        "{spinner:.cyan} [{bar:40}] {bytes}/{total_bytes} ({elapsed})",
    ) {
        pb.set_style(style.progress_chars("=> "));
    }
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    let start = Instant::now();

    ritestream_api::restore_with_progress(&url, &database, |event| {
        use ritestream_api::RestoreProgress::*;
        match event {
            Listed { total_bytes, .. } => {
                pb.set_length(total_bytes);
            }
            Progress { bytes_downloaded, .. } => {
                pb.set_position(bytes_downloaded);
            }
            Writing => {}
        }
    })?;

    pb.finish_and_clear();

    let size = std::fs::metadata(&database).map(|m| m.len()).unwrap_or(0);
    println!(
        "{} restored {} from {} ({}, {:.2?})",
        colors::green("✓"),
        database.display(),
        url,
        HumanBytes(size),
        start.elapsed(),
    );
    Ok(())
}

pub fn stream(cmd: StreamNamespace) -> Result<(), ()> {
    let result = match cmd.command {
        StreamCommand::Sync(args) => sync_impl(args.database, args.url),
        StreamCommand::Restore(args) => restore_impl(args.url, args.database),
    };
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            eprintln!("{error}");
            Err(())
        }
    }
}
