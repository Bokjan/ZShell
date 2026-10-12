//! One running instance per configuration folder, on macOS (and Linux): two would overwrite
//! each other's files, which are written whole (the vault, the settings). macOS opens a
//! second instance of the same app bundle when it is another copy (a release and a debug
//! build), with `open -n`, or when the executable is run directly.
//!
//! Not the single-instance plugin, which Windows uses: on macOS its lock is a socket in /tmp
//! named after the bundle identifier, so an instance with another HOME (end-to-end tests, see
//! DEVELOPMENT.md) couldn't start. This lock is a file in the configuration folder, locked
//! with `flock`, which the system releases when the process ends however it ends; the file
//! holds the owner's pid, to bring it forward.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const LOCK_FILE: &str = "zshell.lock";
/// How long to wait for the owner's pid, which it writes right after taking the lock.
const PID_WAIT: std::time::Duration = std::time::Duration::from_millis(500);
/// How long a second instance waits to become active, to bring the first one forward.
#[cfg(target_os = "macos")]
const HAND_OVER_WAIT: std::time::Duration = std::time::Duration::from_millis(1500);

/// Held for as long as the app runs.
pub struct InstanceLock(#[allow(dead_code)] File);

pub enum Claim {
    Ours(InstanceLock),
    /// Another instance has the folder; its pid, unless it hasn't written it yet.
    Taken(Option<i32>),
}

/// Takes the configuration folder `dir` (which exists) for this instance. An error (a file
/// system without locks) leaves it to run unchecked, as before.
pub fn claim(dir: &Path) -> std::io::Result<Claim> {
    let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600).open(dir.join(LOCK_FILE))?;
    // SAFETY: a valid descriptor, which the lock lasts as long as.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::WouldBlock {
            return Err(error);
        }
        return Ok(Claim::Taken(owner(&mut file)));
    }
    file.set_len(0)?;
    file.write_all(std::process::id().to_string().as_bytes())?;
    Ok(Claim::Ours(InstanceLock(file)))
}

/// The pid in the lock file, waiting a moment for an owner that has just taken the lock.
fn owner(file: &mut File) -> Option<i32> {
    let start = std::time::Instant::now();
    loop {
        let mut pid = String::new();
        if file.rewind().is_ok() && file.read_to_string(&mut pid).is_ok() {
            if let Ok(pid) = pid.trim().parse() {
                return Some(pid);
            }
        }
        if start.elapsed() >= PID_WAIT {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Leaves the folder to the instance `pid` and ends this process, bringing that instance
/// forward as opening the app again does when it is running. On the main thread, from the
/// app's setup; it may return before the process ends, and nothing else must start then.
///
/// Since macOS 14 activation is cooperative: an app comes forward only when the active app
/// yields to it. Opening this instance from the Finder or the Dock makes it active, but not
/// yet when the setup runs, so it waits for that for a moment. Opened in the background
/// (`open -g`, a script) it doesn't become active, and the other instance stays where it is.
#[cfg(target_os = "macos")]
pub fn hand_over(pid: Option<i32>) {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2::{sel, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSApplicationDidBecomeActiveNotification, NSRunningApplication};
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};

    let (Some(target), Some(main)) = (pid.and_then(NSRunningApplication::runningApplicationWithProcessIdentifier), MainThreadMarker::new())
    else {
        std::process::exit(0);
    };
    let options = NSApplicationActivationOptions::ActivateAllWindows;
    if !target.respondsToSelector(sel!(activateFromApplication:options:)) {
        // Before macOS 14: asking is enough.
        #[allow(deprecated)]
        target.activateWithOptions(options | NSApplicationActivationOptions::ActivateIgnoringOtherApps);
        std::process::exit(0);
    }
    let activate = move || {
        NSApplication::sharedApplication(main).yieldActivationToApplication(&target);
        target.activateFromApplication_options(&NSRunningApplication::currentApplication(), options);
        std::process::exit(0);
    };
    if NSRunningApplication::currentApplication().isActive() {
        activate();
    }
    let block = RcBlock::new(move |_: NonNull<NSNotification>| activate());
    // SAFETY: AppKit's notification name; the center keeps the observer until the process ends.
    let _ = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSApplicationDidBecomeActiveNotification),
            None,
            None,
            &block,
        )
    };
    std::thread::spawn(|| {
        std::thread::sleep(HAND_OVER_WAIT);
        std::process::exit(0);
    });
}

/// Linux has no way to ask another process to come forward yet (see M31 in ROADMAP.md).
#[cfg(not(target_os = "macos"))]
pub fn hand_over(_pid: Option<i32>) {
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_has_one_instance_until_it_ends() {
        let dir = std::env::temp_dir().join(format!("zshell-instance-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // `flock` locks belong to the open file, so a second claim in the same process is
        // refused as another process's would be.
        let first = claim(&dir).unwrap();
        assert!(matches!(first, Claim::Ours(_)));
        assert!(matches!(claim(&dir).unwrap(), Claim::Taken(Some(pid)) if pid == std::process::id() as i32));
        drop(first);
        assert!(matches!(claim(&dir).unwrap(), Claim::Ours(_)));

        let other = dir.join("other");
        std::fs::create_dir_all(&other).unwrap();
        let _held = claim(&dir).unwrap();
        assert!(matches!(claim(&other).unwrap(), Claim::Ours(_)), "another folder is another configuration");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
