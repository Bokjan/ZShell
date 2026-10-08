//! ZMODEM framing: headers (hex, 16- and 32-bit CRC binary), data subpackets, ZDLE escaping
//! and the CRCs, as in Chuck Forsberg's specification and lrzsz.
//!
//! Everything sent escapes all control characters (what `rz -e` / ESCCTL asks for), so the
//! stream survives bastion hosts and terminals that filter or interpret them.

/// Header lead-in, and the start of every header.
pub const ZPAD: u8 = b'*';
/// The escape character; also ASCII CAN, five of which in a row cancel a transfer.
pub const ZDLE: u8 = 0x18;
pub const CAN: u8 = ZDLE;
const ZBIN: u8 = b'A';
const ZHEX: u8 = b'B';
const ZBIN32: u8 = b'C';
const XON: u8 = 0x11;
const XOFF: u8 = 0x13;
const BS: u8 = 0x08;

/// Data subpacket ends: CRC next, frame ends (`E`), continues (`G`), continues and wants a
/// `ZACK` (`Q`), or ends and wants a `ZACK` (`W`).
pub const ZCRCE: u8 = b'h';
pub const ZCRCG: u8 = b'i';
pub const ZCRCQ: u8 = b'j';
pub const ZCRCW: u8 = b'k';
/// Escaped 0x7f and 0xff.
const ZRUB0: u8 = b'l';
const ZRUB1: u8 = b'm';

/// `ZRINIT` capability flags (ZF0).
pub const CANFDX: u8 = 0x01;
pub const CANOVIO: u8 = 0x02;
pub const CANFC32: u8 = 0x20;
pub const ESCCTL: u8 = 0x40;

/// `ZFILE` conversion option (ZF0): binary transfer.
pub const ZCBIN: u8 = 1;

/// What ends a transfer from either side: eight CANs (lrzsz sends ten), then backspaces to
/// erase them where they are echoed.
pub const ABORT_SEQUENCE: &[u8] = &[CAN, CAN, CAN, CAN, CAN, CAN, CAN, CAN, CAN, CAN, BS, BS, BS, BS, BS, BS, BS, BS, BS, BS];

/// The start of a hex `ZRQINIT` (sent by `sz`) or `ZRINIT` (sent by `rz`) header: what starts
/// a transfer in a terminal's output.
pub const ZRQINIT_SIGNATURE: &[u8] = b"*\x18B00";
pub const ZRINIT_SIGNATURE: &[u8] = b"*\x18B01";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rqinit,
    Rinit,
    Sinit,
    Ack,
    File,
    Skip,
    Nak,
    Abort,
    Fin,
    Rpos,
    Data,
    Eof,
    Ferr,
    Crc,
    Challenge,
    Compl,
    Can,
    Freecnt,
    Command,
    Stderr,
}

impl Kind {
    const ALL: [Kind; 20] = [
        Kind::Rqinit,
        Kind::Rinit,
        Kind::Sinit,
        Kind::Ack,
        Kind::File,
        Kind::Skip,
        Kind::Nak,
        Kind::Abort,
        Kind::Fin,
        Kind::Rpos,
        Kind::Data,
        Kind::Eof,
        Kind::Ferr,
        Kind::Crc,
        Kind::Challenge,
        Kind::Compl,
        Kind::Can,
        Kind::Freecnt,
        Kind::Command,
        Kind::Stderr,
    ];

    fn from_byte(byte: u8) -> Option<Self> {
        Self::ALL.get(usize::from(byte)).copied()
    }
}

/// A frame header: its type and four bytes that are a file position (little-endian, for
/// `ZRPOS`, `ZDATA`, `ZEOF`, `ZACK`) or flags (ZF3..ZF0, for the others).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: Kind,
    pub data: [u8; 4],
}

impl Header {
    pub fn new(kind: Kind) -> Self {
        Self { kind, data: [0; 4] }
    }

    pub fn with_pos(kind: Kind, pos: u64) -> Self {
        // Positions are 32 bits on the wire; larger files wrap, as in lrzsz.
        Self { kind, data: (pos as u32).to_le_bytes() }
    }

