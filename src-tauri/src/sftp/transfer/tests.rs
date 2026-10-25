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
        let up = upload(&sftp, std::slice::from_ref(&source), remote.to_str().unwrap(), &mut reporter, &mut Left::default()).await;
        let uploaded = last.lock().unwrap().clone();
        let mut reporter = Reporter::with_sink(|_| {}, Default::default());
        let down = download(&sftp, &[remote.join("tree").to_str().unwrap().to_owned()], &back, &mut reporter, &mut Left::default()).await;
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
        upload(&sftp, std::slice::from_ref(&source), &at(&remote), &mut reporter, &mut up).await.unwrap();
        let uploaded = (paths(&up.files), up.dirs.clone(), up.replaced);
        std::fs::write(remote.join("up/edited.txt"), "edited since").unwrap();
        let deletion = delete_remote(&sftp, &mut up).await;
        let after_upload = (counts(&deletion), names(&remote.join("up")), up);

        let mut reporter = Reporter::with_sink(|_| {}, Default::default());
        let mut down = Left::default();
        download(&sftp, &[at(&remote.join("up"))], &back, &mut reporter, &mut down).await.unwrap();
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
        let up = upload(&sftp, std::slice::from_ref(&source), remote.to_str().unwrap(), &mut reporter, &mut Left::default()).await;
        let uploaded = up.is_ok().then(|| (mode(remote.join("keys")), mode(remote.join("keys/id")), mode(remote.join("keys/run.sh"))));
        let mut reporter = Reporter::with_sink(|_| {}, Default::default());
        let paths = [remote.join("keys").to_str().unwrap().to_owned(), remote.join("keys/id").to_str().unwrap().to_owned()];
        let down = download(&sftp, &paths, &back, &mut reporter, &mut Left::default()).await;
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
        let result = download(&sftp, &paths, &local, &mut reporter, &mut Left::default()).await;
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
        let files: Vec<(PathBuf, String, u64)> =
            [("/a/x", "/r/x"), ("/b/x", "/r/x"), ("/c/X", "/r/X"), ("/a/y", "/r/y")].map(|(l, r)| (l.into(), r.into(), 0)).into();
        let turns = same_name_turns(&files);
        assert_eq!(turns.keys().collect::<Vec<_>>(), ["/r/x"]);
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
