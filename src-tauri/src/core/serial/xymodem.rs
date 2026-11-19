//! Self-implemented XModem / XModem-CRC / XModem-1K / YModem (batch, 1K)
//! transfers over a blocking serial port.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serialport::SerialPort;

use crate::core::terminal::pipe::SessionPipe;

const SOH: u8 = 0x01;
const STX: u8 = 0x02;
const EOT: u8 = 0x04;
const ACK: u8 = 0x06;
const NAK: u8 = 0x15;
const CAN: u8 = 0x18;
const CCHAR: u8 = b'C';
const CP: u8 = 0x1A; // padding (CPMEOF)

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub enum XyVariant {
    /// XModem, 128-byte blocks, additive checksum.
    XModem,
    /// XModem with CRC-16.
    XModemCrc,
    /// XModem CRC with 1024-byte blocks.
    XModem1K,
    /// YModem batch: block0 file header + 1K blocks.
    YModem,
}

impl XyVariant {
    fn uses_crc(self) -> bool {
        !matches!(self, XyVariant::XModem)
    }
    fn is_batch(self) -> bool {
        matches!(self, XyVariant::YModem)
    }
}

fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn write_byte(port: &mut Box<dyn SerialPort>, b: u8) -> Result<(), String> {
    port.write_all(&[b])
        .and_then(|_| port.flush())
        .map_err(|e| format!("serial write: {e}"))
}

/// Read one byte, polling through the port's 100ms timeout until the
/// deadline elapses or cancel is set.
fn read_byte(
    port: &mut Box<dyn SerialPort>,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<Option<u8>, String> {
    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 1];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        match port.read(&mut buf) {
            Ok(1) => return Ok(Some(buf[0])),
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(format!("serial read: {e}")),
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
    }
}

fn read_exact(
    port: &mut Box<dyn SerialPort>,
    out: &mut [u8],
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let mut got = 0;
    while got < out.len() {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        match port.read(&mut out[got..]) {
            Ok(n) if n > 0 => got += n,
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(format!("serial read: {e}")),
        }
        if Instant::now() >= deadline {
            return Err("xymodem: read timeout".into());
        }
    }
    Ok(())
}

fn sanitize(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("recv.bin");
    let s: String = base
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '\0') { '_' } else { c })
        .collect();
    if s.is_empty() {
        "recv.bin".into()
    } else {
        s
    }
}

