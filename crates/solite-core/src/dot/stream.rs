//! Stream replication dot command.
//!
//! Provides `.stream sync <url>` and `.stream restore <url>` commands for
//! syncing WAL changes to a replica and restoring from a replica, respectively.

use serde::Serialize;
use std::path::Path;

use super::DotError;
use crate::Runtime;

/// The action to perform for a stream command.
#[derive(Serialize, Debug)]
pub enum StreamAction {
    /// Sync WAL changes to a replica URL.
    Sync { url: String },
    /// Restore a database from a replica URL.
    Restore { url: String },
}

/// A `.stream` dot command.
#[derive(Serialize, Debug)]
pub struct StreamCommand {
    pub action: StreamAction,
}

/// Result of a successful sync operation.
pub struct StreamSyncResult {
    pub txid: u64,
    pub page_count: u32,
}

impl StreamCommand {
    /// Execute the stream command against the given runtime's database.
    pub fn execute(&self, runtime: &Runtime) -> Result<Option<StreamSyncResult>, DotError> {
        let db_path = runtime
            .connection
            .db_name()
            .ok_or_else(|| DotError::Command("no database file open (in-memory?)".to_string()))?;
        let db_path = Path::new(&db_path);

        match &self.action {
            StreamAction::Sync { url } => {
                let result = ritestream_api::sync(url, db_path)
                    .map_err(|e| DotError::Command(e.to_string()))?;
                Ok(result.map(|r| StreamSyncResult {
                    txid: r.txid,
                    page_count: r.page_count,
                }))
            }
            StreamAction::Restore { url } => {
                ritestream_api::restore(url, db_path)
                    .map_err(|e| DotError::Command(e.to_string()))?;
                Ok(None)
            }
        }
    }
}
