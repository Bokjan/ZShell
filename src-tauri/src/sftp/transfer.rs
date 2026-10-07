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
    channel: Channel<Progress>,
    cancelled: Arc<AtomicBool>,
    progress: Progress,
    last_emit: Instant,
}

impl Reporter {
    pub fn new(channel: Channel<Progress>, cancelled: Arc<AtomicBool>) -> Self {
        Self { channel, cancelled, progress: Progress::default(), last_emit: Instant::now() }
    }

    fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            bail!("已取消");
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
        let _ = self.channel.send(self.progress.clone());
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
        let name = local.file_name().context("无效的本地路径")?.to_string_lossy();
        let remote = join(remote_dir, &name);
        let metadata = tokio::fs::metadata(local).await.with_context(|| format!("无法读取 {}", local.display()))?;
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
            sftp.create_dir(dir).await.with_context(|| format!("无法创建目录 {dir}"))?;
        }
    }
    for (local, remote) in &plan.files {
        reporter.start_file(file_name(remote).to_owned());
        let mut src = tokio::fs::File::open(local).await.with_context(|| format!("无法打开 {}", local.display()))?;
        let mut dst = sftp.create(remote).await.with_context(|| format!("无法创建 {remote}"))?;
        let result = copy(&mut src, &mut dst, reporter).await;
        let _ = dst.shutdown().await;
        if let Err(e) = result {
            let _ = sftp.remove_file(remote).await;
            return Err(e.context(format!("上传 {} 失败", local.display())));
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
        let mut entries = tokio::fs::read_dir(&local).await.with_context(|| format!("无法读取 {}", local.display()))?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let child = join(&remote, &entry.file_name().to_string_lossy());
            // Follow symlinks; skip anything that is neither a file nor a directory.
            let Ok(metadata) = tokio::fs::metadata(&path).await else { continue };
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
    let mut plan = Plan { dirs: Vec::new(), files: Vec::new() };
    let mut roots = Vec::new();
    for remote in remote_paths {
        let metadata = sftp.metadata(remote).await.with_context(|| format!("无法读取 {remote}"))?;
        let local = unique_path(local_dir.join(file_name(remote)));
        if metadata.file_type().is_dir() {
            scan_remote_dir(sftp, remote.clone(), local.clone(), &mut plan, reporter).await?;
        } else {
            reporter.progress.total += metadata.size.unwrap_or(0);
            plan.files.push((remote.clone(), local.clone()));
        }
        roots.push(local);
    }
    reporter.progress.files_total = plan.files.len();

    for dir in &plan.dirs {
        tokio::fs::create_dir_all(dir).await.with_context(|| format!("无法创建目录 {}", dir.display()))?;
    }
    for (remote, local) in &plan.files {
        reporter.start_file(file_name(remote).to_owned());
        let mut src = sftp.open(remote).await.with_context(|| format!("无法打开 {remote}"))?;
        let mut dst = tokio::fs::File::create(local).await.with_context(|| format!("无法创建 {}", local.display()))?;
        let result = copy(&mut src, &mut dst, reporter).await;
        let _ = src.shutdown().await;
        if let Err(e) = result {
            drop(dst);
            let _ = tokio::fs::remove_file(local).await;
            return Err(e.context(format!("下载 {remote} 失败")));
        }
        reporter.finish_file();
    }
    Ok(roots)
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
        for entry in sftp.read_dir(&remote).await.with_context(|| format!("无法读取 {remote}"))? {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let child = join(&remote, &name);
            let mut metadata = entry.metadata();
            if metadata.file_type().is_symlink() {
                // Follow symlinks to files only, so a link cycle can't recurse forever.
                match sftp.metadata(&child).await {
                    Ok(target) if target.file_type().is_file() => metadata = target,
                    _ => continue,
                }
            }
            if metadata.file_type().is_dir() {
                pending.push((child, local.join(&name)));
            } else if metadata.file_type().is_file() {
                reporter.progress.total += metadata.size.unwrap_or(0);
                plan.files.push((child, local.join(&name)));
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

/// `name.ext` → `name (1).ext`, `name (2).ext`, … until the path is free.
fn unique_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (1..)
        .map(|n| path.with_file_name(format!("{stem} ({n}){ext}")))
        .find(|candidate| !candidate.exists())
        .unwrap()
}
