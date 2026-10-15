//! Host certificates, trusted through `@cert-authority` lines as OpenSSH's `check_host_key`
//! does: a certificate valid for the host and signed by an authority known_hosts trusts for it
//! stands for the host's key. Any other certificate is set aside and the key inside it checked
//! like a plain one, which is no less safe: that key is what the server signed the key
//! exchange with.

use std::path::{Path, PathBuf};

use russh::keys::ssh_key::certificate::CertType;
use russh::keys::{Algorithm, Certificate, PublicKey};

use super::{entries_at, glob_matches, host_pattern, Entry};
use crate::error::Result;

/// What known_hosts says about a certificate a server presented.
#[derive(Debug, PartialEq)]
pub enum CertificateCheck {
    /// Signed by an authority trusted for the host, and valid for it.
    Trusted,
    /// No authority trusted for the host signed it.
    Untrusted,
    /// Signed by an authority trusted for the host, but not valid for it.
    Invalid(Invalid),
    /// The key in it, or the authority that signed it, is marked `@revoked` on this line of
    /// this file.
    Revoked { path: PathBuf, line: usize },
}

/// Why a certificate from a trusted authority isn't valid, as OpenSSH's `sshkey_cert_check_host`
/// tells.
#[derive(Debug, PartialEq)]
pub enum Invalid {
    /// Signed with an algorithm OpenSSH's `CASignatureAlgorithms` doesn't allow by default (RSA
    /// with SHA-1, DSA) or ZShell can't check; its name as written, e.g. `ssh-rsa`.
    SignatureAlgorithm(String),
    /// The signature doesn't verify: the certificate is damaged or forged.
    Signature,
    /// A user certificate.
    NotHost,
    NotYetValid,
    Expired,
    /// It names no host, which OpenSSH doesn't take as every host.
    NoPrincipals,
    /// None of the host names in it is the one connected to.
    WrongHost,
    /// Host certificates have no critical options, so one that has some isn't understood.
    CriticalOptions,
}

/// The lines with `marker` whose key could be read.
fn marked<'a>(entries: &'a [(&'a Path, Entry)], marker: &'a str) -> impl Iterator<Item = (&'a Path, &'a Entry, &'a PublicKey)> {
    entries
        .iter()
        .filter(move |(_, entry)| entry.marker.as_deref() == Some(marker))
        .filter_map(|(path, entry)| entry.key.as_ref().map(|key| (*path, entry, key)))
}

/// Whether a line trusts an authority for `host_port` (see [`host_pattern`]).
pub(super) fn has_authority(entries: &[(&Path, Entry)], host_port: &str) -> bool {
    marked(entries, "cert-authority").any(|(_, entry, _)| entry.applies_to(host_port))
}

/// Checks `certificate`, presented by `host:port` at `now` (seconds since 1970), against the
/// files in `paths`.
pub(super) fn check_certificate_at(
    paths: &[PathBuf],
    host: &str,
    port: u16,
    certificate: &Certificate,
    now: u64,
) -> Result<CertificateCheck> {
    let host_port = host_pattern(host, port).to_ascii_lowercase();
    let entries = entries_at(paths)?;
    // As for plain keys, a revoked line applies whatever host it names.
    let revoked = |key: &PublicKey| key.key_data() == certificate.public_key() || key.key_data() == certificate.signature_key();
    if let Some((path, entry, _)) = marked(&entries, "revoked").find(|(_, _, key)| revoked(key)) {
        return Ok(CertificateCheck::Revoked { path: path.to_path_buf(), line: entry.line });
    }
    let trusted = marked(&entries, "cert-authority")
        .any(|(_, entry, key)| entry.applies_to(&host_port) && key.key_data() == certificate.signature_key());
    if !trusted {
        return Ok(CertificateCheck::Untrusted);
    }
    Ok(match invalid(certificate, &host.to_ascii_lowercase(), now) {
        Some(why) => CertificateCheck::Invalid(why),
        None => CertificateCheck::Trusted,
    })
}

/// Why `certificate` isn't valid for `host` at `now`, if it isn't.
fn invalid(certificate: &Certificate, host: &str, now: u64) -> Option<Invalid> {
    let algorithm = certificate.signature().algorithm();
    // Checked before the signature: ssh-key may not verify SHA-1 ones at all.
    if !matches!(
        algorithm,
        Algorithm::Ed25519
            | Algorithm::Ecdsa { .. }
            | Algorithm::SkEd25519
            | Algorithm::SkEcdsaSha2NistP256
            | Algorithm::Rsa { hash: Some(_) }
    ) {
        return Some(Invalid::SignatureAlgorithm(algorithm.as_str().to_owned()));
    }
    // The authority is the one known_hosts trusts (compared by the caller), so checking the
    // signature against it is the whole check, which ssh-key leaves to us. OpenSSH checks
    // every certificate's signature as it reads it.
    if certificate.verify_signature().is_err() {
        return Some(Invalid::Signature);
    }
    if certificate.cert_type() != CertType::Host {
        return Some(Invalid::NotHost);
    }
    if now < certificate.valid_after() {
        return Some(Invalid::NotYetValid);
    }
    if now >= certificate.valid_before() {
        return Some(Invalid::Expired);
    }
    let principals = certificate.valid_principals();
    if principals.is_empty() {
        return Some(Invalid::NoPrincipals);
    }
    // OpenSSH's `match_pattern` compares case; here it is ignored, as in host names.
    if !principals.iter().any(|principal| glob_matches(principal.as_bytes(), host.as_bytes())) {
        return Some(Invalid::WrongHost);
    }
    if !certificate.critical_options().is_empty() {
        return Some(Invalid::CriticalOptions);
    }
    None
}

/// The certificate algorithms to ask a host with a trusted authority for: those of the
/// `preferred` host key algorithms that OpenSSH asks for by default (not RSA with SHA-1).
pub(super) fn certificate_algorithms(preferred: &[Algorithm]) -> Vec<Algorithm> {
    preferred.iter().filter(|algorithm| !matches!(algorithm, Algorithm::Rsa { hash: None } | Algorithm::Dsa)).cloned().collect()
}
