//! The macOS menu bar: the app menu the page declares (`appMenu` in keymap.ts, with the labels
//! and keys of its actions), among the system's own items, built here since only native menu
//! items get keys like ⌘W and ⌘Q before the web view does. The page sends it when it loads;
//! until then the menu bar has only the system's items. Its items act on the window that last
//! had the focus: the page runs the action (`menu-action`), except for Close Window and Quit,
//! handled here so that they work whatever the page shows; the debug log window takes only a few
//! (see [`chosen`]). An item that tells that window's state (terminal full screen) gets its
//! label from it; the window's own full screen is labeled here (`show_full_screen`). Elsewhere
//! the page draws the menu.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Deserialize;
use tauri::{AppHandle, WebviewWindow};
use ts_rs::TS;

use crate::events;
use crate::window;

/// The system's Quit (⌘Q): it asks the page first, which asks when tabs of any window are
/// connected, and then quits.
const QUIT: &str = "quit";
/// Closed here; the page may ask first, and the last window closing quits.
const CLOSE_WINDOW: &str = "closeWindow";
/// The items that act on the debug log window while it has the focus (its page handles them,
/// and finds itself already open); the others act on an app window, which comes forward.
pub(crate) const DEBUG_LOG_ACTIONS: [&str; 4] = ["find", "closeTab", CLOSE_WINDOW, "debugLog"];
/// The View menu, whose last item is the window's own full screen.
#[cfg(target_os = "macos")]
const VIEW_SUBMENU_ID: &str = "view";

/// A menu of the menu bar, or a part of the app menu outside macOS.
#[derive(Clone, Copy, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "MenuSection")]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum SectionKind {
    App,
    File,
    Edit,
    View,
    Help,
}

/// The page's items for one menu of the menu bar.
#[derive(Clone, PartialEq, Deserialize, TS)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[ts(rename = "NativeMenuSection")]
pub struct Section {
    section: SectionKind,
    items: Vec<Item>,
}

#[derive(Clone, PartialEq, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(rename = "NativeMenuItem")]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum Item {
    Separator,
    /// An action of the page, with its label and macOS keys as keymap.ts writes them
    /// (`Cmd+Shift+E`).
    Action {
        id: String,
        label: String,
        keys: Option<String>,
    },
}

#[derive(Default)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct NativeMenu(Mutex<Built>);

#[derive(Default)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct Built {
    /// What the menu bar was last built from.
    sections: Option<Vec<Section>>,
    /// Labels the page changed since (`app_menu_set_label`), by item, kept when it is built again.
    labels: HashMap<String, String>,
}

