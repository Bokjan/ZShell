//! Windows 11 shows the snap layouts when the pointer rests on whatever answers
//! `WM_NCHITTEST` with `HTMAXBUTTON`. The web view's own child window covers the whole
//! client area and gets all mouse messages, and a web page can't give that answer, so a
//! transparent child window above the web view, over the frontend's maximize button, does.
//! (Tauri resizes undecorated windows from their top edge the same way.) It never paints,
//! so the frontend's button shows through; it takes the mouse from the page, so it reports
//! hover and press for the button's style, and maximizes or restores on click itself.
//! `WS_EX_LAYERED` or `WS_EX_TRANSPARENT` would make the hit test pass through it.
//! Each window has its own button, kept by the window's handle.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, Once, OnceLock};

use tauri::{AppHandle, Emitter, EventTarget};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_DEFAULT};
use windows_sys::Win32::Graphics::Gdi::{GetStockObject, NULL_BRUSH};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, EnableMenuItem, GetCursorPos, GetParent, GetSystemMenu, IsZoomed, LoadCursorW, PostMessageW,
    RegisterClassW, SetMenuDefaultItem, SetWindowPos, ShowCursor, ShowWindow, TrackPopupMenu, HTMAXBUTTON, HWND_TOP, IDC_ARROW,
    MA_NOACTIVATE, MF_BYCOMMAND, MF_ENABLED, MF_GRAYED, SC_CLOSE, SC_MAXIMIZE, SC_MINIMIZE, SC_MOVE, SC_RESTORE, SC_SIZE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SW_HIDE, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_DESTROY, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_NCLBUTTONDBLCLK,
    WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_SYSCOMMAND, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS,
};

use super::Rect;

/// For reporting the button's state to the frontend from the window procedure.
type IsWindowArranged = unsafe extern "system" fn(HWND) -> BOOL;

/// `IsWindowArranged` (whether the window is snapped), looked up at run time: older
/// Windows 10 builds don't have it, and a static import would keep the app from starting.
fn is_window_arranged() -> Option<IsWindowArranged> {
    static FUNCTION: OnceLock<Option<IsWindowArranged>> = OnceLock::new();
    *FUNCTION.get_or_init(|| unsafe {
        let user32 = GetModuleHandleW(wide("user32.dll").as_ptr());
        if user32.is_null() {
            return None;
        }
        GetProcAddress(user32, c"IsWindowArranged".as_ptr().cast())
            .map(|f| std::mem::transmute::<unsafe extern "system" fn() -> isize, IsWindowArranged>(f))
    })
}

/// The border's color while snapped, a `COLORREF`; the system's until the frontend sets it.
pub(super) static SNAPPED_BORDER: AtomicU32 = AtomicU32::new(DWMWA_COLOR_DEFAULT);

/// `#rrggbb` as a `COLORREF` (`0x00bbggrr`).
pub(super) fn colorref(color: &str) -> Option<u32> {
    let hex = color.strip_prefix('#').filter(|hex| hex.len() == 6)?;
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some(((rgb & 0xff) << 16) | (rgb & 0xff00) | (rgb >> 16))
}

/// The color last set on each window's border, by its handle: moves come often, the
/// border rarely changes.
static BORDERS: Mutex<BTreeMap<isize, u32>> = Mutex::new(BTreeMap::new());

/// # Safety
/// `hwnd` is one of the app's windows.
pub(super) unsafe fn update_border(hwnd: HWND) {
    let Some(is_window_arranged) = is_window_arranged() else { return };
    let color = if is_window_arranged(hwnd) != 0 { SNAPPED_BORDER.load(Ordering::Relaxed) } else { DWMWA_COLOR_DEFAULT };
    if BORDERS.lock().unwrap().insert(hwnd as isize, color) != Some(color) {
        // Fails on Windows 10, which draws no such border.
        DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR as u32, (&color as *const u32).cast(), size_of::<u32>() as u32);
    }
}

