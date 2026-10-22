//! Windows: Windows Hello (a PIN, a fingerprint, a face), the last input's time, and the
//! session's locking and the system's suspending and resuming, told to a hidden window of our
//! own.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use tauri::WebviewWindow;
use windows::core::{factory, HSTRING};
use windows::Security::Credentials::UI::{UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
use windows_future::IAsyncOperation;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK,
};

use super::{Method, System, SystemEvent};
use crate::error::{Error, Result};

pub struct Platform;

fn failed(error: windows::core::Error) -> Error {
    Error::new("appLock.failed").detail(error)
}

impl System for Platform {
    fn method(&self) -> Result<Method> {
        let availability = UserConsentVerifier::CheckAvailabilityAsync().and_then(|check| check.join()).map_err(failed)?;
        match availability {
            // Busy (another app is asking) passes; it isn't gone.
            UserConsentVerifierAvailability::Available | UserConsentVerifierAvailability::DeviceBusy => Ok(Method::WindowsHello),
            UserConsentVerifierAvailability::DisabledByPolicy => Err(Error::new("appLock.helloDisabled")),
            _ => Err(Error::new("appLock.helloNotSetUp")),
        }
    }

    /// Attached to the window: a desktop app's plain `RequestVerificationAsync` may show its
    /// dialog behind the window.
    fn authenticate(&self, window: Option<&WebviewWindow>) -> Result<()> {
        let message = HSTRING::from(t!("appLock.reasonWindows"));
        let request = match window.and_then(|window| window.hwnd().ok()) {
            Some(hwnd) => {
                // The factory initializes the thread for COM if it wasn't.
                let interop = factory::<UserConsentVerifier, IUserConsentVerifierInterop>().map_err(failed)?;
                // SAFETY: `hwnd` is the main window, which outlives the dialog.
                unsafe {
                    interop.RequestVerificationForWindowAsync::<IAsyncOperation<UserConsentVerificationResult>>(HWND(hwnd.0), &message)
                }
            }
            None => UserConsentVerifier::RequestVerificationAsync(&message),
        };
        match request.and_then(|request| request.join()).map_err(failed)? {
            UserConsentVerificationResult::Verified => Ok(()),
            UserConsentVerificationResult::Canceled => Err(Error::new("appLock.cancelled")),
            UserConsentVerificationResult::DisabledByPolicy => Err(Error::new("appLock.helloDisabled")),
            UserConsentVerificationResult::DeviceNotPresent | UserConsentVerificationResult::NotConfiguredForUser => {
                Err(Error::new("appLock.helloNotSetUp"))
            }
            _ => Err(Error::new("appLock.failed")),
        }
    }

    fn idle_seconds(&self) -> Option<u64> {
        let mut info = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        // SAFETY: `info` is a LASTINPUTINFO with its size set.
        if unsafe { GetLastInputInfo(&mut info) } == 0 {
            return None;
        }
        // Both are milliseconds since the system started, wrapping after 49.7 days.
        // SAFETY: takes nothing.
        let idle = unsafe { GetTickCount() }.wrapping_sub(info.dwTime);
        Some(u64::from(idle) / 1000)
    }

    fn watches_system_lock(&self) -> bool {
        WATCHING.load(Ordering::Relaxed)
    }
}

static ON_EVENT: OnceLock<Box<dyn Fn(SystemEvent) + Send + Sync>> = OnceLock::new();
/// Set once the window is registered for session notifications; if that fails, the settings
/// don't offer locking with the system.
static WATCHING: AtomicBool = AtomicBool::new(false);

/// A hidden top-level window on a thread of its own: suspending and resuming are broadcast to
/// top-level windows only, and a window registered for session notifications gets the screen
/// locking and switching user (`WTS_SESSION_LOCK` for both).
pub fn watch_system(on_event: Box<dyn Fn(SystemEvent) + Send + Sync>) {
    if ON_EVENT.set(on_event).is_err() {
        return;
    }
    crate::hidden_window::spawn("ZShellSystemEvents", Some(window_proc), |hwnd| {
        // SAFETY: the window was just made, on this thread.
        if unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } == 0 {
            return false;
        }
        WATCHING.store(true, Ordering::Relaxed);
        true
    });
}

unsafe extern "system" fn window_proc(hwnd: windows_sys::Win32::Foundation::HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let event = match (msg, wparam as u32) {
        (WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK) | (WM_POWERBROADCAST, PBT_APMSUSPEND) => Some(SystemEvent::Locking),
        // Sent on every resume, also when no user is there (`PBT_APMRESUMESUSPEND` only follows
        // once there is input), and on leaving Modern Standby.
        (WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC) => Some(SystemEvent::Woke),
        _ => None,
    };
    if let (Some(event), Some(on_event)) = (event, ON_EVENT.get()) {
        on_event(event);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
