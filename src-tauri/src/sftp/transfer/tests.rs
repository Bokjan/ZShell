use super::left::Stamp;
use super::*;

#[test]
fn a_transfer_is_cancelled_only_by_its_page_or_with_it() {
    let transfers = Transfers::default();
    let mine = transfers.start("main", "t1");
    let other = transfers.start("window-2", "t1");
    transfers.cancel("window-2", "t2");
    assert!(!mine.is_cancelled() && !other.is_cancelled());
    transfers.cancel("window-2", "t1");
    assert!(!mine.is_cancelled() && other.is_cancelled());

    let later = transfers.start("main", "t2");
    transfers.cancel_webview("main");
    assert!(mine.is_cancelled() && later.is_cancelled());
    transfers.finish("window-2", "t1");
    assert!(transfers.running.lock().unwrap().is_empty());
}

#[test]
fn a_transfer_cancelled_as_it_fails_is_not_gone_on_with() {
    let transfers = Transfers::default();
    let job = || Job::download(Vec::new(), PathBuf::from("downloads"));
    let paused = |id: &str| transfers.paused.lock().unwrap().contains_key(&("main".to_owned(), id.to_owned()));

    // Cancelled before it is kept: given back to discard.
    let cancelled = transfers.start("main", "t1");
    transfers.cancel("main", "t1");
    assert!(transfers.pause("main", "t1", job(), &cancelled).is_some());
    assert!(!paused("t1"));

    // Cancelled once kept, still running or ended: taken back to discard.
    let cancelled = transfers.start("main", "t2");
    assert!(transfers.pause("main", "t2", job(), &cancelled).is_none());
    assert!(transfers.cancel("main", "t2").is_some());
    assert!(!paused("t2"));
    let cancelled = transfers.start("main", "t3");
    assert!(transfers.pause("main", "t3", job(), &cancelled).is_none());
    transfers.finish("main", "t3");
    assert!(transfers.cancel("main", "t3").is_some() && !paused("t3"));

    // Not cancelled: kept to go on with.
    let cancelled = transfers.start("main", "t4");
    assert!(transfers.pause("main", "t4", job(), &cancelled).is_none());
    assert!(transfers.take_paused("main", "t4").is_some());
}

#[test]
fn what_a_transfer_left_is_its_pages_until_forgotten() {
    let transfers = Transfers::default();
    let stamp = Stamp { size: 1, modified: None };
    let left = |file: &str| Kept::Local(Left { files: vec![(PathBuf::from(file), stamp)], ..Left::default() });
    transfers.keep("main", "t1", left("a"));
    transfers.keep("main", "t2", left("b"));
    transfers.keep("main", "t3", Kept::Local(Left { replaced: 2, ..Left::default() }));
    transfers.keep("window-2", "t1", left("c"));
    assert!(transfers.take("window-2", "t2").is_none());
    // Nothing to delete: not kept.
    assert!(transfers.take("main", "t3").is_none());
    transfers.forget("main", &["t1".into()]);
    assert!(transfers.take("main", "t1").is_none());
    assert!(matches!(transfers.take("main", "t2"), Some(Kept::Local(left)) if left.files == [(PathBuf::from("b"), stamp)]));
    // Being deleted: not again meanwhile. What is left goes back, unless forgotten meanwhile.
    assert!(transfers.take("main", "t2").is_none());
    transfers.put_back("main", "t2", left("b"));
    assert!(transfers.take("main", "t2").is_some());
    transfers.forget("main", &["t2".into()]);
    transfers.put_back("main", "t2", left("b"));
    assert!(transfers.take("main", "t2").is_none());
    transfers.keep("main", "t4", left("d"));
    assert!(transfers.take("main", "t4").is_some());
    transfers.put_back("main", "t4", Kept::Local(Left::default()));
    assert!(!transfers.left.lock().unwrap().contains_key(&("main".into(), "t4".into())));
    transfers.cancel_webview("window-2");
    assert!(transfers.left.lock().unwrap().is_empty());
}

#[tokio::test]
async fn transfers_on_one_connection_share_the_files_copied_at_once() {
    use std::sync::atomic::AtomicUsize;
    let slots = Arc::new(TransferSlots::default());
    let (copying, most) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let copy = |_| async {
        let now = copying.fetch_add(1, Ordering::SeqCst) + 1;
        most.fetch_max(now, Ordering::SeqCst);
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        copying.fetch_sub(1, Ordering::SeqCst);
        Ok(())
    };
    let small = vec![1; 20];
    let mut first = Reporter::with_sink(|_| {}, Default::default()).sharing(slots.clone());
    let mut second = Reporter::with_sink(|_| {}, Default::default()).sharing(slots);
    let (one, two) = (Run::new(&mut first), Run::new(&mut second));
    let (a, b) = tokio::join!(copy_files(&small, &one, copy), copy_files(&small, &two, copy));
    a.unwrap();
    b.unwrap();
    assert_eq!(most.load(Ordering::SeqCst), CONCURRENT_FILES);
}