pub(super) static APP: OnceLock<AppHandle> = OnceLock::new();

#[derive(Clone, Copy, Default, PartialEq, serde::Serialize)]
struct ButtonState {
    hover: bool,
    pressed: bool,
}

/// A window's maximize button: its native window, the label of the window it is in, and
/// the state the frontend was last told.
struct Button {
    hwnd: HWND,
    label: String,
    state: ButtonState,
}

// Only touched on the main thread, which owns the windows and runs their procedures.
thread_local! {
    /// By the handle of the window each is in.
    static BUTTONS: RefCell<BTreeMap<isize, Button>> = const { RefCell::new(BTreeMap::new()) };
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn state(parent: HWND) -> ButtonState {
    BUTTONS.with_borrow(|buttons| buttons.get(&(parent as isize)).map(|button| button.state).unwrap_or_default())
}

/// Records the state of the button in window `parent`, and tells that window's page.
fn set_state(parent: HWND, state: ButtonState) {
    let label = BUTTONS.with_borrow_mut(|buttons| {
        let button = buttons.get_mut(&(parent as isize))?;
        (button.state != state).then(|| {
            button.state = state;
            button.label.clone()
        })
    });
    if let (Some(label), Some(app)) = (label, APP.get()) {
        let _ = app.emit_to(EventTarget::webview_window(label), "window-maximize-button", state);
    }
}

/// Forgets what was kept about a window that is gone, whose handle may be reused.
pub(super) fn forget(hwnd: isize) {
    BUTTONS.with_borrow_mut(|buttons| buttons.remove(&hwnd));
    BORDERS.lock().unwrap().remove(&hwnd);
}

/// # Safety
/// On the main thread, with `parent` the window labelled `label`.
pub(super) unsafe fn place_maximize_button(parent: HWND, label: String, rect: Option<Rect>, scale: f64) {
    let existing = BUTTONS.with_borrow(|buttons| buttons.get(&(parent as isize)).map(|button| button.hwnd));
    let button = match existing {
        Some(button) => button,
        None => {
            let button = create_button(parent);
            if button.is_null() {
                return;
            }
            let entry = Button { hwnd: button, label, state: ButtonState::default() };
            BUTTONS.with_borrow_mut(|buttons| buttons.insert(parent as isize, entry));
            button
        }
    };
    match rect {
        Some(rect) => {
            // Device pixels, rounded the way the page's edges are.
            let left = (rect.x * scale).round() as i32;
            let top = (rect.y * scale).round() as i32;
            let right = ((rect.x + rect.width) * scale).round() as i32;
            let bottom = ((rect.y + rect.height) * scale).round() as i32;
            // On top again each time: the web view and Tauri's resize border are siblings.
            SetWindowPos(button, HWND_TOP, left, top, right - left, bottom - top, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        None => {
            ShowWindow(button, SW_HIDE);
            set_state(parent, ButtonState::default());
        }
    }
}

unsafe fn create_button(parent: HWND) -> HWND {
    static REGISTER: Once = Once::new();
    let class = wide("ZShellMaximizeButton");
    let instance = GetModuleHandleW(std::ptr::null());
    REGISTER.call_once(|| {
        let class = WNDCLASSW {
            lpfnWndProc: Some(button_proc),
            hInstance: instance,
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: GetStockObject(NULL_BRUSH),
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        RegisterClassW(&class);
    });
    CreateWindowExW(
        0,
        class.as_ptr(),
        std::ptr::null(),
        WS_CHILD | WS_CLIPSIBLINGS,
        0,
        0,
        0,
        0,
        parent,
        std::ptr::null_mut(),
        instance,
        std::ptr::null(),
    )
}

unsafe extern "system" fn button_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => HTMAXBUTTON as LRESULT,
        WM_NCMOUSEMOVE => {
            let parent = GetParent(hwnd);
            if !state(parent).hover {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut track);
                set_state(parent, ButtonState { hover: true, ..state(parent) });
            }
            0
        }
        WM_NCMOUSELEAVE => {
            set_state(GetParent(hwnd), ButtonState::default());
            0
        }
        // Not to `DefWindowProcW`, which would run the caption button logic on this child.
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => {
            set_state(GetParent(hwnd), ButtonState { hover: true, pressed: true });
            0
        }
        WM_NCLBUTTONUP => {
            let parent = GetParent(hwnd);
            if state(parent).pressed {
                set_state(parent, ButtonState { hover: true, pressed: false });
                let command = if IsZoomed(parent) != 0 { SC_RESTORE } else { SC_MAXIMIZE };
                PostMessageW(parent, WM_SYSCOMMAND, command as WPARAM, 0);
            }
            0
        }
        // Keyboard focus stays in the web view.
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        // Destroyed with its window, which may be gone before `destroyed` asks its handle.
        WM_DESTROY => {
            let parent = BUTTONS.with_borrow(|buttons| buttons.iter().find(|(_, button)| button.hwnd == hwnd).map(|(parent, _)| *parent));
            if let Some(parent) = parent {
                forget(parent);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// # Safety
/// On the main thread, with `hwnd` the main window.
pub(super) unsafe fn show_system_menu(hwnd: HWND) {
    let menu = GetSystemMenu(hwnd, 0);
    if menu.is_null() {
        return;
    }
    // As the system sets them for a native title bar.
    let maximized = IsZoomed(hwnd) != 0;
    for (item, enabled) in [
        (SC_RESTORE, maximized),
        (SC_MOVE, !maximized),
        (SC_SIZE, !maximized),
        (SC_MINIMIZE, true),
        (SC_MAXIMIZE, !maximized),
        (SC_CLOSE, true),
    ] {
        EnableMenuItem(menu, item, MF_BYCOMMAND | if enabled { MF_ENABLED } else { MF_GRAYED });
    }
    SetMenuDefaultItem(menu, SC_CLOSE, 0);
    let mut point = POINT { x: 0, y: 0 };
    GetCursorPos(&mut point);
    let command = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, point.x, point.y, 0, hwnd, std::ptr::null());
    if command != 0 {
        PostMessageW(hwnd, WM_SYSCOMMAND, command as WPARAM, 0);
    }
}

// Only touched on the main thread, whose input queue the web view's and the dialogs' windows share.
thread_local! {
    /// The file dialogs open, and how many times the pointer was shown for them.
    static FILE_DIALOGS: Cell<(u32, u32)> = const { Cell::new((0, 0)) };
}

/// A file dialog is opening (`open`) or has closed: the pointer shows while one is open.
///
/// WebView2 follows the "Hide pointer while typing" mouse setting by lowering the cursor
/// display count with `ShowCursor(FALSE)`, and raises it again only once the pointer moves
/// over the web view. The count belongs to the input queue, which the dialogs share, so a
/// dialog opened right after typing (`rz` in the terminal) had no pointer. Raising the count
/// to 0 shows it; lowering it as much once the last dialog has closed leaves it where
/// WebView2 expects it. Chromium stopped calling `ShowCursor` for this in October 2026;
/// TODO.md says when this can go.
pub(super) fn file_dialog(open: bool) {
    let (mut dialogs, mut shown) = FILE_DIALOGS.get();
    if open {
        dialogs += 1;
        // Each call raises the count by one and returns it.
        loop {
            shown += 1;
            if unsafe { ShowCursor(1) } >= 0 {
                break;
            }
        }
    } else if dialogs > 0 {
        dialogs -= 1;
        if dialogs == 0 {
            for _ in 0..shown {
                unsafe { ShowCursor(0) };
            }
            shown = 0;
        }
    }
    FILE_DIALOGS.set((dialogs, shown));
}
