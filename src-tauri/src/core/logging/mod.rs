//! Session audit logging: template-based file naming + background writer.
//!
//! Config keys in the settings table:
//! - `log.enabled`   "true" | "false" (default "false")
//! - `log.dir`       target directory (default: <app data>/logs)
//! - `log.template`  file name template (default "%s_%d_%t.log")
//!
//! Template variables: %h host, %u username, %s session label,
//! %p protocol, %d YYYYMMDD, %t HHMMSS, %% literal percent.

mod session_log;

pub use session_log::SessionLogger;

use std::path::PathBuf;

use crate::core::db::Db;

pub struct LogVars {
    pub host: String,
    pub user: String,
    /// Session label (record name or ad-hoc title).
    pub name: String,
    pub protocol: String,
}

pub struct LogConfig {
    pub enabled: bool,
    pub dir: PathBuf,
    pub template: String,
}

impl LogConfig {
    pub async fn load(db: &Db, data_dir: &std::path::Path) -> Self {
        let enabled = db
            .setting_get("log.enabled")
            .await
            .ok()
            .flatten()
            .as_deref()
            == Some("true");
        let dir = db
            .setting_get("log.dir")
            .await
            .ok()
            .flatten()
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("logs"));
        let template = db
            .setting_get("log.template")
            .await
            .ok()
            .flatten()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "%s_%d_%t.log".into());
        Self {
            enabled,
            dir,
            template,
        }
    }

    /// Expand the template into a sanitized file name.
    pub fn file_name(&self, vars: &LogVars) -> String {
        let now = chrono::Local::now();
        let mut out = String::with_capacity(self.template.len() + 16);
        let mut chars = self.template.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('h') => out.push_str(&vars.host),
                Some('u') => out.push_str(&vars.user),
                Some('s') => out.push_str(&vars.name),
                Some('p') => out.push_str(&vars.protocol),
                Some('d') => out.push_str(&now.format("%Y%m%d").to_string()),
                Some('t') => out.push_str(&now.format("%H%M%S").to_string()),
                Some('%') => out.push('%'),
                Some(other) => {
                    out.push('%');
                    out.push(other);
                }
                None => out.push('%'),
            }
        }
        sanitize(&out)
    }
}

fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0') {
                '_'
            } else {
                c
            }
        })
        .collect();
    if s.trim().is_empty() {
        "session.log".into()
    } else {
        s
    }
}

/// Start a session logger when logging is enabled; None when disabled or
/// the file cannot be created (logged, non-fatal).
pub async fn start_session_logger(
    db: &Db,
    data_dir: &std::path::Path,
    vars: &LogVars,
) -> Option<SessionLogger> {
    let cfg = LogConfig::load(db, data_dir).await;
    if !cfg.enabled {
        return None;
    }
    let name = cfg.file_name(vars);
    let path = cfg.dir.join(name);
    match SessionLogger::start(db.clone(), path) {
        Ok(l) => Some(l),
        Err(e) => {
            tracing::warn!("session logger: {e}");
            None
        }
    }
}
