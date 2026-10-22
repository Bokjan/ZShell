//! Linux: polkit for authenticating (the desktop's own dialog, GNOME Shell's: the account's
//! password, or a fingerprint where fprintd is set up), Mutter's idle monitor for the idle
//! time (GNOME; other desktops have none here), and logind for the session locking
//! (`LockedHint`, and the `Lock` signal of `loginctl lock-session`) and the system sleeping.
//! All through D-Bus.
//!
//! polkit asks for an action of the app's own, `org.boyin.zshell.unlock`, which the deb and
//! rpm packages install (`linux/org.boyin.zshell.policy`); it allows the user after they
//! authenticate, every time (`auth_self`). Without it installed (a build run from the source
//! folder) the lock can't be used.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::WebviewWindow;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, Value};

use super::{Method, System};
use crate::error::{Error, Result};

const ACTION: &str = "org.boyin.zshell.unlock";
const POLKIT: &str = "org.freedesktop.PolicyKit1";
const POLKIT_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
const POLKIT_INTERFACE: &str = "org.freedesktop.PolicyKit1.Authority";
const LOGIN: &str = "org.freedesktop.login1";
/// `AllowUserInteraction`: polkit asks the user through the desktop's authentication agent.
const ALLOW_USER_INTERACTION: u32 = 1;

pub struct Platform;

fn failed(error: zbus::Error) -> Error {
    Error::new("appLock.failed").detail(error)
}

/// The system bus, kept for the idle checks and the queries.
fn system_bus() -> Result<&'static Connection> {
    crate::dbus::system().map_err(|e| Error::new("appLock.failed").detail(e))
}

/// The session bus, for Mutter; `None` without one.
fn session_bus() -> Option<&'static Connection> {
    crate::dbus::session().ok()
}

fn authority(bus: &Connection) -> Result<Proxy<'static>> {
    Proxy::new(bus, POLKIT, POLKIT_PATH, POLKIT_INTERFACE).map_err(failed)
}

impl System for Platform {
    /// polkit knows the app's action once the policy is installed.
    fn method(&self) -> Result<Method> {
        let authority = authority(system_bus()?)?;
        // (action id, description, message, vendor, vendor URL, icon, defaults any / inactive /
        // active, annotations)
        type Action = (String, String, String, String, String, String, u32, u32, u32, HashMap<String, String>);
        let actions: Vec<Action> = authority.call("EnumerateActions", &("",)).map_err(|e| match &e {
            zbus::Error::MethodError(name, _, _) if name.as_str() == "org.freedesktop.DBus.Error.ServiceUnknown" => {
                Error::new("appLock.polkitMissing")
            }
            _ => failed(e),
        })?;
        if actions.iter().any(|action| action.0 == ACTION) {
            Ok(Method::Polkit)
        } else {
            Err(Error::new("appLock.policyMissing"))
        }
    }

    /// The desktop's dialog is modal to the whole screen, so it isn't attached to the window.
    /// Waits as long as the dialog is open.
    fn authenticate(&self, _window: Option<&WebviewWindow>) -> Result<()> {
        // A connection of its own, so the call has no timeout without holding up the others.
        let bus = Connection::system().map_err(failed)?;
        let name = bus.unique_name().ok_or_else(|| Error::new("appLock.failed"))?.to_string();
        // This process, as polkit finds it from its bus name (rather than a pid it would have
        // to trust).
        let subject = ("system-bus-name", HashMap::from([("name", Value::from(name))]));
        let details: HashMap<&str, &str> = HashMap::new();
        let (authorized, _challenge, details): (bool, bool, HashMap<String, String>) =
            authority(&bus)?.call("CheckAuthorization", &(subject, ACTION, details, ALLOW_USER_INTERACTION, "")).map_err(failed)?;
        if authorized {
            Ok(())
        } else if details.get("polkit.dismissed").is_some_and(|v| v == "true") {
            Err(Error::new("appLock.cancelled"))
        } else {
            Err(Error::new("appLock.failed"))
        }
    }

    /// Mutter's idle monitor: GNOME only.
    fn idle_seconds(&self) -> Option<u64> {
        let monitor = Proxy::new(
            session_bus()?,
            "org.gnome.Mutter.IdleMonitor",
            "/org/gnome/Mutter/IdleMonitor/Core",
            "org.gnome.Mutter.IdleMonitor",
        )
        .ok()?;
        let milliseconds: u64 = monitor.call("GetIdletime", &()).ok()?;
        Some(milliseconds / 1000)
    }

    fn watches_system_lock(&self) -> bool {
        WATCHING.load(Ordering::Relaxed)
    }
}

/// Set once logind is watched; if that fails, the settings don't offer locking with the
/// system.
static WATCHING: AtomicBool = AtomicBool::new(false);

/// Watches logind on threads of their own: the system about to sleep (`PrepareForSleep`), and
/// the user's session locking (`LockedHint`, which GNOME Shell sets as its lock screen comes
/// up, also before switching user; and `Lock`, which asks the session to lock).
pub fn watch_system(on_lock: Box<dyn Fn() + Send + Sync>) {
    let on_lock: &'static (dyn Fn() + Send + Sync) = Box::leak(on_lock);
    std::thread::spawn(move || {
        if let Err(e) = watch(on_lock) {
            eprintln!("app lock: can't watch the session: {e}");
        }
    });
}

fn watch(on_lock: &'static (dyn Fn() + Send + Sync)) -> zbus::Result<()> {
    let bus = crate::dbus::system().map_err(|e| zbus::Error::Failure(e.to_owned()))?;
    let manager = Proxy::new(bus, LOGIN, "/org/freedesktop/login1", "org.freedesktop.login1.Manager")?;
    // `auto` is the caller's session, or the user's graphical one for an app outside it (as
    // apps a desktop starts in their own systemd scope are); signals come from its real path.
    let auto = Proxy::new(bus, LOGIN, "/org/freedesktop/login1/session/auto", "org.freedesktop.login1.Session")?;
    let id: String = auto.get_property("Id")?;
    let path: OwnedObjectPath = manager.call("GetSession", &(id,))?;
    let session = Proxy::new(bus, LOGIN, path, "org.freedesktop.login1.Session")?;

    let sleeping = manager.receive_signal("PrepareForSleep")?;
    let lock_signals = session.receive_signal("Lock")?;
    let locked_hints = session.receive_property_changed::<bool>("LockedHint");
    WATCHING.store(true, Ordering::Relaxed);
    std::thread::spawn(move || {
        for message in sleeping {
            // True as the system goes to sleep, false once it has woken up.
            if message.body().deserialize::<bool>().unwrap_or(false) {
                on_lock();
            }
        }
    });
    std::thread::spawn(move || {
        for _ in lock_signals {
            on_lock();
        }
    });
    for change in locked_hints {
        if change.get().unwrap_or(false) {
            on_lock();
        }
    }
    Ok(())
}
