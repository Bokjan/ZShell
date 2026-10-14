//! Recursive uploads and downloads with progress reporting and cancellation.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use russh_sftp::client::SftpSession;
use serde::Serialize;
use tauri::ipc::Channel;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use ts_rs::TS;

use super::replace::{LocalTarget, RemoteTarget};
use super::{file_name, is_entry, join};
use crate::error::Error;
use crate::local_name::{create_unique, create_unique_file, local_file_name, numbered};

const BUFFER_SIZE: usize = 256 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Default, Serialize, TS)]
#[ts(rename = "TransferProgress")]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub transferred: u64,
    pub total: u64,
    pub files_done: usize,
    pub files_total: usize,
    pub current: String,
}

/// Cancellation flags of running transfers, keyed by frontend-chosen transfer id.
#[derive(Default)]
pub struct Transfers(Mutex<HashMap<String, Arc<AtomicBool>>>);

impl Transfers {
    pub fn start(&self, id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.0.lock().unwrap().insert(id.to_owned(), flag.clone());
        flag
    }

    pub fn finish(&self, id: &str) {
        self.0.lock().unwrap().remove(id);
    }

    pub fn cancel(&self, id: &str) {
        if let Some(flag) = self.0.lock().unwrap().get(id) {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

pub struct Reporter {
    send: Box<dyn Fn(Progress) + Send + Sync>,
    cancelled: Arc<AtomicBool>,
    progress: Progress,
    last_emit: Instant,
}

impl Reporter {
    pub fn new(channel: Channel<Progress>, cancelled: Arc<AtomicBool>) -> Self {
        Self::with_sink(move |progress| drop(channel.send(progress)), cancelled)
    }

    /// Reports progress to `send` instead of a channel of its own.
    pub fn with_sink(send: impl Fn(Progress) + Send + Sync + 'static, cancelled: Arc<AtomicBool>) -> Self {
        Self { send: Box::new(send), cancelled, progress: Progress::default(), last_emit: Instant::now() }
    }

    fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            bail!(Error::new("transfer.cancelled"));
        }
        Ok(())
    }

    fn start_file(&mut self, name: String) {
        self.progress.current = name;
        self.emit();
    }

    fn advance(&mut self, bytes: u64) {
        self.progress.transferred += bytes;
        if self.last_emit.elapsed() >= PROGRESS_INTERVAL {
            self.emit();
        }
    }

    fn finish_file(&mut self) {
        self.progress.files_done += 1;
        self.emit();
    }

    fn emit(&mut self) {
        self.last_emit = Instant::now();
        (self.send)(self.progress.clone());
    }
}

struct Plan<S, D> {
    /// Directories to create, parents before children.
    dirs: Vec<D>,
    files: Vec<(S, D)>,
}

pub async fn upload(sftp: &SftpSession, local_paths: &[PathBuf], remote_dir: &str, reporter: &mut Reporter) -> Result<()> {
    let mut plan = Plan { dirs: Vec::new(), files: Vec::new() };
    for local in local_paths {
        let name = local.file_name().context(Error::new("transfer.invalidPath").param("path", local.display()))?.to_string_lossy();
        let remote = join(remote_dir, &name);
        let metadata = tokio::fs::metadata(local).await.context(Error::new("transfer.readFailed").param("path", local.display()))?;
        if metadata.is_dir() {
            scan_local_dir(local, remote, &mut plan, reporter).await?;
        } else {
            reporter.progress.total += metadata.len();
            plan.files.push((local.clone(), remote));
        }
    }
    reporter.progress.files_total = plan.files.len();

    for dir in &plan.dirs {
        if !sftp.try_exists(dir).await? {
            sftp.create_dir(dir).await.context(Error::new("transfer.createDirFailed").param("path", dir))?;
        }
    }
    for (local, remote) in &plan.files {
        reporter.start_file(file_name(remote).to_owned());
        let mut src = tokio::fs::File::open(local).await.context(Error::new("transfer.openFailed").param("path", local.display()))?;
        let mut dst = RemoteTarget::create(sftp, remote).await?;
        let result = match copy(&mut src, &mut dst.file, reporter).await {
            Ok(()) => dst.finish(sftp).await,
            Err(e) => {
                dst.abandon(sftp).await;
                Err(e)
            }
        };
        if let Err(e) = result {
            return Err(with_file_context(e, Error::new("transfer.uploadFailed").param("path", local.display())));
        }
        reporter.finish_file();
    }
    Ok(())
}

async fn scan_local_dir(root: &Path, remote_root: String, plan: &mut Plan<PathBuf, String>, reporter: &mut Reporter) -> Result<()> {
    let mut pending = vec![(root.to_path_buf(), remote_root)];
    while let Some((local, remote)) = pending.pop() {
        reporter.check_cancelled()?;
        plan.dirs.push(remote.clone());
        let mut entries = tokio::fs::read_dir(&local).await.context(Error::new("transfer.readFailed").param("path", local.display()))?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let child = join(&remote, &entry.file_name().to_string_lossy());
            // Follow symlinks to files only, as downloads do, so a link back up the tree
            // (`loop -> ..`, Wine's `z: -> /`) can't walk the whole disk. Skip anything that
            // is neither a file nor a directory.
            let Ok(file_type) = entry.file_type().await else { continue };
            let Ok(metadata) = tokio::fs::metadata(&path).await else { continue };
            if file_type.is_symlink() && !metadata.is_file() {
                continue;
            }
            if metadata.is_dir() {
                pending.push((path, child));
            } else if metadata.is_file() {
                reporter.progress.total += metadata.len();
                plan.files.push((path, child));
            }
        }
    }
    Ok(())
}

