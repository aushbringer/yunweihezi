//! Minimal Telnet client (RFC 854 family) with IAC negotiation.
//!
//! Supports the options a terminal client actually needs: ECHO, SGA,
//! terminal-type (RFC 1091) and NAWS window-size reporting (RFC 1073).
//! Everything else is politely refused.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::core::session::{SessionCommand, SessionState};
use crate::core::terminal::pipe::SessionPipe;
use crate::error::AppResult;

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;

const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;
const OPT_TTYPE: u8 = 24;
const OPT_NAWS: u8 = 31;

const TTYPE_SEND: u8 = 1;
const TTYPE_IS: u8 = 0;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelnetConfig {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_term")]
    pub term: String,
    #[serde(default = "default_cols")]
    pub cols: u32,
    #[serde(default = "default_rows")]
    pub rows: u32,
}

fn default_port() -> u16 {
    23
}
fn default_term() -> String {
    "xterm-256color".into()
}
fn default_cols() -> u32 {
    80
}
fn default_rows() -> u32 {
    24
}

/// Stateful IAC filter. Data bytes pass through (IAC IAC → 0xFF);
/// negotiation produces reply frames queued for the writer.
struct IacParser {
    term: String,
    out: Vec<u8>,
    replies: Vec<Vec<u8>>,
    state: State,
    sb_opt: u8,
    sb_buf: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    Iac,
    Neg(u8), // DO/DONT/WILL/WONT pending option byte
    Sb,      // inside sub-negotiation, waiting for option byte
    SbData,  // collecting sub-negotiation payload
    SbIac,   // got IAC inside sub-negotiation
}

impl IacParser {
    fn new(term: String) -> Self {
        Self {
            term,
            out: Vec::with_capacity(4096),
            replies: Vec::new(),
            state: State::Data,
            sb_opt: 0,
            sb_buf: Vec::new(),
        }
    }

    fn negotiate(&mut self, verb: u8, opt: u8) {
        match verb {
            DO => {
                let accept = matches!(opt, OPT_SGA | OPT_TTYPE | OPT_NAWS);
                self.replies.push(vec![IAC, if accept { WILL } else { WONT }, opt]);
            }
            WILL => {
                let accept = matches!(opt, OPT_ECHO | OPT_SGA);
                self.replies.push(vec![IAC, if accept { DO } else { DONT }, opt]);
            }
            _ => {}
        }
    }

    fn feed(&mut self, data: &[u8]) {
        for &b in data {
            match self.state {
                State::Data => match b {
                    IAC => self.state = State::Iac,
                    _ => self.out.push(b),
                },
                State::Iac => match b {
                    IAC => {
                        self.out.push(IAC);
                        self.state = State::Data;
                    }
                    DO | DONT | WILL | WONT => self.state = State::Neg(b),
                    SB => {
                        self.sb_buf.clear();
                        self.state = State::Sb;
                    }
                    _ => self.state = State::Data,
                },
                State::Neg(verb) => {
                    self.negotiate(verb, b);
                    self.state = State::Data;
                }
                State::Sb => {
                    self.sb_opt = b;
                    self.state = State::SbData;
                }
                State::SbData => {
                    if b == IAC {
                        self.state = State::SbIac;
                    } else {
                        self.sb_buf.push(b);
                    }
                }
                State::SbIac => match b {
                    IAC => {
                        self.sb_buf.push(IAC);
                        self.state = State::SbData;
                    }
                    SE => {
                        // TTYPE SEND → reply with our terminal type.
                        if self.sb_opt == OPT_TTYPE
                            && self.sb_buf.first() == Some(&TTYPE_SEND)
                        {
                            let mut r = vec![IAC, SB, OPT_TTYPE, TTYPE_IS];
                            r.extend_from_slice(self.term.as_bytes());
                            r.extend_from_slice(&[IAC, SE]);
                            self.replies.push(r);
                        }
                        self.state = State::Data;
                    }
                    _ => self.state = State::SbData,
                },
            }
        }
    }

    fn drain(&mut self) -> (Vec<u8>, Vec<Vec<u8>>) {
        (
            std::mem::take(&mut self.out),
            std::mem::take(&mut self.replies),
        )
    }
}

/// Run the telnet session until disconnect or close.
pub async fn run_telnet(
    cfg: TelnetConfig,
    pipe: SessionPipe,
    mut cmd_rx: mpsc::Receiver<SessionCommand>,
) -> AppResult<()> {
    pipe.push_state(SessionState::Connecting.as_str(), None);
    pipe.push_data(
        "\x1b[1;33m[OpenTerm] 警告: Telnet 为明文协议,凭据与数据不加密。\x1b[0m\r\n".as_bytes(),
    );
    let stream = TcpStream::connect((cfg.host.as_str(), cfg.port)).await?;
    let (mut rd, mut wr) = stream.into_split();

    // Proactively advertise what we support.
    let mut hello = Vec::new();
    for opt in [OPT_SGA, OPT_TTYPE, OPT_NAWS] {
        hello.extend_from_slice(&[IAC, WILL, opt]);
    }
    hello.extend_from_slice(&[IAC, DO, OPT_SGA]);
    let _ = wr.write_all(&hello).await;
    send_naws(&mut wr, cfg.cols, cfg.rows).await;

    pipe.push_state(SessionState::Connected.as_str(), None);

    let mut parser = IacParser::new(cfg.term.clone());
    let mut buf = [0u8; 8192];
    loop {
        tokio::select! {
            n = rd.read(&mut buf) => {
                match n {
                    Ok(0) => return Ok(()),
                    Ok(n) => {
                        parser.feed(&buf[..n]);
                        let (out, replies) = parser.drain();
                        if !out.is_empty() {
                            pipe.push_data(&out);
                        }
                        for r in replies {
                            if wr.write_all(&r).await.is_err() {
                                return Ok(());
                            }
                        }
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(SessionCommand::Input(data)) => {
                        let mut esc = Vec::with_capacity(data.len() + 8);
                        for b in data {
                            esc.push(b);
                            if b == IAC {
                                esc.push(IAC);
                            }
                        }
                        if wr.write_all(&esc).await.is_err() {
                            return Ok(());
                        }
                    }
                    Some(SessionCommand::Resize { cols, rows }) => {
                        send_naws(&mut wr, cols, rows).await;
                    }
                    Some(SessionCommand::Close) | None => return Ok(()),
                }
            }
        }
    }
}

async fn send_naws(wr: &mut OwnedWriteHalf, cols: u32, rows: u32) {
    let c = (cols as u16).to_be_bytes();
    let r = (rows as u16).to_be_bytes();
    let frame = [IAC, SB, OPT_NAWS, c[0], c[1], r[0], r[1], IAC, SE];
    let _ = wr.write_all(&frame).await;
}
