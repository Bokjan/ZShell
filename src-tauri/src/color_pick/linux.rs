//! Linux: the screenshot portal's `PickColor` (`org.freedesktop.portal.Screenshot`), which the
//! desktop's portal backend implements with its own picker (GNOME's and KDE's, on X11 and
//! Wayland alike). Like every portal request, the call returns at once with a request
//! object, whose `Response` signal brings the color once the user has picked or cancelled;
//! the request's path is known ahead from the `handle_token` option (since portal 0.9), so
//! the signal is listened for before the call and can't be missed.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use tauri::WebviewWindow;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, Value};

use super::{Done, Rgb};
use crate::error::{Error, Result};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
/// The D-Bus errors that say there is no portal, or no screenshot portal in it.
const UNAVAILABLE: [&str; 4] = [
    "org.freedesktop.DBus.Error.ServiceUnknown",
    "org.freedesktop.DBus.Error.UnknownInterface",
    "org.freedesktop.DBus.Error.UnknownMethod",
    "org.freedesktop.DBus.Error.UnknownObject",
];

/// Waits for the portal on a thread of its own: the D-Bus calls block, for as long as the
/// user takes.
pub(super) fn start(_window: &WebviewWindow, done: Done) -> Result<()> {
    std::thread::spawn(move || {
        let _ = done.send(pick());
    });
    Ok(())
}

fn failed(error: impl std::fmt::Display) -> Error {
    Error::new("colorPick.failed").detail(error)
}

fn pick() -> Result<Option<Rgb>> {
    let bus = Connection::session().map_err(|e| Error::new("colorPick.unavailable").detail(e))?;
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let token = format!("zshell_color_{}", NEXT.fetch_add(1, Ordering::Relaxed));
    // The connection's unique name, `:1.42`, as `1_42`.
    let sender = bus.unique_name().ok_or_else(|| failed("no unique name"))?.trim_start_matches(':').replace('.', "_");
    let path = format!("{PORTAL_PATH}/request/{sender}/{token}");
    let request = Proxy::new(&bus, PORTAL, path.as_str(), "org.freedesktop.portal.Request").map_err(failed)?;
    let mut responses = request.receive_signal("Response").map_err(failed)?;

    let screenshot = Proxy::new(&bus, PORTAL, PORTAL_PATH, "org.freedesktop.portal.Screenshot").map_err(failed)?;
    let options = HashMap::from([("handle_token", Value::from(token.as_str()))]);
    // No parent window: the picker isn't a dialog.
    let handle: OwnedObjectPath = screenshot.call("PickColor", &("", options)).map_err(|e| match &e {
        zbus::Error::MethodError(name, _, _) if UNAVAILABLE.contains(&name.as_str()) => Error::new("colorPick.unavailable").detail(e),
        _ => failed(e),
    })?;

    let response = if handle.as_str() == path {
        responses.next()
    } else {
        // Before xdg-desktop-portal 0.9 the request has a path of its own, whatever the token:
        // listened for only now, which a response would only beat if the user picked at once.
        drop(responses);
        let request = Proxy::new(&bus, PORTAL, handle.as_str(), "org.freedesktop.portal.Request").map_err(failed)?;
        request.receive_signal("Response").map_err(failed)?.next()
    };
    let response = response.ok_or_else(|| failed("no response"))?;
    let body = response.body();
    // 0: picked, 1: cancelled, 2: failed some other way.
    let (code, results): (u32, HashMap<String, Value>) = body.deserialize().map_err(failed)?;
    match code {
        0 => {}
        1 => return Ok(None),
        _ => return Err(Error::new("colorPick.failed")),
    }
    let mut color = results.get("color").ok_or_else(|| failed("no color"))?;
    while let Value::Value(inner) = color {
        color = &**inner;
    }
    // `(ddd)`: red, green and blue, between 0 and 1.
    let Value::Structure(color) = color else { return Err(failed("not a color")) };
    match color.fields() {
        [Value::F64(red), Value::F64(green), Value::F64(blue)] => Ok(Some(Rgb::from_unit(*red, *green, *blue))),
        _ => Err(failed("not a color")),
    }
}
