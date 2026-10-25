//! What a transfer left behind, kept once it ends (done, failed or cancelled) so that the
//! transfers list can delete it again. Only what it created: a file it wrote over is counted
//! but never deleted, as the original is gone already, and a folder it created is deleted only
//! if nothing else is in it once its files are. A file changed since the transfer left it (by
//! a later transfer to the same place, or an editor) isn't its own any more and is kept too:
//! each file is deleted only while its size and modification time are those it was left with.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use futures_util::{stream, StreamExt};
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, FileType, StatusCode};
use serde::Serialize;
use ts_rs::TS;

use super::CONCURRENT_DIRS;
use crate::error::Error;
use crate::model::{host_port, Remote};

/// How many files an upload looks up at once to stamp them, once copied: one request each, many
/// at a time so that they add little to it.
const CONCURRENT_STAMPS: usize = 32;

/// What a file was like when a transfer left it, which it is deleted only while it still is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub(super) size: u64,
    pub(super) modified: Option<SystemTime>,
}

impl Stamp {
    /// A local file's; none for a folder or a link.
    pub fn local(metadata: &std::fs::Metadata) -> Option<Self> {
        metadata.is_file().then(|| Self { size: metadata.len(), modified: metadata.modified().ok() })
    }

    /// A remote file's, to the second; none for a folder or a link, or without a size.
    pub fn remote(attributes: &FileAttributes) -> Option<Self> {
        if matches!(attributes.file_type(), FileType::Dir | FileType::Symlink) {
            return None;
        }
        let modified = attributes.mtime.map(|seconds| SystemTime::UNIX_EPOCH + Duration::from_secs(seconds.into()));
        Some(Self { size: attributes.size?, modified })
    }
}

/// What a transfer created, on this computer (`PathBuf`) or the server (`String`).
#[derive(Debug, Default, PartialEq)]
pub struct Left<P> {
    /// The items it was given that are there, as they were named: what the list shows.
    pub items: Vec<P>,
    /// The files it copied whole where there was none, as it left them.
    pub files: Vec<(P, Stamp)>,
    /// The folders it created.
    pub dirs: Vec<P>,
    /// How many files it wrote over.
    pub replaced: usize,
}

impl<P> Left<P> {
    fn is_empty(&self) -> bool {
        self.files.is_empty() && self.dirs.is_empty()
    }
}

/// The server an upload went to: what it left is deleted there only, though the tab may have
/// been connected elsewhere since (its session edited, then connected again).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    host: String,
    port: u16,
    username: String,
    jump_hosts: Vec<String>,
    proxy: Option<String>,
}

impl Server {
    /// Where a connection goes, as its session was when connecting.
    pub fn of(remote: &Remote) -> Self {
        Self {
            host: remote.host.clone(),
            port: remote.port,
            username: remote.username.clone(),
            jump_hosts: remote.jump_hosts.clone(),
            proxy: remote.proxy.clone(),
        }
    }

    /// `user@host:port`, for messages.
    pub fn name(&self) -> String {
        format!("{}@{}", self.username, host_port(&self.host, self.port))
    }
}

/// What a transfer left, kept by [`super::Transfers`] for `transfer_delete`.
pub enum Kept {
    Local(Left<PathBuf>),
    Remote(Left<String>, Server),
}

impl Kept {
    /// Whether there is nothing to delete.
    pub fn is_empty(&self) -> bool {
        match self {
            Kept::Local(left) => left.is_empty(),
            Kept::Remote(left, _) => left.is_empty(),
        }
    }
}

/// How a transfer ended, for the file panel.
#[derive(Serialize, TS)]
#[ts(rename = "TransferEnd")]
#[serde(rename_all = "camelCase")]
pub struct Ended {
    /// The local items it left, to show (downloads).
    pub paths: Vec<PathBuf>,
    /// How many files and folders it created, which `transfer_delete` deletes (the folders
    /// once nothing else is in them).
    pub created: usize,
    pub folders: usize,
    /// How many files it wrote over, which it doesn't.
    pub replaced: usize,
    /// Why it stopped, if it didn't finish.
    pub error: Option<Error>,
}

impl Ended {
    pub fn new<P>(left: &Left<P>, error: Option<Error>, paths: Vec<PathBuf>) -> Self {
        Self { paths, created: left.files.len(), folders: left.dirs.len(), replaced: left.replaced, error }
    }
}

/// What deleting what a transfer left did, for the file panel.
#[derive(Debug, Default, Serialize, TS)]
#[ts(rename = "TransferDeletion")]
#[serde(rename_all = "camelCase")]
pub struct Deletion {
    /// How many files and folders it deleted.
    pub deleted: usize,
    pub deleted_folders: usize,
    /// How many files it kept, changed since the transfer.
    pub changed: usize,
    /// How many files and folders are still to delete, after a failure (as in [`Ended`]).
    pub created: usize,
    pub folders: usize,
    /// Why it stopped; what is still to delete can be deleted again.
    pub error: Option<Error>,
}

