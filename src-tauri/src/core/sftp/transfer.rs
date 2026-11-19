//! Upload / download with recursion, resume (offset-based) and progress events.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::error::{AppError, AppResult};

use super::{SftpManager, TransferEvent};

const CHUNK: usize = 64 * 1024;

/// A single file to move (already expanded from recursion).
struct Job {
    remote: String,
    local: PathBuf,
    size: u64,
}

fn join_remote(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// Expand a remote path into file jobs for download.
async fn expand_remote(
    sftp: &Arc<SftpSession>,
    remote: &str,
    local: &Path,
) -> AppResult<(Vec<Job>, bool)> {
    let meta = sftp
        .metadata(remote)
        .await
        .map_err(|e| AppError::Sftp(e.to_string()))?;
    if !meta.is_dir() {
        return Ok((
            vec![Job {
                remote: remote.to_string(),
                local: local.to_path_buf(),
                size: meta.size.unwrap_or(0),
            }],
            false,
        ));
    }
    // Recursive walk with an explicit stack.
    let mut jobs = Vec::new();
    let mut stack = vec![(remote.to_string(), local.to_path_buf())];
    while let Some((rdir, ldir)) = stack.pop() {
        let entries = sftp
            .read_dir(&rdir)
            .await
            .map_err(|e| AppError::Sftp(e.to_string()))?;
        for entry in entries {
            let rpath = entry.path();
            let lpath = ldir.join(entry.file_name());
            let m = entry.metadata();
            if m.is_dir() {
                stack.push((rpath, lpath));
            } else if m.is_regular() {
                jobs.push(Job {
                    remote: rpath,
                    local: lpath,
                    size: m.size.unwrap_or(0),
                });
            }
        }
    }
    Ok((jobs, true))
}

/// Expand a local path into file jobs for upload.
async fn expand_local(local: &Path, remote: &str) -> AppResult<(Vec<Job>, bool)> {
    let meta = tokio::fs::metadata(local).await?;
    if !meta.is_dir() {
        return Ok((
            vec![Job {
                remote: remote.to_string(),
                local: local.to_path_buf(),
                size: meta.len(),
            }],
            false,
        ));
    }
    let mut jobs = Vec::new();
    let mut stack = vec![(local.to_path_buf(), remote.to_string())];
    while let Some((ldir, rdir)) = stack.pop() {
        let mut rd = tokio::fs::read_dir(&ldir).await?;
        while let Some(entry) = rd.next_entry().await? {
            let lpath = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let rpath = join_remote(&rdir, &name);
            let m = entry.metadata().await?;
            if m.is_dir() {
                stack.push((lpath, rpath));
            } else if m.is_file() {
                jobs.push(Job {
                    remote: rpath,
                    local: lpath,
                    size: m.len(),
                });
            }
        }
    }
    Ok((jobs, true))
}

/// Download `remote` to `local` (recursive when remote is a directory).
pub async fn download(
    mgr: SftpManager,
    id: String,
    sftp: Arc<SftpSession>,
    remote: String,
    local: PathBuf,
    resume: bool,
) -> AppResult<()> {
    let (jobs, is_dir) = expand_remote(&sftp, &remote, &local).await?;
    let total: u64 = jobs.iter().map(|j| j.size).sum();
    mgr.emit(TransferEvent::State {
        id: id.clone(),
        state: "running".into(),
        message: None,
    })
    .await;

    let mut transferred = 0u64;
    for job in &jobs {
        if is_dir {
            if let Some(parent) = job.local.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        let start = if resume {
            tokio::fs::metadata(&job.local)
                .await
                .map(|m| m.len())
                .unwrap_or(0)
        } else {
            0
        };
        if start >= job.size && job.size > 0 {
            transferred += job.size;
            continue;
        }
        copy_one(
            &mgr,
            &id,
            &sftp,
            &job.remote,
            &job.local,
            start,
            job.size,
            transferred,
            total,
            Direction::Download,
        )
        .await?;
        transferred += job.size.saturating_sub(start);
    }

    mgr.emit(TransferEvent::State {
        id,
        state: "done".into(),
        message: None,
    })
    .await;
    Ok(())
}

/// Upload `local` to `remote` (recursive when local is a directory).
pub async fn upload(
    mgr: SftpManager,
    id: String,
    sftp: Arc<SftpSession>,
    local: PathBuf,
    remote: String,
    resume: bool,
) -> AppResult<()> {
    let (jobs, is_dir) = expand_local(&local, &remote).await?;
    let total: u64 = jobs.iter().map(|j| j.size).sum();
    mgr.emit(TransferEvent::State {
        id: id.clone(),
        state: "running".into(),
        message: None,
    })
    .await;

    // Best-effort: create remote directory tree up front.
    if is_dir {
        let mut dirs: Vec<String> = jobs
            .iter()
            .filter_map(|j| {
                let idx = j.remote.rfind('/')?;
                Some(j.remote[..idx].to_string())
            })
            .collect();
        dirs.sort();
        dirs.dedup();
        // Parents first (sorted order guarantees it for '/'-joined paths).
        for d in dirs {
            let _ = sftp.create_dir(&d).await;
        }
    }

    let mut transferred = 0u64;
    for job in &jobs {
        let start = if resume {
            sftp.metadata(&job.remote)
                .await
                .ok()
                .and_then(|m| m.size)
                .unwrap_or(0)
        } else {
            0
        };
        if start >= job.size && job.size > 0 {
            transferred += job.size;
            continue;
        }
        copy_one(
            &mgr,
            &id,
            &sftp,
            &job.remote,
            &job.local,
            start,
            job.size,
            transferred,
            total,
            Direction::Upload,
        )
        .await?;
        transferred += job.size.saturating_sub(start);
    }

    mgr.emit(TransferEvent::State {
        id,
        state: "done".into(),
        message: None,
    })
    .await;
    Ok(())
}

enum Direction {
    Download,
    Upload,
}

#[allow(clippy::too_many_arguments)]
async fn copy_one(
    mgr: &SftpManager,
    id: &str,
    sftp: &Arc<SftpSession>,
    remote: &str,
    local: &Path,
    offset: u64,
    _size: u64,
    base: u64,
    total: u64,
    dir: Direction,
) -> AppResult<()> {
    match dir {
        Direction::Download => {
            let mut rfile = sftp
                .open(remote)
                .await
                .map_err(|e| AppError::Sftp(e.to_string()))?;
            if offset > 0 {
                rfile
                    .seek(std::io::SeekFrom::Start(offset))
                    .await
                    .map_err(|e| AppError::Sftp(e.to_string()))?;
            }
            let mut lfile = if offset > 0 {
                tokio::fs::OpenOptions::new()
                    .append(true)
                    .open(local)
                    .await?
            } else {
                tokio::fs::File::create(local).await?
            };
            pump(mgr, id, &mut rfile, &mut lfile, base + offset, total).await?;
        }
        Direction::Upload => {
            let mut lfile = tokio::fs::File::open(local).await?;
            if offset > 0 {
                lfile.seek(std::io::SeekFrom::Start(offset)).await?;
            }
            let flags = if offset > 0 {
                OpenFlags::CREATE | OpenFlags::WRITE
            } else {
                OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE
            };
            let mut rfile = sftp
                .open_with_flags(remote, flags)
                .await
                .map_err(|e| AppError::Sftp(e.to_string()))?;
            if offset > 0 {
                rfile
                    .seek(std::io::SeekFrom::Start(offset))
                    .await
                    .map_err(|e| AppError::Sftp(e.to_string()))?;
            }
            pump(mgr, id, &mut lfile, &mut rfile, base + offset, total).await?;
        }
    }
    Ok(())
}

async fn pump<R: AsyncReadExt + Unpin, W: AsyncWriteExt + Unpin>(
    mgr: &SftpManager,
    id: &str,
    reader: &mut R,
    writer: &mut W,
    mut done: u64,
    total: u64,
) -> AppResult<()> {
    let mut buf = vec![0u8; CHUNK];
    let mut last_emit = 0u64;
    loop {
        let n = reader.read(&mut buf).await.map_err(|e| AppError::Io(e))?;
        if n == 0 {
            break;
        }
        writer
            .write_all(&buf[..n])
            .await
            .map_err(AppError::Io)?;
        done += n as u64;
        // Throttle progress events to ~every 256KB.
        if done - last_emit >= 256 * 1024 {
            last_emit = done;
            mgr.emit(TransferEvent::Progress {
                id: id.to_string(),
                transferred: done,
                total,
            })
            .await;
        }
    }
    writer.flush().await.map_err(AppError::Io)?;
    mgr.emit(TransferEvent::Progress {
        id: id.to_string(),
        transferred: done,
        total,
    })
    .await;
    Ok(())
}