/// Receive file(s). XModem* variants receive exactly one file named by the
/// caller via the block0-less protocol — we fall back to a timestamp name.
/// YModem uses block0 filenames. Returns saved paths.
pub fn xymodem_recv(
    port: &mut Box<dyn SerialPort>,
    dir: &Path,
    variant: XyVariant,
    pipe: &SessionPipe,
    cancel: &AtomicBool,
) -> Result<Vec<String>, String> {
    let _ = std::fs::create_dir_all(dir);
    let crc = variant.uses_crc();
    let mut saved: Vec<String> = Vec::new();

    loop {
        // ---- handshake: ask the sender to begin ----
        let start_char = if crc { CCHAR } else { NAK };
        let mut first = None;
        for _ in 0..10 {
            write_byte(port, start_char)?;
            if let Some(b) = read_byte(port, Duration::from_secs(3), cancel)? {
                first = Some(b);
                break;
            }
        }
        let mut pending = match first {
            Some(b @ (SOH | STX)) => b,
            Some(EOT) => {
                write_byte(port, ACK)?;
                return Ok(saved);
            }
            Some(CAN) => return Err("peer cancelled".into()),
            Some(b) => return Err(format!("xymodem: unexpected start byte 0x{b:02x}")),
            None => return Err("xymodem: sender did not respond".into()),
        };

        // ---- per-file state ----
        let mut expected: u8 = if variant.is_batch() { 0 } else { 1 };
        let mut file: Option<std::fs::File> = None;
        let mut cur_path = PathBuf::new();
        let mut cur_size: Option<u64> = None;
        let mut written: u64 = 0;
        let mut retries = 0u32;

        loop {
            if pending == EOT {
                write_byte(port, ACK)?;
                if let Some(f) = file.take() {
                    drop(f);
                    pipe.push_modem(
                        "fileDone",
                        Some(cur_path.to_string_lossy().to_string()),
                        Some(written),
                        cur_size,
                    );
                    saved.push(cur_path.to_string_lossy().to_string());
                }
                if variant.is_batch() {
                    // Next file (or end-of-batch empty block0) follows.
                    write_byte(port, CCHAR)?;
                    match read_byte(port, Duration::from_secs(10), cancel)? {
                        Some(b @ (SOH | STX)) => {
                            expected = 0;
                            pending = b;
                            retries = 0;
                            continue;
                        }
                        Some(CAN) => return Err("peer cancelled".into()),
                        _ => return Ok(saved),
                    }
                }
                return Ok(saved);
            }

            let block_len = if pending == STX { 1024 } else { 128 };
            let mut hdr = [0u8; 2];
            if let Err(e) = read_exact(port, &mut hdr, Duration::from_secs(10), cancel) {
                return Err(e);
            }
            let (bn, bnc) = (hdr[0], hdr[1]);
            if bn != !bnc {
                retries += 1;
                if retries > 10 {
                    return Err("xymodem: too many bad blocks".into());
                }
                write_byte(port, NAK)?;
                pending = match read_byte(port, Duration::from_secs(10), cancel)? {
                    Some(b) => b,
                    None => return Err("xymodem: timeout after NAK".into()),
                };
                continue;
            }

            let mut data = vec![0u8; block_len + if crc { 2 } else { 1 }];
            read_exact(port, &mut data, Duration::from_secs(10), cancel)?;
            let (payload, check) = data.split_at(block_len);
            let ok = if crc {
                let want = u16::from_be_bytes([check[0], check[1]]);
                crc16(payload) == want
            } else {
                payload
                    .iter()
                    .fold(0u8, |a, &b| a.wrapping_add(b))
                    == check[0]
            };

            if !ok || (bn != expected && bn != expected.wrapping_sub(1)) {
                retries += 1;
                if retries > 10 {
                    return Err("xymodem: too many bad blocks".into());
                }
                write_byte(port, NAK)?;
            } else if bn == expected {
                retries = 0;
                if variant.is_batch() && bn == 0 {
                    // File header block: "name\0size mtime mode serial\0"
                    let end = payload
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(payload.len());
                    let name = String::from_utf8_lossy(&payload[..end]).to_string();
                    if let Some(f) = file.take() {
                        drop(f);
                    }
                    if name.is_empty() {
                        // End of batch.
                        write_byte(port, ACK)?;
                        return Ok(saved);
                    }
                    let size_field_end = payload
                        .iter()
                        .skip(end + 1)
                        .position(|&b| b == 0)
                        .map(|p| end + 1 + p)
                        .unwrap_or(payload.len());
                    let fields = String::from_utf8_lossy(&payload[end + 1..size_field_end]);
                    cur_size = fields
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse::<u64>().ok());
                    let safe = sanitize(&name);
                    cur_path = dir.join(&safe);
                    file = Some(
                        std::fs::File::create(&cur_path)
                            .map_err(|e| format!("create {}: {e}", cur_path.display()))?,
                    );
                    written = 0;
                    pipe.push_modem(
                        "fileStart",
                        Some(format!("{safe} ({:?} B)", cur_size)),
                        Some(0),
                        cur_size,
                    );
                } else {
                    if file.is_none() {
                        // XModem single-file: no header block exists.
                        let safe = format!(
                            "xymodem-{}.bin",
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0)
                        );
                        cur_path = dir.join(&safe);
                        file = Some(
                            std::fs::File::create(&cur_path)
                                .map_err(|e| format!("create {}: {e}", cur_path.display()))?,
                        );
                        pipe.push_modem("fileStart", Some(safe), Some(0), None);
                    }
                    let f = file.as_mut().unwrap();
                    f.write_all(payload)
                        .map_err(|e| format!("file write: {e}"))?;
                    written += payload.len() as u64;
                    pipe.push_modem("progress", None, Some(written), cur_size);
                }
                expected = expected.wrapping_add(1);
                write_byte(port, ACK)?;
            } else {
                // Duplicate block: re-ACK without writing.
                write_byte(port, ACK)?;
            }

            pending = match read_byte(port, Duration::from_secs(10), cancel)? {
                Some(b) => b,
                None => return Err("xymodem: timeout waiting next block".into()),
            };
            if pending == CAN {
                return Err("peer cancelled".into());
            }
            if pending != SOH && pending != STX && pending != EOT {
                retries += 1;
                if retries > 10 {
                    return Err("xymodem: garbage between blocks".into());
                }
                write_byte(port, NAK)?;
            }
        }
    }
}