/// Downloads into `local_dir`; top-level items get a unique name instead of overwriting
/// existing files. Returns the local paths of the top-level items.
pub async fn download(sftp: &SftpSession, remote_paths: &[String], local_dir: &Path, reporter: &mut Reporter) -> Result<Vec<PathBuf>> {
    let mut items = Vec::with_capacity(remote_paths.len());
    let mut result = Ok(());
    for remote in remote_paths {
        match claim(sftp, remote, local_dir).await {
            Ok(local) => items.push((remote.clone(), local)),
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    let mut done = 0;
    if result.is_ok() {
        result = download_counting(sftp, &items, reporter, &mut done).await;
    }
    if let Err(e) = result {
        // Give back the names of the items that never arrived. Those that did stay, even an
        // empty file or folder.
        for (_, local) in &items[done..] {
            let _ = match std::fs::symlink_metadata(local) {
                Ok(metadata) if metadata.is_dir() => std::fs::remove_dir(local),
                Ok(metadata) if metadata.len() == 0 => std::fs::remove_file(local),
                _ => Ok(()),
            };
        }
        return Err(e);
    }
    Ok(items.into_iter().map(|(_, local)| local).collect())
}

/// Takes a free local name for the remote item, creating an empty file or folder there.
async fn claim(sftp: &SftpSession, remote: &str, local_dir: &Path) -> Result<PathBuf> {
    let name = local_file_name(file_name(remote)).ok_or_else(|| Error::new("transfer.invalidName").param("name", remote))?;
    let metadata = sftp.metadata(remote).await.context(Error::new("transfer.readFailed").param("path", remote))?;
    let path = local_dir.join(name);
    let created = if metadata.file_type().is_dir() {
        create_unique(&path, |path| std::fs::create_dir(path))
    } else {
        create_unique_file(&path).map(|(path, _)| (path, ()))
    };
    let (path, ()) = created.context(Error::new("transfer.createFailed").param("path", path.display()))?;
    Ok(path)
}

/// Downloads each remote file or directory to the given local path, replacing files there.
pub async fn download_to(sftp: &SftpSession, items: &[(String, PathBuf)], reporter: &mut Reporter) -> Result<()> {
    download_counting(sftp, items, reporter, &mut 0).await
}

/// [`download_to`], keeping in `done` how many of the items, from the first, have arrived
/// whole.
async fn download_counting(sftp: &SftpSession, items: &[(String, PathBuf)], reporter: &mut Reporter, done: &mut usize) -> Result<()> {
    let mut plan = Plan { dirs: Vec::new(), files: Vec::new() };
    // The items of one download go to one folder, or one disk at least.
    let mut taken = Taken::new(items.first().is_some_and(|(_, local)| ignores_case(local)));
    // Where each item's files end in `plan.files`.
    let mut ends = Vec::with_capacity(items.len());
    for (remote, local) in items {
        let metadata = sftp.metadata(remote).await.context(Error::new("transfer.readFailed").param("path", remote))?;
        if metadata.file_type().is_dir() {
            scan_remote_dir(sftp, remote.clone(), local.clone(), &mut plan, &mut taken, reporter).await?;
        } else {
            reporter.progress.total += metadata.size.unwrap_or(0);
            plan.files.push((remote.clone(), local.clone()));
        }
        ends.push(plan.files.len());
    }
    // `+=`: the items of a drag are downloaded one call at a time with the same reporter.
    reporter.progress.files_total += plan.files.len();

    // The folders the items go into (for files given a path of their own), then the remote
    // folders' copies.
    let parents = items.iter().filter_map(|(_, local)| local.parent());
    for dir in parents.chain(plan.dirs.iter().map(PathBuf::as_path)) {
        tokio::fs::create_dir_all(dir).await.context(Error::new("transfer.createDirFailed").param("path", dir.display()))?;
    }
    for (i, ((remote, local), clashes)) in plan.files.iter().zip(clashing(&plan.files, &mut taken)).enumerate() {
        *done = ends.partition_point(|&end| end <= i);
        reporter.start_file(file_name(remote).to_owned());
        let mut src = sftp.open(remote).await.context(Error::new("transfer.openFailed").param("path", remote))?;
        // A file that would land where another of this download (or a folder) did, or where
        // a folder is, gets a free name instead.
        let local = if clashes || local.is_dir() {
            let (path, _) = create_unique_file(local).context(Error::new("transfer.createFailed").param("path", local.display()))?;
            path
        } else {
            local.clone()
        };
        let mut dst = LocalTarget::create(&local).await?;
        let result = copy(&mut src, &mut dst.file, reporter).await;
        let _ = src.shutdown().await;
        let result = match result {
            Ok(()) => dst.finish().await,
            Err(e) => {
                dst.abandon().await;
                Err(e)
            }
        };
        if let Err(e) = result {
            return Err(with_file_context(e, Error::new("transfer.downloadFailed").param("path", remote)));
        }
        reporter.finish_file();
    }
    *done = items.len();
    Ok(())
}

/// The local paths a download has taken, as the local file system tells names apart: two
/// remote names can land on one (`README` and `readme` where case is ignored; `a:b` and `a?b`
/// on Windows, where `local_file_name` makes both `a_b`).
struct Taken {
    ignores_case: bool,
    keys: HashSet<String>,
}

impl Taken {
    fn new(ignores_case: bool) -> Self {
        Self { ignores_case, keys: HashSet::new() }
    }

    /// Takes `path`; false if it was taken already.
    fn take(&mut self, path: &Path) -> bool {
        let name = path.to_string_lossy();
        self.keys.insert(if self.ignores_case { name.to_lowercase() } else { name.into_owned() })
    }

    /// Takes `path`, or `name (1)`, `name (2)`, … if it is taken.
    fn take_free(&mut self, path: PathBuf) -> PathBuf {
        (0..).map(|n| numbered(&path, n)).find(|candidate| self.take(candidate)).unwrap()
    }
}

/// Whether the file system `path` is on (or would be on) ignores case: macOS and Windows
/// disks usually do, but not always, and Linux can mount ones that do (exFAT, NTFS, SMB).
/// Found out with a file of our own in the nearest existing folder; when that can't be
/// written, as the system usually is.
fn ignores_case(path: &Path) -> bool {
    let usual = cfg!(any(windows, target_os = "macos"));
    let Some(dir) = path.ancestors().skip(1).find(|dir| dir.is_dir()) else {
        return usual;
    };
    let name = format!(".zshell-case-{}", uuid::Uuid::new_v4().simple());
    let probe = dir.join(&name);
    if std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).is_err() {
        return usual;
    }
    let ignores = dir.join(name.to_uppercase()).exists();
    let _ = std::fs::remove_file(&probe);
    ignores
}

/// Which of a download's files would land on a path taken before them, by a folder or a file.
fn clashing(files: &[(String, PathBuf)], taken: &mut Taken) -> Vec<bool> {
    files.iter().map(|(_, local)| !taken.take(local)).collect()
}

/// Lists a remote folder's files and folders, recursively, into `plan`. Two folders that would
/// land on one local path (see `Taken`) get one each, rather than being merged: the second is
/// named `name (1)`, and what is in it goes there.
async fn scan_remote_dir(
    sftp: &SftpSession,
    root: String,
    local_root: PathBuf,
    plan: &mut Plan<String, PathBuf>,
    taken: &mut Taken,
    reporter: &mut Reporter,
) -> Result<()> {
    // Its name was chosen by the caller.
    taken.take(&local_root);
    let mut pending = vec![(root, local_root)];
    while let Some((remote, local)) = pending.pop() {
        reporter.check_cancelled()?;
        plan.dirs.push(local.clone());
        for entry in sftp.read_dir(&remote).await.context(Error::new("transfer.readFailed").param("path", &remote))? {
            let name = entry.file_name();
            if !is_entry(&name) {
                continue;
            }
            let child = join(&remote, &name);
            // The server chooses the names: one that is not a plain file name here (`../x`,
            // or `a\\b` on Windows) must not lead outside the folder.
            let Some(local_name) = local_file_name(&name) else {
                continue;
            };
            let mut metadata = entry.metadata();
            if metadata.file_type().is_symlink() {
                // Follow symlinks to files only, so a link cycle can't recurse forever.
                match sftp.metadata(&child).await {
                    Ok(target) if target.file_type().is_file() => metadata = target,
                    _ => continue,
                }
            }
            if metadata.file_type().is_dir() {
                pending.push((child, taken.take_free(local.join(&local_name))));
            } else if metadata.file_type().is_file() {
                reporter.progress.total += metadata.size.unwrap_or(0);
                plan.files.push((child, local.join(&local_name)));
            }
        }
    }
    Ok(())
}

async fn copy<R, W>(src: &mut R, dst: &mut W, reporter: &mut Reporter) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0; BUFFER_SIZE];
    loop {
        reporter.check_cancelled()?;
        let n = src.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n]).await?;
        reporter.advance(n as u64);
    }
    dst.flush().await?;
    Ok(())
}

