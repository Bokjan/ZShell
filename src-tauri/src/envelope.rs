//! The JSON files that may be encrypted, the vault and exported sessions: a header in clear,
//! and the data either in clear (`data`) or encrypted (`sealed`):
//!
//! ```json
//! { "header": { "format": "…", "version": 2, … }, "sealed": { "nonce": "…", "data": "…" } }
//! ```
//!
//! The header's bytes, exactly as in the file, are the associated data of the encryption: a
//! changed header (another key's id, other parameters) doesn't open. They are kept as read,
//! never serialized again, which could give other bytes. See "加密保险库" in
//! `docs/ARCHITECTURE.md`.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use zeroize::Zeroizing;

use crate::crypto::{self, Key, Sealed};

/// A file read, its header understood.
pub struct Envelope<H> {
    pub header: H,
    header_raw: String,
    body: Body,
}

enum Body {
    Plain(Box<RawValue>),
    Sealed(Sealed),
}

/// Why a file isn't one of a format.
#[derive(Debug, PartialEq)]
pub enum Invalid {
    /// Not of this format at all.
    Format,
    /// Of this format, but of another version (the one it has).
    Version(u32),
    /// Of this format and version, but not as it should be (with the parser's error).
    Damaged(Option<String>),
}

/// Why a file's data can't be had.
#[derive(Debug, PartialEq)]
pub enum Unopened {
    /// Encrypted with another key (or changed since).
    WrongKey,
    /// Not as it should be (with the parser's error): data in clear where it should be
    /// encrypted or the other way round, or not the data expected.
    Damaged(Option<String>),
}

#[derive(Deserialize)]
struct FileIn {
    header: Option<Box<RawValue>>,
    data: Option<Box<RawValue>>,
    sealed: Option<Sealed>,
}

#[derive(Serialize)]
struct PlainOut<'a, T> {
    header: &'a RawValue,
    data: &'a T,
}

#[derive(Serialize)]
struct SealedOut<'a> {
    header: &'a RawValue,
    sealed: &'a Sealed,
}

#[derive(Deserialize)]
struct Versioned {
    format: String,
    version: u32,
}

#[derive(Serialize)]
struct VersionedOut<'a, H> {
    format: &'a str,
    version: u32,
    #[serde(flatten)]
    header: &'a H,
}

fn damaged(e: serde_json::Error) -> Option<String> {
    Some(e.to_string())
}

/// Reads a file of `format` and `version`, as far as its header.
pub fn parse<H: DeserializeOwned>(bytes: &[u8], format: &str, version: u32) -> Result<Envelope<H>, Invalid> {
    let file: FileIn = serde_json::from_slice(bytes).map_err(|e| Invalid::Damaged(damaged(e)))?;
    let header_raw = file.header.ok_or(Invalid::Format)?;
    let versioned: Versioned = serde_json::from_str(header_raw.get()).map_err(|_| Invalid::Format)?;
    if versioned.format != format {
        return Err(Invalid::Format);
    }
    if versioned.version != version {
        return Err(Invalid::Version(versioned.version));
    }
    let header = serde_json::from_str(header_raw.get()).map_err(|e| Invalid::Damaged(damaged(e)))?;
    let body = match (file.data, file.sealed) {
        (Some(data), None) => Body::Plain(data),
        (None, Some(sealed)) => Body::Sealed(sealed),
        _ => return Err(Invalid::Damaged(None)),
    };
    Ok(Envelope { header, header_raw: header_raw.get().to_owned(), body })
}

impl<H> Envelope<H> {
    /// The data's bytes: as in the file without `key`, decrypted with it.
    fn plaintext(&self, key: Option<&Key>) -> Result<Zeroizing<Vec<u8>>, Unopened> {
        match (&self.body, key) {
            (Body::Plain(data), None) => Ok(Zeroizing::new(data.get().as_bytes().to_vec())),
            (Body::Sealed(sealed), Some(key)) => crypto::open(key, self.header_raw.as_bytes(), sealed).ok_or(Unopened::WrongKey),
            _ => Err(Unopened::Damaged(None)),
        }
    }

    /// The data: in clear without `key`, decrypted with it. Which one the file should be is
    /// its header's to say; the other is `Damaged`.
    pub fn open<T: DeserializeOwned>(&self, key: Option<&Key>) -> Result<T, Unopened> {
        serde_json::from_slice(&self.plaintext(key)?).map_err(|e| Unopened::Damaged(damaged(e)))
    }
}