    pub fn with_flags(kind: Kind, zf0: u8) -> Self {
        Self { kind, data: [0, 0, 0, zf0] }
    }

    pub fn pos(&self) -> u32 {
        u32::from_le_bytes(self.data)
    }

    pub fn zf0(&self) -> u8 {
        self.data[3]
    }

    fn raw(&self) -> [u8; 5] {
        let [a, b, c, d] = self.data;
        [self.kind as u8, a, b, c, d]
    }

    fn from_raw(raw: [u8; 5]) -> Option<Self> {
        Some(Self { kind: Kind::from_byte(raw[0])?, data: [raw[1], raw[2], raw[3], raw[4]] })
    }
}

/// How a header was (or is to be) encoded; binary headers set the CRC of the data
/// subpackets that follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Hex,
    Bin16,
    Bin32,
}

pub fn encode_header(header: &Header, encoding: Encoding) -> Vec<u8> {
    let raw = header.raw();
    let mut out = Vec::with_capacity(24);
    match encoding {
        Encoding::Hex => {
            out.extend([ZPAD, ZPAD, ZDLE, ZHEX]);
            push_hex(&mut out, &raw);
            push_hex(&mut out, &crc16(&raw).to_be_bytes());
            // CR, LF with the high bit set (as lrzsz), and XON to restart a sender stopped by
            // XOFF, except where the other side may be about to exit.
            out.extend([b'\r', 0x8a]);
            if !matches!(header.kind, Kind::Ack | Kind::Fin) {
                out.push(XON);
            }
        }
        Encoding::Bin16 => {
            out.extend([ZPAD, ZDLE, ZBIN]);
            escape_into(&mut out, &raw);
            escape_into(&mut out, &crc16(&raw).to_be_bytes());
        }
        Encoding::Bin32 => {
            out.extend([ZPAD, ZDLE, ZBIN32]);
            escape_into(&mut out, &raw);
            escape_into(&mut out, &crc32(&raw).to_le_bytes());
        }
    }
    out
}

/// Appends a data subpacket: the escaped data, ZDLE + `end`, and the escaped CRC of both.
pub fn encode_subpacket(out: &mut Vec<u8>, data: &[u8], end: u8, crc32_mode: bool) {
    escape_into(out, data);
    out.extend([ZDLE, end]);
    if crc32_mode {
        let mut crc = Crc32::new();
        crc.update(data);
        crc.update(&[end]);
        escape_into(out, &crc.finish().to_le_bytes());
    } else {
        let mut crc = Crc16::new();
        crc.update(data);
        crc.update(&[end]);
        escape_into(out, &crc.finish().to_be_bytes());
    }
}

/// Checks a received subpacket's CRC (`crc` as it arrived, after unescaping).
pub fn subpacket_crc_ok(data: &[u8], end: u8, crc: &[u8], crc32_mode: bool) -> bool {
    if crc32_mode {
        let mut c = Crc32::new();
        c.update(data);
        c.update(&[end]);
        crc == c.finish().to_le_bytes()
    } else {
        let mut c = Crc16::new();
        c.update(data);
        c.update(&[end]);
        crc == c.finish().to_be_bytes()
    }
}

/// Parses the 14 hex digits of a hex header (type, four data bytes, CRC).
pub fn parse_hex_header(digits: &[u8; 14]) -> Option<Header> {
    let mut bytes = [0u8; 7];
    for (byte, pair) in bytes.iter_mut().zip(digits.chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    let raw = [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4]];
    if crc16(&raw).to_be_bytes() != [bytes[5], bytes[6]] {
        return None;
    }
    Header::from_raw(raw)
}

/// Checks a binary header (type and data, then the CRC as received) and builds it.
pub fn parse_bin_header(raw: [u8; 5], crc: &[u8], encoding: Encoding) -> Option<Header> {
    let ok = match encoding {
        Encoding::Bin32 => crc == crc32(&raw).to_le_bytes(),
        _ => crc == crc16(&raw).to_be_bytes(),
    };
    if ok {
        Header::from_raw(raw)
    } else {
        None
    }
}

