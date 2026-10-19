//! Windows 11 shows the snap layouts when the pointer rests on whatever answers
//! `WM_NCHITTEST` with `HTMAXBUTTON`. The web view's own child window covers the whole
//! client area and gets all mouse messages, and a web page can't give that answer, so a
//! transparent child window above the web view, over the frontend's maximize button, does.
//! (Tauri resizes undecorated windows from their top edge the same way.) It never paints,
//! so the frontend's button shows through; it takes the mouse from the page, so it reports
//! hover and press for the button's style, and maximizes or restores on click itself.
//! `WS_EX_LAYERED` or `WS_EX_TRANSPARENT` would make the hit test pass through it.
//! Each window has its own button, kept by the window's handle.
//!
//! Also the rest of Windows' part of `window` (see its module docs): the window has no
//! decorations, the system menu shows at a right click on the title bar, the border takes the
//! title bar's color while snapped, and the pointer shows in file dialogs opened right after
//! typing.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, Once, OnceLock};

use tauri::{AppHandle, Manager, WebviewWindow, WebviewWindowBuilder, Wry};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_DEFAULT};
use windows_sys::Win32::Graphics::Gdi::{GetStockObject, NULL_BRUSH};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, EnableMenuItem, GetClassNameW, GetCursorPos, GetForegroundWindow, GetParent, GetSystemMenu,
    GetWindowLongPtrW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, IsZoomed, LoadCursorW, PostMessageW, RegisterClassW,
    SetMenuDefaultItem, SetWindowPos, ShowCursor, ShowWindow, TrackPopupMenu, EVENT_SYSTEM_FOREGROUND, GWL_EXSTYLE, HTMAXBUTTON, HWND_TOP,
    IDC_ARROW, MA_NOACTIVATE, MF_BYCOMMAND, MF_ENABLED, MF_GRAYED, SC_CLOSE, SC_MAXIMIZE, SC_MINIMIZE, SC_MOVE, SC_RESTORE, SC_SIZE,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINEVENT_OUTOFCONTEXT, WM_DESTROY,
    WM_MOUSEACTIVATE, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_SYSCOMMAND,
    WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_TOPMOST,
};

use super::{DoubleClick, Rect, WindowButton};

/// For reporting the button's state to the frontend from the window procedure.
type IsWindowArranged = unsafe extern "system" fn(HWND) -> BOOL;

/// `IsWindowArranged` (whether the window is snapped), looked up at run time: older
/// Windows 10 builds don't have it, and a static import would keep the app from starting.
fn is_window_arranged() -> Option<IsWindowArranged> {
    static FUNCTION: OnceLock<Option<IsWindowArranged>> = OnceLock::new();
    // SAFETY: the name is a NUL-terminated wide string; user32 stays loaded (the app's windows
    // need it), and the function found has the signature documented for `IsWindowArranged`.
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
static SNAPPED_BORDER: AtomicU32 = AtomicU32::new(DWMWA_COLOR_DEFAULT);

/// `#rrggbb` as a `COLORREF` (`0x00bbggrr`).
fn colorref(color: &str) -> Option<u32> {
    let hex = color.strip_prefix('#').filter(|hex| hex.len() == 6)?;
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some(((rgb & 0xff) << 16) | (rgb & 0xff00) | (rgb >> 16))
}

/// The color last set on each window's border, by its handle: moves come often, the
/// border rarely changes.
static BORDERS: Mutex<BTreeMap<isize, u32>> = Mutex::new(BTreeMap::new());

/// # Safety
/// `hwnd` is one of the app's windows.
unsafe fn update_border_color(hwnd: HWND) {
    let Some(is_window_arranged) = is_window_arranged() else { return };
    let color = if is_window_arranged(hwnd) != 0 { SNAPPED_BORDER.load(Ordering::Relaxed) } else { DWMWA_COLOR_DEFAULT };
    if BORDERS.lock().unwrap().insert(hwnd as isize, color) != Some(color) {
        // Fails on Windows 10, which draws no such border.
        DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR as u32, (&color as *const u32).cast(), size_of::<u32>() as u32);
    }
}

static APP: OnceLock<AppHandle> = OnceLock::new();

use super::MaximizeButtonState as ButtonState;

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
        crate::events::emit_to(app, label, crate::events::MaximizeButton(state));
    }
}

