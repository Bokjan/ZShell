//! The encryption of the vault and of the passwords in exported files (ring): AES-256-GCM
//! with a random nonce for each message, and keys from passphrases by PBKDF2-HMAC-SHA256.
//! Binary values are base64 in the files.

use std::num::NonZeroU32;

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const KEY_LEN: usize = 32;

/// A key, cleared from memory when dropped.
pub type Key = Zeroizing<[u8; KEY_LEN]>;

const KDF_NAME: &str = "pbkdf2-sha256";
/// OWASP's recommendation for PBKDF2-HMAC-SHA256. Files keep their own count, so it can be
/// raised without making older ones unreadable; tests use few to stay fast.
const ITERATIONS: u32 = if cfg!(test) { 1_000 } else { 600_000 };
/// What a file may ask for: more would only hang the app on a file made to do so.
const MAX_ITERATIONS: u32 = 10_000_000;
const SALT_LEN: usize = 16;

fn fill(bytes: &mut [u8]) {
    // Only fails if the system has no random source at all, which nothing here can recover from.
    SystemRandom::new().fill(bytes).expect("the system's random number generator failed");
}

pub fn random_key() -> Key {
    let mut key = Zeroizing::new([0; KEY_LEN]);
    fill(&mut *key);
    key
}

/// An identifier for `key` that tells which key a file was written with, without telling
/// anything about the key: the first 8 bytes of its SHA-256, in hex.
pub fn key_id(key: &[u8; KEY_LEN]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, key);
    digest.as_ref()[..8].iter().map(|b| format!("{b:02x}")).collect()
}

pub fn encode(bytes: &[u8]) -> String {
    data_encoding::BASE64.encode(bytes)
}

pub fn decode(text: &str) -> Option<Vec<u8>> {
    data_encoding::BASE64.decode(text.as_bytes()).ok()
}

/// A key as stored in the system's credential store.
pub fn decode_key(text: &str) -> Option<Key> {
    let bytes = Zeroizing::new(decode(text)?);
    let mut key = Zeroizing::new([0; KEY_LEN]);
    (bytes.len() == KEY_LEN).then(|| key.copy_from_slice(&bytes))?;
    Some(key)
}

/// An encrypted message: `data` is the ciphertext followed by the tag.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sealed {
    pub nonce: String,
    pub data: String,
}

fn aead_key(key: &[u8; KEY_LEN]) -> LessSafeKey {
    LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).expect("a 256-bit key"))
}

/// Encrypts `plaintext` with `key`, authenticating `aad` with it.
pub fn seal(key: &[u8; KEY_LEN], aad: &[u8], plaintext: &[u8]) -> Sealed {
    let mut nonce = [0; NONCE_LEN];
    fill(&mut nonce);
    let mut data = plaintext.to_vec();
    aead_key(key)
        .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(aad), &mut data)
        .expect("a message within AES-GCM's limits");
    Sealed { nonce: encode(&nonce), data: encode(&data) }
}

/// The plaintext of `sealed`; `None` if it wasn't encrypted with `key` and `aad`, or was
/// changed since.
pub fn open(key: &[u8; KEY_LEN], aad: &[u8], sealed: &Sealed) -> Option<Zeroizing<Vec<u8>>> {
    let nonce: [u8; NONCE_LEN] = decode(&sealed.nonce)?.try_into().ok()?;
    let mut data = Zeroizing::new(decode(&sealed.data)?);
    let len = aead_key(key).open_in_place(Nonce::assume_unique_for_key(nonce), Aad::from(aad), &mut data).ok()?.len();
    data.truncate(len);
    Some(data)
}

/// How a key is derived from a passphrase, as stored next to what it encrypts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Kdf {
    pub name: String,
    pub iterations: u32,
    pub salt: String,
}

impl Kdf {
    /// The current parameters, with a new salt.
    pub fn new() -> Self {
        let mut salt = [0; SALT_LEN];
        fill(&mut salt);
        Self { name: KDF_NAME.to_owned(), iterations: ITERATIONS, salt: encode(&salt) }
    }

    /// The key for `passphrase`; `None` for parameters this version doesn't know or accept.
    /// Slow on purpose (tenths of a second): call it off the async threads.
    pub fn derive(&self, passphrase: &str) -> Option<Key> {
        if self.name != KDF_NAME || self.iterations > MAX_ITERATIONS {
            return None;
        }
        let salt = decode(&self.salt)?;
        let mut key = Zeroizing::new([0; KEY_LEN]);
        let iterations = NonZeroU32::new(self.iterations)?;
        ring::pbkdf2::derive(ring::pbkdf2::PBKDF2_HMAC_SHA256, iterations, &salt, passphrase.as_bytes(), &mut *key);
        Some(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seals_and_opens() {
        let key = random_key();
        let sealed = seal(&key, b"header", b"secret");
        assert_eq!(open(&key, b"header", &sealed).unwrap().as_slice(), b"secret");
        // Another key, other associated data, or a changed message don't open.
        assert!(open(&random_key(), b"header", &sealed).is_none());
        assert!(open(&key, b"other", &sealed).is_none());
        let mut changed = decode(&sealed.data).unwrap();
        changed[0] ^= 1;
        assert!(open(&key, b"header", &Sealed { data: encode(&changed), ..sealed.clone() }).is_none());
        // A new nonce each time.
        assert_ne!(seal(&key, b"", b"x").nonce, seal(&key, b"", b"x").nonce);
    }

    #[test]
    fn derives_keys_from_passphrases() {
        let kdf = Kdf::new();
        assert_eq!(*kdf.derive("correct").unwrap(), *kdf.derive("correct").unwrap());
        assert_ne!(*kdf.derive("correct").unwrap(), *kdf.derive("wrong").unwrap());
        assert_ne!(*Kdf::new().derive("correct").unwrap(), *kdf.derive("correct").unwrap());
        assert!(Kdf { iterations: MAX_ITERATIONS + 1, ..kdf.clone() }.derive("x").is_none());
        assert!(Kdf { iterations: 0, ..kdf.clone() }.derive("x").is_none());
        assert!(Kdf { name: "argon2id".into(), ..kdf }.derive("x").is_none());
    }

    #[test]
    fn known_pbkdf2_vector() {
        // RFC 7914 section 11: PBKDF2-HMAC-SHA256, P = "passwd", S = "salt", c = 1.
        let kdf = Kdf { name: KDF_NAME.into(), iterations: 1, salt: encode(b"salt") };
        let key = kdf.derive("passwd").unwrap();
        assert_eq!(
            encode(&key[..16]),
            encode(&[0x55, 0xac, 0x04, 0x6e, 0x56, 0xe3, 0x08, 0x9f, 0xec, 0x16, 0x91, 0xc2, 0x25, 0x44, 0xb6, 0x05])
        );
    }

    #[test]
    fn key_ids_and_stored_keys() {
        let key = random_key();
        assert_eq!(key_id(&key).len(), 16);
        assert_eq!(*decode_key(&encode(&*key)).unwrap(), *key);
        assert!(decode_key(&encode(&[1, 2, 3])).is_none());
    }
}