/// The menu bar with the page's `sections` (none before it has loaded), the `labels` it
/// changed, and the window's own full screen item labeled for a window in `full_screen` or not.
#[cfg(target_os = "macos")]
pub fn build<R: tauri::Runtime>(
    app: &AppHandle<R>,
    sections: &[Section],
    labels: &HashMap<String, String>,
    full_screen: bool,
) -> tauri::Result<tauri::menu::Menu<R>> {
    use tauri::menu::{
        AboutMetadata, IsMenuItem, Menu, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu, HELP_SUBMENU_ID, WINDOW_SUBMENU_ID,
    };

    let separator = || PredefinedMenuItem::separator(app).map(MenuItemKind::Predefined);
    let page = |kind: SectionKind| -> tauri::Result<Vec<MenuItemKind<R>>> {
        let items = sections.iter().find(|section| section.section == kind).map_or(&[][..], |section| &section.items[..]);
        items
            .iter()
            .map(|item| match item {
                Item::Separator => separator(),
                Item::Action { id, label, keys } => {
                    let label = labels.get(id).unwrap_or(label);
                    MenuItem::with_id(app, id.as_str(), label, true, keys.as_deref()).map(MenuItemKind::MenuItem)
                }
            })
            .collect()
    };
    let submenu = |id: Option<&str>, title: String, items: Vec<MenuItemKind<R>>| {
        let items: Vec<&dyn IsMenuItem<R>> = items.iter().map(|item| item as &dyn IsMenuItem<R>).collect();
        match id {
            Some(id) => Submenu::with_id_and_items(app, id, title, true, &items),
            None => Submenu::with_items(app, title, true, &items),
        }
    };
    /// The page's items after the system's, with a separator between.
    fn after<R: tauri::Runtime>(
        mut items: Vec<MenuItemKind<R>>,
        separator: impl Fn() -> tauri::Result<MenuItemKind<R>>,
        page: Vec<MenuItemKind<R>>,
    ) -> tauri::Result<Vec<MenuItemKind<R>>> {
        if !page.is_empty() {
            items.push(separator()?);
            items.extend(page);
        }
        Ok(items)
    }

    let info = app.package_info();
    let about = AboutMetadata { name: Some(info.name.clone()), version: Some(info.version.to_string()), ..Default::default() };
    let predefined = |item: tauri::Result<PredefinedMenuItem<R>>| item.map(MenuItemKind::Predefined);

    let mut app_menu = after(
        vec![predefined(PredefinedMenuItem::about(app, Some(&t!("menu.about", name = info.name)), Some(about)))?],
        separator,
        page(SectionKind::App)?,
    )?;
    app_menu.extend([
        separator()?,
        predefined(PredefinedMenuItem::services(app, Some(&t!("menu.services"))))?,
        separator()?,
        predefined(PredefinedMenuItem::hide(app, Some(&t!("menu.hide", name = info.name))))?,
        predefined(PredefinedMenuItem::hide_others(app, Some(&t!("menu.hideOthers"))))?,
        separator()?,
        MenuItemKind::MenuItem(MenuItem::with_id(app, QUIT, t!("menu.quit", name = info.name), true, Some("Cmd+Q"))?),
    ]);
    let edit = after(
        vec![
            predefined(PredefinedMenuItem::undo(app, Some(&t!("menu.undo"))))?,
            predefined(PredefinedMenuItem::redo(app, Some(&t!("menu.redo"))))?,
            separator()?,
            predefined(PredefinedMenuItem::cut(app, Some(&t!("menu.cut"))))?,
            predefined(PredefinedMenuItem::copy(app, Some(&t!("menu.copy"))))?,
            predefined(PredefinedMenuItem::paste(app, Some(&t!("menu.paste"))))?,
            predefined(PredefinedMenuItem::select_all(app, Some(&t!("menu.selectAll"))))?,
        ],
        separator,
        page(SectionKind::Edit)?,
    )?;
    let window = vec![
        predefined(PredefinedMenuItem::minimize(app, Some(&t!("menu.minimize"))))?,
        predefined(PredefinedMenuItem::maximize(app, Some(&t!("menu.zoom"))))?,
    ];

    let mut menus = vec![submenu(None, info.name.clone(), app_menu)?];
    let file = page(SectionKind::File)?;
    if !file.is_empty() {
        menus.push(submenu(None, t!("menu.file"), file)?);
    }
    menus.push(submenu(None, t!("menu.edit"), edit)?);
    // The window's own full screen (⌃⌘F), besides the page's terminal full screen; without
    // it, AppKit adds one with another key. AppKit doesn't name it: without a label it shows
    // muda's "Toggle Full Screen".
    let mut view = page(SectionKind::View)?;
    if !view.is_empty() {
        view.push(separator()?);
    }
    view.push(predefined(PredefinedMenuItem::fullscreen(app, Some(&full_screen_label(full_screen))))?);
    menus.push(submenu(Some(VIEW_SUBMENU_ID), t!("menu.view"), view)?);
    menus.push(submenu(Some(WINDOW_SUBMENU_ID), t!("menu.window"), window)?);
    // Its own items, after which macOS adds a search field.
    menus.push(submenu(Some(HELP_SUBMENU_ID), t!("menu.help"), page(SectionKind::Help)?)?);
    let menus: Vec<&dyn IsMenuItem<R>> = menus.iter().map(|menu| menu as &dyn IsMenuItem<R>).collect();
    Menu::with_items(app, &menus)
}

/// Puts the app menu the page declares into the menu bar; every window's page sends the same
/// one. Only macOS has a menu bar.
pub fn set(app: AppHandle, menu: &NativeMenu, sections: Vec<Section>) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let mut built = menu.0.lock().unwrap();
        if built.sections.as_ref() != Some(&sections) {
            let full_screen = window::current(&app).is_some_and(|window| window.is_fullscreen().unwrap_or(false));
            app.set_menu(build(&app, &sections, &built.labels, full_screen)?)?;
            // After Tauri puts the menu in place, which it also does on the main thread.
            app.run_on_main_thread(keep_keys)?;
            built.sections = Some(sections);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, menu, sections);
    Ok(())
}

