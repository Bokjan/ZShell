//! The Telnet protocol (RFC 854): commands interleaved with the data, and option
//! negotiation (RFC 1143's Q method, without the queue bits, and since we never turn an
//! option off, without its WANTNO state) for the options a terminal needs. Everything else
//! the server proposes is refused.
//!
//! - BINARY (RFC 856), both ways: 8-bit data passes unchanged (UTF-8, GBK).
//! - ECHO (RFC 857): the server echoes; while it doesn't, the session echoes locally.
//! - SGA (RFC 858), both ways: no go-aheads, character at a time.
//! - TTYPE (RFC 1091): we report the profile's terminal type.
//! - NAWS (RFC 1073): we report the window size, and again on every resize.

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const BRK: u8 = 243;
const SE: u8 = 240;

const BINARY: u8 = 0;
const ECHO: u8 = 1;
const SGA: u8 = 3;
const TTYPE: u8 = 24;
const NAWS: u8 = 31;

const TTYPE_IS: u8 = 0;
const TTYPE_SEND: u8 = 1;

/// Longest subnegotiation kept; the rest is dropped (we only read TTYPE SEND).
const SB_LIMIT: usize = 256;

/// Telnet's break command.
pub const BREAK: [u8; 2] = [IAC, BRK];

/// One side of an option (RFC 1143).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Q {
    #[default]
    No,
    Yes,
    /// We asked to turn it on and wait for the answer.
    WantYes,
}

#[derive(Clone, Copy, Default)]
struct Opt {
    /// Whether we perform the option (WILL / WONT).
    us: Q,
    /// Whether the server performs it (DO / DONT).
    him: Q,
}

enum State {
    Data,
    Iac,
    /// After WILL, WONT, DO or DONT: the option comes next.
    Negotiate(u8),
    Sb,
    SbIac,
}

/// What a chunk from the server contained: data for the terminal, and what to send back.
#[derive(Debug, Default, PartialEq)]
pub struct Received {
    pub data: Vec<u8>,
    pub reply: Vec<u8>,
}

pub struct Telnet {
    options: [Opt; 256],
    state: State,
    sb: Vec<u8>,
    /// The previous data byte was a CR, after which NVT sends a NUL that isn't data.
    after_cr: bool,
    term_type: String,
    size: (u16, u16),
}

impl Telnet {
    pub fn new(term_type: &str, size: (u16, u16)) -> Self {
        Self {
            options: [Opt::default(); 256],
            state: State::Data,
            sb: Vec::new(),
            after_cr: false,
            term_type: term_type.to_owned(),
            size,
        }
    }

    /// What we propose when the connection opens.
    pub fn start(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        for option in [BINARY, ECHO, SGA] {
            self.ask_him(option, &mut out);
        }
        for option in [BINARY, SGA, TTYPE, NAWS] {
            self.ask_us(option, &mut out);
        }
        out
    }

    /// Whether the server echoes what we send; otherwise the client is expected to.
    pub fn remote_echoes(&self) -> bool {
        self.options[ECHO as usize].him == Q::Yes
    }

    /// Splits a chunk from the server into data and commands, answering the commands.
    pub fn receive(&mut self, input: &[u8]) -> Received {
        let mut received = Received::default();
        for &byte in input {
            match self.state {
                State::Data if byte == IAC => self.state = State::Iac,
                State::Data => self.data(byte, &mut received.data),
                State::Iac => {
                    self.state = State::Data;
                    match byte {
                        IAC => self.data(IAC, &mut received.data),
                        WILL | WONT | DO | DONT => self.state = State::Negotiate(byte),
                        SB => {
                            self.sb.clear();
                            self.state = State::Sb;
                        }
                        // NOP, go-ahead, data mark and the like mean nothing to a terminal.
                        _ => {}
                    }
                }
                State::Negotiate(verb) => {
                    self.state = State::Data;
                    self.negotiate(verb, byte, &mut received.reply);
                }
                State::Sb if byte == IAC => self.state = State::SbIac,
                State::Sb => self.sb_push(byte),
                State::SbIac => match byte {
                    IAC => {
                        self.sb_push(IAC);
                        self.state = State::Sb;
                    }
                    // SE, or a malformed subnegotiation: either way it ends here.
                    _ => {
                        self.state = State::Data;
                        self.subnegotiation(&mut received.reply);
                    }
                },
            }
        }
        received
    }

