//! Character encodings of remote sessions (a profile's `encoding`), for older servers and
//! network devices that don't use UTF-8.
//!
//! Everything inside the app is UTF-8: the terminal, session logs, prompts, file names in the
//! SFTP panel. Text is converted where it crosses to the remote side: shell output and input
//! (see [`crate::session`]), SFTP paths ([`crate::sftp::names`]) and ZMODEM file names. UTF-8
//! sessions skip all of it.

use std::sync::Mutex;

use encoding_rs::{Decoder, EncoderResult, Encoding, UTF_8};

/// The encodings offered, by WHATWG label (what profiles store). GBK and GB18030 decode
/// alike; GBK cannot encode the characters GB18030 needs four bytes for.
pub const SUPPORTED: &[&str] = &["utf-8", "gb18030", "gbk", "big5", "shift_jis", "euc-jp", "euc-kr", "windows-1252"];

pub const DEFAULT: &str = "utf-8";

/// The encoding for one of the [`SUPPORTED`] labels.
pub fn lookup(label: &str) -> Option<&'static Encoding> {
    SUPPORTED.contains(&label).then(|| Encoding::for_label(label.as_bytes())).flatten()
}

/// The encoding a profile names; UTF-8 for anything unknown (hand-edited files).
pub fn for_profile(label: &str) -> &'static Encoding {
    lookup(label).unwrap_or(UTF_8)
}

/// Encodes `text`, replacing what the encoding cannot represent with `?` (`encoding_rs`
/// would write HTML character references, which a shell would take literally).
pub fn encode(encoding: &'static Encoding, text: &str) -> Vec<u8> {
    let mut encoder = encoding.new_encoder();
    let mut out = Vec::with_capacity(text.len() + 4);
    let mut rest = text;
    loop {
        let needed = encoder.max_buffer_length_from_utf8_without_replacement(rest.len()).unwrap_or(rest.len() * 4 + 16);
        out.reserve(needed);
        let (result, read) = encoder.encode_from_utf8_to_vec_without_replacement(rest, &mut out, true);
        rest = &rest[read..];
        match result {
            EncoderResult::InputEmpty => return out,
            EncoderResult::Unmappable(_) => out.push(b'?'),
            EncoderResult::OutputFull => {}
        }
    }
}

/// Decodes a complete string (a file name), replacing malformed sequences.
pub fn decode(encoding: &'static Encoding, bytes: &[u8]) -> String {
    encoding.decode_without_bom_handling(bytes).0.into_owned()
}

/// A session's conversion of its output and input. Output is decoded as a stream, so a
/// character split between two reads comes out whole.
pub struct Codec {
    encoding: &'static Encoding,
    decoder: Mutex<Decoder>,
}

impl Codec {
    /// `None` for UTF-8, which needs no conversion.
    pub fn new(encoding: &'static Encoding) -> Option<Self> {
        (encoding != UTF_8).then(|| Self { encoding, decoder: Mutex::new(encoding.new_decoder_without_bom_handling()) })
    }

    pub fn encoding(&self) -> &'static Encoding {
        self.encoding
    }

    /// Remote output to UTF-8.
    pub fn decode(&self, bytes: &[u8]) -> Vec<u8> {
        let mut decoder = self.decoder.lock().unwrap();
        let mut out = String::with_capacity(decoder.max_utf8_buffer_length(bytes.len()).unwrap_or(bytes.len() * 3 + 16));
        // Sized above, so all input is consumed in one call.
        let _ = decoder.decode_to_string(bytes, &mut out, false);
        out.into_bytes()
    }

    /// Ends the output stream: a character left incomplete becomes U+FFFD, and the next output
    /// starts afresh. For the end of a session, and before a ZMODEM transfer, whose data
    /// would otherwise be taken for the rest of the character.
    pub fn flush(&self) -> Vec<u8> {
        let mut decoder = self.decoder.lock().unwrap();
        let mut out = String::with_capacity(decoder.max_utf8_buffer_length(0).unwrap_or(16));
        let _ = decoder.decode_to_string(&[], &mut out, true);
        // A decoder is done once given the last input.
        *decoder = self.encoding.new_decoder_without_bom_handling();
        out.into_bytes()
    }

    /// Input (UTF-8, from the terminal) to the remote encoding.
    pub fn encode(&self, bytes: &[u8]) -> Vec<u8> {
        encode(self.encoding, &String::from_utf8_lossy(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_supported_labels_resolve() {
        for label in SUPPORTED {
            assert!(lookup(label).is_some(), "{label}");
        }
        assert!(lookup("utf-16le").is_none());
        assert_eq!(for_profile("nonsense"), UTF_8);
    }

    #[test]
    fn decodes_characters_split_between_reads() {
        let codec = Codec::new(for_profile("gbk")).unwrap();
        // "你好" in GBK, split inside the second character.
        let mut out = codec.decode(&[0xc4, 0xe3, 0xba]);
        out.extend(codec.decode(&[0xc3, b'\n']));
        assert_eq!(String::from_utf8(out).unwrap(), "你好\n");
    }

    #[test]
    fn flushing_ends_an_incomplete_character() {
        let codec = Codec::new(for_profile("gbk")).unwrap();
        assert_eq!(codec.decode(&[b'a', 0xc4]), b"a");
        assert_eq!(String::from_utf8(codec.flush()).unwrap(), "\u{fffd}");
        // What follows is not taken for the rest of it.
        assert_eq!(String::from_utf8(codec.decode(&[b'*', 0xc4, 0xe3])).unwrap(), "*你");
        assert!(codec.flush().is_empty());
    }

    #[test]
    fn encodes_input_and_replaces_unmappable_characters() {
        let codec = Codec::new(for_profile("gbk")).unwrap();
        assert_eq!(codec.encode("ls 你好\r".as_bytes()), b"ls \xc4\xe3\xba\xc3\r");
        assert_eq!(encode(for_profile("gbk"), "a😀b"), b"a?b");
        assert_eq!(encode(for_profile("gb18030"), "😀"), [0x94, 0x39, 0xfc, 0x36]);
        assert_eq!(encode(for_profile("big5"), "中文"), b"\xa4\xa4\xa4\xe5");
    }

    #[test]
    fn utf8_needs_no_codec() {
        assert!(Codec::new(UTF_8).is_none());
    }
}
