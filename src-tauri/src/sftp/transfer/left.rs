//! What a transfer left behind, kept once it ends (done, failed or cancelled) so that the
//! transfers list can delete it again. Only what it created: a file it wrote over is counted
//! but never deleted, as the original is gone already, and a folder it created is deleted only
//! if nothing else is in it once its files are.

use std::path::{Path, PathBuf};

use futures_util::{stream, StreamExt};
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::StatusCode;
use serde::Serialize;
use ts_rs::TS;

use super::CONCURRENT_DIRS;
use crate::error::Error;

/// What a transfer created, on this computer (`PathBuf`) or the server (`String`).
#[derive(Debug, Default, PartialEq)]
pub struct Left<P> {
    /// The items it was given that are there, as they were named: what the list shows.
    pub items: Vec<P>,
    /// The files it copied whole where there was none.
    pub files: Vec<P>,
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

/// What a transfer left, kept by [`super::Transfers`] for `transfer_delete`.
pub enum Kept {
    Local(Left<PathBuf>),
    Remote(Left<String>),
}

impl Kept {
    /// Whether there is nothing to delete.
    pub fn is_empty(&self) -> bool {
        match self {
            Kept::Local(left) => left.is_empty(),
            Kept::Remote(left) => left.is_empty(),
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
    /// How many files it created, which `transfer_delete` deletes.
    pub created: usize,
    /// How many files it wrote over, which it doesn't.
    pub replaced: usize,
    /// Why it stopped, if it didn't finish.
    pub error: Option<Error>,
}

impl Ended {
    pub fn new<P>(left: &Left<P>, error: Option<Error>, paths: Vec<PathBuf>) -> Self {
        Self { paths, created: left.files.len(), replaced: left.replaced, error }
    }
}

/// Deletes the local files and then the folders of `left`, deepest first, those that are
/// empty. A file or folder no longer there is passed over, and so is a folder where one of
/// the files was; one that can't be deleted stays in `left`, and the first such failure is
/// returned. Returns how many files were deleted.
pub fn delete_local(left: &mut Left<PathBuf>) -> (usize, Result<(), Error>) {
    let mut deleted = 0;
    let mut first = Ok(());
    left.files.retain(|file| {
        let result = match std::fs::symlink_metadata(file) {
            Ok(metadata) if metadata.is_dir() => return false,
            Ok(_) => std::fs::remove_file(file),
            Err(e) => Err(e),
        };
        match result {
            Ok(()) => deleted += 1,
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
            let _ = std::fs::remove_dir(dir);
        }
    }
    (deleted, first)
}

fn depth(path: &Path) -> usize {
    path.components().count()
}

/// [`delete_local`] on the server, a few files at once.
pub async fn delete_remote(sftp: &SftpSession, left: &mut Left<String>) -> (usize, Result<(), Error>) {
    let files = &left.files;
    // By index: closures given references make futures the compiler can't prove `Send`.
    let results: Vec<Result<bool, SftpError>> = stream::iter(0..files.len())
        .map(|i| async move {
            match sftp.remove_file(&files[i]).await {
                Ok(()) => Ok(true),
                Err(SftpError::Status(status)) if status.status_code == StatusCode::NoSuchFile => Ok(false),
                Err(e) => Err(e),
            }
        })
        .buffered(CONCURRENT_DIRS)
        .collect()
        .await;
    let mut deleted = 0;
    let mut first = Ok(());
    let mut results = results.into_iter();
    left.files.retain(|file| match results.next().expect("one result for each file") {
        Ok(removed) => {
            deleted += usize::from(removed);
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
            let _ = sftp.remove_dir(&dir).await;
        }
    }
    (deleted, first)
}