    /// Our input in Telnet form: IAC doubled, and outside binary mode a CR not followed by
    /// LF sent as CR NUL (NVT's carriage return).
    pub fn encode(&self, data: &[u8]) -> Vec<u8> {
        let binary = self.options[BINARY as usize].us == Q::Yes;
        let mut out = Vec::with_capacity(data.len() + 8);
        for (i, &byte) in data.iter().enumerate() {
            out.push(byte);
            if byte == IAC {
                out.push(IAC);
            } else if byte == b'\r' && !binary && data.get(i + 1) != Some(&b'\n') {
                out.push(0);
            }
        }
        out
    }

    /// The new window size, to report if the server accepted NAWS.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Vec<u8> {
        self.size = (cols, rows);
        let mut out = Vec::new();
        if self.options[NAWS as usize].us == Q::Yes {
            self.naws(&mut out);
        }
        out
    }

    fn data(&mut self, byte: u8, data: &mut Vec<u8>) {
        let binary = self.options[BINARY as usize].him == Q::Yes;
        if !(self.after_cr && byte == 0 && !binary) {
            data.push(byte);
        }
        self.after_cr = byte == b'\r';
    }

    fn sb_push(&mut self, byte: u8) {
        if self.sb.len() < SB_LIMIT {
            self.sb.push(byte);
        }
    }

    fn supported_by_us(option: u8) -> bool {
        matches!(option, BINARY | SGA | TTYPE | NAWS)
    }

    fn supported_by_him(option: u8) -> bool {
        matches!(option, BINARY | ECHO | SGA)
    }

    fn ask_us(&mut self, option: u8, out: &mut Vec<u8>) {
        let opt = &mut self.options[option as usize];
        if opt.us == Q::No {
            opt.us = Q::WantYes;
            out.extend([IAC, WILL, option]);
        }
    }

    fn ask_him(&mut self, option: u8, out: &mut Vec<u8>) {
        let opt = &mut self.options[option as usize];
        if opt.him == Q::No {
            opt.him = Q::WantYes;
            out.extend([IAC, DO, option]);
        }
    }

    fn negotiate(&mut self, verb: u8, option: u8, out: &mut Vec<u8>) {
        let opt = &mut self.options[option as usize];
        match verb {
            WILL => match opt.him {
                Q::No if Self::supported_by_him(option) => {
                    opt.him = Q::Yes;
                    out.extend([IAC, DO, option]);
                }
                Q::No => out.extend([IAC, DONT, option]),
                Q::Yes | Q::WantYes => opt.him = Q::Yes,
            },
            WONT => {
                if opt.him == Q::Yes {
                    out.extend([IAC, DONT, option]);
                }
                opt.him = Q::No;
            }
            DO => match opt.us {
                Q::No if Self::supported_by_us(option) => {
                    opt.us = Q::Yes;
                    out.extend([IAC, WILL, option]);
                    self.enabled_us(option, out);
                }
                Q::No => out.extend([IAC, WONT, option]),
                Q::WantYes => {
                    opt.us = Q::Yes;
                    self.enabled_us(option, out);
                }
                Q::Yes => {}
            },
            DONT => {
                if opt.us == Q::Yes {
                    out.extend([IAC, WONT, option]);
                }
                opt.us = Q::No;
            }
            _ => {}
        }
    }

    /// We just started performing `option`.
    fn enabled_us(&mut self, option: u8, out: &mut Vec<u8>) {
        if option == NAWS {
            self.naws(out);
        }
    }

    fn subnegotiation(&mut self, out: &mut Vec<u8>) {
        if self.sb.as_slice() == [TTYPE, TTYPE_SEND] && self.options[TTYPE as usize].us == Q::Yes {
            out.extend([IAC, SB, TTYPE, TTYPE_IS]);
            out.extend(self.term_type.bytes().filter(|&b| b != IAC));
            out.extend([IAC, SE]);
        }
    }

    fn naws(&self, out: &mut Vec<u8>) {
        let (cols, rows) = self.size;
        out.extend([IAC, SB, NAWS]);
        for byte in [cols.to_be_bytes(), rows.to_be_bytes()].concat() {
            out.push(byte);
            if byte == IAC {
                out.push(IAC);
            }
        }
        out.extend([IAC, SE]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn received(data: &[u8], reply: &[u8]) -> Received {
        Received { data: data.to_vec(), reply: reply.to_vec() }
    }

    #[test]
    fn negotiates_what_a_terminal_needs() {
        let mut telnet = Telnet::new("xterm-256color", (80, 24));
        let start = telnet.start();
        assert_eq!(start, [
            IAC, DO, BINARY, IAC, DO, ECHO, IAC, DO, SGA, IAC, WILL, BINARY, IAC, WILL, SGA, IAC, WILL, TTYPE, IAC, WILL, NAWS
        ]);
        assert!(!telnet.remote_echoes());

        // Answers to our requests need no reply, except the window size once NAWS is on.
        let answers = [IAC, WILL, ECHO, IAC, WILL, SGA, IAC, DO, TTYPE, IAC, DO, NAWS, IAC, DONT, BINARY, IAC, WONT, BINARY];
        assert_eq!(telnet.receive(&answers), received(&[], &[IAC, SB, NAWS, 0, 80, 0, 24, IAC, SE]));
        assert!(telnet.remote_echoes());

        // Repeated requests for what is already on are not answered (no loops).
        assert_eq!(telnet.receive(&[IAC, WILL, ECHO, IAC, DO, NAWS]), received(&[], &[]));

        // The terminal type, when asked.
        let mut reply = vec![IAC, SB, TTYPE, TTYPE_IS];
        reply.extend(b"xterm-256color");
        reply.extend([IAC, SE]);
        assert_eq!(telnet.receive(&[IAC, SB, TTYPE, TTYPE_SEND, IAC, SE]), received(&[], &reply));

        // Window sizes containing 255 double it.
        assert_eq!(telnet.resize(255, 50), [IAC, SB, NAWS, 0, 255, 255, 0, 50, IAC, SE]);

        // The server stops echoing.
        assert_eq!(telnet.receive(&[IAC, WONT, ECHO]), received(&[], &[IAC, DONT, ECHO]));
        assert!(!telnet.remote_echoes());
    }

    #[test]
    fn refuses_other_options() {
        let mut telnet = Telnet::new("vt100", (80, 24));
        // NEW-ENVIRON, LINEMODE, and the server asking us to echo.
        assert_eq!(
            telnet.receive(&[IAC, DO, 39, IAC, DO, 34, IAC, WILL, 5, IAC, DO, ECHO]),
            received(&[], &[IAC, WONT, 39, IAC, WONT, 34, IAC, DONT, 5, IAC, WONT, ECHO])
        );
        // NAWS is not reported before the server asks for it.
        assert!(telnet.resize(100, 30).is_empty());
    }

    #[test]
    fn separates_data_from_commands() {
        let mut telnet = Telnet::new("vt100", (80, 24));
        // IAC IAC is a 255 data byte; NOP and GA vanish; CR NUL is a CR; split anywhere.
        let mut data = Vec::new();
        for chunk in [&b"a\xff"[..], b"\xffb\xff", b"\xf1c\r", b"\0d\xff\xf9\r\n"] {
            let received = telnet.receive(chunk);
            assert!(received.reply.is_empty());
            data.extend(received.data);
        }
        assert_eq!(data, b"a\xffbc\rd\r\n");

        // A subnegotiation split across chunks, with an escaped IAC inside.
        assert_eq!(telnet.receive(&[b'x', IAC, SB, 42, IAC]).data, b"x");
        assert_eq!(telnet.receive(&[IAC, 1, IAC, SE, b'y']), received(b"y", &[]));
    }

    #[test]
    fn encodes_input() {
        let mut telnet = Telnet::new("vt100", (80, 24));
        assert_eq!(telnet.encode(b"ls\r"), b"ls\r\0");
        assert_eq!(telnet.encode(b"a\r\nb\xff"), b"a\r\nb\xff\xff");
        telnet.start();
        telnet.receive(&[IAC, DO, BINARY, IAC, WILL, BINARY]);
        assert_eq!(telnet.encode(b"ls\r\xff"), b"ls\r\xff\xff");
        // In binary mode a CR NUL from the server is data.
        assert_eq!(telnet.receive(b"\r\0").data, b"\r\0");
    }
}
