//! Windows: a hidden top-level window on a thread of its own, for the messages the system
//! sends only to top-level windows (a setting changed, the system suspending, the session
//! locking), which the app's windows, run by Tao, don't pass on.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DispatchMessageW, GetMessageW, RegisterClassW, MSG, WNDCLASSW, WNDPROC, WS_OVERLAPPED,
};

/// Starts a thread with a hidden window of class `name` (one of its own), whose messages
/// go to `procedure` for as long as the app runs. `ready` gets the window once it is made (to
/// register it for notifications); the thread ends if it says no, with the last error logged.
/// Never shown, and not a message-only window: those don't get broadcasts.
pub fn spawn(name: &'static str, procedure: WNDPROC, ready: impl FnOnce(HWND) -> bool + Send + 'static) {
    std::thread::spawn(move || {
        let class: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        // SAFETY: a window class and a window of this thread, which runs their messages from
        // here on; the class name outlives them, and `procedure` is a window procedure.
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let window_class =
                WNDCLASSW { lpfnWndProc: procedure, hInstance: instance, lpszClassName: class.as_ptr(), ..Default::default() };
            RegisterClassW(&window_class);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                eprintln!("{name}: can't create the window: {}", std::io::Error::last_os_error());
                return;
            }
            if !ready(hwnd) {
                eprintln!("{name}: can't register for the system's messages: {}", std::io::Error::last_os_error());
                return;
            }
            let mut message: MSG = std::mem::zeroed();
            while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                DispatchMessageW(&message);
            }
        }
    });
}
