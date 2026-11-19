//! Trigger engine: scans session output line-by-line (ANSI-stripped) and
//! fires configured actions when a pattern matches.
//!
//! Actions: `highlight` (frontend decoration flash), `alert` (notification),
//! `send` (write command back into the session), `script` (python feature).

use std::sync::Mutex;

use regex::bytes::Regex;

use crate::core::db::models::TriggerRule;

/// One fired trigger action, queued for async dispatch.
#[derive(Debug, Clone)]
pub struct TriggerFire {
    pub name: String,
    /// highlight | alert | send | script
    pub action: String,
    pub param: String,
    /// The matched text (for display / templates).
    pub matched: String,
}

struct Compiled {
    name: String,
    re: Regex,
    action: String,
    param: String,
}

/// Incremental line scanner. `feed()` is called from the pipe byte tap
/// (sync context); fired actions are pushed into an mpsc for async handling.
pub struct TriggerEngine {
    rules: Vec<Compiled>,
    /// Partial-line buffer with ANSI escape sequences removed.
    buf: Mutex<Vec<u8>>,
    tx: tokio::sync::mpsc::UnboundedSender<TriggerFire>,
}

const MAX_LINE: usize = 8192;
const MAX_FIRES_PER_LINE: usize = 8;

impl TriggerEngine {
    /// Build an engine from DB rules. Invalid regexes are skipped.
    pub fn build(
        rules: &[TriggerRule],
        tx: tokio::sync::mpsc::UnboundedSender<TriggerFire>,
    ) -> Option<std::sync::Arc<Self>> {
        let compiled: Vec<Compiled> = rules
            .iter()
            .filter(|r| r.enabled && !r.pattern.is_empty())
            .filter_map(|r| {
                let pat = if r.is_regex {
                    r.pattern.clone()
                } else {
                    regex::escape(&r.pattern)
                };
                Regex::new(&format!("(?i){pat}")).ok().map(|re| Compiled {
                    name: r.name.clone(),
                    re,
                    action: r.action.clone(),
                    param: r.action_param.clone(),
                })
            })
            .collect();
        if compiled.is_empty() {
            return None;
        }
        Some(std::sync::Arc::new(Self {
            rules: compiled,
            buf: Mutex::new(Vec::with_capacity(1024)),
            tx,
        }))
    }

    /// Feed raw output bytes. Cheap enough for the pipe hot path: escape
    /// stripping + newline scan, regex only on completed lines.
    pub fn feed(&self, bytes: &[u8]) {
        let mut buf = match self.buf.lock() {
            Ok(b) => b,
            Err(_) => return,
        };
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            match b {
                // Strip CSI / OSC / simple escapes.
                0x1b => {
                    i += 1;
                    if i < bytes.len() {
                        match bytes[i] {
                            b'[' => {
                                // CSI: skip until final byte 0x40..=0x7e
                                i += 1;
                                while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                                    i += 1;
                                }
                                i += 1;
                            }
                            b']' => {
                                // OSC: skip until BEL or ST (ESC \)
                                i += 1;
                                while i < bytes.len() {
                                    if bytes[i] == 0x07 {
                                        i += 1;
                                        break;
                                    }
                                    if bytes[i] == 0x1b
                                        && i + 1 < bytes.len()
                                        && bytes[i + 1] == b'\\'
                                    {
                                        i += 2;
                                        break;
                                    }
                                    i += 1;
                                }
                            }
                            _ => i += 1,
                        }
                    }
                }
                b'\n' => {
                    self.scan_line(&buf);
                    buf.clear();
                    i += 1;
                }
                0x08 | 0x0d => {
                    // Backspace / CR: drop (CR resets the visual line; keeping
                    // it simple we just ignore both).
                    i += 1;
                }
                _ => {
                    if buf.len() < MAX_LINE {
                        buf.push(b);
                    }
                    i += 1;
                }
            }
        }
    }

    fn scan_line(&self, line: &[u8]) {
        if line.is_empty() {
            return;
        }
        let mut fires = 0;
        for rule in &self.rules {
            if fires >= MAX_FIRES_PER_LINE {
                break;
            }
            if let Some(m) = rule.re.find(line) {
                let matched = String::from_utf8_lossy(&line[m.start()..m.end()]).into_owned();
                let _ = self.tx.send(TriggerFire {
                    name: rule.name.clone(),
                    action: rule.action.clone(),
                    param: rule.param.clone(),
                    matched,
                });
                fires += 1;
            }
        }
    }
}

/// Expand `{match}` / `{line}`-style template variables in action params.
pub fn expand_template(param: &str, fire: &TriggerFire) -> String {
    param.replace("{match}", &fire.matched)
}
