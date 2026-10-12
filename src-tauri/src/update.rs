//! Checking GitHub for a newer release: the version of the latest published release (drafts
//! and pre-releases don't count), at most once a day while the setting is on, and whenever
//! the user asks. Only a notice: the frontend shows it and opens the release page in the
//! browser; nothing is downloaded. The request carries no more than any HTTPS request does,
//! with `ZShell` as its user agent (see PRIVACY.md). The Microsoft Store package, which the
//! Store updates, never checks.
//!
//! When the last check was, and what it found, are kept in `update.json`, so that starting
//! the app several times a day doesn't check each time.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::persist;
use crate::settings::SettingsStore;

const LATEST: &str = "https://api.github.com/repos/Bokjan/ZShell/releases/latest";
const RELEASE_PAGE: &str = "https://github.com/Bokjan/ZShell/releases/tag/v";
/// How often the automatic check runs.
const INTERVAL: Duration = Duration::from_secs(24 * 3600);
/// How often the automatic check sees whether it is due (and retries one that failed).
const TICK: Duration = Duration::from_secs(3600);
/// Not while the app is starting.
const STARTUP_DELAY: Duration = Duration::from_secs(10);
const TIMEOUT: Duration = Duration::from_secs(20);

/// A published release.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Release {
    /// Without the tag's `v`: `2.11.0`.
    pub version: String,
    /// Its page on GitHub, with the release notes and the downloads.
    pub url: String,
}

/// What the frontend shows.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "UpdateStatus")]
pub struct Status {
    /// False in the Microsoft Store package, which never checks.
    pub supported: bool,
    /// The latest release, when it is newer than this version.
    pub newer: Option<Release>,
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    /// Seconds since the Unix epoch.
    checked: Option<u64>,
    latest: Option<Release>,
}

pub struct Updates {
    path: PathBuf,
    saved: Mutex<Saved>,
    /// One check at a time.
    checking: tokio::sync::Mutex<()>,
}

impl Updates {
    /// A missing or unreadable file is as if no check had run yet: it is only a cache.
    pub fn load(path: PathBuf) -> Self {
        let saved = std::fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        Self { path, saved: Mutex::new(saved), checking: tokio::sync::Mutex::new(()) }
    }

    pub fn status(&self) -> Status {
        let newer = self.saved.lock().unwrap().latest.clone().filter(|release| is_newer(&release.version));
        Status { supported: supported(), newer }
    }

    fn due(&self) -> bool {
        let checked = self.saved.lock().unwrap().checked;
        checked.is_none_or(|checked| now().saturating_sub(checked) >= INTERVAL.as_secs())
    }

    /// Asks GitHub, and tells every window what was found.
    pub async fn check(&self, app: &AppHandle) -> Result<Status> {
        let _checking = self.checking.lock().await;
        let latest = latest().await?;
        {
            let mut saved = self.saved.lock().unwrap();
            *saved = Saved { checked: Some(now()), latest: Some(latest) };
            // Only a cache: a failed write means checking again at the next start.
            let _ = persist::write_json_atomic(&self.path, &*saved);
        }
        let status = self.status();
        let _ = app.emit("update-status", &status);
        Ok(status)
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// Whether `version` is newer than this one.
fn is_newer(version: &str) -> bool {
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("the package version is semver");
    semver::Version::parse(version).is_ok_and(|version| version > current)
}

/// The latest published release.
async fn latest() -> Result<Release> {
    #[derive(Deserialize)]
    struct GitHubRelease {
        tag_name: String,
    }

    let failed = |e: reqwest::Error| Error::new("update.failed").detail(e);
    let client = reqwest::Client::builder().user_agent("ZShell").timeout(TIMEOUT).build().map_err(failed)?;
    let response = client
        .get(LATEST)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(failed)?;
    let release: GitHubRelease = response.json().await.map_err(failed)?;
    // The page is made here rather than taken from the answer: only a version goes into it.
    let version = release.tag_name.trim_start_matches('v');
    let version = semver::Version::parse(version).map_err(|e| Error::new("update.failed").detail(e))?.to_string();
    Ok(Release { url: format!("{RELEASE_PAGE}{version}"), version })
}

/// Whether this copy of the app checks: not the Microsoft Store package, which has a package
/// identity.
pub fn supported() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::APPMODEL_ERROR_NO_PACKAGE;
        use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
        let mut length = 0u32;
        // Without a buffer: ERROR_INSUFFICIENT_BUFFER with a package, APPMODEL_ERROR_NO_PACKAGE without.
        // SAFETY: a length of 0 with no buffer only asks for the length needed.
        unsafe { GetCurrentPackageFullName(&mut length, std::ptr::null_mut()) == APPMODEL_ERROR_NO_PACKAGE }
    }
    #[cfg(not(windows))]
    true
}

/// Checks once a day while the setting is on, the first time shortly after the start.
pub fn watch(app: AppHandle) {
    if !supported() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        let mut timer = tokio::time::interval(TICK);
        loop {
            timer.tick().await;
            let updates = app.state::<Updates>();
            if app.state::<SettingsStore>().get().updates.check && updates.due() {
                // A failure (offline, GitHub unreachable) is tried again at the next tick.
                let _ = updates.check(&app).await;
            }
        }
    });
}

#[tauri::command]
pub fn update_status(updates: tauri::State<Updates>) -> Status {
    updates.status()
}

/// Checks now, as the user asked.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Status> {
    if !supported() {
        return Ok(app.state::<Updates>().status());
    }
    app.state::<Updates>().check(&app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_later_versions_are_newer() {
        assert!(is_newer("999.0.0"));
        assert!(!is_newer(env!("CARGO_PKG_VERSION")));
        assert!(!is_newer("0.1.0"));
        assert!(!is_newer("not a version"));
    }
}