/// The file of `header` (with `format` and `version`) and `data`, encrypted with `key` or in
/// clear without. Read back before it is returned, to check that it opens to the same data;
/// `None` if it doesn't.
pub fn write<H: Serialize, T: Serialize>(format: &str, version: u32, header: &H, key: Option<&Key>, data: &T) -> Option<Vec<u8>> {
    let header_raw = RawValue::from_string(serde_json::to_string(&VersionedOut { format, version, header }).ok()?).ok()?;
    let bytes = match key {
        None => serde_json::to_vec_pretty(&PlainOut { header: &header_raw, data }).ok()?,
        Some(key) => {
            let plaintext = Zeroizing::new(serde_json::to_vec(data).ok()?);
            let sealed = crypto::seal(key, header_raw.get().as_bytes(), &plaintext);
            let bytes = serde_json::to_vec_pretty(&SealedOut { header: &header_raw, sealed: &sealed }).ok()?;
            let read: Envelope<serde::de::IgnoredAny> = parse(&bytes, format, version).ok()?;
            (*read.plaintext(Some(key)).ok()? == *plaintext).then_some(bytes)?
        }
    };
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "protection", rename_all = "camelCase")]
    enum Header {
        None,
        Keyed { id: String },
    }

    #[test]
    fn plain_and_sealed_files() {
        let data = vec!["a".to_owned(), "b".to_owned()];
        let plain = write("test", 1, &Header::None, None, &data).unwrap();
        let text = String::from_utf8(plain.clone()).unwrap();
        assert!(text.contains(r#""header": {"format":"test","version":1,"protection":"none"}"#), "{text}");
        let read: Envelope<Header> = parse(&plain, "test", 1).unwrap();
        assert_eq!(read.header, Header::None);
        assert_eq!(read.open::<Vec<String>>(None).unwrap(), data);

        let key = crypto::random_key();
        let header = Header::Keyed { id: "k".into() };
        let sealed = write("test", 1, &header, Some(&key), &data).unwrap();
        assert!(!String::from_utf8_lossy(&sealed).contains(r#""a""#));
        let read: Envelope<Header> = parse(&sealed, "test", 1).unwrap();
        assert_eq!(read.header, header);
        assert_eq!(read.open::<Vec<String>>(Some(&key)).unwrap(), data);
        assert_eq!(read.open::<Vec<String>>(Some(&crypto::random_key())).unwrap_err(), Unopened::WrongKey);
        // In clear where the header says encrypted, or the other way round.
        assert!(matches!(read.open::<Vec<String>>(None), Err(Unopened::Damaged(_))));
        let read: Envelope<Header> = parse(&plain, "test", 1).unwrap();
        assert!(matches!(read.open::<Vec<String>>(Some(&key)), Err(Unopened::Damaged(_))));
    }

    #[test]
    fn a_changed_header_does_not_open() {
        let key = crypto::random_key();
        let sealed = write("test", 1, &Header::Keyed { id: "k".into() }, Some(&key), &1).unwrap();
        let changed = String::from_utf8(sealed).unwrap().replace(r#""id":"k""#, r#""id":"j""#);
        let read: Envelope<Header> = parse(changed.as_bytes(), "test", 1).unwrap();
        assert_eq!(read.open::<i32>(Some(&key)).unwrap_err(), Unopened::WrongKey);
    }

    #[test]
    fn other_formats_and_versions() {
        let file = write("test", 2, &Header::None, None, &1).unwrap();
        assert!(matches!(parse::<Header>(&file, "other", 2), Err(Invalid::Format)));
        assert!(matches!(parse::<Header>(&file, "test", 1), Err(Invalid::Version(2))));
        assert!(matches!(parse::<Header>(b"{}", "test", 2), Err(Invalid::Format)));
        assert!(matches!(parse::<Header>(b"{", "test", 2), Err(Invalid::Damaged(Some(_)))));
        let unknown = br#"{ "header": { "format": "test", "version": 2, "protection": "other" }, "data": 1 }"#;
        assert!(matches!(parse::<Header>(unknown, "test", 2), Err(Invalid::Damaged(Some(_)))));
        let both =
            br#"{ "header": { "format": "test", "version": 2, "protection": "none" }, "data": 1, "sealed": { "nonce": "", "data": "" } }"#;
        assert!(matches!(parse::<Header>(both, "test", 2), Err(Invalid::Damaged(None))));
    }
}