#[tokio::test]
async fn a_large_file_leaves_room_for_a_small_one() {
    use std::sync::atomic::AtomicUsize;
    let (copying, most) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let copy = |_| async {
        let now = copying.fetch_add(1, Ordering::SeqCst) + 1;
        most.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(5)).await;
        copying.fetch_sub(1, Ordering::SeqCst);
        Ok(())
    };
    let sizes = [SMALL_FILE + 1, SMALL_FILE + 1, 1, 1, 1];
    let mut reporter = Reporter::with_sink(|_| {}, Default::default());
    let run = Run::new(&mut reporter);
    copy_files(&sizes, &run, copy).await.unwrap();
    // A large one and a small one; never two large ones.
    assert_eq!(most.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn large_files_waiting_leave_room_for_a_small_one() {
    use std::sync::atomic::AtomicUsize;
    let (large_done, large_done_by_small) = (AtomicUsize::new(0), AtomicUsize::new(usize::MAX));
    let sizes = [SMALL_FILE + 1, SMALL_FILE + 1, SMALL_FILE + 1, 1];
    let copy = |i| {
        let (large_done, large_done_by_small) = (&large_done, &large_done_by_small);
        async move {
            if sizes[i] > SMALL_FILE {
                tokio::time::sleep(Duration::from_millis(5)).await;
                large_done.fetch_add(1, Ordering::SeqCst);
            } else {
                large_done_by_small.store(large_done.load(Ordering::SeqCst), Ordering::SeqCst);
            }
            Ok(())
        }
    };
    let mut reporter = Reporter::with_sink(|_| {}, Default::default());
    let run = Run::new(&mut reporter);
    copy_files(&sizes, &run, copy).await.unwrap();
    // Beside the first large one, not after the others waiting for their slots.
    assert_eq!(large_done_by_small.load(Ordering::SeqCst), 0);
}

/// Files going to one name are copied in their order, the last one ending up there, even when
/// an earlier one waits for a slot that a later one doesn't need.
#[tokio::test]
async fn files_going_to_one_name_are_copied_in_their_order() {
    // A large one held up behind another large one, then a small one of the same name.
    let sizes = [SMALL_FILE + 1, SMALL_FILE + 1, 1];
    let turns = Turns::new(["other", "x", "x"]);
    let order = Mutex::new(Vec::new());
    let copy = |i| {
        let order = &order;
        async move {
            if sizes[i] > SMALL_FILE {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            order.lock().unwrap().push(i);
            Ok(())
        }
    };
    let mut reporter = Reporter::with_sink(|_| {}, Default::default());
    let run = Run::new(&mut reporter);
    copy_files_in_turns(&sizes, &turns, &run, copy).await.unwrap();
    let order = order.into_inner().unwrap();
    let place = |i| order.iter().position(|&j| j == i).unwrap();
    assert!(place(1) < place(2), "{order:?}");
}

#[tokio::test]
async fn a_transfer_waiting_for_its_turn_is_cancelled_at_once() {
    let slots = Arc::new(TransferSlots::default());
    // Another transfer copies files for as long as the test runs.
    let _busy = slots.files.acquire_many(CONCURRENT_FILES as u32).await.unwrap();
    let cancelled = CancellationToken::new();
    let mut reporter = Reporter::with_sink(|_| {}, cancelled.clone()).sharing(slots.clone());
    let run = Run::new(&mut reporter);
    let copy = |_| async { Ok(()) };
    let (result, ()) = tokio::join!(copy_files(&[1], &run, copy), async {
        tokio::task::yield_now().await;
        cancelled.cancel();
    });
    assert_eq!(result.unwrap_err().downcast_ref::<Error>().map(Error::code), Some("transfer.cancelled"));
}

/// Uploads `local_paths` into `remote_dir` in one go, as a transfer that isn't gone on with.
#[cfg(unix)]
async fn upload_all(
    sftp: &russh_sftp::client::SftpSession,
    local_paths: &[PathBuf],
    remote_dir: &str,
    reporter: &mut Reporter,
    left: &mut Left<String>,
) -> anyhow::Result<()> {
    let server = Server::of(&crate::model::Remote::new("localhost".into(), 22, "alice".into()));
    let mut job = upload::Upload::new(local_paths.to_vec(), remote_dir.to_owned(), server, encoding_rs::UTF_8);
    upload::upload(sftp, &mut job, reporter, left, false).await
}

/// Downloads `remote_paths` into `folder` in one go, giving back the names of the items that
/// didn't arrive if it fails, as a transfer that isn't gone on with.
#[cfg(unix)]
async fn download_into(
    sftp: &russh_sftp::client::SftpSession,
    remote_paths: &[String],
    folder: &std::path::Path,
    reporter: &mut Reporter,
    left: &mut Left<PathBuf>,
) -> anyhow::Result<()> {
    let mut job = download::Download::into(remote_paths.to_vec(), folder.to_owned());
    let result = download::download(sftp, &mut job, reporter, left, false).await;
    if result.is_err() {
        job.discard().await;
    }
    result
}

/// Transfers going on after a lost connection, through the SFTP server in this process.
mod resuming {
    use std::path::Path;

    use russh_sftp::client::SftpSession;

    use super::*;
    use crate::session::Viewers;
    use crate::sftp::channels::Channels;
    use crate::sftp::testing::{Counts, SftpServer};
    use crate::ssh::testing::{connect_stalling, Stall};
    use crate::ssh::Connection;

    /// Large enough to be gone on with (see `resume::RESUMABLE`).
    const LARGE: usize = 20 << 20;

    /// `len` bytes that differ with `seed`.
    fn bytes(len: usize, seed: u32) -> Vec<u8> {
        let mut state = seed.wrapping_mul(2_654_435_761).max(1);
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect()
    }

    /// A new connection to an SFTP server serving `root`, and its transfer session.
    async fn connect(root: &Path, counts: &Counts) -> (Arc<Connection>, Stall, Arc<SftpSession>) {
        let (connection, stall) = connect_stalling(SftpServer::new(root, counts), Duration::from_millis(100)).await;
        let sftp = Channels::new(connection.info(), Viewers::default()).transfer(&connection).await.unwrap();
        (connection, stall, sftp)
    }

    /// A file that LSTAT can't tell is there is written as if it were: beside it, and left
    /// alone if the copy fails, rather than truncated and then removed as one the copy made.
    #[tokio::test]
    async fn a_file_lstat_fails_on_is_kept_if_the_copy_fails() {
        let (root, _, remote) = folders("lstat-fails");
        std::fs::write(remote.join("notes.txt"), "mine").unwrap();
        let counts = Counts::default();
        let (_connection, _stall, sftp) = connect(&remote, &counts).await;
        counts.fail_lstat();
        let mut target = crate::sftp::replace::RemoteTarget::create(&sftp, "/notes.txt", None, false).await.unwrap();
        target.file.write_all(b"half").await.unwrap();
        let during = std::fs::read_to_string(remote.join("notes.txt")).unwrap();
        target.abandon(&sftp).await;
        let after = std::fs::read_to_string(remote.join("notes.txt")).ok();
        let left = names(&remote);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(during, "mine");
        assert_eq!(after.as_deref(), Some("mine"));
        assert_eq!(left, ["notes.txt"]);
    }

    /// A folder for a test, with `local` and `remote` folders in it.
    fn folders(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("zshell-{name}-{}", uuid::Uuid::new_v4()));
        let (local, remote) = (root.join("local"), root.join("remote"));
        std::fs::create_dir_all(&local).unwrap();
        std::fs::create_dir_all(&remote).unwrap();
        (root, local, remote)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    /// A reporter, and the last progress it reported.
    fn reporting() -> (Reporter, Arc<Mutex<Progress>>) {
        let last = Arc::new(Mutex::new(Progress::default()));
        let reported = last.clone();
        (Reporter::with_sink(move |progress| *reported.lock().unwrap() = progress, Default::default()), last)
    }

    /// Uploads a large file and a small one on a connection lost midway: the large one isn't
    /// left in place incomplete, and the upload fails as lost.
    async fn upload_cut_short(local: &Path, remote: &Path, counts: &Counts) -> (Job, Kept) {
        std::fs::write(local.join("large.bin"), bytes(LARGE, 1)).unwrap();
        std::fs::write(local.join("small.txt"), "small").unwrap();
        let (connection, stall, sftp) = connect(remote, counts).await;
        counts.stall_after(LARGE as u64 / 2, &stall);
        let server = Server::of(&connection.profile().ssh.remote);
        let mut job = Job::upload(vec![local.join("large.bin"), local.join("small.txt")], "/".into(), server, encoding_rs::UTF_8);
        let mut left = job.nothing_left();
        let (mut reporter, _) = reporting();
        let failed = job.run(&sftp, &mut reporter, &mut left, false).await.unwrap_err();
        assert!(ended_by_loss(&connection, &failed).await, "{failed:#}");
        assert!(!remote.join("large.bin").exists());
        (job, left)
    }

    #[tokio::test]
    async fn an_upload_goes_on_where_the_connection_was_lost() {
        let (root, local, remote) = folders("resume-upload");
        let counts = Counts::default();
        let (mut job, mut left) = upload_cut_short(&local, &remote, &counts).await;
        let before = counts.written();

        let (_connection, _stall, sftp) = connect(&remote, &counts).await;
        let (mut reporter, last) = reporting();
        job.run(&sftp, &mut reporter, &mut left, false).await.unwrap();
        let again = counts.written() - before;
        let arrived = std::fs::read(remote.join("large.bin")).unwrap() == bytes(LARGE, 1);
        let (names, last) = (names(&remote), last.lock().unwrap().clone());
        std::fs::remove_dir_all(&root).unwrap();

        assert!(arrived);
        // Nothing left beside them.
        assert_eq!(names, ["large.bin", "small.txt"]);
        // Went on rather than starting over, and counted each byte once.
        assert!(again < LARGE as u64, "wrote {again} bytes again");
        assert_eq!((last.transferred, last.files_done), (last.total, 2));
        let Kept::Remote(left, _) = left else { unreachable!() };
        assert_eq!(left.files.len(), 2);
    }

    /// Lost as the last writes were to be acknowledged, as the file closes: gone on with too,
    /// rather than written again from the start beside what is left of it.
    #[tokio::test]
    async fn an_upload_lost_as_it_closes_goes_on() {
        let (root, local, remote) = folders("resume-closing");
        let counts = Counts::default();
        std::fs::write(local.join("large.bin"), bytes(LARGE, 1)).unwrap();
        let (connection, stall, sftp) = connect(&remote, &counts).await;
        counts.stall_after(LARGE as u64, &stall);
        let server = Server::of(&connection.profile().ssh.remote);
        let mut job = Job::upload(vec![local.join("large.bin")], "/".into(), server, encoding_rs::UTF_8);
        let mut left = job.nothing_left();
        let (mut reporter, _) = reporting();
        let failed = job.run(&sftp, &mut reporter, &mut left, false).await.unwrap_err();
        let lost = ended_by_loss(&connection, &failed).await;
        let before = counts.written();

        let (_connection, _stall, sftp) = connect(&remote, &counts).await;
        let (mut reporter, _) = reporting();
        let resumed = job.run(&sftp, &mut reporter, &mut left, false).await;
        let again = counts.written() - before;
        let arrived = std::fs::read(remote.join("large.bin")).is_ok_and(|data| data == bytes(LARGE, 1));
        let names = names(&remote);
        std::fs::remove_dir_all(&root).unwrap();

        assert!(lost, "{failed:#}");
        resumed.unwrap();
        assert!(arrived);
        assert_eq!(names, ["large.bin"]);
        assert!(again < LARGE as u64, "wrote {again} bytes again");
    }

    /// A file that lands where a folder is gets a free name (`x (1)`), taken with an empty
    /// file: the download failing removes it, rather than leaving it empty.
    #[tokio::test]
    async fn a_download_given_a_free_name_leaves_nothing_when_it_fails() {
        let (root, local, remote) = folders("claimed");
        std::fs::write(remote.join("x"), bytes(LARGE, 3)).unwrap();
        std::fs::create_dir(local.join("x")).unwrap();
        let counts = Counts::default();
        let (_connection, stall, sftp) = connect(&remote, &counts).await;
        counts.stall_after(LARGE as u64 / 2, &stall);
        let (mut reporter, _) = reporting();
        let failed = download::download_to(&sftp, &[("/x".into(), local.join("x"))], &mut reporter, &mut Left::default()).await;
        let names = names(&local);
        std::fs::remove_dir_all(&root).unwrap();
        assert!(failed.is_err());
        assert_eq!(names, ["x"]);
    }

    #[tokio::test]
    async fn a_file_changed_since_is_copied_again_from_the_start() {
        let (root, local, remote) = folders("resume-changed");
        let counts = Counts::default();
        let (mut job, mut left) = upload_cut_short(&local, &remote, &counts).await;
        std::fs::write(local.join("large.bin"), bytes(LARGE + 1, 2)).unwrap();
        let before = counts.written();

        let (_connection, _stall, sftp) = connect(&remote, &counts).await;
        let (mut reporter, last) = reporting();
        job.run(&sftp, &mut reporter, &mut left, false).await.unwrap();
        let again = counts.written() - before;
        let arrived = std::fs::read(remote.join("large.bin")).unwrap() == bytes(LARGE + 1, 2);
        let (names, last) = (names(&remote), last.lock().unwrap().clone());
        std::fs::remove_dir_all(&root).unwrap();

        assert!(arrived);
        assert_eq!(names, ["large.bin", "small.txt"]);
        assert!(again > LARGE as u64, "wrote {again} bytes again");
        assert_eq!(last.transferred, last.total);
    }

    #[tokio::test]
    async fn a_discarded_upload_leaves_nothing_cut_short() {
        let (root, local, remote) = folders("resume-discard");
        let counts = Counts::default();
        let (job, _) = upload_cut_short(&local, &remote, &counts).await;
        let (_connection, _stall, sftp) = connect(&remote, &counts).await;
        job.discard(Some(&sftp)).await;
        let names = names(&remote);
        std::fs::remove_dir_all(&root).unwrap();
        assert!(names.iter().all(|name| !name.starts_with('.')), "{names:?}");
    }

    #[tokio::test]
    async fn a_download_goes_on_where_the_connection_was_lost() {
        let (root, local, remote) = folders("resume-download");
        std::fs::write(remote.join("large.bin"), bytes(LARGE, 3)).unwrap();
        let counts = Counts::default();
        let (connection, stall, sftp) = connect(&remote, &counts).await;
        counts.stall_after(LARGE as u64 / 2, &stall);
        let mut job = Job::download(vec!["/large.bin".into()], local.clone());
        let mut left = job.nothing_left();
        let (mut reporter, _) = reporting();
        let failed = job.run(&sftp, &mut reporter, &mut left, false).await.unwrap_err();
        assert!(ended_by_loss(&connection, &failed).await, "{failed:#}");
        // Its name is kept, empty, and what arrived is beside it.
        let kept = std::fs::metadata(local.join("large.bin")).map(|metadata| metadata.len()).ok();
        let before = counts.read();

        let (_connection, _stall, sftp) = connect(&remote, &counts).await;
        let (mut reporter, last) = reporting();
        let resumed = job.run(&sftp, &mut reporter, &mut left, false).await;
        let again = counts.read() - before;
        let arrived = std::fs::read(local.join("large.bin")).ok() == Some(bytes(LARGE, 3));
        let (names, last) = (names(&local), last.lock().unwrap().clone());
        let paths = job.paths(&mut left, resumed.is_ok());
        std::fs::remove_dir_all(&root).unwrap();

        resumed.unwrap();
        assert_eq!(kept, Some(0));
        assert!(arrived);
        assert_eq!(names, ["large.bin"]);
        assert!(again < LARGE as u64, "read {again} bytes again");
        assert_eq!(last.transferred, last.total);
        assert_eq!(paths, [local.join("large.bin")]);
    }

    #[tokio::test]
    async fn a_discarded_download_gives_back_its_names() {
        let (root, local, remote) = folders("resume-download-discard");
        std::fs::write(remote.join("large.bin"), bytes(LARGE, 4)).unwrap();
        let counts = Counts::default();
        let (_connection, stall, sftp) = connect(&remote, &counts).await;
        counts.stall_after(LARGE as u64 / 2, &stall);
        let mut job = Job::download(vec!["/large.bin".into()], local.clone());
        let mut left = job.nothing_left();
        let (mut reporter, _) = reporting();
        assert!(job.run(&sftp, &mut reporter, &mut left, false).await.is_err());
        job.discard(None).await;
        let names = names(&local);
        std::fs::remove_dir_all(&root).unwrap();
        assert!(names.is_empty(), "{names:?}");
    }

    /// Retrying copies again only the files copied whole that aren't there as they were left;
    /// going on after a lost connection takes them as they were.
    #[tokio::test]
    async fn a_retry_copies_again_what_is_gone() {
        let (root, local, remote) = folders("resume-retry");
        std::fs::write(local.join("a.txt"), "aaa").unwrap();
        std::fs::write(local.join("b.txt"), "bbbb").unwrap();
        let counts = Counts::default();
        let (connection, _stall, sftp) = connect(&remote, &counts).await;
        let server = Server::of(&connection.profile().ssh.remote);
        let mut job = Job::upload(vec![local.join("a.txt"), local.join("b.txt")], "/".into(), server, encoding_rs::UTF_8);
        let mut left = job.nothing_left();
        let (mut reporter, _) = reporting();
        job.run(&sftp, &mut reporter, &mut left, false).await.unwrap();
        std::fs::remove_file(remote.join("b.txt")).unwrap();

        let before = counts.written();
        job.run(&sftp, &mut reporter, &mut left, false).await.unwrap();
        let trusted = counts.written() - before;
        job.run(&sftp, &mut reporter, &mut left, true).await.unwrap();
        let checked = counts.written() - before;
        let names = names(&remote);
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!((trusted, checked), (0, 4));
        assert_eq!(names, ["a.txt", "b.txt"]);
    }
}

#[cfg(unix)]
mod unix {
    use std::collections::HashSet;
    use std::path::Path;

    use super::super::download::*;
    use super::super::upload::*;
    use super::*;

    #[tokio::test]
    async fn upload_scan_skips_directory_links() {
        let root = std::env::temp_dir().join(format!("zshell-scan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/a.txt"), "abc").unwrap();
        std::os::unix::fs::symlink("..", root.join("sub/loop")).unwrap();
        std::os::unix::fs::symlink("a.txt", root.join("sub/link.txt")).unwrap();

        let mut plan = Plan::new();
        let mut reporter = Reporter::with_sink(|_| {}, Default::default());
        let result = scan_local_dir(&root, "/r".into(), &mut plan, &mut reporter).await;
        std::fs::remove_dir_all(&root).unwrap();
        result.unwrap();

        assert_eq!(plan.dirs, ["/r", "/r/sub"]);
        let mut remote: Vec<_> = plan.files.iter().map(|(_, remote, _)| remote.as_str()).collect();
        remote.sort();
        assert_eq!(remote, ["/r/sub/a.txt", "/r/sub/link.txt"]);
        assert_eq!(reporter.progress.total, 6);
    }

    /// Folders of many files, some small (copied several at once) and one large (alone), go
    /// up and come back whole, into folders that are new and one that was there, and the last
    /// progress counts them all. Skipped without `sftp-server`.
    #[tokio::test]
    async fn copies_folders_of_many_files_both_ways() {
        let Some(sftp) = crate::sftp::replace::tests::local_sftp_server().await else { return };
        let root = std::env::temp_dir().join(format!("zshell-tree-{}", uuid::Uuid::new_v4()));
        let (source, remote, back) = (root.join("tree"), root.join("remote"), root.join("back"));
        let mut expected = Vec::new();
        for dir in ["", "a", "a/b", "c"] {
            std::fs::create_dir_all(source.join(dir)).unwrap();
            for n in 0..20 {
                let path = format!("{dir}/{n}.txt");
                std::fs::write(source.join(path.trim_start_matches('/')), path.repeat(n + 1)).unwrap();
                expected.push(path);
            }
        }
        let large: Vec<u8> = (0..SMALL_FILE as usize * 3).map(|i| (i % 251) as u8).collect();
        std::fs::write(source.join("a/large.bin"), &large).unwrap();
        // Already there: its files are looked for before they are written.
        std::fs::create_dir_all(remote.join("tree/c")).unwrap();
        std::fs::create_dir_all(&back).unwrap();

        let last = Arc::new(Mutex::new(Progress::default()));
        let mut reporter = {
            let last = last.clone();
            Reporter::with_sink(move |progress| *last.lock().unwrap() = progress, Default::default())
        };
        let up = upload_all(&sftp, std::slice::from_ref(&source), remote.to_str().unwrap(), &mut reporter, &mut Left::default()).await;
        let uploaded = last.lock().unwrap().clone();
        let mut reporter = Reporter::with_sink(|_| {}, Default::default());
        let down =
            download_into(&sftp, &[remote.join("tree").to_str().unwrap().to_owned()], &back, &mut reporter, &mut Left::default()).await;
        let same = |path: &str| std::fs::read(source.join(path)).ok() == std::fs::read(back.join("tree").join(path)).ok();
        let all_same = expected.iter().all(|path| same(path.trim_start_matches('/'))) && same("a/large.bin");
        std::fs::remove_dir_all(&root).unwrap();

        up.unwrap();
        down.unwrap();
        assert_eq!((uploaded.files_done, uploaded.files_total), (81, 81));
        assert!(all_same);
    }

    /// What an upload or a download left is what it created: deleting it takes away the
    /// files and folders it made, and leaves those it wrote over, those that were there, those
    /// changed since, and a folder where something else was put since. Skipped without
    /// `sftp-server`.
    #[tokio::test]
    async fn deletes_only_what_a_transfer_created() {
        let Some(sftp) = crate::sftp::replace::tests::local_sftp_server().await else { return };
        let root = std::env::temp_dir().join(format!("zshell-left-{}", uuid::Uuid::new_v4()));
        let (source, remote, back) = (root.join("up"), root.join("remote"), root.join("back"));
        std::fs::create_dir_all(source.join("sub")).unwrap();
        std::fs::create_dir_all(remote.join("up")).unwrap();
        std::fs::create_dir_all(&back).unwrap();
        for file in ["new.txt", "old.txt", "edited.txt", "sub/x.txt"] {
            std::fs::write(source.join(file), file).unwrap();
        }
        std::fs::write(remote.join("up/old.txt"), "old").unwrap();
        std::fs::write(remote.join("up/keep"), "keep").unwrap();
        let names = |dir: &Path| {
            let mut names: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
            names.sort();
            names
        };
        fn paths<P: Clone + Ord>(files: &[(P, Stamp)]) -> Vec<P> {
            let mut paths: Vec<_> = files.iter().map(|(path, _)| path.clone()).collect();
            paths.sort();
            paths
        }
        let at = |path: &Path| path.to_str().unwrap().to_owned();
        // Deleted files and folders, those kept as changed, and whether all went well.
        let counts = |d: &Deletion| (d.deleted, d.deleted_folders, d.changed, d.created + d.folders, d.error.is_none());

        let mut reporter = Reporter::with_sink(|_| {}, Default::default());
        let mut up = Left::default();
        upload_all(&sftp, std::slice::from_ref(&source), &at(&remote), &mut reporter, &mut up).await.unwrap();
        let uploaded = (paths(&up.files), up.dirs.clone(), up.replaced);
        std::fs::write(remote.join("up/edited.txt"), "edited since").unwrap();
        let deletion = delete_remote(&sftp, &mut up).await;
        let after_upload = (counts(&deletion), names(&remote.join("up")), up);

        let mut reporter = Reporter::with_sink(|_| {}, Default::default());
        let mut down = Left::default();
        download_into(&sftp, &[at(&remote.join("up"))], &back, &mut reporter, &mut down).await.unwrap();
        let downloaded = (down.items.clone(), paths(&down.files), down.dirs.clone(), down.replaced);
        std::fs::write(back.join("up/mine"), "mine").unwrap();
        std::fs::write(back.join("up/keep"), "keep, edited since").unwrap();
        let deletion = delete_local(&mut down);
        let after_download = (counts(&deletion), names(&back.join("up")));
        std::fs::remove_dir_all(&root).unwrap();

        let r = |path: &str| at(&remote.join(path));
        assert_eq!(uploaded, (vec![r("up/edited.txt"), r("up/new.txt"), r("up/sub/x.txt")], vec![r("up/sub")], 1));
        let kept = vec!["edited.txt".into(), "keep".into(), "old.txt".into()];
        assert_eq!(after_upload, ((2, 1, 1, 0, true), kept, Left { replaced: 1, ..Left::default() }));
        let b = |path: &str| back.join(path);
        assert_eq!(downloaded, (vec![b("up")], vec![b("up/edited.txt"), b("up/keep"), b("up/old.txt")], vec![b("up")], 0));
        assert_eq!(after_download, ((2, 0, 1, 0, true), vec!["keep".into(), "mine".into()]));
    }

    /// New files and folders get the permission bits of those copied, as with `sftp put` and
    /// `get`: a key stays private, a script executable. Skipped without `sftp-server`.
    #[tokio::test]
    async fn copies_keep_their_permissions_both_ways() {
        use std::os::unix::fs::PermissionsExt;
        let Some(sftp) = crate::sftp::replace::tests::local_sftp_server().await else { return };
        let root = std::env::temp_dir().join(format!("zshell-modes-{}", uuid::Uuid::new_v4()));
        let (source, remote, back) = (root.join("keys"), root.join("remote"), root.join("back"));
        for dir in [&source, &remote, &back] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let set = |path: &Path, mode| std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        let mode = |path: PathBuf| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        std::fs::write(source.join("id"), "key").unwrap();
        std::fs::write(source.join("run.sh"), "true").unwrap();
        set(&source.join("id"), 0o600);
        set(&source.join("run.sh"), 0o755);
        set(&source, 0o700);

        let mut reporter = Reporter::with_sink(|_| {}, Default::default());
        let up = upload_all(&sftp, std::slice::from_ref(&source), remote.to_str().unwrap(), &mut reporter, &mut Left::default()).await;
        let uploaded = up.is_ok().then(|| (mode(remote.join("keys")), mode(remote.join("keys/id")), mode(remote.join("keys/run.sh"))));
        let mut reporter = Reporter::with_sink(|_| {}, Default::default());
        let paths = [remote.join("keys").to_str().unwrap().to_owned(), remote.join("keys/id").to_str().unwrap().to_owned()];
        let down = download_into(&sftp, &paths, &back, &mut reporter, &mut Left::default()).await;
        let downloaded = down.is_ok().then(|| (mode(back.join("keys")), mode(back.join("keys/id")), mode(back.join("id"))));
        set(&source, 0o755);
        set(&remote.join("keys"), 0o755);
        set(&back.join("keys"), 0o755);
        std::fs::remove_dir_all(&root).unwrap();

        up.unwrap();
        down.unwrap();
        assert_eq!(uploaded, Some((0o700, 0o600, 0o755)));
        assert_eq!(downloaded, Some((0o700, 0o600, 0o600)));
    }

    /// A file downloaded to edit is never executable, which opening it would run (an
    /// executable file without an extension opens in Terminal on macOS). Skipped without
    /// `sftp-server`.
    #[tokio::test]
    async fn a_copy_to_edit_is_not_executable() {
        use std::os::unix::fs::PermissionsExt;
        let Some(sftp) = crate::sftp::replace::tests::local_sftp_server().await else { return };
        let root = std::env::temp_dir().join(format!("zshell-edit-mode-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("edit")).unwrap();
        let (remote, local) = (root.join("cleanup"), root.join("edit/cleanup"));
        std::fs::write(&remote, "rm -rf ~/tmp").unwrap();
        std::fs::set_permissions(&remote, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut reporter = Reporter::with_sink(|_| {}, Default::default());
        let result = download::download_to_edit(&sftp, remote.to_str().unwrap(), &local, &mut reporter).await;
        let mode = result.is_ok().then(|| std::fs::metadata(&local).unwrap().permissions().mode() & 0o111);
        std::fs::remove_dir_all(&root).unwrap();

        result.unwrap();
        assert_eq!(mode, Some(0));
    }

    /// A failed download takes back only the names of the items that did not arrive: an
    /// empty file that did stays, and one copied at the same time as the failure stays if it
    /// arrived. Skipped without `sftp-server`.
    #[tokio::test]
    async fn a_failed_download_keeps_the_items_that_arrived() {
        use std::os::unix::fs::PermissionsExt;
        let Some(sftp) = crate::sftp::replace::tests::local_sftp_server().await else { return };
        let root = std::env::temp_dir().join(format!("zshell-download-{}", uuid::Uuid::new_v4()));
        let (remote, local) = (root.join("remote"), root.join("local"));
        std::fs::create_dir_all(&remote).unwrap();
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(remote.join("empty"), "").unwrap();
        std::fs::write(remote.join("secret"), "x").unwrap();
        std::fs::set_permissions(remote.join("secret"), std::fs::Permissions::from_mode(0o000)).unwrap();
        std::fs::write(remote.join("later"), "").unwrap();

        let paths: Vec<String> = ["empty", "secret", "later"].iter().map(|n| remote.join(n).to_str().unwrap().to_owned()).collect();
        let mut reporter = Reporter::with_sink(|_| {}, Default::default());
        let result = download_into(&sftp, &paths, &local, &mut reporter, &mut Left::default()).await;
        let mut left: Vec<_> = std::fs::read_dir(&local).unwrap().map(|e| e.unwrap().file_name()).collect();
        left.sort();
        std::fs::remove_dir_all(&root).unwrap();

        assert!(result.is_err());
        // "later" may have arrived before "secret" failed, or not have started.
        left.retain(|name| name != "later");
        assert_eq!(left, ["empty"]);
    }

    #[test]
    fn files_going_to_one_remote_name_take_turns() {
        let turns = same_name_turns(["/r/x", "/r/x", "/r/X", "/r/y"], encoding_rs::UTF_8);
        assert_eq!((0..4).map(|i| turns.shared(i)).collect::<Vec<_>>(), [true, true, true, false]);
        // Both are `?.txt` on a GBK server.
        let turns = same_name_turns(["/r/😀.txt", "/r/😃.txt", "/r/中.txt"], encoding_rs::GBK);
        assert_eq!((0..3).map(|i| turns.shared(i)).collect::<Vec<_>>(), [true, true, false]);
        let turns = same_name_turns(["/r/😀.txt", "/r/😃.txt"], encoding_rs::UTF_8);
        assert!(!turns.shared(0) && !turns.shared(1));
    }

    #[test]
    fn an_item_arrived_whole_with_all_its_files() {
        // A file, a folder of two, an empty folder, a file after the one that failed.
        let ends = [1, 3, 3, 4];
        assert_eq!(items_whole(&ends, &[true, true, false, true]), [true, false, true, true]);
        assert_eq!(items_whole(&ends, &[false; 4]), [false, false, true, false]);
    }

    #[test]
    fn finds_files_that_land_on_the_same_name() {
        let files: Vec<(String, PathBuf, u64)> =
            ["/d/README", "/d/x", "/d/readme", "/d/README", "/d/Sub"].iter().map(|&r| (r.to_owned(), PathBuf::from(r), 0)).collect();
        for ignores_case in [false, true] {
            let mut taken = Taken::new(ignores_case);
            // A folder took it.
            taken.take(Path::new("/d/sub"));
            assert_eq!(clashing(&files, &mut taken), [false, false, ignores_case, true, ignores_case]);
        }
        let mut taken = Taken::new(true);
        let dirs = ["/d/Dir", "/d/dir", "/d/DIR", "/d/dir (1)"].map(|d| taken.take_free(PathBuf::from(d)));
        assert_eq!(dirs.map(|d| d.to_string_lossy().into_owned()), ["/d/Dir", "/d/dir (1)", "/d/DIR (2)", "/d/dir (1) (1)"]);
    }

    /// A file renamed because its name clashed doesn't take a name that a later file of the
    /// download has on the server (`README`, `readme`, `readme (1)` where case is ignored).
    #[test]
    fn renamed_files_keep_clear_of_the_others() {
        let dir = std::env::temp_dir().join(format!("zshell-clash-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let files: Vec<(String, PathBuf, u64)> =
            ["README", "readme", "readme (1)"].iter().map(|&n| (n.to_owned(), dir.join(n), 0)).collect();
        let mut taken = Taken::new(true);
        assert_eq!(clashing(&files, &mut taken), [false, true, false]);
        std::fs::write(&files[0].1, "").unwrap();
        let (renamed, _) = taken.create_free_file(&files[1].1, None).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(renamed, dir.join("readme (2)"));
    }

    /// The temporary folder's disk is told apart from one that ignores case.
    #[test]
    fn finds_whether_a_disk_ignores_case() {
        let dir = std::env::temp_dir().join(format!("zshell-case-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a"), "").unwrap();
        let expected = dir.join("A").exists();
        // A path that doesn't exist yet is on its nearest folder's disk.
        assert_eq!(ignores_case(&dir.join("new/sub/file")), expected);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Two remote folders that land on one local name each get their own, with what is in
    /// them. Needs a disk that tells case apart (on the server's side), and `sftp-server`.
    #[tokio::test]
    async fn folders_that_land_on_one_name_are_not_merged() {
        let Some(sftp) = crate::sftp::replace::tests::local_sftp_server().await else { return };
        let root = std::env::temp_dir().join(format!("zshell-download-dirs-{}", uuid::Uuid::new_v4()));
        let (remote, local) = (root.join("remote"), root.join("local"));
        std::fs::create_dir_all(remote.join("Dir")).unwrap();
        if remote.join("dir").exists() {
            std::fs::remove_dir_all(&root).unwrap();
            return;
        }
        std::fs::create_dir_all(remote.join("dir")).unwrap();
        std::fs::write(remote.join("Dir/f"), "upper").unwrap();
        std::fs::write(remote.join("dir/f"), "lower").unwrap();
        std::fs::create_dir_all(&local).unwrap();

        let mut plan = Plan::new();
        let mut taken = Taken::new(true);
        let mut reporter = Reporter::with_sink(|_| {}, Default::default());
        let result =
            scan_remote_dir(&sftp, remote.to_str().unwrap().to_owned(), local.join("r"), None, &mut plan, &mut taken, &mut reporter).await;
        std::fs::remove_dir_all(&root).unwrap();
        result.unwrap();

        // Which of the two is `dir (1)` depends on the order the server lists them in.
        let mut dirs: Vec<_> = plan.dirs.iter().map(|d| d.strip_prefix(&local).unwrap().to_string_lossy().to_lowercase()).collect();
        dirs.sort();
        assert_eq!(dirs, ["r", "r/dir", "r/dir (1)"]);
        // Each file follows its folder, so neither replaces the other.
        let files: HashSet<_> = plan.files.iter().map(|(_, local, _)| local.clone()).collect();
        assert_eq!(files.len(), 2);
        assert!(clashing(&plan.files, &mut taken).iter().all(|&clashes| !clashes));
    }
}
