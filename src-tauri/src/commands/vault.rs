//! Commands for the vault (see `vault.rs`): its state, unlocking it, resetting it and changing
//! its protection. They wait on the blocking pool: the credential store may ask the user, and
//! keys are derived from passwords.

use std::sync::Arc;

use tauri::{AppHandle, Manager, WebviewWindow};

use crate::applock::AppLock;
use crate::config::ProfileStore;
use crate::error::Result;
use crate::persist::SetAside;
use crate::quick::QuickCommandStore;
use crate::vault::{Target, Vault, VaultStatus};

use super::{blocking, notified, Change};

/// The stores take what the vault has, once it is unlocked or reset.
fn reload(app: &AppHandle) {
    app.state::<ProfileStore>().reload();
    app.state::<QuickCommandStore>().reload();
}

#[tauri::command]
pub async fn vault_status(app: AppHandle) -> Result<VaultStatus> {
    blocking(app, |app| Ok(app.state::<Arc<Vault>>().status())).await
}

/// Unlocks with the key in the credential store, reading the file again first.
#[tauri::command]
pub async fn vault_try_unlock(app: AppHandle) -> Result<VaultStatus> {
    blocking(app, |app| {
        let status = app.state::<Arc<Vault>>().try_unlock();
        reload(app);
        Ok(status)
    })
    .await
}

/// Unlocks with the master password; `vault.wrongPassword` if it isn't the one. The app lock
/// then doesn't ask again, if the password was checked (the vault wasn't open already).
#[tauri::command]
pub async fn vault_unlock(app: AppHandle, password: String) -> Result<VaultStatus> {
    blocking(app, move |app| {
        let (status, opened) = app.state::<Arc<Vault>>().unlock(&password)?;
        if opened {
            app.state::<Arc<AppLock>>().unlocked_by_password();
        }
        reload(app);
        Ok(status)
    })
    .await
}

/// Deletes the locked vault and everything in it.
#[tauri::command]
pub async fn vault_reset(app: AppHandle) -> Result<VaultStatus> {
    blocking(app, |app| {
        let status = app.state::<Arc<Vault>>().reset(&app.state::<SetAside>())?;
        reload(app);
        Ok(status)
    })
    .await
}

/// Changes the protection (see `Vault::set_protection`).
#[tauri::command]
pub async fn vault_set_protection(
    app: AppHandle,
    window: WebviewWindow,
    change: Target,
    current: Option<String>,
    skip_unreadable: bool,
) -> Result<VaultStatus> {
    let result = blocking(app, move |app| app.state::<Arc<Vault>>().set_protection(change, current.as_deref(), skip_unreadable)).await;
    notified(&window, Change::Vault, result)
}

/// Whether the system's credential store can be used; on Linux there may be none.
#[tauri::command]
pub async fn credential_store_available(app: AppHandle) -> Result<bool> {
    blocking(app, |app| Ok(app.state::<Arc<Vault>>().store_available())).await
}
