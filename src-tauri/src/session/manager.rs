//! The open sessions, and the pages they belong to.

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
        let entry = SessionEntry { webview: page.webview.clone(), input, sink, lifetime, flow, foreground, zmodem, task };
        sessions.insert(id, entry);
        Ok(id)
    }

    pub fn send(&self, id: SessionId, input: SessionInput) -> Result<()> {
        let sessions = self.sessions.lock().unwrap();
        let entry = sessions.get(&id).ok_or_else(|| Error::new("session.notFound"))?;
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

    /// Moves a session to a tab on `page` (another window's), which receives its output and
    /// events on `channel` from now on (see [`SessionSink::move_to`]). Not during a ZMODEM
    /// transfer, whose file dialogs belong to the old tab (`session.busy`). `move_id` names the
    /// move for the old tab (see [`SessionEvent::Moved`](super::SessionEvent::Moved)).
    pub fn adopt(&self, id: SessionId, page: &Page, channel: Channel, move_id: String) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if self.page(&page.webview).generation != page.generation {
            return Err(Error::new("session.pageGone"));
        }
        let entry = sessions.get_mut(&id).ok_or_else(|| Error::new("session.notFound"))?;
        if entry.zmodem.is_active() {
            return Err(Error::new("session.busy"));
        }
        entry.webview = page.webview.clone();
        entry.sink.move_to(channel, move_id);
        Ok(())
    }

    /// How many sessions `webview`'s tabs show.
    pub fn owned(&self, webview: &str) -> u32 {
        let sessions = self.sessions.lock().unwrap();
        sessions.values().filter(|entry| entry.webview == webview).count() as u32
    }

    /// The webview whose tab shows the session.
    pub fn owner(&self, id: SessionId) -> Result<String> {
        let sessions = self.sessions.lock().unwrap();
        Ok(sessions.get(&id).ok_or_else(|| Error::new("session.notFound"))?.webview.clone())
    }

    /// The program running in the session's foreground other than its shell, if any (see
    /// [`ForegroundProbe`](super::ForegroundProbe)); always `None` for sessions that cannot tell.
    pub fn foreground(&self, id: SessionId) -> Result<Option<String>> {
        let foreground = {
            let sessions = self.sessions.lock().unwrap();
            sessions.get(&id).ok_or_else(|| Error::new("session.notFound"))?.foreground.clone()
        };
        Ok(foreground.get())
    }

    /// The session's output side: its log, and events for its tab.
    pub fn sink(&self, id: SessionId) -> Result<SessionSink> {
        let sessions = self.sessions.lock().unwrap();
        Ok(sessions.get(&id).ok_or_else(|| Error::new("session.notFound"))?.sink.clone())
    }

    pub fn zmodem(&self, id: SessionId) -> Result<Arc<Zmodem>> {
        let sessions = self.sessions.lock().unwrap();
        Ok(sessions.get(&id).ok_or_else(|| Error::new("session.notFound"))?.zmodem.clone())
    }

    /// Closes a session: releases what it holds, then aborts its task.
    pub fn remove(&self, id: SessionId) -> Result<()> {
        let entry = self.sessions.lock().unwrap().remove(&id).ok_or_else(|| Error::new("session.notFound"))?;
        entry.lifetime.close();
        entry.task.abort();
        Ok(())
    }

    /// Closes the sessions of a webview whose page is going away (reloading, or its window
    /// closing): nothing would show them or close them any more.
    pub fn remove_webview(&self, webview: &str) {
        let ids: Vec<SessionId> = {
            let sessions = self.sessions.lock().unwrap();
            *self.pages.lock().unwrap().entry(webview.to_owned()).or_default() += 1;
            sessions.iter().filter(|(_, entry)| entry.webview == webview).map(|(id, _)| *id).collect()
        };
        for id in ids {
            let _ = self.remove(id);
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }
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

        sessions.remove(other).unwrap();
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
        sessions.adopt(id, &sessions.page("other"), new, "m1".into()).unwrap();
        sink.write(b"after".to_vec());
        assert_eq!(*old_messages.lock().unwrap(), [b"\0before".to_vec(), b"\x01{\"type\":\"moved\",\"moveId\":\"m1\"}".to_vec()]);
        assert_eq!(*new_messages.lock().unwrap(), [b"\0after".to_vec()]);
        assert_eq!(sessions.owner(id).unwrap(), "other");
        assert_eq!((sessions.owned("main"), sessions.owned("other")), (0, 1));

        let unacked = || sink.flow().unacked();
        assert_eq!(unacked(), 5);
        sessions.ack("main", id, 5).unwrap();
        assert_eq!(unacked(), 5);
        sessions.ack("other", id, 5).unwrap();
        assert_eq!(unacked(), 0);

        // Not to a page that has gone away since.
        let gone = sessions.page("third");
        sessions.remove_webview("third");
        assert_eq!(sessions.adopt(id, &gone, Channel::new(|_| Ok(())), "m2".into()).unwrap_err().code(), "session.pageGone");
    }
}
