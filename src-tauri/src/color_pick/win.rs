//! Windows: a screenshot of the whole virtual screen, shown in a window of its own over every
//! monitor, with a crosshair pointer. A click picks the screenshot's pixel under the pointer,
//! so that what the user sees is what they get and the click reaches no other app; Escape or
//! a right click cancels, as does anything that takes the window out of the foreground. From
//! the keyboard, the arrow keys move the pointer by a pixel (ten with Shift) and Enter or
//! Space picks. The window's title tells screen readers what to do.
//!
//! The main thread is per-monitor DPI aware (v2, set by tao), so the virtual screen's
//! bounds, the screenshot, the window and the pointer all count in physical pixels and match
//! across monitors of different scales; monitors left of or above the primary one have
//! negative coordinates, which the window's origin takes off. The window ignores
//! `WM_DPICHANGED` to keep that size, and cancels when the displays change under it.
//!
//! The window lives on the main thread, whose message loop runs its procedure: picking
//! blocks nothing, and the window's state is kept there.

use std::cell::RefCell;
use std::sync::Once;

use tauri::WebviewWindow;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EndPaint, GetDC, GetPixel, ReleaseDC,
    SelectObject, CAPTUREBLT, CLR_INVALID, HBITMAP, HDC, HGDIOBJ, PAINTSTRUCT, SRCCOPY,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetSystemMetrics, LoadCursorW, RegisterClassW, SetCursor, SetCursorPos,
    SetForegroundWindow, ShowWindow, IDC_CROSS, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOW,
    WA_INACTIVE, WM_ACTIVATE, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use super::{Done, Rgb};
use crate::error::{Error, Result};

/// The picker shown: its window, the screenshot it shows (a memory DC holding the bitmap,
/// and the object the DC held before), the virtual screen's origin, and where the result goes.
struct Picker {
    hwnd: HWND,
    shot: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    origin: POINT,
    /// Whether the left button went down in the window, so that its release picks.
    pressed: bool,
    done: Done,
}

// Only touched on the main thread, which owns the window and runs its procedure.
thread_local! {
    static PICKER: RefCell<Option<Picker>> = const { RefCell::new(None) };
}

/// Shows the picker over the screen, from `window`, which it belongs to (so that `window`
/// comes back to the foreground after it).
pub(super) fn start(window: &WebviewWindow, done: Done) -> Result<()> {
    let owner = window.hwnd().map_err(|e| Error::new("colorPick.failed").detail(e))?.0 as isize;
    window
        .run_on_main_thread(move || {
            // SAFETY: on the main thread, with the window's own handle.
            if let Err((e, done)) = unsafe { show(owner as HWND, done) } {
                let _ = done.send(Err(e));
            }
        })
        .map_err(|e| Error::new("colorPick.failed").detail(e))
}

fn failed() -> Error {
    Error::new("colorPick.failed").detail(std::io::Error::last_os_error())
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// # Safety
/// On the main thread, with `owner` one of the app's windows.
unsafe fn show(owner: HWND, done: Done) -> std::result::Result<(), (Error, Done)> {
    let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
    let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
    let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
    let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);

    // The screenshot. `CAPTUREBLT` includes layered windows (menus, tooltips, other apps'
    // translucent windows).
    let screen = GetDC(std::ptr::null_mut());
    if screen.is_null() {
        return Err((failed(), done));
    }
    let shot = CreateCompatibleDC(screen);
    let bitmap = CreateCompatibleBitmap(screen, width, height);
    if shot.is_null() || bitmap.is_null() {
        let error = failed();
        if !bitmap.is_null() {
            DeleteObject(bitmap);
        }
        if !shot.is_null() {
            DeleteDC(shot);
        }
        ReleaseDC(std::ptr::null_mut(), screen);
        return Err((error, done));
    }
    let previous = SelectObject(shot, bitmap);
    let copied = BitBlt(shot, 0, 0, width, height, screen, x, y, SRCCOPY | CAPTUREBLT);
    ReleaseDC(std::ptr::null_mut(), screen);
    let release = || {
        SelectObject(shot, previous);
        DeleteObject(bitmap);
        DeleteDC(shot);
    };
    if copied == 0 {
        let error = failed();
        release();
        return Err((error, done));
    }

    static REGISTER: Once = Once::new();
    let class = wide("ZShellColorPicker");
    let instance = GetModuleHandleW(std::ptr::null());
    REGISTER.call_once(|| {
        let class = WNDCLASSW {
            lpfnWndProc: Some(picker_proc),
            hInstance: instance,
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_CROSS),
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        RegisterClassW(&class);
    });
    let title = wide(&t!("colorPick.instructions"));
    // Owned by the app's window, which comes back to the foreground once it is gone; a tool
    // window, so that it has no taskbar button.
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        class.as_ptr(),
        title.as_ptr(),
        WS_POPUP,
        x,
        y,
        width,
        height,
        owner,
        std::ptr::null_mut(),
        instance,
        std::ptr::null(),
    );
    if hwnd.is_null() {
        let error = failed();
        release();
        return Err((error, done));
    }
    PICKER.with_borrow_mut(|picker| *picker = Some(Picker { hwnd, shot, bitmap, previous, origin: POINT { x, y }, pressed: false, done }));
    // Shown at once, without fading in over the screen it shows.
    let disabled: i32 = 1;
    DwmSetWindowAttribute(hwnd, DWMWA_TRANSITIONS_FORCEDISABLED as u32, (&disabled as *const i32).cast(), size_of::<i32>() as u32);
    crate::window::need_pointer(true);
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
    SetFocus(hwnd);
    // The class's crosshair shows once the pointer moves; it shows at once this way.
    SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_CROSS));
    Ok(())
}