/// Send one file (YModem sends it as a one-file batch).
pub fn xymodem_send(
    port: &mut Box<dyn SerialPort>,
    path: &str,
    variant: XyVariant,
    pipe: &SessionPipe,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let p = Path::new(path);
    let meta = std::fs::metadata(p).map_err(|e| format!("stat {path}: {e}"))?;
    if meta.is_dir() {
        return Err(format!("{path} 是目录,仅支持文件"));
    }
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or("bad file name")?;
    let size = meta.len();
    let mut file = std::fs::File::open(p).map_err(|e| format!("open {path}: {e}"))?;
    let crc = variant.uses_crc();

    pipe.push_modem("fileStart", Some(format!("{name} ({size} B)")), Some(0), Some(size));

    // ---- wait for receiver ----
    let want = if crc { CCHAR } else { NAK };
    let mut got = None;
    for _ in 0..120 {
        if let Some(b) = read_byte(port, Duration::from_millis(500), cancel)? {
            if b == want || (crc && b == NAK) || b == CCHAR {
                got = Some(b);
                break;
            }
        }
    }
    let start = got.ok_or("xymodem: receiver did not respond")?;
    let use_crc = crc && start != NAK;

    // ---- YModem block0 ----
    let mut block: u8 = 1;
    if variant.is_batch() {
        let mut b0 = vec![CP; 128];
        let hdr = format!("{name}\0{size} \0");
        b0[..hdr.len()].copy_from_slice(hdr.as_bytes());
        send_block(port, 0, &b0, true, cancel)?;
        wait_ack(port, cancel)?;
        // Receiver requests file data.
        loop {
            match read_byte(port, Duration::from_secs(10), cancel)? {
                Some(CCHAR) => break,
                Some(CAN) => return Err("peer cancelled".into()),
                _ => {}
            }
        }
        block = 1;
    }

    // ---- data blocks ----
    let mut sent: u64 = 0;
    let mut block_no = block;
    loop {
        let cap = 1024usize;
        let mut buf = vec![CP; cap];
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("file read: {e}"))?;
        if n == 0 {
            break;
        }
        buf.truncate(if n <= 128 { 128 } else { 1024 });
        let mut attempts = 0;
        loop {
            send_block(port, block_no, &buf, use_crc, cancel)?;
            match wait_ack(port, cancel) {
                Ok(()) => break,
                Err(e) => {
                    attempts += 1;
                    if attempts > 10 {
                        return Err(e);
                    }
                }
            }
        }
        sent += buf.len() as u64;
        block_no = block_no.wrapping_add(1);
        pipe.push_modem("progress", Some(name.clone()), Some(sent.min(size)), Some(size));
    }

    // ---- EOT ----
    let mut eot_acks = 0;
    for _ in 0..10 {
        write_byte(port, EOT)?;
        match read_byte(port, Duration::from_secs(10), cancel)? {
            Some(ACK) => {
                eot_acks += 1;
                break;
            }
            Some(CAN) => return Err("peer cancelled".into()),
            _ => {}
        }
    }
    if eot_acks == 0 {
        return Err("xymodem: EOT not acknowledged".into());
    }

    // ---- YModem end-of-batch ----
    if variant.is_batch() {
        loop {
            match read_byte(port, Duration::from_secs(10), cancel)? {
                Some(CCHAR) => break,
                Some(CAN) => return Err("peer cancelled".into()),
                _ => {}
            }
        }
        let empty = vec![CP; 128];
        send_block(port, 0, &empty, true, cancel)?;
        wait_ack(port, cancel)?;
    }

    Ok(())
}

fn send_block(
    port: &mut Box<dyn SerialPort>,
    block_no: u8,
    data: &[u8],
    crc: bool,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    let mut frame = Vec::with_capacity(data.len() + 5);
    frame.push(if data.len() > 128 { STX } else { SOH });
    frame.push(block_no);
    frame.push(!block_no);
    frame.extend_from_slice(data);
    if crc {
        frame.extend_from_slice(&crc16(data).to_be_bytes());
    } else {
        frame.push(data.iter().fold(0u8, |a, &b| a.wrapping_add(b)));
    }
    port.write_all(&frame)
        .and_then(|_| port.flush())
        .map_err(|e| format!("serial write: {e}"))
}

fn wait_ack(port: &mut Box<dyn SerialPort>, cancel: &AtomicBool) -> Result<(), String> {
    match read_byte(port, Duration::from_secs(10), cancel)? {
        Some(ACK) => Ok(()),
        Some(CAN) => Err("peer cancelled".into()),
        Some(_) => Err("xymodem: block rejected".into()),
        None => Err("xymodem: ACK timeout".into()),
    }
}
