//! The progress programs report in the terminals (OSC 9;4), on the taskbar button of each
//! window on Windows, and on the Dock icon (macOS) or the launcher (Linux, where the desktop
//! has the libunity interface: Ubuntu Dock, Dash to Dock), which the whole app has one of:
//! there it merges the windows' progress.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::window::{ProgressBarState, ProgressBarStatus};
use tauri::{AppHandle, Manager, WebviewWindow};
use ts_rs::TS;

/// A program's progress, or a window's: the merge of its tabs'.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TaskProgress {
    pub state: TaskState,
    /// From 0 to 100; not shown while indeterminate.
    pub percent: u8,
}

/// The states of OSC 9;4 (besides 0, which removes the progress).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TaskState {
    Normal,
    Error,
    Indeterminate,
    Paused,
}

impl TaskState {
    /// Which of several shows: what most needs the user's eyes first.
    fn rank(self) -> u8 {
        match self {
            Self::Error => 3,
            Self::Paused => 2,
            Self::Normal => 1,
            Self::Indeterminate => 0,
        }
    }
}

/// The progress shown for several: the state that ranks highest, and of several in progress
/// the one furthest from done. Kept the same as `mergeProgress` in the frontend, which merges
/// a window's tabs.
pub fn merge(all: impl IntoIterator<Item = TaskProgress>) -> Option<TaskProgress> {
    all.into_iter().reduce(|a, b| match a.state.rank().cmp(&b.state.rank()) {
        std::cmp::Ordering::Greater => a,
        std::cmp::Ordering::Less => b,
        std::cmp::Ordering::Equal => TaskProgress { percent: a.percent.min(b.percent), ..a },
    })
}

/// The progress each window shows, by label.
#[derive(Default)]
pub struct Progresses(HashMap<String, TaskProgress>);

impl Progresses {
    /// Sets window `label`'s progress (none: it has none); returns what the app shows now, for
    /// the platforms with one for the whole app.
    fn set(&mut self, label: &str, progress: Option<TaskProgress>) -> Option<TaskProgress> {
        match progress {
            Some(progress) => self.0.insert(label.to_owned(), progress),
            None => self.0.remove(label),
        };
        merge(self.0.values().copied())
    }
}

fn bar(progress: Option<TaskProgress>) -> ProgressBarState {
    let status = match progress.map(|p| p.state) {
        None => ProgressBarStatus::None,
        Some(TaskState::Normal) => ProgressBarStatus::Normal,
        Some(TaskState::Error) => ProgressBarStatus::Error,
        Some(TaskState::Indeterminate) => ProgressBarStatus::Indeterminate,
        Some(TaskState::Paused) => ProgressBarStatus::Paused,
    };
    ProgressBarState { status: Some(status), progress: progress.map(|p| u64::from(p.percent.min(100))) }
}

/// Shows `window`'s progress (the merge of its tabs'), or that it has none.
pub fn show(window: &WebviewWindow, progress: Option<TaskProgress>) -> tauri::Result<()> {
    let windows = window.state::<super::Windows>();
    let app_wide = windows.progress.lock().unwrap().set(window.label(), progress);
    window.set_progress_bar(bar(if cfg!(windows) { progress } else { app_wide }))
}

/// Window `label` is gone, or its page loads again with none of its tabs: its progress goes.
pub fn forget(app: &AppHandle, label: &str) {
    let app_wide = app.state::<super::Windows>().progress.lock().unwrap().set(label, None);
    // On Windows the taskbar button went with the window, or the page tells it again.
    if cfg!(windows) {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.set_progress_bar(bar(None));
        }
        return;
    }
    if let Some(window) = app.webview_windows().into_values().next() {
        let _ = window.set_progress_bar(bar(app_wide));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn p(state: TaskState, percent: u8) -> TaskProgress {
        TaskProgress { state, percent }
    }

    #[test]
    fn merging_shows_what_most_needs_attention() {
        use TaskState::*;
        assert_eq!(merge([]), None);
        assert_eq!(merge([p(Normal, 40), p(Normal, 70)]), Some(p(Normal, 40)));
        assert_eq!(merge([p(Indeterminate, 0), p(Normal, 70)]), Some(p(Normal, 70)));
        assert_eq!(merge([p(Normal, 10), p(Paused, 50), p(Indeterminate, 0)]), Some(p(Paused, 50)));
        assert_eq!(merge([p(Paused, 50), p(Error, 90), p(Normal, 10)]), Some(p(Error, 90)));
    }

    #[test]
    fn the_app_shows_the_merge_of_its_windows() {
        use TaskState::*;
        let mut all = Progresses::default();
        assert_eq!(all.set("main", Some(p(Normal, 60))), Some(p(Normal, 60)));
        assert_eq!(all.set("window-1", Some(p(Normal, 20))), Some(p(Normal, 20)));
        assert_eq!(all.set("window-1", None), Some(p(Normal, 60)));
        assert_eq!(all.set("main", None), None);
    }
}
