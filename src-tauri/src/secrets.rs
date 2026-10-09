//! Session and proxy passwords in the OS credential store (macOS Keychain / Windows Credential
//! Manager).
//!
//! The calls block, sometimes for long: after an update the app's signature changes and macOS
//! asks the user again for every entry. Async code uses [`password`] and [`proxy_password`],
//! which wait on the blocking pool, so that the other sessions keep running meanwhile;
//! commands run the rest on the blocking pool too.

use keyring::Entry;

const SERVICE: &str = "org.boyin.zshell";

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

/// Proxy passwords are kept apart from session passwords, which use the bare profile id.
fn proxy_entry(proxy_id: &str) -> String {
    format!("proxy:{proxy_id}")
}

pub fn get_proxy_password(proxy_id: &str) -> Option<String> {
    get_password(&proxy_entry(proxy_id))
}

pub fn set_proxy_password(proxy_id: &str, password: &str) -> keyring::Result<()> {
    set_password(&proxy_entry(proxy_id), password)
}

pub fn delete_proxy_password(proxy_id: &str) -> keyring::Result<()> {
    delete_password(&proxy_entry(proxy_id))
}

/// [`get_password`] from async code.
pub async fn password(profile_id: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || get_password(&profile_id)).await.ok().flatten()
}

/// [`get_proxy_password`] from async code.
pub async fn proxy_password(proxy_id: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || get_proxy_password(&proxy_id)).await.ok().flatten()
}