impl Deletion {
    /// What is still to delete, once `left` has been through deleting.
    pub fn with_left<P>(self, left: &Left<P>) -> Self {
        Self { created: left.files.len(), folders: left.dirs.len(), ..self }
    }
}

/// What became of one file.
enum Fate {
    Deleted,
    /// Changed since: kept.
    Changed,
    /// No longer there.
    Gone,
}

/// The files an upload created, with their stamps; one that can't be looked up is left out,
/// and kept.
pub async fn stamp_remote(sftp: &SftpSession, files: Vec<String>) -> Vec<(String, Stamp)> {
    let paths = &files;
    // By index: closures given references make futures the compiler can't prove `Send`.
    let stamps: Vec<Option<Stamp>> = stream::iter(0..paths.len())
        .map(|i| async move { sftp.symlink_metadata(&paths[i]).await.ok().and_then(|attributes| Stamp::remote(&attributes)) })
        .buffered(CONCURRENT_STAMPS)
        .collect()
        .await;
    files.into_iter().zip(stamps).filter_map(|(file, stamp)| Some((file, stamp?))).collect()
}

/// Deletes the local files and then the folders of `left`, deepest first, those that are
/// empty. A file changed since is kept and passed over, as is a file or folder no longer there
/// and a folder where one of the files was; one that can't be deleted stays in `left`, and the
/// first such failure is given.
pub fn delete_local(left: &mut Left<PathBuf>) -> Deletion {
    let mut deletion = Deletion::default();
    let mut first = Ok(());
    left.files.retain(|(file, stamp)| {
        let fate = match std::fs::symlink_metadata(file) {
            Ok(metadata) if Stamp::local(&metadata) != Some(*stamp) => Ok(Fate::Changed),
            Ok(_) => std::fs::remove_file(file).map(|()| Fate::Deleted),
            Err(e) => Err(e),
        };
        match fate {
            Ok(fate) => deletion.count(fate),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                if first.is_ok() {
                    first = Err(Error::new("transfer.deleteFailed").param("path", file.display()).detail(e));
                }
                return true;
            }
        }
        false
    });
    if first.is_ok() {
        left.dirs.sort_by_key(|dir| std::cmp::Reverse(depth(dir)));
        for dir in left.dirs.drain(..) {
            deletion.deleted_folders += usize::from(std::fs::remove_dir(dir).is_ok());
        }
    }
    Deletion { error: first.err(), ..deletion }.with_left(left)
}

impl Deletion {
    fn count(&mut self, fate: Fate) {
        match fate {
            Fate::Deleted => self.deleted += 1,
            Fate::Changed => self.changed += 1,
            Fate::Gone => {}
        }
    }
}

fn depth(path: &Path) -> usize {
    path.components().count()
}

/// [`delete_local`] on the server, a few files at once.
pub async fn delete_remote(sftp: &SftpSession, left: &mut Left<String>) -> Deletion {
    let files = &left.files;
    let gone = |e: &SftpError| matches!(e, SftpError::Status(status) if status.status_code == StatusCode::NoSuchFile);
    // By index: closures given references make futures the compiler can't prove `Send`.
    let results: Vec<Result<Fate, SftpError>> = stream::iter(0..files.len())
        .map(|i| async move {
            let (file, stamp) = &files[i];
            match sftp.symlink_metadata(file).await {
                Ok(attributes) if Stamp::remote(&attributes) != Some(*stamp) => return Ok(Fate::Changed),
                Ok(_) => {}
                Err(e) if gone(&e) => return Ok(Fate::Gone),
                Err(e) => return Err(e),
            }
            match sftp.remove_file(file).await {
                Ok(()) => Ok(Fate::Deleted),
                Err(e) if gone(&e) => Ok(Fate::Gone),
                Err(e) => Err(e),
            }
        })
        .buffered(CONCURRENT_DIRS)
        .collect()
        .await;
    let mut deletion = Deletion::default();
    let mut first = Ok(());
    let mut results = results.into_iter();
    left.files.retain(|(file, _)| match results.next().expect("one result for each file") {
        Ok(fate) => {
            deletion.count(fate);
            false
        }
        Err(e) => {
            if first.is_ok() {
                first = Err(Error::new("transfer.deleteFailed").param("path", file).detail(e));
            }
            true
        }
    });
    if first.is_ok() {
        left.dirs.sort_by_key(|dir| std::cmp::Reverse(dir.matches('/').count()));
        for dir in left.dirs.drain(..) {
            deletion.deleted_folders += usize::from(sftp.remove_dir(&dir).await.is_ok());
        }
    }
    Deletion { error: first.err(), ..deletion }.with_left(left)
}
