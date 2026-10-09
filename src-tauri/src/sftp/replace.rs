//! Writing over files without losing them: when the file exists, what is written goes to a
//! temporary file beside it (`.<name>.zshell-<random>`), which takes its place only once
//! complete, so a cancelled or failed transfer, or a lost connection, leaves the original as
//! it was. Uploads, uploads of edited files and "Download to…" go through here.
//!
//! The file is written in place, as before, when replacing it would change more than its
//! contents: a symbolic link (replacing it would put a file where the link was, instead of
//! writing where it points), a file owned by another user or group (writable by us, but the
//! new one would be ours), or a folder we can't create files in. The copy gets the original's
//! permissions. Hard links to the original keep its old contents.
//!
//! SFTP v3's rename doesn't replace an existing file, and russh-sftp can't send OpenSSH's
//! `posix-rename@openssh.com`, so the original is removed just before the rename: only a
//! failure between the two leaves the new contents under the temporary name.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use russh_sftp::client::fs::File;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use tokio::io::AsyncWriteExt;

use super::{file_name, join};
use crate::error::Error;

fn temporary_name(name: &str) -> String {
    format!(".{name}.zshell-{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
}

/// A remote file being written (see the module comment).
pub struct RemoteTarget {
    pub file: File,
    path: String,
    /// Where the file is written instead, until it replaces `path`.
    temporary: Option<String>,
    /// Whether `path` existed before; a new file left incomplete is removed.
    existed: bool,
    permissions: Option<u32>,
}

impl RemoteTarget {
    /// Creates `path`, or the temporary file that is to replace it.
    pub async fn create(sftp: &SftpSession, path: &str) -> Result<Self> {
        let created = |e| anyhow::Error::from(e).context(Error::new("transfer.createFailed").param("path", path));
        let Ok(original) = sftp.symlink_metadata(path).await else {
            let file = sftp.create(path).await.map_err(created)?;
            return Ok(Self { file, path: path.to_owned(), temporary: None, existed: false, permissions: None });
        };
        let permissions = original.permissions.map(|mode| mode & 0o7777);
        if original.file_type().is_file() {
            if let Some((file, temporary)) = create_beside(sftp, path, &original, permissions).await {
                return Ok(Self { file, path: path.to_owned(), temporary: Some(temporary), existed: true, permissions });
            }
        }
        let file = sftp.create(path).await.map_err(created)?;
        Ok(Self { file, path: path.to_owned(), temporary: None, existed: true, permissions: None })
    }

    /// Closes the file, checking what the server says (some, like NFS or quotas, report a
    /// failed write only then), and puts it in place. Removes what was written if that fails.
    pub async fn finish(mut self, sftp: &SftpSession) -> Result<()> {
        if let Err(e) = self.file.shutdown().await {
            self.remove_incomplete(sftp).await;
            return Err(e.into());
        }
        let Some(temporary) = &self.temporary else { return Ok(()) };
        if let Some(permissions) = self.permissions {
            // Created with them, less the server's umask.
            let attributes = FileAttributes { permissions: Some(permissions), ..FileAttributes::empty() };
            let _ = sftp.set_metadata(temporary, attributes).await;
        }
        if let Err(e) = sftp.remove_file(&self.path).await {
            let _ = sftp.remove_file(temporary).await;
            return Err(anyhow::Error::from(e).context(Error::new("transfer.createFailed").param("path", &self.path)));
        }
        sftp.rename(temporary, &self.path)
            .await
            .context(Error::new("transfer.replaceFailed").param("path", &self.path).param("temporary", temporary))?;
        Ok(())
    }

    /// Gives up on the file after a failed or cancelled transfer.
    pub async fn abandon(mut self, sftp: &SftpSession) {
        let _ = self.file.shutdown().await;
        self.remove_incomplete(sftp).await;
    }

    /// Removes the temporary file, or a file that didn't exist before. An original written in
    /// place is left as far as it got: removing it would lose all of it.
    async fn remove_incomplete(&self, sftp: &SftpSession) {
        match (&self.temporary, self.existed) {
            (Some(temporary), _) => drop(sftp.remove_file(temporary).await),
            (None, false) => drop(sftp.remove_file(&self.path).await),
            (None, true) => {}
        }
    }
}

/// A new file beside `path` that can replace it without changing its owner; `None` if there
/// can't be one (see the module comment).
async fn create_beside(
    sftp: &SftpSession,
    path: &str,
    original: &FileAttributes,
    permissions: Option<u32>,
) -> Option<(File, String)> {
    let dir = match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) => "/",
        Some((dir, _)) => dir,
        None => ".",
    };
    let temporary = join(dir, &temporary_name(file_name(path)));
    let flags = OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE;
    let attributes = FileAttributes { permissions, ..FileAttributes::empty() };
    let file = sftp.open_with_flags_and_attributes(&temporary, flags, attributes).await.ok()?;
    let ours = file.metadata().await.ok();
    let same_owner = ours.is_some_and(|ours| {
        let same = |a: Option<u32>, b: Option<u32>| a.is_none() || b.is_none() || a == b;
        same(original.uid, ours.uid) && same(original.gid, ours.gid)
    });
    if !same_owner {
        drop(file);
        let _ = sftp.remove_file(&temporary).await;
        return None;
    }
    Some((file, temporary))
}

