//! macOS: VoiceOver, from `NSWorkspace`'s `voiceOverEnabled`, which has no notification of its
//! own but can be observed (key-value observing). Asking only when the app becomes active
//! wouldn't do: VoiceOver is often turned on and off (⌘F5) while ZShell is in front.

use std::ffi::c_void;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{
    ns_string, NSDictionary, NSKeyValueChangeKey, NSKeyValueObservingOptions, NSObjectNSKeyValueObserverRegistration, NSString,
};

use super::Changed;

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `Observer` does not implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "ZShellVoiceOverObserver"]
    #[ivars = Changed]
    struct Observer;

    impl Observer {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe(
            &self,
            _key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
            _context: *mut c_void,
        ) {
            (self.ivars())(NSWorkspace::sharedWorkspace().isVoiceOverEnabled());
        }
    }
);

pub fn watch(changed: Changed) {
    let observer = Observer::alloc().set_ivars(changed);
    // SAFETY: NSObject's `init`.
    let observer: Retained<Observer> = unsafe { msg_send![super(observer), init] };
    // SAFETY: `voiceOverEnabled` is the property's key, and the observer gets no context.
    // Observers aren't retained, so it is kept for as long as the app runs. `Initial` calls it
    // once with the current state, now.
    unsafe {
        NSWorkspace::sharedWorkspace().addObserver_forKeyPath_options_context(
            &observer,
            ns_string!("voiceOverEnabled"),
            NSKeyValueObservingOptions::Initial,
            std::ptr::null_mut(),
        );
    }
    std::mem::forget(observer);
}
