//! The open sessions, and the pages they belong to.
//!
//! A session belongs to the webview whose tab shows it, and only that webview's commands
//! reach it: one from another window gets `session.notFound`, as for a session that doesn't
//! exist. Session ids are counted from 1, so knowing one is no proof of anything. Moving a
//! session to another window takes the owner's say: it offers the session to that window
//! ([`SessionManager::offer`]), which can then take it once ([`SessionManager::adopt`]).

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use tokio::sync::mpsc;

use super::{Flow, Foreground, Lifetime, SessionId, SessionInput, SessionSink, TermIo};
use crate::error::{Error, Result};
use crate::logging::LogSlot;
use crate::zmodem::Zmodem;

struct SessionEntry {
    /// The webview whose tab shows the session.
    webview: String,
    /// The webview it is offered to (see [`SessionManager::offer`]).
    offered_to: Option<String>,
    /// The webview it last moved from, and the move's id, until that webview has handed its
    /// screen over (see [`SessionManager::handover`]).
    moved_from: Option<(String, String)>,
    input: mpsc::UnboundedSender<SessionInput>,
    sink: SessionSink,
    lifetime: Lifetime,
    flow: Arc<Flow>,
    foreground: Foreground,
    zmodem: Arc<Zmodem>,
    task: JoinHandle<()>,
}

#[derive(Default)]
pub struct SessionManager {
    next_id: AtomicU32,
    sessions: Mutex<HashMap<SessionId, SessionEntry>>,
    /// How many times each webview's page has gone away (see [`Page`]). Locked after
    /// `sessions`, never before.
    pages: Mutex<HashMap<String, u64>>,
}

/// The page a session is opened for: a webview, and the page it shows at the time. A
/// session opened while the page reloads (opening waits for its log) belongs to no page,
/// and would never be shown or closed.
pub struct Page {
    webview: String,
    generation: u64,
}

impl SessionManager {
    /// The page `webview` shows now.
    pub fn page(&self, webview: &str) -> Page {
        let generation = self.pages.lock().unwrap().get(webview).copied().unwrap_or(0);
        Page { webview: webview.to_owned(), generation }
    }

