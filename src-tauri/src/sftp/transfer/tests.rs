use super::*;

#[test]
fn a_transfer_is_cancelled_only_by_its_page_or_with_it() {
    let transfers = Transfers::default();
    let mine = transfers.start("main", "t1");
    let other = transfers.start("window-2", "t1");
    transfers.cancel("window-2", "t2");
    assert!(!mine.load(Ordering::Relaxed) && !other.load(Ordering::Relaxed));
    transfers.cancel("window-2", "t1");
    assert!(!mine.load(Ordering::Relaxed) && other.load(Ordering::Relaxed));

    let later = transfers.start("main", "t2");
    transfers.cancel_webview("main");
    assert!(mine.load(Ordering::Relaxed) && later.load(Ordering::Relaxed));
    transfers.finish("window-2", "t1");
    assert!(transfers.0.lock().unwrap().is_empty());
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
    let mut first = Reporter::with_sink(|_| {}, Arc::default()).sharing(slots.clone());
    let mut second = Reporter::with_sink(|_| {}, Arc::default()).sharing(slots);
    let (one, two) = (Run::new(&mut first), Run::new(&mut second));
    let (a, b) = tokio::join!(copy_files(&small, &one, copy), copy_files(&small, &two, copy));
    a.unwrap();
    b.unwrap();
    assert_eq!(most.load(Ordering::SeqCst), CONCURRENT_FILES);
}

#[cfg(unix)]
mod unix {
    use super::*;

    #[tokio::test]
    async fn upload_scan_skips_directory_links() {
        let root = std::env::temp_dir().join(format!("zshell-scan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/a.txt"), "abc").unwrap();
        std::os::unix::fs::symlink("..", root.join("sub/loop")).unwrap();
        std::os::unix::fs::symlink("a.txt", root.join("sub/link.txt")).unwrap();

        let mut plan = Plan::new();
        let mut reporter = Reporter::with_sink(|_| {}, Arc::default());
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
            Reporter::with_sink(move |progress| *last.lock().unwrap() = progress, Arc::default())
        };
        let up = upload(&sftp, std::slice::from_ref(&source), remote.to_str().unwrap(), &mut reporter).await;
        let uploaded = last.lock().unwrap().clone();
        let mut reporter = Reporter::with_sink(|_| {}, Arc::default());
        let down = download(&sftp, &[remote.join("tree").to_str().unwrap().to_owned()], &back, &mut reporter).await;
        let same = |path: &str| std::fs::read(source.join(path)).ok() == std::fs::read(back.join("tree").join(path)).ok();
        let all_same = expected.iter().all(|path| same(path.trim_start_matches('/'))) && same("a/large.bin");
        std::fs::remove_dir_all(&root).unwrap();

        up.unwrap();
        down.unwrap();
        assert_eq!((uploaded.files_done, uploaded.files_total), (81, 81));
        assert!(all_same);
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

        let mut reporter = Reporter::with_sink(|_| {}, Arc::default());
        let up = upload(&sftp, std::slice::from_ref(&source), remote.to_str().unwrap(), &mut reporter).await;
        let uploaded = up.is_ok().then(|| (mode(remote.join("keys")), mode(remote.join("keys/id")), mode(remote.join("keys/run.sh"))));
        let mut reporter = Reporter::with_sink(|_| {}, Arc::default());
        let paths = [remote.join("keys").to_str().unwrap().to_owned(), remote.join("keys/id").to_str().unwrap().to_owned()];
        let down = download(&sftp, &paths, &back, &mut reporter).await;
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
        let mut reporter = Reporter::with_sink(|_| {}, Arc::default());
        let result = download(&sftp, &paths, &local, &mut reporter).await;
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
        let mut reporter = Reporter::with_sink(|_| {}, Arc::default());
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