/// Keeps AppKit from moving the menu bar's keys to other keys on some layouts (the automatic
/// localization of key equivalents, from macOS 12): the page leaves the menu's keys to it by
/// the characters they type (`toAppMenu` in keymap.ts), so the menu must take them the same way.
#[cfg(target_os = "macos")]
fn keep_keys() {
    use objc2::{sel, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSMenu};
    use objc2_foundation::NSObjectProtocol;

    fn keep(menu: &NSMenu) {
        for item in menu.itemArray() {
            item.setAllowsAutomaticKeyEquivalentLocalization(false);
            if let Some(submenu) = item.submenu() {
                keep(&submenu);
            }
        }
    }

    let Some(main) = MainThreadMarker::new() else { return };
    let Some(menu) = NSApplication::sharedApplication(main).mainMenu() else { return };
    let Some(item) = menu.itemArray().firstObject() else { return };
    if item.respondsToSelector(sel!(setAllowsAutomaticKeyEquivalentLocalization:)) {
        keep(&menu);
    }
}

/// Changes the label of one of the page's items: one that tells the state of the window that
/// last had the focus, which sends it as that state changes and when it gets the focus.
pub fn set_label(app: AppHandle, window: WebviewWindow, menu: &NativeMenu, id: String, label: String) {
    #[cfg(target_os = "macos")]
    {
        use tauri::menu::MenuItemKind;

        if window::current(&app).is_none_or(|current| current.label() != window.label()) {
            return;
        }
        let items = app.menu().and_then(|bar| bar.items().ok()).unwrap_or_default();
        let item = items.into_iter().find_map(|item| match item {
            MenuItemKind::Submenu(submenu) => submenu.get(id.as_str()),
            _ => None,
        });
        if let Some(MenuItemKind::MenuItem(item)) = item {
            let _ = item.set_text(&label);
        }
        menu.0.lock().unwrap().labels.insert(id, label);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, window, menu, id, label);
}

/// The label of the window's own full screen item.
#[cfg(target_os = "macos")]
fn full_screen_label(full_screen: bool) -> String {
    if full_screen {
        t!("menu.exitFullScreen")
    } else {
        t!("menu.enterFullScreen")
    }
}

/// Labels the window's own full screen item (Enter / Exit Full Screen) for the window the menu
/// acts on (see [`window::current`]): called as a window enters or leaves full screen, and as
/// one gets the focus.
#[cfg(target_os = "macos")]
pub fn show_full_screen(app: &AppHandle) {
    use tauri::menu::MenuItemKind;

    let full_screen = window::current(app).is_some_and(|window| window.is_fullscreen().unwrap_or(false));
    let Some(MenuItemKind::Submenu(view)) = app.menu().and_then(|bar| bar.get(VIEW_SUBMENU_ID)) else { return };
    if let Some(MenuItemKind::Predefined(item)) = view.items().ok().and_then(|items| items.into_iter().last()) {
        let _ = item.set_text(full_screen_label(full_screen));
    }
}

/// An item of the menu bar was chosen. While the debug log window has the focus, those of
/// [`DEBUG_LOG_ACTIONS`] act on it; the others bring the app window forward and act on it, so
/// that what they show (a dialog, a new tab, the lock screen) is seen.
pub fn chosen(app: &AppHandle, id: &str) {
    let debug_log = window::focused_debug_log(app);
    if let Some(debug_log) = debug_log.as_ref().filter(|_| DEBUG_LOG_ACTIONS.contains(&id)) {
        match id {
            CLOSE_WINDOW => drop(debug_log.close()),
            _ => events::emit_to(app, debug_log.label(), events::MenuAction(id.to_owned())),
        }
        return;
    }
    let Some(current) = window::current(app) else {
        if id == QUIT {
            app.exit(0);
        }
        return;
    };
    if debug_log.is_some() {
        let _ = current.set_focus();
    }
    match id {
        QUIT => {
            events::emit_to(app, current.label(), events::QuitRequested);
        }
        CLOSE_WINDOW => {
            let _ = current.close();
        }
        _ => {
            events::emit_to(app, current.label(), events::MenuAction(id.to_owned()));
        }
    }
}