    /// Starts a session whose backend task is produced by `backend`, for a tab on `page`,
    /// logging to `log` (which may already be writing, so that the log starts with the
    /// session's first output). `encoding` is the remote side's character encoding. Fails
    /// with `session.pageGone`, starting nothing, if the page has gone away since.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn<F, Fut>(
        &self,
        page: &Page,
        channel: Channel,
        size: (u16, u16),
        log: Arc<LogSlot>,
        encoding: &'static encoding_rs::Encoding,
        backend: F,
    ) -> Result<SessionId>
    where
        F: FnOnce(SessionId, TermIo) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        // Held until the session is in, so that `remove_webview` either sees it or has
        // already moved the page on.
        let mut sessions = self.sessions.lock().unwrap();
        if self.page(&page.webview).generation != page.generation {
            return Err(Error::new("session.pageGone"));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (io, input) = TermIo::new(channel, size, log, encoding);
        let sink = io.sink();
        let lifetime = io.lifetime.clone();
        let flow = io.sink.flow();
        let foreground = io.foreground();
        let zmodem = io.sink.zmodem.clone();
        let task = tauri::async_runtime::spawn(backend(id, io));
        let entry = SessionEntry {
            webview: page.webview.clone(),
            offered_to: None,
            moved_from: None,
            input,
            sink,
            lifetime,
            flow,
            foreground,
            zmodem,
            task,
        };
        sessions.insert(id, entry);
        Ok(id)
    }

    /// Fails with `session.notFound` unless session `id` belongs to `webview`.
    pub fn check(&self, webview: &str, id: SessionId) -> Result<()> {
        owned(&self.sessions.lock().unwrap(), webview, id).map(drop)
    }

    /// Whether session `id` belongs to `webview`.
    pub fn owns(&self, webview: &str, id: SessionId) -> bool {
        self.check(webview, id).is_ok()
    }

    pub fn send(&self, webview: &str, id: SessionId, input: SessionInput) -> Result<()> {
        let sessions = self.sessions.lock().unwrap();
        let entry = owned(&sessions, webview, id)?;
        // A send error means the backend already exited; it will be removed on close.
        let _ = entry.input.send(input);
        Ok(())
    }

    /// Records that the frontend has processed `bytes` of output, in `webview`. Ignored from
    /// a webview the session has moved away from, whose output no longer counts.
    pub fn ack(&self, webview: &str, id: SessionId, bytes: usize) -> Result<()> {
        let sessions = self.sessions.lock().unwrap();
        let entry = sessions.get(&id).ok_or_else(|| Error::new("session.notFound"))?;
        if entry.webview == webview {
            entry.flow.ack(bytes);
        }
        Ok(())
    }

    /// Offers the sessions `ids`, which belong to `webview`, to the webview `to`, which a tab
    /// showing them is given to: it may then take them ([`adopt`](Self::adopt)), once. A later
    /// offer replaces this one. Offers nothing unless `webview` has all of them.
    pub fn offer(&self, webview: &str, ids: &[SessionId], to: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        for id in ids {
            owned(&sessions, webview, *id)?;
        }
        for id in ids {
            if let Some(entry) = sessions.get_mut(id) {
                entry.offered_to = Some(to.to_owned());
            }
        }
        Ok(())
    }

    /// Moves a session offered to `page`'s webview ([`offer`](Self::offer)) to a tab there,
    /// which receives its output and events on `channel` from now on (see
    /// [`SessionSink::move_to`]). Not during a ZMODEM transfer, whose file dialogs belong to the
    /// old tab (`session.busy`). `move_id` names the move for the old tab (see
    /// [`SessionEvent::Moved`](super::SessionEvent::Moved)), which hands its screen over with it.
    pub fn adopt(&self, id: SessionId, page: &Page, channel: Channel, move_id: String) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if self.page(&page.webview).generation != page.generation {
            return Err(Error::new("session.pageGone"));
        }
        let entry = sessions.get_mut(&id).filter(|entry| entry.offered_to.as_ref() == Some(&page.webview));
        let entry = entry.ok_or_else(|| Error::new("session.notFound"))?;
        if entry.zmodem.is_active() {
            return Err(Error::new("session.busy"));
        }
        entry.offered_to = None;
        let from = std::mem::replace(&mut entry.webview, page.webview.clone());
        entry.moved_from = Some((from, move_id.clone()));
        entry.sink.move_to(channel, move_id);
        Ok(())
    }

    /// The webview a session has moved to, for the screen that `webview`, which it moved from
    /// with `move_id`, hands over; once.
    pub fn handover(&self, webview: &str, id: SessionId, move_id: &str) -> Result<String> {
        let mut sessions = self.sessions.lock().unwrap();
        let entry = sessions.get_mut(&id).ok_or_else(|| Error::new("session.notFound"))?;
        match &entry.moved_from {
            Some((from, moved)) if from == webview && moved == move_id => {
                entry.moved_from = None;
                Ok(entry.webview.clone())
            }
            _ => Err(Error::new("session.notFound")),
        }
    }

    /// How many sessions `webview`'s tabs show.
    pub fn owned(&self, webview: &str) -> u32 {
        let sessions = self.sessions.lock().unwrap();
        sessions.values().filter(|entry| entry.webview == webview).count() as u32
    }

    /// The program running in the session's foreground other than its shell, if any (see
    /// [`ForegroundProbe`](super::ForegroundProbe)); always `None` for sessions that cannot tell.
    pub fn foreground(&self, webview: &str, id: SessionId) -> Result<Option<String>> {
        let foreground = owned(&self.sessions.lock().unwrap(), webview, id)?.foreground.clone();
        Ok(foreground.get())
    }

    /// The session's output side: its log, and events for its tab.
    pub fn sink(&self, webview: &str, id: SessionId) -> Result<SessionSink> {
        Ok(owned(&self.sessions.lock().unwrap(), webview, id)?.sink.clone())
    }

    pub fn zmodem(&self, webview: &str, id: SessionId) -> Result<Arc<Zmodem>> {
        Ok(owned(&self.sessions.lock().unwrap(), webview, id)?.zmodem.clone())
    }

    /// Closes a session: releases what it holds, then aborts its task.
    pub fn remove(&self, webview: &str, id: SessionId) -> Result<()> {
        let entry = {
            let mut sessions = self.sessions.lock().unwrap();
            owned(&sessions, webview, id)?;
            sessions.remove(&id)
        };
        if let Some(entry) = entry {
            close(entry);
        }
        Ok(())
    }

    /// Closes the sessions of a webview whose page is going away (reloading, or its window
    /// closing): nothing would show them or close them any more. Sessions offered to it are
    /// still offered: a new window's page starts loading after it was offered the sessions it
    /// is to take.
    pub fn remove_webview(&self, webview: &str) {
        let entries: Vec<SessionEntry> = {
            let mut sessions = self.sessions.lock().unwrap();
            *self.pages.lock().unwrap().entry(webview.to_owned()).or_default() += 1;
            let ids: Vec<SessionId> = sessions.iter().filter(|(_, entry)| entry.webview == webview).map(|(id, _)| *id).collect();
            ids.iter().filter_map(|id| sessions.remove(id)).collect()
        };
        entries.into_iter().for_each(close);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }
}

