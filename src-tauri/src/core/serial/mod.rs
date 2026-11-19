//! Serial port sessions: parameterised UART link plus ZModem (auto rz/sz
//! detect) and self-implemented X/YModem transfers.

pub mod xymodem;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serialport::SerialPort;
use zmodem2::{State as ZState, Transmission};

use crate::core::terminal::pipe::SessionPipe;

use xymodem::XyVariant;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialConfig {
    pub port: String,
    #[serde(default = "default_baud")]
    pub baud_rate: u32,
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    /// none | odd | even
    #[serde(default)]
    pub parity: String,
    /// none | hardware | software
    #[serde(default)]
    pub flow_control: String,
    /// Directory for auto-received files (rz). Defaults to ~/Downloads.
    #[serde(default)]
    pub download_dir: Option<String>,
}

fn default_baud() -> u32 {
    9600
}
fn default_data_bits() -> u8 {
    8
}
fn default_stop_bits() -> u8 {
    1
}

/// Operations the control plane can push into the serial IO thread.
#[derive(Debug)]
pub enum SerialOp {
    Input(Vec<u8>),
    ZSend(Vec<String>),
    XSend { path: String, variant: XyVariant },
    XRecv { variant: XyVariant },
    Close,
}

#[derive(Clone)]
pub struct SerialHandle {
    pub op_tx: std::sync::mpsc::Sender<SerialOp>,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortInfo {
    pub name: String,
    pub description: String,
}

pub fn list_ports() -> Vec<SerialPortInfo> {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let description = match &p.port_type {
                serialport::SerialPortType::UsbPort(u) => {
                    format!("USB vid:{:04x} pid:{:04x} {}", u.vid, u.pid, u.product.clone().unwrap_or_default())
                }
                serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
                serialport::SerialPortType::PciPort => "PCI".into(),
                _ => "Serial".into(),
            };
            SerialPortInfo {
                name: p.port_name,
                description,
            }
        })
        .collect()
}

pub fn open_port(cfg: &SerialConfig) -> crate::error::AppResult<Box<dyn SerialPort>> {
    let data_bits = match cfg.data_bits {
        5 => serialport::DataBits::Five,
        6 => serialport::DataBits::Six,
        7 => serialport::DataBits::Seven,
        _ => serialport::DataBits::Eight,
    };
    let stop_bits = match cfg.stop_bits {
        2 => serialport::StopBits::Two,
        _ => serialport::StopBits::One,
    };
    let parity = match cfg.parity.as_str() {
        "odd" => serialport::Parity::Odd,
        "even" => serialport::Parity::Even,
        _ => serialport::Parity::None,
    };
    let flow = match cfg.flow_control.as_str() {
        "hardware" => serialport::FlowControl::Hardware,
        "software" => serialport::FlowControl::Software,
        _ => serialport::FlowControl::None,
    };
    let port = serialport::new(&cfg.port, cfg.baud_rate)
        .data_bits(data_bits)
        .stop_bits(stop_bits)
        .parity(parity)
        .flow_control(flow)
        .timeout(std::time::Duration::from_millis(100))
        .open()
        .map_err(|e| crate::error::AppError::Config(format!("open {}: {e}", cfg.port)))?;
    Ok(port)
}

fn default_download_dir(cfg: &SerialConfig) -> PathBuf {
    if let Some(d) = &cfg.download_dir {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    dirs_like_downloads()
}

fn dirs_like_downloads() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .map(|h| h.join("Downloads"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// zmodem2 port adapter: serialport timeouts become "no data yet".
struct PortIo<'a> {
    port: &'a mut Box<dyn SerialPort>,
}

impl zmodem2::Read for PortIo<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<Option<u32>, zmodem2::Error> {
        match std::io::Read::read(&mut *self.port, buf) {
            Ok(0) => Ok(None),
            Ok(n) => Ok(Some(n as u32)),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Ok(None),
            Err(e) => Err(zmodem2::Error::Read(zmodem2::String::from(e.to_string().as_str()))),
        }
    }

    fn read_byte(&mut self) -> Result<Option<u8>, zmodem2::Error> {
        let mut b = [0u8; 1];
        match std::io::Read::read(&mut *self.port, &mut b) {
            Ok(1) => Ok(Some(b[0])),
            Ok(_) => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Ok(None),
            Err(e) => Err(zmodem2::Error::Read(zmodem2::String::from(e.to_string().as_str()))),
        }
    }
}

impl zmodem2::Write for PortIo<'_> {
    fn write_all(&mut self, buf: &[u8]) -> Result<Option<()>, zmodem2::Error> {
        std::io::Write::write_all(&mut *self.port, buf)
            .and_then(|_| std::io::Write::flush(&mut *self.port))
            .map(Some)
            .map_err(|e| zmodem2::Error::Write(zmodem2::String::from(e.to_string().as_str())))
    }
}

/// File sink for zmodem receive: silently discards before the first ZFILE
/// metadata has been parsed (no data is written before that anyway).
enum Sink {
    Closed,
    Open(std::fs::File),
}

