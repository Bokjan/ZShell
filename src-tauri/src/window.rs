//! The main window, whose title bar the frontend draws: the tab bar takes its place (see
//! ARCHITECTURE.md). macOS keeps the traffic lights over the web view; on Windows the window
//! has no decorations and the frontend draws the window buttons, helped by the commands here
//! for what a web page can't do natively.

use tauri::{App, WebviewWindow, WebviewWindowBuilder};

const MAIN: &str = "main";

/// Where the traffic lights sit, centered in the frontend's 34px title bar.
#[cfg(target_os = "macos")]
const TRAFFIC_LIGHTS: tauri::LogicalPosition<f64> = tauri::LogicalPosition { x: 14.0, y: 19.0 };

/// Creates the main window from its configuration (which has `create: false`, so the
/// platform's title bar options can be set here rather than in per-platform config files).
pub fn create_main(app: &App) -> tauri::Result<()> {
    let config = app.config().app.windows.iter().find(|w| w.label == MAIN).cloned().unwrap_or_default();
    let builder = WebviewWindowBuilder::from_config(app.handle(), &config)?;
    #[cfg(target_os = "macos")]
    let builder =
        builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true).traffic_light_position(TRAFFIC_LIGHTS);
    #[cfg(windows)]
    let builder = builder.decorations(false);
    builder.build()?;
    Ok(())
}

/// A double click on the title bar: on macOS what the system setting says ("Double-click a
/// window's title bar to"), elsewhere maximize or restore.
#[tauri::command]
pub fn window_title_double_click(window: WebviewWindow) -> tauri::Result<()> {
    match double_click_action() {
        DoubleClick::Zoom if window.is_maximized()? => window.unmaximize(),
        DoubleClick::Zoom => window.maximize(),
        DoubleClick::Minimize => window.minimize(),
        DoubleClick::Nothing => Ok(()),
    }
}

// Elsewhere than macOS the double click always zooms.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
enum DoubleClick {
    Zoom,
    Minimize,
    Nothing,
}

#[cfg(target_os = "macos")]
fn double_click_action() -> DoubleClick {
    use objc2_foundation::{NSString, NSUserDefaults};

    let defaults = NSUserDefaults::standardUserDefaults();
    // "Maximize" (Zoom), "Fill" (macOS 15; zooming fills the screen too), "Minimize" or "None";
    // older systems only had the minimize switch.
    match defaults.stringForKey(&NSString::from_str("AppleActionOnDoubleClick")).map(|v| v.to_string()).as_deref() {
        Some("Minimize") => DoubleClick::Minimize,
        Some("None") => DoubleClick::Nothing,
        Some(_) => DoubleClick::Zoom,
        None if defaults.boolForKey(&NSString::from_str("AppleMiniaturizeOnDoubleClick")) => DoubleClick::Minimize,
        None => DoubleClick::Zoom,
    }
}

#[cfg(not(target_os = "macos"))]
fn double_click_action() -> DoubleClick {
    DoubleClick::Zoom
}

/// Shows the window's system menu (Restore, Move, Size, Minimize, Maximize, Close) at the
/// pointer, as a right click on a native title bar does. Windows only.
#[tauri::command]
pub fn window_system_menu(window: WebviewWindow) -> tauri::Result<()> {
    #[cfg(windows)]
    {
        let hwnd = window.hwnd()?.0 as isize;
        window.run_on_main_thread(move || unsafe { win::show_system_menu(hwnd as _) })?;
    }
    #[cfg(not(windows))]
    let _ = window;
    Ok(())
}

/// The frontend's maximize button, in CSS pixels from the top left of the window.
#[derive(Clone, Copy, serde::Deserialize)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Places the native window that makes the frontend's maximize button behave like a native
/// one (the Windows 11 snap layouts), over `rect`; `None` hides it. Windows only.
#[tauri::command]
pub fn window_set_maximize_button(window: WebviewWindow, rect: Option<Rect>) -> tauri::Result<()> {
    #[cfg(windows)]
    {
        use tauri::Manager;
        win::APP.get_or_init(|| window.app_handle().clone());
        let parent = window.hwnd()?.0 as isize;
        let scale = window.scale_factor()?;
        window.run_on_main_thread(move || unsafe { win::place_maximize_button(parent as _, rect, scale) })?;
    }
    #[cfg(not(windows))]
    let _ = (window, rect);
    Ok(())
}

