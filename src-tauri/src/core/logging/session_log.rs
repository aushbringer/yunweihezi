//! Background file writer for one session's audit log.

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::core::db::{now_ms, Db};

/// Handle to a live session log. Dropping it closes the channel; the
/// writer thread flushes and finalises the DB record.
#[derive(Clone)]
pub struct SessionLogger {
    tx: std::sync::mpsc::Sender<Vec<u8>>,
    bytes: Arc<AtomicU64>,
    path: Arc<PathBuf>,
}

impl SessionLogger {
    /// Create the file, register the log_files row, spawn the writer.
    /// Must be called from within the tokio runtime.
    pub fn start(db: Db, path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::File::create(&path)?;
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let bytes = Arc::new(AtomicU64::new(0));
        let path_arc = Arc::new(path);
        let rt = tokio::runtime::Handle::current();

        let record_id = uuid::Uuid::new_v4().to_string();
        let started = now_ms();
        {
            let db = db.clone();
            let rid = record_id.clone();
            let pstr = path_arc.to_string_lossy().to_string();
            tokio::spawn(async move {
                let _ = db.log_add(&rid, &pstr, started).await;
            });
        }

        {
            let bytes = bytes.clone();
            let path_arc = path_arc.clone();
            std::thread::spawn(move || {
                let mut w = BufWriter::with_capacity(64 * 1024, file);
                let mut written: u64 = 0;
                while let Ok(data) = rx.recv() {
                    if w.write_all(&data).is_err() {
                        break;
                    }
                    written += data.len() as u64;
                    // Flush eagerly: audit logs must survive a crash.
                    let _ = w.flush();
                    bytes.store(written, Ordering::Relaxed);
                }
                let _ = w.flush();
                let rid = record_id;
                let p = path_arc.to_string_lossy().to_string();
                rt.spawn(async move {
                    let _ = db.log_finish(&rid, &p, written as i64, now_ms()).await;
                });
            });
        }

        Ok(Self {
            tx,
            bytes,
            path: path_arc,
        })
    }

    /// Append raw output bytes (non-blocking; cheap clone into the queue).
    pub fn write(&self, data: &[u8]) {
        let _ = self.tx.send(data.to_vec());
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }
}
