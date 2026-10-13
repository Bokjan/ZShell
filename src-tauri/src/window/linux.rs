//! Linux (GTK): the window keeps GTK's client-side decorations, so it has the desktop's own
//! shadow, rounded corners and resizing from outside its edges, but its title bar is an empty
//! widget, and the frontend's tab bar takes its place. The frontend draws the window buttons
//! the desktop's layout asks for ([`buttons`]); a double click and a right click on the tab
//! bar do what the desktop's settings say, as on a native title bar.
//!
//! The appearance is GTK's `gtk-application-prefer-dark-theme`, which WebKitGTK's
//! `prefers-color-scheme` follows, and which is the whole app's. tao takes "follow the system"
//! (`None`) as light, so the desktop's color scheme is read from the settings portal
//! ([`system_theme`]), as GTK 4 and libadwaita apps do. tao also sets it to the desktop's
//! scheme on its own, whenever it creates a window and whenever the scheme changes, however the
//! app's is set: a theme chosen in the settings is put back each time ([`watch_theme`]).

use std::cell::RefCell;
use std::sync::Mutex;

use gtk::glib::Propagation;
use gtk::prelude::*;
use tauri::{AppHandle, Emitter, Manager, Theme, WebviewWindow};

use super::{DoubleClick, WindowButton};

thread_local! {
    /// The last button press on a web view (on the main thread, where GTK runs). Wayland only
    /// shows the window menu for an input event, which a web page can't hand over.
    static LAST_PRESS: RefCell<Option<gtk::gdk::Event>> = const { RefCell::new(None) };
}

/// Before the window is first shown, on the main thread: gives it an empty title bar (which
/// GTK only takes before the window is realized), and keeps its web view's button presses
/// for the window menu.
pub fn prepare(window: &WebviewWindow) -> tauri::Result<()> {
    window.gtk_window()?.set_titlebar(Some(&gtk::EventBox::new()));
    window.with_webview(|webview| {
        webview.inner().connect_button_press_event(|_, event| {
            LAST_PRESS.with(|press| *press.borrow_mut() = Some((**event).clone()));
            Propagation::Proceed
        });
    })
}

/// The window's web view: the first child of the box Tao puts in the window.
pub fn web_view(window: &gtk::ApplicationWindow) -> Option<gtk::Widget> {
    let content = window.child()?.downcast::<gtk::Container>().ok()?;
    content.children().into_iter().next()
}

/// Brings `window` forward, with `token` from the desktop if there is one: without it, GNOME
/// only says the window is ready. On the main thread.
pub fn present(window: &WebviewWindow, token: &str) {
    let Ok(gtk_window) = window.gtk_window() else { return };
    if !token.is_empty() {
        gtk_window.set_startup_id(token);
    }
    gtk_window.present();
}

/// The window menu (Minimize, Maximize, Move, Resize, Always on Top, Close…) at the pointer,
/// as a right click on a native title bar shows it. On the main thread.
pub fn show_window_menu(window: &WebviewWindow) {
    let Some(mut event) = LAST_PRESS.with(|press| press.borrow().clone()) else { return };
    if let Some(gdk_window) = window.gtk_window().ok().and_then(|window| window.window()) {
        gdk_window.show_window_menu(&mut event);
    }
}

/// What a double click on the title bar does, as the desktop says (GNOME's "Titlebar
/// Actions"); lowering the window and its menu aren't offered. On the main thread.
pub fn double_click_action() -> DoubleClick {
    let action = gtk::Settings::default().and_then(|settings| settings.property::<Option<String>>("gtk-titlebar-double-click"));
    match action.as_deref() {
        Some("minimize") => DoubleClick::Minimize,
        Some("none" | "lower" | "menu") => DoubleClick::Nothing,
        _ => DoubleClick::Zoom,
    }
}

