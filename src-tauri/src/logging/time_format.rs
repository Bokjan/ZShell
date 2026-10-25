//! The settings' time format in effect (see `model::time_format`), for our own messages in the
//! terminal; logs take theirs as they start.

use std::sync::{LazyLock, RwLock};

pub use crate::model::time_format::{TimeFormat, TimePart};

static CURRENT: LazyLock<RwLock<TimeFormat>> = LazyLock::new(RwLock::default);

/// Puts the settings' format into effect; one that isn't valid was replaced by the default when
/// the settings were read.
pub fn apply(format: &str) {
    *CURRENT.write().unwrap() = TimeFormat::parse(format).unwrap_or_default();
}

pub fn current() -> TimeFormat {
    CURRENT.read().unwrap().clone()
}
