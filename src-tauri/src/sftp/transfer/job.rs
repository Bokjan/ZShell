//! A transfer the file panel started, run once and gone on with after it fails (see
//! [`super::resume`]).

use std::path::PathBuf;

use anyhow::Result;
use russh_sftp::client::SftpSession;

use super::download::{self, Download};
use super::left::{Kept, Left, Server};
use super::upload::{self, Upload};
use super::Reporter;

/// An upload or a download, and how far it has got.
pub enum Job {
    Upload(Upload),
    Download(Download),
}

impl Job {
    /// Uploads `local_paths` into `remote_dir` on `server`, replacing files there.
    /// Uploads `local_paths` into `remote_dir` on `server`, whose names are in `encoding`.
    pub fn upload(local_paths: Vec<PathBuf>, remote_dir: String, server: Server, encoding: &'static encoding_rs::Encoding) -> Self {
        Self::Upload(Upload::new(local_paths, remote_dir, server, encoding))
    }

    /// Downloads `remote_paths` into `folder`, each under a name of its own.
    pub fn download(remote_paths: Vec<String>, folder: PathBuf) -> Self {
        Self::Download(Download::into(remote_paths, folder))
    }

    /// Downloads `remote` to `local`, replacing what is there.
    pub fn download_as(remote: String, local: PathBuf) -> Self {
        Self::Download(Download::to(vec![(remote, local)], true))
    }

    /// The server an upload goes to.
    pub fn server(&self) -> Option<&Server> {
        match self {
            Job::Upload(upload) => Some(&upload.server),
            Job::Download(_) => None,
        }
    }

    /// What it has left before it first runs: nothing.
    pub fn nothing_left(&self) -> Kept {
        match self {
            Job::Upload(upload) => Kept::Remote(Left::default(), upload.server.clone()),
            Job::Download(_) => Kept::Local(Left::default()),
        }
    }

    /// Runs it, or goes on with it, through `sftp`; what it creates goes into `left` (which
    /// [`Job::nothing_left`] or an earlier run made). With `check`, the files copied whole
    /// before are copied again unless they are there as they were left.
    pub async fn run(&mut self, sftp: &SftpSession, reporter: &mut Reporter, left: &mut Kept, check: bool) -> Result<()> {
        match (self, left) {
            (Job::Upload(job), Kept::Remote(left, _)) => upload::upload(sftp, job, reporter, left, check).await,
            (Job::Download(job), Kept::Local(left)) => download::download(sftp, job, reporter, left, check).await,
            _ => unreachable!("a transfer's job and what it left are of the same kind"),
        }
    }

    /// The local items to show once it ends, taken from what it `left`: for a download, those
    /// that are there.
    pub fn paths(&self, left: &mut Kept, done: bool) -> Vec<PathBuf> {
        match (self, left) {
            (Job::Download(job), Kept::Local(left)) => job.paths(left, done),
            _ => Vec::new(),
        }
    }

    /// Removes what its files cut short left (on the server through `sftp`, on the server the
    /// upload went to; left there without one), and gives back the names it claimed for the
    /// items that never arrived, as it won't be gone on with.
    pub async fn discard(self, sftp: Option<&SftpSession>) {
        match self {
            Job::Upload(job) => job.discard(sftp).await,
            Job::Download(job) => job.discard().await,
        }
    }
}