/// The buttons the desktop puts on title bars, from its decoration layout
/// (`appmenu:minimize,maximize,close` on Ubuntu, `appmenu:close` on GNOME's default), in order
/// from left to right. They all go at the right end of the tab bar, even where the layout puts
/// them on the left (the left end has the sidebar's controls): those go in mirror order, so the
/// one at the window's edge stays at the edge. On the main thread.
pub fn buttons() -> Vec<WindowButton> {
    let layout = gtk::Settings::default().and_then(|settings| settings.gtk_decoration_layout()).map(|layout| layout.to_string());
    parse_layout(layout.as_deref().unwrap_or(":minimize,maximize,close"))
}

fn parse_layout(layout: &str) -> Vec<WindowButton> {
    // Without a colon, all on the left, as GTK reads it.
    let (left, right) = layout.split_once(':').unwrap_or((layout, ""));
    let mut buttons = Vec::new();
    for name in right.split(',').chain(left.split(',').rev()) {
        let button = match name.trim() {
            "minimize" => WindowButton::Minimize,
            "maximize" => WindowButton::Maximize,
            "close" => WindowButton::Close,
            _ => continue,
        };
        if !buttons.contains(&button) {
            buttons.push(button);
        }
    }
    buttons
}

/// Tells the windows when the desktop's button layout changes (`window-buttons-changed`). On
/// the main thread, once.
pub fn watch_buttons(app: &AppHandle) {
    let Some(settings) = gtk::Settings::default() else { return };
    let app = app.clone();
    settings.connect_gtk_decoration_layout_notify(move |_| {
        let _ = app.emit("window-buttons-changed", ());
    });
}

/// The desktop's color scheme (see `desktop`). Blocks for a moment at most.
pub fn system_theme() -> Theme {
    if crate::desktop::prefers_dark() {
        Theme::Dark
    } else {
        Theme::Light
    }
}

/// The theme chosen in the settings, or `None` to follow the desktop's.
static CHOSEN: Mutex<Option<Theme>> = Mutex::new(None);

/// Sets the app's appearance to `theme`, or the desktop's (`None`), through `window`.
pub fn set_theme(window: &WebviewWindow, theme: Option<Theme>) -> tauri::Result<()> {
    *CHOSEN.lock().unwrap() = theme;
    window.set_theme(Some(theme.unwrap_or_else(system_theme)))
}

/// Keeps GTK in the theme chosen in the settings, which it starts in, when tao sets it to the
/// desktop's (see the module docs). On the main thread, once, before the first window.
pub fn watch_theme(app: &AppHandle) {
    *CHOSEN.lock().unwrap() = match app.state::<crate::settings::SettingsStore>().get().appearance {
        crate::settings::Appearance::System => None,
        crate::settings::Appearance::Dark => Some(Theme::Dark),
        crate::settings::Appearance::Light => Some(Theme::Light),
    };
    let Some(settings) = gtk::Settings::default() else { return };
    settings.connect_gtk_application_prefer_dark_theme_notify(|settings| {
        // Unlocked before setting it, which notifies again.
        let chosen = *CHOSEN.lock().unwrap();
        if let Some(theme) = chosen {
            let dark = theme == Theme::Dark;
            if settings.is_gtk_application_prefer_dark_theme() != dark {
                settings.set_gtk_application_prefer_dark_theme(dark);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_decoration_layout() {
        use WindowButton::*;
        assert_eq!(parse_layout("appmenu:minimize,maximize,close"), [Minimize, Maximize, Close]);
        assert_eq!(parse_layout("appmenu:close"), [Close]);
        assert_eq!(parse_layout("close,minimize,maximize:"), [Maximize, Minimize, Close]);
        assert_eq!(parse_layout("close:minimize"), [Minimize, Close]);
        assert_eq!(parse_layout("minimize,maximize,close"), [Close, Maximize, Minimize]);
        assert_eq!(parse_layout("icon:spacer,close"), [Close]);
        assert!(parse_layout("menu:").is_empty());
    }
}