impl zmodem2::Write for Sink {
    fn write_all(&mut self, buf: &[u8]) -> Result<Option<()>, zmodem2::Error> {
        match self {
            Sink::Closed => Ok(Some(())),
            Sink::Open(f) => std::io::Write::write_all(f, buf)
                .map(Some)
                .map_err(|e| zmodem2::Error::Write(zmodem2::String::from(
                    e.to_string().as_str(),
                ))),
        }
    }
}

const IDLE_LIMIT: u32 = 300; // ~30s at 100ms port timeout granularity

/// Receive one ZModem batch into `dir`. Blocks the IO thread for the
/// duration; `cancel` aborts.
fn zmodem_recv(
    port: &mut Box<dyn SerialPort>,
    dir: &Path,
    pipe: &SessionPipe,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let _ = std::fs::create_dir_all(dir);
    let mut tx = Transmission::new();
    let mut sink = Sink::Closed;
    let mut cur_name = String::new();
    let mut idle = 0u32;
    let mut io = PortIo { port };

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let prev = tx.state();
        match tx.receive(&mut io, &mut sink) {
            Ok(Some(())) => idle = 0,
            Ok(None) => {
                idle += 1;
                if idle > IDLE_LIMIT {
                    return Err("zmodem receive timed out".into());
                }
            }
            Err(e) => {
                // Protocol-level errors after internal ZNAK: give up.
                return Err(format!("zmodem: {e}"));
            }
        }

        let now = tx.state();
        if now == ZState::FileBegin && prev != ZState::FileBegin {
            if let Sink::Open(f) = std::mem::replace(&mut sink, Sink::Closed) {
                drop(f);
                pipe.push_modem("fileDone", Some(cur_name.clone()), None, None);
            }
            if !tx.file_name().is_empty() {
                let raw = String::from_utf8_lossy(tx.file_name()).to_string();
                let safe = raw
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or("recv.bin")
                    .replace(['/', '\\', ':'], "_");
                cur_name = safe.clone();
                let path = dir.join(&safe);
                match std::fs::File::create(&path) {
                    Ok(f) => {
                        sink = Sink::Open(f);
                        pipe.push_modem(
                            "fileStart",
                            Some(format!("{} ({} B)", safe, tx.file_size())),
                            Some(0),
                            Some(tx.file_size() as u64),
                        );
                    }
                    Err(e) => return Err(format!("create {}: {e}", path.display())),
                }
            }
        }
        if now == ZState::FileReadingSubpacket {
            pipe.push_modem(
                "progress",
                Some(cur_name.clone()),
                Some(tx.count() as u64),
                Some(tx.file_size() as u64),
            );
        }
        if now == ZState::SessionEnd {
            return Ok(());
        }
    }
}

/// Send files with ZModem (answers a remote `rz`).
fn zmodem_send(
    port: &mut Box<dyn SerialPort>,
    paths: &[String],
    pipe: &SessionPipe,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut tx: Option<Transmission> = None;
    let mut io = PortIo { port };

    for (i, p) in paths.iter().enumerate() {
        let path = Path::new(p);
        let meta = std::fs::metadata(path).map_err(|e| format!("stat {p}: {e}"))?;
        if meta.is_dir() {
            return Err(format!("{p} 是目录,ZMODEM 仅支持文件"));
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| format!("bad file name: {p}"))?;
        let size = meta.len() as u32;
        let mut file = std::fs::File::open(path).map_err(|e| format!("open {p}: {e}"))?;

        let mut t = match tx.take() {
            None => Transmission::set_first_file(&name, size).map_err(|e| e.to_string())?,
            Some(mut t) => {
                t.set_next_file(&name, size).map_err(|e| e.to_string())?;
                t
            }
        };
        pipe.push_modem(
            "fileStart",
            Some(format!("{name} ({size} B) [{}/{}]", i + 1, paths.len())),
            Some(0),
            Some(size as u64),
        );

        let mut idle = 0u32;
        while t.state() != ZState::FileEnd {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            match t.send(&mut io, &mut file) {
                Ok(Some(())) => idle = 0,
                Ok(None) => {
                    idle += 1;
                    if idle > IDLE_LIMIT {
                        return Err("zmodem send timed out".into());
                    }
                }
                Err(e) => return Err(format!("zmodem: {e}")),
            }
            pipe.push_modem(
                "progress",
                Some(name.clone()),
                Some(t.count() as u64),
                Some(size as u64),
            );
        }
        tx = Some(t);
    }

    let mut t = tx.ok_or("no files")?;
    let mut idle = 0u32;
    while t.state() != ZState::SessionEnd {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        match t.finish(&mut io) {
            Ok(Some(())) => idle = 0,
            Ok(None) => {
                idle += 1;
                if idle > IDLE_LIMIT {
                    return Err("zmodem finish timed out".into());
                }
            }
            Err(e) => return Err(format!("zmodem: {e}")),
        }
    }
    Ok(())
}

/// Detects ZModem hex-header signatures in the output stream.
/// ZRQINIT (`**\x18B00`) → remote wants to send (sz) → we auto-receive.
/// ZRINIT (`**\x18B01`) → remote wants to receive (rz) → user picks files.
struct ZDetect {
    window: [u8; 6],
    len: usize,
    armed: bool,
}

