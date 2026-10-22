//! Linux: the session manager's inhibitor for suspending, which the desktop's automatic suspend
//! respects: GNOME's (`org.gnome.SessionManager`, which Cinnamon, MATE and Budgie provide too),
//! or else the one KDE and Xfce provide (`org.freedesktop.PowerManagement.Inhibit`). And
//! UPower's `OnBattery` for the power source. All through D-Bus.
//!
//! Not the desktop portal's Inhibit, which passes the request on to the same services but tells
//! of a failure only later, in a signal: these answer the call, so that a desktop with neither
//! is known to refuse (wlroots compositors with swayidle, say, which have no such service).
//! Not logind's own `sleep` lock either: GNOME doesn't start counting its idle time again once
//! that one is released, until the next input (gnome-settings-daemon#591).

use std::sync::Arc;

use zbus::blocking::{Connection, Proxy};
use zbus::proxy::CacheProperties;

use super::{Hold, PowerChanged};

/// The name the session manager shows with the inhibitor.
const APP: &str = "ZShell";

const GNOME: &str = "org.gnome.SessionManager";
const GNOME_PATH: &str = "/org/gnome/SessionManager";
/// GNOME's flag for keeping the session from suspending (rather than logging out, switching
/// user, or going idle, which would keep the screen from locking).
const GNOME_SUSPEND: u32 = 4;

const FREEDESKTOP: &str = "org.freedesktop.PowerManagement";
const FREEDESKTOP_PATH: &str = "/org/freedesktop/PowerManagement/Inhibit";
const FREEDESKTOP_INTERFACE: &str = "org.freedesktop.PowerManagement.Inhibit";

const UPOWER: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";

/// An inhibitor, by the cookie its service gave. Both services also end the inhibitors of a
/// connection that closes, which the bus kept by `crate::dbus` never does while the app runs.
enum Inhibition {
    Gnome(u32),
    Freedesktop(u32),
}

struct Inhibitor {
    bus: &'static Connection,
    inhibition: Inhibition,
}

impl Drop for Inhibitor {
    fn drop(&mut self) {
        let result = match self.inhibition {
            Inhibition::Gnome(cookie) => self.bus.call_method(Some(GNOME), GNOME_PATH, Some(GNOME), "Uninhibit", &(cookie,)),
            Inhibition::Freedesktop(cookie) => {
                self.bus.call_method(Some(FREEDESKTOP), FREEDESKTOP_PATH, Some(FREEDESKTOP_INTERFACE), "UnInhibit", &(cookie,))
            }
        };
        if let Err(e) = result {
            eprintln!("can't let the system sleep again: {e}");
        }
    }
}

/// Waits for the answer: called on the thread that holds the system (see `keep_awake::work`).
pub fn hold(reason: &str) -> Option<Hold> {
    let bus = match crate::dbus::session() {
        Ok(bus) => bus,
        Err(e) => {
            eprintln!("can't keep the system awake: {e}");
            return None;
        }
    };
    let gnome = || -> zbus::Result<Inhibition> {
        let reply = bus.call_method(Some(GNOME), GNOME_PATH, Some(GNOME), "Inhibit", &(APP, 0u32, reason, GNOME_SUSPEND))?;
        Ok(Inhibition::Gnome(reply.body().deserialize()?))
    };
    let freedesktop = || -> zbus::Result<Inhibition> {
        let reply = bus.call_method(Some(FREEDESKTOP), FREEDESKTOP_PATH, Some(FREEDESKTOP_INTERFACE), "Inhibit", &(APP, reason))?;
        Ok(Inhibition::Freedesktop(reply.body().deserialize()?))
    };
    match gnome().or_else(|_| freedesktop()) {
        Ok(inhibition) => Some(Box::new(Inhibitor { bus, inhibition })),
        Err(e) => {
            eprintln!("can't keep the system awake: {e}");
            None
        }
    }
}

/// On threads of their own, for as long as the app runs. Without UPower, the computer counts as
/// plugged in.
pub fn watch_power(changed: PowerChanged) {
    std::thread::spawn(move || {
        if let Err(e) = watch(changed) {
            eprintln!("can't watch the power source: {e}");
        }
    });
}

/// Reads `OnBattery` again whenever UPower says its properties changed, and whenever UPower
/// starts (again, after an upgrade say), which doesn't announce them. Uncached: zbus reads the
/// properties once, and not again for a new owner.
fn watch(changed: PowerChanged) -> zbus::Result<()> {
    let bus = crate::dbus::system().map_err(|e| zbus::Error::Failure(e.to_owned()))?;
    let upower: Proxy<'static> = zbus::blocking::proxy::Builder::new(bus)
        .destination(UPOWER)?
        .path(UPOWER_PATH)?
        .interface(UPOWER)?
        .cache_properties(CacheProperties::No)
        .build()?;
    let properties = Proxy::new(bus, UPOWER, UPOWER_PATH, "org.freedesktop.DBus.Properties")?;
    let changes = properties.receive_signal("PropertiesChanged")?;
    let owners = upower.receive_owner_changed()?;
    let changed = Arc::new(changed);
    let read = move || {
        // True on a battery or a UPS. UPower gone: what it said last holds.
        if let Ok(on_battery) = upower.get_property::<bool>("OnBattery") {
            changed(!on_battery);
        }
    };
    read();
    let on_change = read.clone();
    std::thread::spawn(move || {
        for owner in owners {
            if owner.is_some() {
                on_change();
            }
        }
    });
    for _ in changes {
        read();
    }
    Ok(())
}

pub fn limited_on_battery() -> bool {
    false
}