/// A local file being written (see the module comment).
pub struct LocalTarget {
    pub file: tokio::fs::File,
    path: PathBuf,
    temporary: Option<PathBuf>,
    existed: bool,
    permissions: Option<std::fs::Permissions>,
}

impl LocalTarget {
    /// Creates `path`, or the temporary file that is to replace it.
    pub async fn create(path: &Path) -> Result<Self> {
        let created = |e| anyhow::Error::from(e).context(Error::new("transfer.createFailed").param("path", path.display()));
        let original = tokio::fs::symlink_metadata(path).await.ok();
        if let Some(original) = original.as_ref().filter(|m| m.is_file()) {
            let temporary = path.with_file_name(temporary_name(&path.file_name().unwrap_or_default().to_string_lossy()));
            let opened = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).await;
            if let Ok(file) = opened {
                let permissions = Some(original.permissions());
                return Ok(Self { file, path: path.to_owned(), temporary: Some(temporary), existed: true, permissions });
            }
        }
        let file = tokio::fs::File::create(path).await.map_err(created)?;
        Ok(Self { file, path: path.to_owned(), temporary: None, existed: original.is_some(), permissions: None })
    }

    /// Flushes the file and puts it in place. Removes what was written if that fails.
    pub async fn finish(mut self) -> Result<()> {
        if let Err(e) = self.file.flush().await {
            self.remove_incomplete().await;
            return Err(e.into());
        }
        let Some(temporary) = &self.temporary else { return Ok(()) };
        if let Some(permissions) = self.permissions.take() {
            let _ = tokio::fs::set_permissions(temporary, permissions).await;
        }
        // Replaces the original at once, on Windows too (MoveFileEx with REPLACE_EXISTING).
        if let Err(e) = tokio::fs::rename(temporary, &self.path).await {
            let _ = tokio::fs::remove_file(temporary).await;
            return Err(anyhow::Error::from(e).context(Error::new("transfer.createFailed").param("path", self.path.display())));
        }
        Ok(())
    }

    pub async fn abandon(self) {
        self.remove_incomplete().await;
    }

    async fn remove_incomplete(&self) {
        match (&self.temporary, self.existed) {
            (Some(temporary), _) => drop(tokio::fs::remove_file(temporary).await),
            (None, false) => drop(tokio::fs::remove_file(&self.path).await),
            (None, true) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_files_are_replaced_once_complete() {
        let dir = std::env::temp_dir().join(format!("zshell-replace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.txt");
        std::fs::write(&path, "old").unwrap();

        // Abandoned: the original stays, and nothing else is left.
        let mut target = LocalTarget::create(&path).await.unwrap();
        target.file.write_all(b"half").await.unwrap();
        target.abandon().await;
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

        // Finished: replaced, keeping the permissions.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        }
        let mut target = LocalTarget::create(&path).await.unwrap();
        target.file.write_all(b"new").await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        target.finish().await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o640);
        }

        // A new file left incomplete is removed.
        let new = dir.join("b.txt");
        let target = LocalTarget::create(&new).await.unwrap();
        target.abandon().await;
        assert!(!new.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn links_are_written_through() {
        let dir = std::env::temp_dir().join(format!("zshell-replace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("real.txt"), "old").unwrap();
        std::os::unix::fs::symlink("real.txt", dir.join("link.txt")).unwrap();
        let mut target = LocalTarget::create(&dir.join("link.txt")).await.unwrap();
        target.file.write_all(b"new").await.unwrap();
        target.finish().await.unwrap();
        assert!(std::fs::symlink_metadata(dir.join("link.txt")).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(dir.join("real.txt")).unwrap(), "new");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
