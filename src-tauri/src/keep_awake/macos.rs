//! macOS: a power assertion against idle sleep (what `caffeinate -i` takes), and the power
//! source from IOKit, whose changes come on the main run loop.

use std::ffi::c_void;

use objc2_core_foundation::{kCFRunLoopDefaultMode, CFRetained, CFRunLoop, CFString};
use objc2_io_kit::{
    kIOPMACPowerKey, kIOPMAssertionLevelOn, kIOReturnSuccess, IOPMAssertionCreateWithName, IOPMAssertionID, IOPMAssertionRelease,
    IOPSCopyPowerSourcesInfo, IOPSGetProvidingPowerSourceType, IOPSNotificationCreateRunLoopSource,
};

use super::{Hold, PowerChanged};

/// `kIOPMAssertPreventUserIdleSystemSleep`, a macro in the headers.
const PREVENT_IDLE_SLEEP: &str = "PreventUserIdleSystemSleep";

struct Assertion(IOPMAssertionID);

impl Drop for Assertion {
    fn drop(&mut self) {
        IOPMAssertionRelease(self.0);
    }
}

pub fn hold(reason: &str) -> Option<Hold> {
    let mut id: IOPMAssertionID = 0;
    // SAFETY: two strings and a place for the id; the assertion lasts until it is released.
    let result = unsafe {
        IOPMAssertionCreateWithName(
            Some(&CFString::from_str(PREVENT_IDLE_SLEEP)),
            kIOPMAssertionLevelOn,
            Some(&CFString::from_str(reason)),
            &mut id,
        )
    };
    if result != kIOReturnSuccess {
        eprintln!("can't keep the system awake: IOKit error {result:#x}");
        return None;
    }
    Some(Box::new(Assertion(id)))
}

/// "AC Power", rather than "Battery Power" or "UPS Power". A Mac without a battery says AC.
fn external_power() -> bool {
    let info = IOPSCopyPowerSourcesInfo();
    // SAFETY: the snapshot that function returns, or none.
    let source = unsafe { IOPSGetProvidingPowerSourceType(info.as_deref()) };
    source.is_none_or(|source| source.to_string().as_bytes() == kIOPMACPowerKey.to_bytes())
}

pub fn watch_power(changed: PowerChanged) {
    changed(external_power());
    unsafe extern "C-unwind" fn notified(context: *mut c_void) {
        // SAFETY: the callback leaked below, for as long as the app runs.
        let changed = unsafe { &*(context as *const PowerChanged) };
        changed(external_power());
    }
    let context = Box::into_raw(Box::new(changed)) as *mut c_void;
    // SAFETY: the context is the callback's, never freed; the source runs on the main run
    // loop, which the app runs for as long as it lives.
    let source: Option<CFRetained<_>> = unsafe { IOPSNotificationCreateRunLoopSource(Some(notified), context) };
    match (source, CFRunLoop::main()) {
        // SAFETY: a source and a mode for the main run loop, which keeps the source.
        (Some(source), Some(main)) => unsafe { main.add_source(Some(&source), kCFRunLoopDefaultMode) },
        _ => eprintln!("can't watch the power source"),
    }
}

pub fn limited_on_battery() -> bool {
    false
}