/// After the window moved or was resized: Windows 11 draws a 1px border around a window
/// that isn't maximized, which stays when the window is snapped to part of the screen, where
/// it looks like a gap along the screen's edges; it is hidden there. Windows only.
pub fn update_border(window: &tauri::Window) {
    #[cfg(windows)]
    {
        if let Ok(hwnd) = window.hwnd() {
            unsafe { win::update_border(hwnd.0) };
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

#[cfg(windows)]
mod win {
    //! Windows 11 shows the snap layouts when the pointer rests on whatever answers
    //! `WM_NCHITTEST` with `HTMAXBUTTON`. The web view's own child window covers the whole
    //! client area and gets all mouse messages, and a web page can't give that answer, so a
    //! transparent child window above the web view, over the frontend's maximize button, does.
    //! (Tauri resizes undecorated windows from their top edge the same way.) It never paints,
    //! so the frontend's button shows through; it takes the mouse from the page, so it reports
    //! hover and press for the button's style, and maximizes or restores on click itself.
    //! `WS_EX_LAYERED` or `WS_EX_TRANSPARENT` would make the hit test pass through it.

    use std::cell::Cell;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::{Once, OnceLock};

    use tauri::{AppHandle, Emitter};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_DEFAULT, DWMWA_COLOR_NONE};
    use windows_sys::Win32::Graphics::Gdi::{GetStockObject, NULL_BRUSH};
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, EnableMenuItem, GetCursorPos, GetParent, GetSystemMenu, IsZoomed, LoadCursorW,
        PostMessageW, RegisterClassW, SetMenuDefaultItem, SetWindowPos, ShowWindow, TrackPopupMenu, HTMAXBUTTON, HWND_TOP,
        IDC_ARROW, MA_NOACTIVATE, MF_BYCOMMAND, MF_ENABLED, MF_GRAYED, SC_CLOSE, SC_MAXIMIZE, SC_MINIMIZE, SC_MOVE,
        SC_RESTORE, SC_SIZE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_MOUSEACTIVATE,
        WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE,
        WM_SYSCOMMAND, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS,
    };

    use super::{Rect, MAIN};

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

    /// # Safety
    /// `hwnd` is the main window.
    pub(super) unsafe fn update_border(hwnd: HWND) {
        // 0: not set yet, 1: default, 2: none. Moves come often; the border rarely changes.
        static BORDER: AtomicU8 = AtomicU8::new(0);
        let Some(is_window_arranged) = is_window_arranged() else { return };
        let snapped = is_window_arranged(hwnd) != 0;
        let (state, color) = if snapped { (2, DWMWA_COLOR_NONE) } else { (1, DWMWA_COLOR_DEFAULT) };
        if BORDER.swap(state, Ordering::Relaxed) != state {
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

    // Only touched on the main thread, which owns the windows and runs their procedures.
    thread_local! {
        static BUTTON: Cell<HWND> = const { Cell::new(std::ptr::null_mut()) };
        static STATE: Cell<ButtonState> = const { Cell::new(ButtonState { hover: false, pressed: false }) };
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    fn set_state(state: ButtonState) {
        if STATE.replace(state) != state {
            if let Some(app) = APP.get() {
                let _ = app.emit_to(MAIN, "window-maximize-button", state);
            }
        }
    }

    /// # Safety
    /// On the main thread, with `parent` the main window.
    pub(super) unsafe fn place_maximize_button(parent: HWND, rect: Option<Rect>, scale: f64) {
        let mut button = BUTTON.get();
        if button.is_null() {
            button = create_button(parent);
            if button.is_null() {
                return;
            }
            BUTTON.set(button);
        }
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
                set_state(ButtonState::default());
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
                if !STATE.get().hover {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE | TME_NONCLIENT,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut track);
                    set_state(ButtonState { hover: true, ..STATE.get() });
                }
                0
            }
            WM_NCMOUSELEAVE => {
                set_state(ButtonState::default());
                0
            }
            // Not to `DefWindowProcW`, which would run the caption button logic on this child.
            WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => {
                set_state(ButtonState { hover: true, pressed: true });
                0
            }
            WM_NCLBUTTONUP => {
                if STATE.get().pressed {
                    set_state(ButtonState { hover: true, pressed: false });
                    let parent = GetParent(hwnd);
                    let command = if IsZoomed(parent) != 0 { SC_RESTORE } else { SC_MAXIMIZE };
                    PostMessageW(parent, WM_SYSCOMMAND, command as WPARAM, 0);
                }
                0
            }
            // Keyboard focus stays in the web view.
            WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
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
        for (item, enabled) in
            [(SC_RESTORE, maximized), (SC_MOVE, !maximized), (SC_SIZE, !maximized), (SC_MINIMIZE, true), (SC_MAXIMIZE, !maximized), (SC_CLOSE, true)]
        {
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
}
