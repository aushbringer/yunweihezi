//! SQLite persistence layer (rusqlite, bundled) with schema migrations.

pub mod models;

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};
use models::*;

pub const SCHEMA_VERSION: i64 = 1;

const MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS groups (
    id        TEXT PRIMARY KEY,
    name      TEXT NOT NULL,
    parent_id TEXT REFERENCES groups(id) ON DELETE SET NULL,
    sort      INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS credentials (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL,
    username   TEXT NOT NULL DEFAULT '',
    secret_enc TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id            TEXT PRIMARY KEY,
    group_id      TEXT REFERENCES groups(id) ON DELETE SET NULL,
    name          TEXT NOT NULL,
    protocol      TEXT NOT NULL DEFAULT 'ssh',
    host          TEXT NOT NULL DEFAULT '',
    port          INTEGER NOT NULL DEFAULT 22,
    username      TEXT NOT NULL DEFAULT '',
    credential_id TEXT REFERENCES credentials(id) ON DELETE SET NULL,
    options       TEXT NOT NULL DEFAULT '{}',
    sort          INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS tunnels (
    id          TEXT PRIMARY KEY,
    session_id  TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL,
    listen_host TEXT NOT NULL DEFAULT '127.0.0.1',
    listen_port INTEGER NOT NULL,
    target_host TEXT NOT NULL DEFAULT '',
    target_port INTEGER NOT NULL DEFAULT 0,
    autostart   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS quick_commands (
    id      TEXT PRIMARY KEY,
    label   TEXT NOT NULL,
    content TEXT NOT NULL,
    grp     TEXT NOT NULL DEFAULT '',
    sort    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS triggers (
    id           TEXT PRIMARY KEY,
    session_id   TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    pattern      TEXT NOT NULL,
    is_regex     INTEGER NOT NULL DEFAULT 1,
    action       TEXT NOT NULL,
    action_param TEXT NOT NULL DEFAULT '',
    enabled      INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS log_files (
    id         TEXT PRIMARY KEY,
    session_id TEXT,
    path       TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at   INTEGER,
    bytes      INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> AppResult<()> {
        let conn = self.conn.blocking_lock();
        // meta must exist before we can read the schema version from it.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        let version: i64 = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if version < 1 {
            conn.execute_batch(MIGRATION_V1)?;
            conn.execute(
                "INSERT OR REPLACE INTO meta(key, value) VALUES ('schema_version', ?1)",
                params![SCHEMA_VERSION.to_string()],
            )?;
        }
        Ok(())
    }

    /// Run a blocking closure against the connection on a blocking thread.
    pub async fn run<F, R>(&self, f: F) -> AppResult<R>
    where
        F: FnOnce(&Connection) -> AppResult<R> + Send + 'static,
        R: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            f(&conn)
        })
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("db task join: {e}")))?
    }

    pub async fn meta_get(&self, key: &str) -> AppResult<Option<String>> {
        let key = key.to_string();
        self.run(move |c| {
            Ok(c
                .query_row(
                    "SELECT value FROM meta WHERE key = ?1",
                    params![key],
                    |r| r.get(0),
                )
                .optional()?)
        })
        .await
    }

    pub async fn meta_set(&self, key: &str, value: &str) -> AppResult<()> {
        let (key, value) = (key.to_string(), value.to_string());
        self.run(move |c| {
            c.execute(
                "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
                params![key, value],
            )?;
            Ok(())
        })
        .await
    }

    // ---- groups ----

    pub async fn group_list(&self) -> AppResult<Vec<Group>> {
        self.run(|c| {
            let mut st = c.prepare(
                "SELECT id, name, parent_id, sort FROM groups ORDER BY sort, name",
            )?;
            let rows = st
                .query_map([], |r| {
                    Ok(Group {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        parent_id: r.get(2)?,
                        sort: r.get(3)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
    }

    pub async fn group_upsert(&self, g: &Group) -> AppResult<()> {
        let g = g.clone();
        self.run(move |c| {
            c.execute(
                "INSERT INTO groups(id, name, parent_id, sort) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name,
                    parent_id = excluded.parent_id, sort = excluded.sort",
                params![g.id, g.name, g.parent_id, g.sort],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn group_delete(&self, id: &str) -> AppResult<()> {
        let id = id.to_string();
        self.run(move |c| {
            c.execute("DELETE FROM groups WHERE id = ?1", params![id])?;
            // Orphaned sessions keep existing but ungrouped.
            c.execute(
                "UPDATE sessions SET group_id = NULL WHERE group_id = ?1",
                params![id],
            )?;
            Ok(())
        })
        .await
    }

    // ---- credentials ----

    pub async fn credential_list(&self) -> AppResult<Vec<CredentialMeta>> {
        self.run(|c| {
            let mut st = c.prepare(
                "SELECT id, name, kind, username, secret_enc IS NOT NULL, created_at, updated_at
                 FROM credentials ORDER BY name",
            )?;
            let rows = st
                .query_map([], |r| {
                    Ok(CredentialMeta {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        kind: r.get(2)?,
                        username: r.get(3)?,
                        has_secret: r.get::<_, i64>(4)? != 0,
                        created_at: r.get(5)?,
                        updated_at: r.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
    }

    pub async fn credential_insert(
        &self,
        id: &str,
        name: &str,
        kind: &str,
        username: &str,
        secret_enc: Option<&str>,
    ) -> AppResult<()> {
        let (id, name, kind, username) = (
            id.to_string(),
            name.to_string(),
            kind.to_string(),
            username.to_string(),
        );
        let secret_enc = secret_enc.map(|s| s.to_string());
        self.run(move |c| {
            let now = now_ms();
            c.execute(
                "INSERT INTO credentials(id, name, kind, username, secret_enc, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![id, name, kind, username, secret_enc, now],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn credential_update(
        &self,
        id: &str,
        name: &str,
        kind: &str,
        username: &str,
        secret_enc: Option<Option<String>>,
    ) -> AppResult<()> {
        let (id, name, kind, username) = (
            id.to_string(),
            name.to_string(),
            kind.to_string(),
            username.to_string(),
        );
        self.run(move |c| {
            c.execute(
                "UPDATE credentials SET name = ?2, kind = ?3, username = ?4, updated_at = ?5
                 WHERE id = ?1",
                params![id, name, kind, username, now_ms()],
            )?;
            if let Some(sec) = secret_enc {
                c.execute(
                    "UPDATE credentials SET secret_enc = ?2, updated_at = ?3 WHERE id = ?1",
                    params![id, sec, now_ms()],
                )?;
            }
            Ok(())
        })
        .await
    }

    /// Replace only the encrypted secret blob (used by master-password changes).
    pub async fn credential_set_secret(&self, id: &str, secret_enc: &str) -> AppResult<()> {
        let (id, secret_enc) = (id.to_string(), secret_enc.to_string());
        self.run(move |c| {
            c.execute(
                "UPDATE credentials SET secret_enc = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, secret_enc, now_ms()],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn credential_secret_enc(&self, id: &str) -> AppResult<Option<String>> {
        let id = id.to_string();
        self.run(move |c| {
            Ok(c
                .query_row(
                    "SELECT secret_enc FROM credentials WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?
                .flatten())
        })
        .await
    }

    pub async fn credential_delete(&self, id: &str) -> AppResult<()> {
        let id = id.to_string();
        self.run(move |c| {
            c.execute("DELETE FROM credentials WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await
    }

    // ---- sessions ----

    pub async fn session_list(&self) -> AppResult<Vec<SessionRecord>> {
        self.run(|c| {
            let mut st = c.prepare(
                "SELECT id, group_id, name, protocol, host, port, username,
                        credential_id, options, sort, created_at, updated_at
                 FROM sessions ORDER BY sort, name",
            )?;
            let rows = st
                .query_map([], |r| {
                    Ok(SessionRecord {
                        id: r.get(0)?,
                        group_id: r.get(1)?,
                        name: r.get(2)?,
                        protocol: r.get(3)?,
                        host: r.get(4)?,
                        port: r.get(5)?,
                        username: r.get(6)?,
                        credential_id: r.get(7)?,
                        options: r.get(8)?,
                        sort: r.get(9)?,
                        created_at: r.get(10)?,
                        updated_at: r.get(11)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
    }

    pub async fn session_upsert(&self, s: &SessionRecord) -> AppResult<()> {
        let mut s = s.clone();
        s.updated_at = now_ms();
        if s.created_at == 0 {
            s.created_at = s.updated_at;
        }
        self.run(move |c| {
            c.execute(
                "INSERT INTO sessions(id, group_id, name, protocol, host, port, username,
                                     credential_id, options, sort, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(id) DO UPDATE SET
                    group_id = excluded.group_id, name = excluded.name,
                    protocol = excluded.protocol, host = excluded.host, port = excluded.port,
                    username = excluded.username, credential_id = excluded.credential_id,
                    options = excluded.options, sort = excluded.sort,
                    updated_at = excluded.updated_at",
                params![
                    s.id, s.group_id, s.name, s.protocol, s.host, s.port, s.username,
                    s.credential_id, s.options, s.sort, s.created_at, s.updated_at
                ],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn session_delete(&self, id: &str) -> AppResult<()> {
        let id = id.to_string();
        self.run(move |c| {
            c.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await
    }

    // ---- tunnels ----

    pub async fn tunnel_list(&self, session_id: Option<String>) -> AppResult<Vec<TunnelRecord>> {
        self.run(move |c| {
            fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<TunnelRecord> {
                Ok(TunnelRecord {
                    id: r.get(0)?,
                    session_id: r.get(1)?,
                    name: r.get(2)?,
                    kind: r.get(3)?,
                    listen_host: r.get(4)?,
                    listen_port: r.get(5)?,
                    target_host: r.get(6)?,
                    target_port: r.get(7)?,
                    autostart: r.get::<_, i64>(8)? != 0,
                })
            }
            let out = match &session_id {
                Some(sid) => {
                    let mut st = c.prepare(
                        "SELECT id, session_id, name, kind, listen_host, listen_port,
                                target_host, target_port, autostart
                         FROM tunnels WHERE session_id = ?1",
                    )?;
                    let rows = st
                        .query_map(params![sid], map)?
                        .collect::<Result<Vec<_>, _>>()?;
                    rows
                }
                None => {
                    let mut st = c.prepare(
                        "SELECT id, session_id, name, kind, listen_host, listen_port,
                                target_host, target_port, autostart
                         FROM tunnels",
                    )?;
                    let rows = st.query_map([], map)?.collect::<Result<Vec<_>, _>>()?;
                    rows
                }
            };
            Ok(out)
        })
        .await
    }

    pub async fn tunnel_upsert(&self, t: &TunnelRecord) -> AppResult<()> {
        let t = t.clone();
        self.run(move |c| {
            c.execute(
                "INSERT INTO tunnels(id, session_id, name, kind, listen_host, listen_port,
                                    target_host, target_port, autostart)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                    session_id = excluded.session_id, name = excluded.name,
                    kind = excluded.kind, listen_host = excluded.listen_host,
                    listen_port = excluded.listen_port, target_host = excluded.target_host,
                    target_port = excluded.target_port, autostart = excluded.autostart",
                params![
                    t.id,
                    t.session_id,
                    t.name,
                    t.kind,
                    t.listen_host,
                    t.listen_port,
                    t.target_host,
                    t.target_port,
                    t.autostart as i64,
                ],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn tunnel_delete(&self, id: &str) -> AppResult<()> {
        let id = id.to_string();
        self.run(move |c| {
            c.execute("DELETE FROM tunnels WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await
    }

    // ---- quick commands ----

    pub async fn quick_command_list(&self) -> AppResult<Vec<QuickCommand>> {
        self.run(|c| {
            let mut st = c.prepare(
                "SELECT id, label, content, grp, sort FROM quick_commands ORDER BY grp, sort, label",
            )?;
            let rows = st
                .query_map([], |r| {
                    Ok(QuickCommand {
                        id: r.get(0)?,
                        label: r.get(1)?,
                        content: r.get(2)?,
                        group: r.get(3)?,
                        sort: r.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
    }

    pub async fn quick_command_upsert(&self, q: &QuickCommand) -> AppResult<()> {
        let q = q.clone();
        self.run(move |c| {
            c.execute(
                "INSERT INTO quick_commands(id, label, content, grp, sort) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET label = excluded.label,
                    content = excluded.content, grp = excluded.grp, sort = excluded.sort",
                params![q.id, q.label, q.content, q.group, q.sort],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn quick_command_delete(&self, id: &str) -> AppResult<()> {
        let id = id.to_string();
        self.run(move |c| {
            c.execute("DELETE FROM quick_commands WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await
    }

    // ---- triggers ----

    pub async fn trigger_list(&self) -> AppResult<Vec<TriggerRule>> {
        self.run(|c| {
            let mut st = c.prepare(
                "SELECT id, session_id, name, pattern, is_regex, action, action_param, enabled
                 FROM triggers ORDER BY name",
            )?;
            let rows = st
                .query_map([], |r| {
                    Ok(TriggerRule {
                        id: r.get(0)?,
                        session_id: r.get(1)?,
                        name: r.get(2)?,
                        pattern: r.get(3)?,
                        is_regex: r.get::<_, i64>(4)? != 0,
                        action: r.get(5)?,
                        action_param: r.get(6)?,
                        enabled: r.get::<_, i64>(7)? != 0,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
    }

    pub async fn trigger_upsert(&self, t: &TriggerRule) -> AppResult<()> {
        let t = t.clone();
        self.run(move |c| {
            c.execute(
                "INSERT INTO triggers(id, session_id, name, pattern, is_regex, action, action_param, enabled)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET session_id = excluded.session_id,
                    name = excluded.name, pattern = excluded.pattern,
                    is_regex = excluded.is_regex, action = excluded.action,
                    action_param = excluded.action_param, enabled = excluded.enabled",
                params![
                    t.id,
                    t.session_id,
                    t.name,
                    t.pattern,
                    t.is_regex as i64,
                    t.action,
                    t.action_param,
                    t.enabled as i64,
                ],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn trigger_delete(&self, id: &str) -> AppResult<()> {
        let id = id.to_string();
        self.run(move |c| {
            c.execute("DELETE FROM triggers WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await
    }

    // ---- log files ----

    pub async fn log_add(&self, id: &str, path: &str, started_at: i64) -> AppResult<()> {
        let id = id.to_string();
        let path = path.to_string();
        self.run(move |c| {
            c.execute(
                "INSERT INTO log_files(id, session_id, path, started_at, ended_at, bytes)
                 VALUES (?1, NULL, ?2, ?3, NULL, 0)",
                params![id, path, started_at],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn log_finish(&self, id: &str, path: &str, bytes: i64, ended_at: i64) -> AppResult<()> {
        let id = id.to_string();
        let path = path.to_string();
        self.run(move |c| {
            c.execute(
                "UPDATE log_files SET ended_at = ?2, bytes = ?3 WHERE id = ?1",
                params![id, ended_at, bytes],
            )?;
            // Fallback for rows inserted before this build (or failed insert):
            c.execute(
                "INSERT OR IGNORE INTO log_files(id, session_id, path, started_at, ended_at, bytes)
                 VALUES (?1, NULL, ?2, ?3, ?3, ?4)",
                params![id, path, ended_at, bytes],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn log_list(&self) -> AppResult<Vec<LogFileRecord>> {
        self.run(|c| {
            let mut st = c.prepare(
                "SELECT id, session_id, path, started_at, ended_at, bytes
                 FROM log_files ORDER BY started_at DESC",
            )?;
            let rows = st
                .query_map([], |r| {
                    Ok(LogFileRecord {
                        id: r.get(0)?,
                        session_id: r.get(1)?,
                        path: r.get(2)?,
                        started_at: r.get(3)?,
                        ended_at: r.get(4)?,
                        bytes: r.get(5)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
    }

    pub async fn log_delete(&self, id: &str) -> AppResult<()> {
        let id = id.to_string();
        self.run(move |c| {
            c.execute("DELETE FROM log_files WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await
    }

    // ---- settings ----

    pub async fn setting_get(&self, key: &str) -> AppResult<Option<String>> {
        let key = key.to_string();
        self.run(move |c| {
            Ok(c
                .query_row(
                    "SELECT value FROM settings WHERE key = ?1",
                    params![key],
                    |r| r.get(0),
                )
                .optional()?)
        })
        .await
    }

    pub async fn setting_set(&self, key: &str, value: &str) -> AppResult<()> {
        let (key, value) = (key.to_string(), value.to_string());
        self.run(move |c| {
            c.execute(
                "INSERT OR REPLACE INTO settings(key, value) VALUES (?1, ?2)",
                params![key, value],
            )?;
            Ok(())
        })
        .await
    }
}
