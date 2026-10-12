//! macOS: LocalAuthentication (Touch ID, else the login password), the HID system's idle time,
//! and the workspace's notifications. LocalAuthentication works with an ad-hoc signature and
//! needs no entitlement; only keychain items requiring biometrics would need a Developer ID.

use std::ptr::NonNull;
use std::sync::mpsc;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_app_kit::{NSWorkspace, NSWorkspaceSessionDidResignActiveNotification, NSWorkspaceWillSleepNotification};
use objc2_foundation::{NSDistributedNotificationCenter, NSError, NSNotification, NSString};
use objc2_local_authentication::{LAContext, LAError, LAPolicy};
use tauri::WebviewWindow;

use super::{Method, System};
use crate::error::{Error, Result};

pub struct Platform;

impl System for Platform {
    fn method(&self) -> Result<Method> {
        // SAFETY: a context of our own, used on this thread only.
        unsafe {
            let context = LAContext::new();
            if context.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthenticationWithBiometrics).is_ok() {
                return Ok(Method::TouchId);
            }
            match context.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthentication) {
                Ok(()) => Ok(Method::Password),
                Err(error) => Err(Error::new("appLock.unavailable").detail(error.localizedDescription())),
            }
        }
    }

    /// The system's own dialog; it isn't attached to the window.
    fn authenticate(&self, _window: Option<&WebviewWindow>) -> Result<()> {
        let (sender, receiver) = mpsc::channel();
        let reply = RcBlock::new(move |success: Bool, error: *mut NSError| {
            // SAFETY: when not null, `error` is a valid NSError for the duration of the block.
            let error = unsafe { error.as_ref() }.map(|e| (e.code(), e.localizedDescription().to_string()));
            let _ = sender.send((success.as_bool(), error));
        });
        // macOS shows it as "“ZShell” is trying to <reason>."
        let reason = NSString::from_str(&t!("appLock.reasonMacos"));
        // SAFETY: the reply block is sendable (it only holds a channel's sender), and the
        // context lives until the reply has come, as evaluation requires.
        let context = unsafe { LAContext::new() };
        unsafe { context.evaluatePolicy_localizedReason_reply(LAPolicy::DeviceOwnerAuthentication, &reason, &reply) };
        let (success, error) = receiver.recv().map_err(|_| Error::new("appLock.failed"))?;
        drop(context);
        // The password is in the same dialog with this policy, so the fallback button shouldn't
        // end it; if it does, the user only chose not to use Touch ID.
        const CANCELLED: [LAError; 4] = [LAError::UserCancel, LAError::SystemCancel, LAError::AppCancel, LAError::UserFallback];
        match (success, error) {
            (true, _) => Ok(()),
            (false, Some((code, _))) if CANCELLED.contains(&LAError(code)) => Err(Error::new("appLock.cancelled")),
            (false, Some((_, message))) => Err(Error::new("appLock.failed").detail(message)),
            (false, None) => Err(Error::new("appLock.failed")),
        }
    }

    fn idle_seconds(&self) -> Option<u64> {
        // Any input event: keys, the mouse, the trackpad, a tablet.
        const ANY_INPUT_EVENT: u32 = u32::MAX;
        const HID_SYSTEM_STATE: i32 = 1;
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
        }
        // SAFETY: a pure query, callable from any thread.
        let seconds = unsafe { CGEventSourceSecondsSinceLastEventType(HID_SYSTEM_STATE, ANY_INPUT_EVENT) };
        Some(seconds as u64)
    }

    fn watches_system_lock(&self) -> bool {
        true
    }
}

/// The workspace's sleeping and switching user, and the screen locking, which only a
/// distributed notification tells. Locking as they happen, so that the window is covered by
/// the time the screen shows again.
pub fn watch_system(on_lock: Box<dyn Fn() + Send + Sync>) {
    let on_lock: std::sync::Arc<dyn Fn() + Send + Sync> = on_lock.into();
    let block = |on_lock: std::sync::Arc<dyn Fn() + Send + Sync>| RcBlock::new(move |_: NonNull<NSNotification>| on_lock());
    // SAFETY: the notification names are AppKit's constants and a literal; the centers keep
    // the observers for as long as the app runs, so the tokens can go.
    unsafe {
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        for name in [NSWorkspaceWillSleepNotification, NSWorkspaceSessionDidResignActiveNotification] {
            let _ = workspace.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block(on_lock.clone()));
        }
        let screen_locked = NSString::from_str("com.apple.screenIsLocked");
        let _ = NSDistributedNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(&screen_locked),
            None,
            None,
            &block(on_lock),
        );
    }
}
