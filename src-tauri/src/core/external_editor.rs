//! Open a remote SFTP file with the local default application ("external
//! editor") and upload it back automatically once the editor saves.
//!
//! Flow: download to `%TEMP%/openterm-edit/<uuid>_<name>` → open with the OS
//! default handler → poll the local file; when its (len, mtime) changes and
//! stays stable across two samples, upload it back over SFTP.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use russh_sftp::client::SftpSession;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::core::sftp::SftpManager;
use crate::error::{AppError, AppResult};

/// Refuse to external-edit very large files.
const MAX_BYTES: usize = 32 * 1024 * 1024;
/// Give up watching after this long.
const WATCH_TTL: Duration = Duration::from_secs(12 * 3600);
const POLL_INTERVAL: Duration = Duration::from_millis(1500);

pub async fn open_external(
    app: AppHandle,
    sftp_mgr: SftpManager,
    sftp_id: &str,
    remote_path: &str,
) -> AppResult<String> {
    let sftp = sftp_mgr.get(sftp_id).await?;
    let data = sftp
        .read(remote_path)
        .await
        .map_err(|e| AppError::Sftp(format!("read {remote_path}: {e}")))?;
    if data.len() > MAX_BYTES {
        return Err(AppError::Config(
            "文件超过 32MB,不适合外部编辑".into(),
        ));
    }

    let dir = std::env::temp_dir().join("openterm-edit");
    std::fs::create_dir_all(&dir)?;
    let fname = remote_path.rsplit('/').next().unwrap_or("file");
    let local = dir.join(format!("{}_{fname}", uuid::Uuid::new_v4()));
    std::fs::write(&local, &data)?;

    app.opener()
        .open_path(local.to_string_lossy().as_ref(), None::<&str>)
        .map_err(|e| AppError::Config(format!("open with default app: {e}")))?;

    tokio::spawn(watch_and_upload(sftp, remote_path.to_string(), local.clone()));
    Ok(local.to_string_lossy().into_owned())
}

/// (len, mtime-secs) fingerprint of the local temp file.
fn fingerprint(p: &PathBuf) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(p).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some((meta.len(), mtime))
}

async fn watch_and_upload(sftp: Arc<SftpSession>, remote: String, local: PathBuf) {
    let deadline = Instant::now() + WATCH_TTL;
    let mut baseline = fingerprint(&local);
    // Candidate change awaiting a second stable sample.
    let mut pending: Option<(u64, u64)> = None;

    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        if Instant::now() > deadline {
            tracing::info!(%remote, "external editor watch expired");
            return;
        }
        let Some(cur) = fingerprint(&local) else {
            // Temp file deleted (editor closed/cleaned up) → stop watching.
            tracing::info!(%remote, "external editor temp file gone, watch stopped");
            return;
        };
        if Some(cur) == baseline {
            pending = None;
            continue;
        }
        if pending == Some(cur) {
            // Changed and stable across two samples → upload.
            match tokio::fs::read(&local).await {
                Ok(bytes) => match sftp.write(&remote, &bytes).await {
                    Ok(()) => {
                        tracing::info!(%remote, bytes = bytes.len(), "external editor upload ok");
                        baseline = Some(cur);
                    }
                    Err(e) => {
                        tracing::warn!(%remote, "external editor upload failed: {e}");
                        baseline = Some(cur); // avoid retry storms
                    }
                },
                Err(e) => {
                    tracing::warn!(%remote, "external editor local read failed: {e}");
                    return;
                }
            }
            pending = None;
        } else {
            pending = Some(cur);
        }
    }
}