/// Forgets what was kept about a window that is gone, whose handle may be reused.
fn forget_handle(hwnd: isize) {
    BUTTONS.with_borrow_mut(|buttons| buttons.remove(&hwnd));
    BORDERS.lock().unwrap().remove(&hwnd);
}

/// # Safety
/// On the main thread, with `parent` the window labelled `label`.
unsafe fn place_maximize_button(parent: HWND, label: String, rect: Option<Rect>, scale: f64) {
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
                forget_handle(parent);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// # Safety
/// On the main thread, with `hwnd` one of the app's windows.
unsafe fn show_system_menu(hwnd: HWND) {
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
    /// The windows open that need the pointer, and how many times it was shown for them.
    static POINTER_WINDOWS: Cell<(u32, u32)> = const { Cell::new((0, 0)) };
}

/// A window that needs the pointer (a file dialog, the color picker's) is opening (`open`) or
/// has closed: the pointer shows while one is open. On the main thread.
///
/// WebView2 follows the "Hide pointer while typing" mouse setting by lowering the cursor
/// display count with `ShowCursor(FALSE)`, and raises it again only once the pointer moves
/// over the web view. The count belongs to the input queue, which the dialogs share, so a
/// dialog opened right after typing (`rz` in the terminal) had no pointer. Raising the count
/// to 0 shows it; lowering it as much once the last window has closed leaves it where
/// WebView2 expects it. Chromium stopped calling `ShowCursor` for this in October 2026;
/// TODO.md says when this can go.
pub fn need_pointer(open: bool) {
    let (mut windows, mut shown) = POINTER_WINDOWS.get();
    if open {
        windows += 1;
        // Each call raises the count by one and returns it.
        loop {
            shown += 1;
            // SAFETY: takes no pointers.
            if unsafe { ShowCursor(1) } >= 0 {
                break;
            }
        }
    } else if windows > 0 {
        windows -= 1;
        if windows == 0 {
            for _ in 0..shown {
                // SAFETY: takes no pointers.
                unsafe { ShowCursor(0) };
            }
            shown = 0;
        }
    }
    POINTER_WINDOWS.set((windows, shown));
}

/// One of the app's windows coming to the foreground: the window, the one in the foreground
/// before it, and when (`GetTickCount`).
#[derive(Clone, Copy)]
struct Activation {
    window: isize,
    previous: isize,
    at: u32,
}

// Only touched on the main thread, which installs the hook and gets its events.
thread_local! {
    /// The foreground window, as last reported.
    static FOREGROUND: Cell<isize> = const { Cell::new(0) };
    /// The last time one of the app's windows came to the foreground.
    static ACTIVATION: Cell<Option<Activation>> = const { Cell::new(None) };
}

/// How long before the page hears a press the window may have come to the foreground for the
/// press to count as what brought it there: the system activates the window, then hands the
/// press to the web view.
const PRESS_ACTIVATION_SLACK: u32 = 500;

/// Follows the foreground window, for [`keep_behind`]. On the main thread.
fn watch_foreground() {
    // SAFETY: no module handle (out of context), and a callback with the hook's signature,
    // called on this thread, which pumps messages.
    unsafe {
        FOREGROUND.set(GetForegroundWindow() as isize);
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            std::ptr::null_mut(),
            Some(foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
    }
}

unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    time: u32,
) {
    let previous = FOREGROUND.replace(hwnd as isize);
    let mut process = 0;
    GetWindowThreadProcessId(hwnd, &mut process);
    if process == GetCurrentProcessId() && previous != hwnd as isize {
        ACTIVATION.set(Some(Activation { window: hwnd as isize, previous, at: time }));
    }
}

/// Called as a drag out of `hwnd` starts, `since_press` milliseconds after the mouse button
/// was pressed. A press on a window in the background brings it to the foreground before
/// the page hears of it (WebView2 activates its window on any press, so it can't wait for the
/// press to become a drag, as Explorer does), and a maximized window then covers the window
/// the files were probably meant for: the window goes back behind the one that was in the
/// foreground, keeping the keyboard focus. On the main thread.
///
/// # Safety
/// `hwnd` is one of the app's windows.
pub unsafe fn keep_behind(hwnd: HWND, since_press: u32) {
    let Some(activation) = ACTIVATION.get() else { return };
    let pressed_at = GetTickCount().wrapping_sub(since_press);
    if activation.window != hwnd as isize || pressed_at.wrapping_sub(activation.at) as i32 > PRESS_ACTIVATION_SLACK as i32 {
        return;
    }
    let previous = activation.previous as HWND;
    if previous.is_null()
        || IsWindow(previous) == 0
        || IsWindowVisible(previous) == 0
        || IsIconic(previous) != 0
        // Behind the taskbar or another topmost window, the window would become topmost.
        || GetWindowLongPtrW(previous, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0
        // Behind the desktop, it would be out of sight.
        || matches!(class_name(previous).as_str(), "Progman" | "WorkerW")
    {
        return;
    }
    SetWindowPos(hwnd, previous, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
}

fn class_name(hwnd: HWND) -> String {
    let mut name = [0u16; 64];
    // SAFETY: the buffer and its length in characters; a handle that is gone only fails.
    let length = unsafe { GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32) };
    String::from_utf16_lossy(&name[..length.max(0) as usize])
}

/// No decorations: the page draws the title bar and the window buttons.
pub(super) fn title_bar(builder: WebviewWindowBuilder<'_, Wry, AppHandle>) -> WebviewWindowBuilder<'_, Wry, AppHandle> {
    builder.decorations(false)
}

/// Nothing to do before the window is first shown.
pub(super) const PREPARED_HIDDEN: bool = false;

pub(super) fn prepare(_window: &WebviewWindow) -> tauri::Result<()> {
    Ok(())
}

pub(super) fn created(_window: &WebviewWindow) -> tauri::Result<()> {
    Ok(())
}

/// Before the first window, on the main thread: follows the foreground window.
pub(super) fn starting() {
    watch_foreground();
}

/// What was kept about `window`, now destroyed, goes.
pub(super) fn forget(window: &tauri::Window) {
    if let Ok(hwnd) = window.hwnd() {
        forget_handle(hwnd.0 as isize);
    }
}

/// On the main thread.
pub(super) fn bell() {
    // SAFETY: takes no pointers; `MB_OK` is the default sound.
    unsafe { windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(windows_sys::Win32::UI::WindowsAndMessaging::MB_OK) };
}

/// A double click on the title bar always maximizes or restores.
pub(super) fn double_click_action() -> DoubleClick {
    DoubleClick::Zoom
}

/// See `window::system_menu`.
pub(super) fn system_menu(window: WebviewWindow) -> tauri::Result<()> {
    let hwnd = window.hwnd()?.0 as isize;
    // SAFETY: on the main thread, with the window's own handle.
    window.run_on_main_thread(move || unsafe { show_system_menu(hwnd as _) })
}

/// See [`need_pointer`].
pub(super) fn file_dialog(window: WebviewWindow, open: bool) -> tauri::Result<()> {
    window.run_on_main_thread(move || need_pointer(open))
}

/// Windows has no drag area: a press always brings the window forward, and a drag out of it
/// puts it back ([`keep_behind`]).
pub(super) fn set_drag_area(_window: WebviewWindow, _rect: Option<Rect>) -> tauri::Result<()> {
    Ok(())
}

pub(super) fn buttons() -> Vec<WindowButton> {
    vec![WindowButton::Minimize, WindowButton::Maximize, WindowButton::Close]
}

pub(super) fn set_theme(window: &WebviewWindow, theme: Option<tauri::Theme>) -> tauri::Result<()> {
    window.set_theme(theme)
}

/// See `window::set_snapped_border_color`.
pub(super) fn set_snapped_border_color(window: WebviewWindow, color: String) -> tauri::Result<()> {
    if let Some(color) = colorref(&color) {
        SNAPPED_BORDER.store(color, Ordering::Relaxed);
        let hwnd = window.hwnd()?.0 as isize;
        // SAFETY: the window's own handle.
        unsafe { update_border_color(hwnd as _) };
    }
    Ok(())
}

/// See `window::set_maximize_button`.
pub(super) fn set_maximize_button(window: WebviewWindow, rect: Option<Rect>) -> tauri::Result<()> {
    APP.get_or_init(|| window.app_handle().clone());
    let parent = window.hwnd()?.0 as isize;
    let scale = window.scale_factor()?;
    let label = window.label().to_owned();
    // SAFETY: on the main thread, with the handle of the window labelled `label`.
    window.run_on_main_thread(move || unsafe { place_maximize_button(parent as _, label, rect, scale) })
}

/// See `window::update_border`.
pub(super) fn update_border(window: &tauri::Window) {
    if let Ok(hwnd) = window.hwnd() {
        // SAFETY: the window's own handle.
        unsafe { update_border_color(hwnd.0) };
    }
}