/// Session `id`, if it belongs to `webview` (see the module's docs).
fn owned<'a>(sessions: &'a HashMap<SessionId, SessionEntry>, webview: &str, id: SessionId) -> Result<&'a SessionEntry> {
    sessions.get(&id).filter(|entry| entry.webview == webview).ok_or_else(|| Error::new("session.notFound"))
}

/// Releases what a session holds, then aborts its task.
fn close(entry: SessionEntry) {
    entry.lifetime.close();
    entry.task.abort();
}

#[cfg(test)]
mod tests {
    use tauri::ipc::InvokeResponseBody;

    use super::*;

    /// Counts how often something was released.
    fn counter() -> (Arc<std::sync::atomic::AtomicUsize>, impl Fn() -> usize) {
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let read = count.clone();
        (count, move || read.load(Ordering::SeqCst))
    }

    /// Starts a session for `webview` whose backend holds a release and runs until aborted;
    /// returns how often its release ran, and whether its task has been dropped.
    fn hold(sessions: &SessionManager, webview: &str) -> (SessionId, impl Fn() -> usize, impl Fn() -> usize) {
        let (released, released_count) = counter();
        let (dropped, dropped_count) = counter();
        let page = sessions.page(webview);
        let id = sessions
            .spawn(
                &page,
                Channel::new(|_| Ok(())),
                (80, 24),
                LogSlot::new(Default::default()),
                encoding_rs::UTF_8,
                move |_, io| async move {
                    io.on_close(move || {
                        released.fetch_add(1, Ordering::SeqCst);
                    });
                    struct Dropped(Arc<std::sync::atomic::AtomicUsize>);
                    impl Drop for Dropped {
                        fn drop(&mut self) {
                            self.0.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    let _dropped = Dropped(dropped);
                    let _io = io;
                    std::future::pending::<()>().await;
                },
            )
            .unwrap();
        (id, released_count, dropped_count)
    }

    async fn eventually(done: impl Fn() -> bool) {
        for _ in 0..100 {
            if done() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("timed out");
    }

    /// A page that reloads (or a window that closes) takes its sessions with it, and only its.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_page_reloading_closes_its_sessions() {
        let sessions = SessionManager::default();
        let (_, a_released, a_dropped) = hold(&sessions, "main");
        let (_, b_released, b_dropped) = hold(&sessions, "main");
        let (other, other_released, _) = hold(&sessions, "other");
        // The tasks register their releases once running.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        sessions.remove_webview("main");
        assert_eq!(sessions.len(), 1);
        // Released right away, before the aborted tasks are dropped.
        assert_eq!((a_released(), b_released(), other_released()), (1, 1, 0));
        eventually(|| a_dropped() == 1 && b_dropped() == 1).await;
        // Dropping the task's terminal side doesn't release twice.
        assert_eq!((a_released(), b_released()), (1, 1));

        sessions.remove("other", other).unwrap();
        assert_eq!((sessions.len(), other_released()), (0, 1));
    }

    /// A session opened for a page that has reloaded since is refused, and the new page's
    /// sessions are not.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_session_for_a_page_gone_away_is_refused() {
        let sessions = SessionManager::default();
        let old = sessions.page("main");
        sessions.remove_webview("main");
        let refused =
            sessions.spawn(&old, Channel::new(|_| Ok(())), (80, 24), LogSlot::new(Default::default()), encoding_rs::UTF_8, |_, _| async {});
        assert_eq!(refused.unwrap_err().code(), "session.pageGone");
        assert_eq!(sessions.len(), 0);
        let _ = hold(&sessions, "main");
        assert_eq!(sessions.len(), 1);
    }

    /// A channel that keeps what it is sent.
    fn recording() -> (Channel, Arc<Mutex<Vec<Vec<u8>>>>) {
        let messages = Arc::new(Mutex::new(Vec::new()));
        let kept = messages.clone();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(message) = body {
                kept.lock().unwrap().push(message);
            }
            Ok(())
        });
        (channel, messages)
    }

    /// A session moving to another window: the old tab gets the output sent before and then
    /// `moved`, the new one what follows; only the new tab's acknowledgements count.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_session_moves_to_another_window() {
        let sessions = SessionManager::default();
        let (old, old_messages) = recording();
        let (sink_tx, sink_rx) = std::sync::mpsc::channel();
        let page = sessions.page("main");
        let id = sessions
            .spawn(&page, old, (80, 24), LogSlot::new(Default::default()), encoding_rs::UTF_8, move |_, io| async move {
                let _ = sink_tx.send(io.sink());
                let _io = io;
                std::future::pending::<()>().await;
            })
            .unwrap();
        let sink = sink_rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
        sink.write(b"before".to_vec());

        let (new, new_messages) = recording();
        assert_eq!(sessions.owned("main"), 1);
        sessions.offer("main", &[id], "other").unwrap();
        sessions.adopt(id, &sessions.page("other"), new, "m1".into()).unwrap();
        sink.write(b"after".to_vec());
        assert_eq!(*old_messages.lock().unwrap(), [b"\0before".to_vec(), b"\x01{\"type\":\"moved\",\"moveId\":\"m1\"}".to_vec()]);
        assert_eq!(*new_messages.lock().unwrap(), [b"\0after".to_vec()]);
        assert!(sessions.owns("other", id) && !sessions.owns("main", id));
        assert_eq!((sessions.owned("main"), sessions.owned("other")), (0, 1));
        // The old window hands its screen over to the new one, once, for that move only.
        assert_eq!(sessions.handover("other", id, "m1").unwrap_err().code(), "session.notFound");
        assert_eq!(sessions.handover("main", id, "m0").unwrap_err().code(), "session.notFound");
        assert_eq!(sessions.handover("main", id, "m1").unwrap(), "other");
        assert_eq!(sessions.handover("main", id, "m1").unwrap_err().code(), "session.notFound");

        let unacked = || sink.flow().unacked();
        assert_eq!(unacked(), 5);
        sessions.ack("main", id, 5).unwrap();
        assert_eq!(unacked(), 5);
        sessions.ack("other", id, 5).unwrap();
        assert_eq!(unacked(), 0);

        // Not to a page that has gone away since.
        let gone = sessions.page("third");
        sessions.offer("other", &[id], "third").unwrap();
        sessions.remove_webview("third");
        assert_eq!(sessions.adopt(id, &gone, Channel::new(|_| Ok(())), "m2".into()).unwrap_err().code(), "session.pageGone");
        // A new window is offered its sessions before its page starts loading.
        sessions.offer("other", &[id], "fourth").unwrap();
        sessions.remove_webview("fourth");
        sessions.adopt(id, &sessions.page("fourth"), Channel::new(|_| Ok(())), "m3".into()).unwrap();
        assert!(sessions.owns("fourth", id));
    }

    /// Only the window showing a session can act on it, or give it to another window.
    #[tokio::test(flavor = "multi_thread")]
    async fn sessions_answer_only_their_window() {
        let sessions = SessionManager::default();
        let (id, released, _) = hold(&sessions, "main");
        let (theirs, _, _) = hold(&sessions, "other");
        // The tasks register their releases once running.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let not_found = |result: Result<()>| result.unwrap_err().code();
        assert_eq!(not_found(sessions.send("other", id, SessionInput::Break)), "session.notFound");
        assert_eq!(not_found(sessions.sink("other", id).map(drop)), "session.notFound");
        assert_eq!(not_found(sessions.zmodem("other", id).map(drop)), "session.notFound");
        assert_eq!(not_found(sessions.foreground("other", id).map(drop)), "session.notFound");
        assert_eq!(not_found(sessions.remove("other", id)), "session.notFound");
        assert_eq!(not_found(sessions.check("other", id)), "session.notFound");
        sessions.send("main", id, SessionInput::Break).unwrap();

        // Taking it needs an offer, from its own window, for this one; it is used up.
        let take = |webview: &str| sessions.adopt(id, &sessions.page(webview), Channel::new(|_| Ok(())), "m".into());
        assert_eq!(not_found(take("other")), "session.notFound");
        assert_eq!(not_found(sessions.offer("other", &[id], "other")), "session.notFound");
        // Nothing is offered unless all of them are its own.
        assert_eq!(not_found(sessions.offer("main", &[id, theirs], "third")), "session.notFound");
        assert_eq!(not_found(take("third")), "session.notFound");
        sessions.offer("main", &[id], "third").unwrap();
        assert_eq!(not_found(take("other")), "session.notFound");
        take("third").unwrap();
        assert_eq!(not_found(take("third")), "session.notFound");
        assert!(sessions.owns("third", id));
        sessions.remove("third", id).unwrap();
        assert_eq!(released(), 1);
    }
}
