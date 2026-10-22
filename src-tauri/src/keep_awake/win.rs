//! Windows: a power request (`powercfg /requests` lists it with its reason), and the power
//! source, whose changes are told to a hidden window of our own.

use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::Power::{
    GetPwrCapabilities, GetSystemPowerStatus, PowerClearRequest, PowerCreateRequest, PowerRequestSystemRequired, PowerSetRequest,
    RegisterPowerSettingNotification, POWERBROADCAST_SETTING, SYSTEM_POWER_CAPABILITIES, SYSTEM_POWER_STATUS,
};
use windows_sys::Win32::System::SystemServices::{GUID_ACDC_POWER_SOURCE, POWER_REQUEST_CONTEXT_VERSION};
use windows_sys::Win32::System::Threading::{POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0};
use windows_sys::Win32::UI::WindowsAndMessaging::{DefWindowProcW, DEVICE_NOTIFY_WINDOW_HANDLE, PBT_POWERSETTINGCHANGE, WM_POWERBROADCAST};

use super::{Hold, PowerChanged};

struct Request(HANDLE);

// SAFETY: a kernel object's handle, usable from any thread.
unsafe impl Send for Request {}

impl Drop for Request {
    fn drop(&mut self) {
        // SAFETY: the request this holds, set once and closed only here.
        unsafe {
            PowerClearRequest(self.0, PowerRequestSystemRequired);
            CloseHandle(self.0);
        }
    }
}

/// System required: the system doesn't sleep after a period without input. On Modern Standby
/// on battery, Windows ends it 5 minutes after the sleep time (see `limited_on_battery`).
pub fn hold(reason: &str) -> Option<Hold> {
    let mut reason: Vec<u16> = reason.encode_utf16().chain(Some(0)).collect();
    let context = REASON_CONTEXT {
        Version: POWER_REQUEST_CONTEXT_VERSION,
        Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
        Reason: REASON_CONTEXT_0 { SimpleReasonString: reason.as_mut_ptr() },
    };
    // SAFETY: the context and its string are valid for the call, which copies the reason.
    let request = unsafe { PowerCreateRequest(&context) };
    if request == INVALID_HANDLE_VALUE {
        eprintln!("can't keep the system awake: {}", std::io::Error::last_os_error());
        return None;
    }
    let request = Request(request);
    // SAFETY: the request just made.
    if unsafe { PowerSetRequest(request.0, PowerRequestSystemRequired) } == 0 {
        eprintln!("can't keep the system awake: {}", std::io::Error::last_os_error());
        return None;
    }
    Some(Box::new(request))
}

static CHANGED: OnceLock<PowerChanged> = OnceLock::new();

/// Plugged in, or unknown (`ACLineStatus` 255); a computer without a battery is plugged in.
fn external_power() -> bool {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: a structure to fill.
    let known = unsafe { GetSystemPowerStatus(&mut status) } != 0;
    !known || status.ACLineStatus != 0
}

/// The power source changes are sent only to windows registered for them: a hidden top-level
/// window on a thread of its own.
pub fn watch_power(changed: PowerChanged) {
    changed(external_power());
    if CHANGED.set(changed).is_err() {
        return;
    }
    crate::hidden_window::spawn("ZShellPowerEvents", Some(window_proc), |hwnd| {
        // SAFETY: the window was just made, on this thread; the registration lasts as long as it.
        unsafe { RegisterPowerSettingNotification(hwnd, &GUID_ACDC_POWER_SOURCE, DEVICE_NOTIFY_WINDOW_HANDLE) != 0 }
    });
}

unsafe extern "system" fn window_proc(hwnd: windows_sys::Win32::Foundation::HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_POWERBROADCAST && wparam as u32 == PBT_POWERSETTINGCHANGE && lparam != 0 {
        // With this event, `lparam` points to the setting, the only one the window is registered
        // for (`GUID_ACDC_POWER_SOURCE`), whose data follows it: a DWORD, 0 on AC power, 1 on
        // battery, 2 on a short-term source such as a UPS. Read through the pointer, as the data
        // goes past the structure's one byte.
        let setting = lparam as *const POWERBROADCAST_SETTING;
        // SAFETY: the setting the system sent, valid during the message.
        let length = unsafe { (*setting).DataLength } as usize;
        if length >= size_of::<u32>() {
            // SAFETY: `length` bytes of data, maybe unaligned.
            let source = unsafe { std::ptr::read_unaligned(std::ptr::addr_of!((*setting).Data) as *const u32) };
            if let Some(changed) = CHANGED.get() {
                changed(source == 0);
            }
        }
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Modern Standby on a computer with a battery.
pub fn limited_on_battery() -> bool {
    let mut capabilities = SYSTEM_POWER_CAPABILITIES::default();
    // SAFETY: a structure to fill.
    let known = unsafe { GetPwrCapabilities(&mut capabilities) };
    known && capabilities.AoAc && capabilities.SystemBatteriesPresent
}
