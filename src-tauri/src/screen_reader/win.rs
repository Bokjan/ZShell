//! Windows: the system's screen reader flag (`SPI_GETSCREENREADER`), which Narrator, NVDA and
//! JAWS set while they run. Its changes are broadcast to top-level windows
//! (`WM_SETTINGCHANGE`), so a hidden one of our own, on a thread of its own, hears them.

use std::ffi::c_void;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, SystemParametersInfoW, SPI_GETSCREENREADER, SPI_SETSCREENREADER, WM_SETTINGCHANGE,
};

use super::Changed;

static CHANGED: OnceLock<Changed> = OnceLock::new();

fn running() -> bool {
    let mut on: i32 = 0;
    // SAFETY: SPI_GETSCREENREADER writes a BOOL to the pointer.
    let read = unsafe { SystemParametersInfoW(SPI_GETSCREENREADER, 0, &mut on as *mut i32 as *mut c_void, 0) };
    read != 0 && on != 0
}

pub fn watch(changed: Changed) {
    if CHANGED.set(changed).is_err() {
        return;
    }
    if let Some(changed) = CHANGED.get() {
        changed(running());
    }
    crate::hidden_window::spawn("ZShellScreenReader", Some(window_proc), |_| true);
}

unsafe extern "system" fn window_proc(hwnd: windows_sys::Win32::Foundation::HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_SETTINGCHANGE && wparam as u32 == SPI_SETSCREENREADER {
        if let Some(changed) = CHANGED.get() {
            changed(running());
        }
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