/// The header format byte after ZPAD ZDLE.
pub fn encoding_of(format: u8) -> Option<Encoding> {
    match format {
        ZHEX => Some(Encoding::Hex),
        ZBIN => Some(Encoding::Bin16),
        ZBIN32 => Some(Encoding::Bin32),
        _ => None,
    }
}

/// Flow control characters that may appear anywhere and are not data.
pub fn is_flow_control(byte: u8) -> bool {
    matches!(byte & 0x7f, XON | XOFF)
}

/// What a ZDLE followed by `byte` stands for.
pub enum Escaped {
    Byte(u8),
    /// A subpacket end (`ZCRCE` etc.).
    End(u8),
    Invalid,
}

pub fn unescape(byte: u8) -> Escaped {
    match byte {
        ZCRCE | ZCRCG | ZCRCQ | ZCRCW => Escaped::End(byte),
        ZRUB0 => Escaped::Byte(0x7f),
        ZRUB1 => Escaped::Byte(0xff),
        b if b & 0x60 == 0x40 => Escaped::Byte(b ^ 0x40),
        _ => Escaped::Invalid,
    }
}

fn escape_into(out: &mut Vec<u8>, data: &[u8]) {
    for &byte in data {
        match byte {
            0x7f => out.extend([ZDLE, ZRUB0]),
            0xff => out.extend([ZDLE, ZRUB1]),
            b if b & 0x7f < 0x20 => out.extend([ZDLE, b ^ 0x40]),
            b => out.push(b),
        }
    }
}

fn push_hex(out: &mut Vec<u8>, bytes: &[u8]) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)]);
        out.push(DIGITS[usize::from(byte & 0xf)]);
    }
}

/// CRC-16/XMODEM (CCITT polynomial, initial value 0).
struct Crc16(u16);

impl Crc16 {
    fn new() -> Self {
        Self(0)
    }

    fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.0 ^= u16::from(byte) << 8;
            for _ in 0..8 {
                self.0 = if self.0 & 0x8000 != 0 { (self.0 << 1) ^ 0x1021 } else { self.0 << 1 };
            }
        }
    }

    fn finish(self) -> u16 {
        self.0
    }
}

fn crc16(data: &[u8]) -> u16 {
    let mut crc = Crc16::new();
    crc.update(data);
    crc.finish()
}

/// CRC-32 (IEEE 802.3, as in zip and Ethernet).
struct Crc32(u32);

const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

impl Crc32 {
    fn new() -> Self {
        Self(0xffff_ffff)
    }

    fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.0 = CRC32_TABLE[usize::from((self.0 as u8) ^ byte)] ^ (self.0 >> 8);
        }
    }

    fn finish(self) -> u32 {
        !self.0
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(data);
    crc.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crcs_match_the_standard_check_values() {
        assert_eq!(crc16(b"123456789"), 0x31c3);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn hex_headers_match_lrzsz() {
        // What `rz` sends: ZRINIT with CANFDX | CANOVIO | CANFC32.
        let header = Header::with_flags(Kind::Rinit, CANFDX | CANOVIO | CANFC32);
        assert_eq!(encode_header(&header, Encoding::Hex), b"**\x18B0100000023be50\r\x8a\x11");
        let digits: [u8; 14] = *b"0100000023be50";
        assert_eq!(parse_hex_header(&digits), Some(header));
        assert_eq!(parse_hex_header(b"0100000023be51"), None);
        // What `sz` sends first.
        assert!(encode_header(&Header::new(Kind::Rqinit), Encoding::Hex).starts_with(b"**\x18B00000000000000"));
    }

    #[test]
    fn escaping_covers_every_control_character() {
        let all: Vec<u8> = (0..=255).collect();
        let mut out = Vec::new();
        escape_into(&mut out, &all);
        let mut decoded = Vec::new();
        let mut bytes = out.iter();
        while let Some(&byte) = bytes.next() {
            if byte == ZDLE {
                match unescape(*bytes.next().unwrap()) {
                    Escaped::Byte(b) => decoded.push(b),
                    _ => panic!("bad escape"),
                }
            } else {
                assert!(byte & 0x7f >= 0x20 && byte != 0x7f && byte != 0xff, "unescaped {byte:#x}");
                decoded.push(byte);
            }
        }
        assert_eq!(decoded, all);
    }
}
