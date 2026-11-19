//! Global application state managed by Tauri.

use std::path::{Path, PathBuf};

use crate::core::crypto::vault::Vault;
use crate::core::db::Db;
use crate::error::AppResult;

pub struct AppState {
    pub db: Db,
    pub vault: Vault,
    /// Application data directory (parent of openterm.db).
    pub data_dir: PathBuf,
}

impl AppState {
    pub fn new(db_path: &Path) -> AppResult<Self> {
        Ok(Self {
            db: Db::open(db_path)?,
            vault: Vault::new(),
            data_dir: db_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(".")),
        })
    }
}
