//! Recursive uploads and downloads with progress reporting and cancellation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use russh_sftp::client::SftpSession;
use serde::Serialize;
use tauri::ipc::Channel;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::{file_name, join};
use crate::error::Error;
use crate::local_name::{create_unique, create_unique_file, local_file_name};

const BUFFER_SIZE: usize = 256 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Default, Serialize)]
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
        let mut dst = sftp.create(remote).await.context(Error::new("transfer.createFailed").param("path", remote))?;
        let result = copy(&mut src, &mut dst, reporter).await;
        let _ = dst.shutdown().await;
        if let Err(e) = result {
            let _ = sftp.remove_file(remote).await;
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
pub async fn download(
    sftp: &SftpSession,
    remote_paths: &[String],
    local_dir: &Path,
    reporter: &mut Reporter,
) -> Result<Vec<PathBuf>> {
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
    if result.is_ok() {
        result = download_to(sftp, &items, reporter).await;
    }
    if let Err(e) = result {
        // Give back the names of the items that never arrived; an empty file or folder that
        // did is taken with them.
        for (_, local) in &items {
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
    let mut plan = Plan { dirs: Vec::new(), files: Vec::new() };
    for (remote, local) in items {
        let metadata = sftp.metadata(remote).await.context(Error::new("transfer.readFailed").param("path", remote))?;
        if metadata.file_type().is_dir() {
            scan_remote_dir(sftp, remote.clone(), local.clone(), &mut plan, reporter).await?;
        } else {
            reporter.progress.total += metadata.size.unwrap_or(0);
            plan.files.push((remote.clone(), local.clone()));
        }
    }
    // `+=`: the items of a drag are downloaded one call at a time with the same reporter.
    reporter.progress.files_total += plan.files.len();

    // The folders the items go into (for files given a path of their own), then the remote
    // folders' copies.
    let parents = items.iter().filter_map(|(_, local)| local.parent());
    for dir in parents.chain(plan.dirs.iter().map(PathBuf::as_path)) {
        tokio::fs::create_dir_all(dir).await.context(Error::new("transfer.createDirFailed").param("path", dir.display()))?;
    }
    for (remote, local) in &plan.files {
        reporter.start_file(file_name(remote).to_owned());
        let mut src = sftp.open(remote).await.context(Error::new("transfer.openFailed").param("path", remote))?;
        let mut dst = tokio::fs::File::create(local).await.context(Error::new("transfer.createFailed").param("path", local.display()))?;
        let result = copy(&mut src, &mut dst, reporter).await;
        let _ = src.shutdown().await;
        if let Err(e) = result {
            drop(dst);
            let _ = tokio::fs::remove_file(local).await;
            return Err(with_file_context(e, Error::new("transfer.downloadFailed").param("path", remote)));
        }
        reporter.finish_file();
    }
    Ok(())
}

async fn scan_remote_dir(
    sftp: &SftpSession,
    root: String,
    local_root: PathBuf,
    plan: &mut Plan<String, PathBuf>,
    reporter: &mut Reporter,
) -> Result<()> {
    let mut pending = vec![(root, local_root)];
    while let Some((remote, local)) = pending.pop() {
        reporter.check_cancelled()?;
        plan.dirs.push(local.clone());
        for entry in sftp.read_dir(&remote).await.context(Error::new("transfer.readFailed").param("path", &remote))? {
            let name = entry.file_name();
            if name == "." || name == ".." {
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
                pending.push((child, local.join(&local_name)));
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
}
