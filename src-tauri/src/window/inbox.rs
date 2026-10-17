//! The state behind moving tabs between windows and quitting, without the windows themselves:
//! what each window is to show until its page takes it ([`Inbox`]), and the questions quitting
//! asks every window ([`BusyQueries`]).

use std::collections::{HashMap, HashSet};

use tokio::sync::oneshot;

use super::WindowContent;

/// What the windows are to show, until their pages take it.
#[derive(Default)]
pub(super) struct Inbox {
    pending: HashMap<String, Vec<WindowContent>>,
    /// Windows closing once their last tab has gone, which take nothing more.
    closing: HashSet<String>,
}

impl Inbox {
    /// What new window `label` opens for, first.
    pub fn opened(&mut self, label: &str, content: WindowContent) {
        self.pending.insert(label.to_owned(), vec![content]);
    }

    /// Whether window `label` still takes tabs: not once it is closing.
    pub fn accepts(&self, label: &str) -> bool {
        !self.closing.contains(label)
    }

    /// Gives `content` to window `label`, after what it has not taken yet.
    pub fn give(&mut self, label: &str, content: WindowContent) {
        self.pending.entry(label.to_owned()).or_default().push(content);
    }

    /// What window `label` is to show and hasn't taken yet, in order. Each is taken once.
    pub fn take(&mut self, label: &str) -> Vec<WindowContent> {
        self.pending.remove(label).unwrap_or_default()
    }

    /// Whether window `label`, which has no tabs left, closes: not if it is the `last` one, nor
    /// if it has been given a tab it hasn't taken yet. From then on it takes none.
    pub fn close_empty(&mut self, label: &str, last: bool) -> bool {
        let given = self.pending.get(label).is_some_and(|pending| !pending.is_empty());
        if given || last {
            return false;
        }
        self.closing.insert(label.to_owned());
        true
    }

    /// Window `label` is gone (or never opened), with what it hadn't taken.
    pub fn forget(&mut self, label: &str) {
        self.pending.remove(label);
        self.closing.remove(label);
    }
}

/// Asking every window how many of its tabs quitting would end (see `window::busy_count`).
#[derive(Default)]
pub(super) struct BusyQueries {
    next: u64,
    queries: HashMap<u64, BusyQuery>,
}

struct BusyQuery {
    waiting: HashSet<String>,
    count: u32,
    done: Option<oneshot::Sender<()>>,
}

impl BusyQuery {
    fn answered(&mut self) {
        if self.waiting.is_empty() {
            if let Some(done) = self.done.take() {
                let _ = done.send(());
            }
        }
    }
}

impl BusyQueries {
    /// Starts asking the windows `labels`: the query's id, and what is told once all have
    /// answered (or are gone).
    pub fn ask(&mut self, labels: HashSet<String>) -> (u64, oneshot::Receiver<()>) {
        let id = self.next;
        self.next += 1;
        let (done, answered) = oneshot::channel();
        let mut query = BusyQuery { waiting: labels, count: 0, done: Some(done) };
        query.answered();
        self.queries.insert(id, query);
        (id, answered)
    }

    /// Window `label`'s answer to query `id`: `count` of its tabs are busy.
    pub fn reply(&mut self, id: u64, label: &str, count: u32) {
        let Some(query) = self.queries.get_mut(&id) else { return };
        if query.waiting.remove(label) {
            query.count += count;
        }
        query.answered();
    }

    /// Window `label` is gone, with its sessions: no query waits for it any more.
    pub fn gone(&mut self, label: &str) {
        for query in self.queries.values_mut() {
            if query.waiting.remove(label) {
                query.answered();
            }
        }
    }

    /// Ends query `id`: the busy tabs counted, and the windows that didn't answer.
    pub fn finish(&mut self, id: u64) -> Option<(u32, HashSet<String>)> {
        self.queries.remove(&id).map(|query| (query.count, query.waiting))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(title: &str) -> WindowContent {
        WindowContent::Profile { profile_id: "p".into(), title: title.into() }
    }

    fn titles(contents: Vec<WindowContent>) -> Vec<String> {
        contents
            .into_iter()
            .map(|content| match content {
                WindowContent::Profile { title, .. } => title,
                WindowContent::Tab { .. } => "tab".into(),
            })
            .collect()
    }

    #[test]
    fn a_window_takes_what_it_was_given_once_in_order() {
        let mut inbox = Inbox::default();
        inbox.opened("window-1", profile("a"));
        inbox.give("window-1", profile("b"));
        assert_eq!(titles(inbox.take("window-1")), ["a", "b"]);
        assert!(inbox.take("window-1").is_empty());
    }

    #[test]
    fn a_window_given_a_tab_or_the_last_one_stays_open() {
        let mut inbox = Inbox::default();
        assert!(!inbox.close_empty("main", true));
        inbox.give("window-1", profile("a"));
        assert!(!inbox.close_empty("window-1", false));
        assert!(inbox.accepts("window-1"));

        inbox.take("window-1");
        assert!(inbox.close_empty("window-1", false));
        // Closing: it takes no more tabs.
        assert!(!inbox.accepts("window-1"));
        inbox.forget("window-1");
        assert!(inbox.accepts("window-1"));
    }

    #[test]
    fn a_query_ends_once_every_window_answered_or_went() {
        let mut busy = BusyQueries::default();
        let labels: HashSet<String> = ["main", "window-1", "window-2"].map(String::from).into();
        let (id, mut answered) = busy.ask(labels);
        busy.reply(id, "main", 2);
        // Twice, or from a window not asked: not counted.
        busy.reply(id, "main", 2);
        busy.reply(id, "window-9", 5);
        assert!(answered.try_recv().is_err());
        busy.gone("window-1");
        assert!(answered.try_recv().is_err());
        busy.reply(id, "window-2", 1);
        assert!(answered.try_recv().is_ok());
        assert_eq!(busy.finish(id), Some((3, HashSet::new())));
        assert_eq!(busy.finish(id), None);
    }

    #[test]
    fn a_query_without_an_answer_says_who_didnt() {
        let mut busy = BusyQueries::default();
        let (id, _) = busy.ask(["main".to_owned()].into());
        let (other, mut answered) = busy.ask(HashSet::new());
        assert_ne!(id, other);
        // Nobody to ask: answered at once.
        assert!(answered.try_recv().is_ok());
        assert_eq!(busy.finish(id), Some((0, ["main".to_owned()].into())));
    }
}