/// Closes the picker, if it is still shown, and sends `result`.
///
/// # Safety
/// On the main thread.
unsafe fn finish(result: Result<Option<Rgb>>) {
    end(result, true);
}

/// # Safety
/// On the main thread; `destroy` unless the window is being destroyed already.
unsafe fn end(result: Result<Option<Rgb>>, destroy: bool) {
    // Taken out first: destroying the window calls its procedure again.
    let Some(picker) = PICKER.with_borrow_mut(Option::take) else { return };
    let _ = picker.done.send(result);
    if destroy {
        DestroyWindow(picker.hwnd);
    }
    SelectObject(picker.shot, picker.previous);
    DeleteObject(picker.bitmap);
    DeleteDC(picker.shot);
    crate::window::need_pointer(false);
}

/// The screenshot's pixel at `point`, in the window's coordinates.
///
/// # Safety
/// On the main thread.
unsafe fn pixel(point: POINT) -> Option<Rgb> {
    let shot = PICKER.with_borrow(|picker| picker.as_ref().map(|picker| picker.shot))?;
    // A `COLORREF`, `0x00bbggrr`.
    let color = GetPixel(shot, point.x, point.y);
    (color != CLR_INVALID).then_some(Rgb(color as u8, (color >> 8) as u8, (color >> 16) as u8))
}

/// Picks the pixel at `point`, in the window's coordinates.
///
/// # Safety
/// On the main thread.
unsafe fn pick(point: POINT) {
    match pixel(point) {
        Some(color) => finish(Ok(Some(color))),
        None => finish(Err(Error::new("colorPick.failed"))),
    }
}

/// Where the pointer is, in the window's coordinates.
///
/// # Safety
/// On the main thread.
unsafe fn pointer() -> Option<POINT> {
    let origin = PICKER.with_borrow(|picker| picker.as_ref().map(|picker| picker.origin))?;
    let mut point = POINT { x: 0, y: 0 };
    (GetCursorPos(&mut point) != 0).then(|| POINT { x: point.x - origin.x, y: point.y - origin.y })
}

/// The point in a mouse message's `lparam`: signed, for the window's coordinates.
fn point_of(lparam: LPARAM) -> POINT {
    POINT { x: (lparam & 0xffff) as u16 as i16 as i32, y: ((lparam >> 16) & 0xffff) as u16 as i16 as i32 }
}

unsafe extern "system" fn picker_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut paint);
            if let Some(shot) = PICKER.with_borrow(|picker| picker.as_ref().map(|picker| picker.shot)) {
                let rect = paint.rcPaint;
                BitBlt(hdc, rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, shot, rect.left, rect.top, SRCCOPY);
            }
            EndPaint(hwnd, &paint);
            0
        }
        // The screenshot covers it all.
        WM_ERASEBKGND => 1,
        WM_LBUTTONDOWN => {
            SetCapture(hwnd);
            PICKER.with_borrow_mut(|picker| picker.as_mut().map(|picker| picker.pressed = true));
            0
        }
        // On release rather than press, so that the release doesn't reach the window below.
        WM_LBUTTONUP => {
            ReleaseCapture();
            if PICKER.with_borrow(|picker| picker.as_ref().is_some_and(|picker| picker.pressed)) {
                pick(point_of(lparam));
            }
            0
        }
        WM_RBUTTONDOWN => 0,
        // As for the left button: the window below would show its context menu.
        WM_RBUTTONUP => {
            finish(Ok(None));
            0
        }
        WM_KEYDOWN => {
            let step = if GetKeyState(VK_SHIFT as i32) < 0 { 10 } else { 1 };
            let key = wparam as u16;
            let (dx, dy) = match key {
                VK_LEFT => (-step, 0),
                VK_RIGHT => (step, 0),
                VK_UP => (0, -step),
                VK_DOWN => (0, step),
                VK_ESCAPE => {
                    finish(Ok(None));
                    return 0;
                }
                VK_RETURN | VK_SPACE => {
                    if let Some(point) = pointer() {
                        pick(point);
                    }
                    return 0;
                }
                _ => return 0,
            };
            let mut point = POINT { x: 0, y: 0 };
            if GetCursorPos(&mut point) != 0 {
                SetCursorPos(point.x + dx, point.y + dy);
            }
            0
        }
        // Another window came to the foreground (Alt+Tab, the Start menu): cancelled.
        WM_ACTIVATE if (wparam & 0xffff) as u32 == WA_INACTIVE => {
            finish(Ok(None));
            0
        }
        // The screenshot no longer matches the displays.
        WM_DISPLAYCHANGE => {
            finish(Ok(None));
            0
        }
        // Keeps the virtual screen's size rather than the size suggested for the new scale.
        WM_DPICHANGED => 0,
        // Closed some other way (Alt+F4).
        WM_DESTROY => {
            if PICKER.with_borrow(|picker| picker.as_ref().is_some_and(|picker| picker.hwnd == hwnd)) {
                end(Ok(None), false);
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
