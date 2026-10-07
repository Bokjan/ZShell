//! Profile passwords in the OS credential store (macOS Keychain / Windows Credential Manager).

use keyring::Entry;

const SERVICE: &str = "com.bokjan.zshell";

pub fn get_password(profile_id: &str) -> Option<String> {
    Entry::new(SERVICE, profile_id).ok()?.get_password().ok()
}

pub fn set_password(profile_id: &str, password: &str) -> keyring::Result<()> {
    Entry::new(SERVICE, profile_id)?.set_password(password)
}

pub fn delete_password(profile_id: &str) -> keyring::Result<()> {
    match Entry::new(SERVICE, profile_id)?.delete_credential() {
        Err(keyring::Error::NoEntry) => Ok(()),
        result => result,
    }
}