#[derive(PartialEq, Clone, Copy)]
enum ZEvent {
    RqInit,
    RInit,
}

impl ZDetect {
    fn new() -> Self {
        Self {
            window: [0; 6],
            len: 0,
            armed: true,
        }
    }

    fn feed(&mut self, data: &[u8]) -> Option<ZEvent> {
        if !self.armed {
            return None;
        }
        for &b in data {
            if self.len < 6 {
                self.window[self.len] = b;
                self.len += 1;
            } else {
                self.window.copy_within(1..6, 0);
                self.window[5] = b;
            }
            if self.len == 6
                && self.window[0] == b'*'
                && self.window[1] == b'*'
                && self.window[2] == 0x18
                && self.window[3] == b'B'
            {
                if self.window[4] == b'0' && self.window[5] == b'0' {
                    self.armed = false;
                    return Some(ZEvent::RqInit);
                }
                if self.window[4] == b'0' && self.window[5] == b'1' {
                    self.armed = false;
                    return Some(ZEvent::RInit);
                }
            }
        }
        None
    }

    fn rearm(&mut self) {
        self.armed = true;
        self.len = 0;
    }
}

/// The serial IO thread: owns the port. Normal mode forwards bytes both
/// ways and watches for rz/sz; transfers run inline.
pub fn serial_io_loop(
    mut port: Box<dyn SerialPort>,
    cfg: SerialConfig,
    pipe: SessionPipe,
    op_rx: std::sync::mpsc::Receiver<SerialOp>,
    op_tx: std::sync::mpsc::Sender<SerialOp>,
    cancel: Arc<AtomicBool>,
) {
    let mut detect = ZDetect::new();
    let mut buf = [0u8; 4096];

    loop {
        // Drain pending ops first.
        while let Ok(op) = op_rx.try_recv() {
            match op {
                SerialOp::Input(data) => {
                    if port.write_all(&data).is_err() {
                        pipe.push_state("disconnected", Some("serial write failed".into()));
                        return;
                    }
                }
                SerialOp::ZSend(paths) => {
                    pipe.push_modem("start", Some("ZMODEM 发送".into()), None, None);
                    match zmodem_send(&mut port, &paths, &pipe, &cancel) {
                        Ok(()) => pipe.push_modem("done", None, None, None),
                        Err(e) => pipe.push_modem("error", Some(e), None, None),
                    }
                    detect.rearm();
                    drain_inputs(&op_rx, &op_tx);
                }
                SerialOp::XSend { path, variant } => {
                    pipe.push_modem("start", Some(format!("{variant:?} 发送")), None, None);
                    match xymodem::xymodem_send(&mut port, &path, variant, &pipe, &cancel) {
                        Ok(()) => pipe.push_modem("done", None, None, None),
                        Err(e) => pipe.push_modem("error", Some(e), None, None),
                    }
                    detect.rearm();
                    drain_inputs(&op_rx, &op_tx);
                }
                SerialOp::XRecv { variant } => {
                    let dir = default_download_dir(&cfg);
                    pipe.push_modem("start", Some(format!("{variant:?} 接收")), None, None);
                    match xymodem::xymodem_recv(&mut port, &dir, variant, &pipe, &cancel) {
                        Ok(_) => pipe.push_modem("done", None, None, None),
                        Err(e) => pipe.push_modem("error", Some(e), None, None),
                    }
                    detect.rearm();
                    drain_inputs(&op_rx, &op_tx);
                }
                SerialOp::Close => return,
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return;
        }

        match port.read(&mut buf) {
            Ok(n) if n > 0 => {
                let chunk = &buf[..n];
                pipe.push_data(chunk);
                if let Some(ev) = detect.feed(chunk) {
                    match ev {
                        ZEvent::RqInit => {
                            // Remote `sz` → auto receive.
                            let dir = default_download_dir(&cfg);
                            pipe.push_modem(
                                "start",
                                Some(format!("ZMODEM 自动接收 → {}", dir.display())),
                                None,
                                None,
                            );
                            match zmodem_recv(&mut port, &dir, &pipe, &cancel) {
                                Ok(()) => pipe.push_modem("done", None, None, None),
                                Err(e) => pipe.push_modem("error", Some(e), None, None),
                            }
                            detect.rearm();
                            drain_inputs(&op_rx, &op_tx);
                        }
                        ZEvent::RInit => {
                            // Remote `rz` → ask frontend for files.
                            pipe.push_modem("zrinit", None, None, None);
                            detect.rearm();
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                pipe.push_state("disconnected", Some(format!("serial read: {e}")));
                return;
            }
        }
    }
}

/// Drop user input queued while a transfer was running; re-queue control ops.
fn drain_inputs(
    op_rx: &std::sync::mpsc::Receiver<SerialOp>,
    op_tx: &std::sync::mpsc::Sender<SerialOp>,
) {
    let mut stash = Vec::new();
    while let Ok(op) = op_rx.try_recv() {
        if !matches!(op, SerialOp::Input(_)) {
            stash.push(op);
        }
    }
    for op in stash {
        let _ = op_tx.send(op);
    }
}
