//! Linux: AT-SPI's `ScreenReaderEnabled` (`org.a11y.Status` of the session bus's
//! `org.a11y.Bus`), which follows GNOME's screen reader setting and which Orca sets, on any
//! desktop. Without it (no AT-SPI bus), no screen reader is running.

use zbus::blocking::{Connection, Proxy};

use super::Changed;

/// Watches on a thread of its own: the bus may take a moment to start.
pub fn watch(changed: Changed) {
    std::thread::spawn(move || {
        if let Err(e) = watch_status(&changed) {
            log::warn!("screen reader: can't watch AT-SPI: {e}");
        }
    });
}

fn watch_status(changed: &Changed) -> zbus::Result<()> {
    let bus = Connection::session()?;
    let status = Proxy::new(&bus, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Status")?;
    let changes = status.receive_property_changed::<bool>("ScreenReaderEnabled");
    changed(status.get_property("ScreenReaderEnabled")?);
    for change in changes {
        if let Ok(running) = change.get() {
            changed(running);
        }
    }
    Ok(())
}