/// Adds which file failed, except for cancellation, which is reported as is.
fn with_file_context(e: anyhow::Error, context: Error) -> anyhow::Error {
    if e.downcast_ref::<Error>().is_some_and(|e| e.code() == "transfer.cancelled") {
        e
    } else {
        e.context(context)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upload_scan_skips_directory_links() {
        let root = std::env::temp_dir().join(format!("zshell-scan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/a.txt"), "abc").unwrap();
        std::os::unix::fs::symlink("..", root.join("sub/loop")).unwrap();
        std::os::unix::fs::symlink("a.txt", root.join("sub/link.txt")).unwrap();

        let mut plan = Plan { dirs: Vec::new(), files: Vec::new() };
        let mut reporter = Reporter::with_sink(|_| {}, Arc::default());
        let result = scan_local_dir(&root, "/r".into(), &mut plan, &mut reporter).await;
        std::fs::remove_dir_all(&root).unwrap();
        result.unwrap();

        assert_eq!(plan.dirs, ["/r", "/r/sub"]);
        let mut remote: Vec<_> = plan.files.iter().map(|(_, remote)| remote.as_str()).collect();
        remote.sort();
        assert_eq!(remote, ["/r/sub/a.txt", "/r/sub/link.txt"]);
        assert_eq!(reporter.progress.total, 6);
    }

    /// A failed download takes back only the names of the items that did not arrive: an
    /// empty file before the failure stays. Skipped without `sftp-server`.
    #[tokio::test]
    async fn a_failed_download_keeps_the_items_that_arrived() {
        use std::os::unix::fs::PermissionsExt;
        let Some(sftp) = super::super::replace::tests::local_sftp_server().await else { return };
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
        assert_eq!(left, ["empty"]);
    }

    #[test]
    fn finds_files_that_land_on_the_same_name() {
        let files: Vec<(String, PathBuf)> =
            ["/d/README", "/d/x", "/d/readme", "/d/README", "/d/Sub"].iter().map(|&r| (r.to_owned(), PathBuf::from(r))).collect();
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
        let Some(sftp) = super::super::replace::tests::local_sftp_server().await else { return };
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

        let mut plan = Plan { dirs: Vec::new(), files: Vec::new() };
        let mut taken = Taken::new(true);
        let mut reporter = Reporter::with_sink(|_| {}, Arc::default());
        let result =
            scan_remote_dir(&sftp, remote.to_str().unwrap().to_owned(), local.join("r"), &mut plan, &mut taken, &mut reporter).await;
        std::fs::remove_dir_all(&root).unwrap();
        result.unwrap();

        // Which of the two is `dir (1)` depends on the order the server lists them in.
        let mut dirs: Vec<_> = plan.dirs.iter().map(|d| d.strip_prefix(&local).unwrap().to_string_lossy().to_lowercase()).collect();
        dirs.sort();
        assert_eq!(dirs, ["r", "r/dir", "r/dir (1)"]);
        // Each file follows its folder, so neither replaces the other.
        let files: HashSet<_> = plan.files.iter().map(|(_, local)| local.clone()).collect();
        assert_eq!(files.len(), 2);
        assert!(clashing(&plan.files, &mut taken).iter().all(|&clashes| !clashes));
    }
}
