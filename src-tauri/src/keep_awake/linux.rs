//! Linux: the desktop portal's inhibitor (`org.freedesktop.portal.Inhibit`, for suspending),
//! which GNOME passes on to its session manager, whose inhibitors its automatic suspend
//! respects; and UPower's `OnBattery` for the power source. All through D-Bus.
//!
//! Not logind's own `sleep` lock: GNOME doesn't start counting its idle time again once that
//! one is released, until the next input (gnome-settings-daemon#591).

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, Value};

use super::{Hold, PowerChanged};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
/// The portal's flag for keeping the session from suspending (rather than logging out,
/// switching user, or going idle, which would keep the screen from locking).
const SUSPEND: u32 = 4;

enum Request {
    Hold(String),
    Release,
}

/// The thread that talks to the portal, which takes the requests in order: holding and
/// releasing return at once, without waiting for D-Bus on the thread that asks.
fn worker() -> &'static Sender<Request> {
    static WORKER: OnceLock<Sender<Request>> = OnceLock::new();
    WORKER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || work(receiver));
        sender
    })
}

struct Inhibition;

impl Drop for Inhibition {
    fn drop(&mut self) {
        let _ = worker().send(Request::Release);
    }
}

pub fn hold(reason: &str) -> Option<Hold> {
    worker().send(Request::Hold(reason.to_owned())).ok()?;
    Some(Box::new(Inhibition))
}

fn work(requests: Receiver<Request>) {
    // Kept open: the portal ends the inhibitions of a connection that closes.
    let bus = match Connection::session() {
        Ok(bus) => bus,
        Err(e) => {
            eprintln!("can't keep the system awake: {e}");
            return;
        }
    };
    // The request object of the inhibition held, which closing ends.
    let mut held: Option<OwnedObjectPath> = None;
    for request in requests {
        match request {
            Request::Hold(reason) => match inhibit(&bus, &reason) {
                Ok(handle) => held = Some(handle),
                Err(e) => eprintln!("can't keep the system awake: {e}"),
            },
            Request::Release => {
                if let Some(handle) = held.take() {
                    let _ = bus.call_method(Some(PORTAL), &handle, Some("org.freedesktop.portal.Request"), "Close", &());
                }
            }
        }
    }
}

fn inhibit(bus: &Connection, reason: &str) -> zbus::Result<OwnedObjectPath> {
    let options = HashMap::from([("reason", Value::from(reason))]);
    // No parent window: the inhibition is the app's, not a window's.
    let reply = bus.call_method(
        Some(PORTAL),
        "/org/freedesktop/portal/desktop",
        Some("org.freedesktop.portal.Inhibit"),
        "Inhibit",
        &("", SUSPEND, options),
    )?;
    reply.body().deserialize()
}

/// On a thread of its own, for as long as the app runs. Without UPower, the computer counts as
/// plugged in.
pub fn watch_power(changed: PowerChanged) {
    std::thread::spawn(move || {
        if let Err(e) = watch(&changed) {
            eprintln!("can't watch the power source: {e}");
        }
    });
}

fn watch(changed: &PowerChanged) -> zbus::Result<()> {
    let bus = Connection::system()?;
    let upower = Proxy::new(&bus, "org.freedesktop.UPower", "/org/freedesktop/UPower", "org.freedesktop.UPower")?;
    let changes = upower.receive_property_changed::<bool>("OnBattery");
    // True on a battery or a UPS.
    changed(!upower.get_property::<bool>("OnBattery")?);
    for change in changes {
        if let Ok(on_battery) = change.get() {
            changed(!on_battery);
        }
    }
    Ok(())
}

pub fn limited_on_battery() -> bool {
    false
}
