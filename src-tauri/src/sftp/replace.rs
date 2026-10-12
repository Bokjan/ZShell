//! Writing over files without losing them: when the file exists, what is written goes to a
//! temporary file beside it (`.<name>.zshell-<random>`), which takes its place only once
//! complete, so a cancelled or failed transfer, or a lost connection, leaves the original as
//! it was. Uploads, uploads of edited files and "Download to…" go through here.
//!
//! The file is written in place, as before, when replacing it would change more than its
//! contents: a symbolic link (replacing it would put a file where the link was, instead of
//! writing where it points), a file owned by another user or group (writable by us, but the
//! new one would be ours), or a folder we may not create files in. Any other failure to create
//! the temporary file (a full disk, a quota) fails the transfer rather than write in place: the
//! original would be cut short first, and likely left so. The copy gets the original's
//! permissions. Hard links to the original keep its old contents.
//!
//! SFTP v3's rename doesn't replace an existing file, and russh-sftp can't send OpenSSH's
//! `posix-rename@openssh.com`, so the original is first renamed out of the way (to another
//! `.<name>.zshell-<random>`), then removed once the new file is in place. A connection lost
//! in between leaves both: the error names the files they are in.

use std::path::{Path, PathBuf};

use anyhow::Result;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::fs::File;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags, Status, StatusCode};
use tokio::io::AsyncWriteExt;

use super::{file_name, join};
use crate::error::Error;

/// The longest file name most file systems take, in bytes.
const NAME_MAX: usize = 255;

/// A name for a file beside `name`: `.<name>.zshell-<random>`, or `.zshell-<random>` when that
/// would be too long a name.
fn temporary_name(name: &str) -> String {
    let random = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let beside = format!(".{name}.zshell-{random}");
    if beside.len() <= NAME_MAX {
        beside
    } else {
        short_temporary_name(random)
    }
}

fn short_temporary_name(random: &str) -> String {
    format!(".zshell-{random}")
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
            if let Some((file, temporary)) = create_beside(sftp, path, &original, permissions).await.map_err(created)? {
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
        let original = join(parent(&self.path), &temporary_name(file_name(&self.path)));
        if let Err(e) = sftp.rename(&self.path, &original).await {
            let _ = sftp.remove_file(temporary).await;
            return Err(anyhow::Error::from(e).context(Error::new("transfer.createFailed").param("path", &self.path)));
        }
        if let Err(e) = sftp.rename(temporary, &self.path).await {
            let error = anyhow::Error::from(e);
            // The original back where it was, if the connection still allows.
            return Err(match sftp.rename(&original, &self.path).await {
                Ok(()) => error.context(Error::new("transfer.replaceFailed").param("path", &self.path).param("temporary", temporary)),
                Err(_) => error.context(
                    Error::new("transfer.replaceInterrupted")
                        .param("path", &self.path)
                        .param("temporary", temporary)
                        .param("original", &original),
                ),
            });
        }
        // The upload is done; a copy of the old contents left behind (the connection lost
        // just now) is a hidden file beside it.
        let _ = sftp.remove_file(&original).await;
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

/// The folder `path` is in.
fn parent(path: &str) -> &str {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) => "/",
        Some((dir, _)) => dir,
        None => ".",
    }
}

/// A new file beside `path` that can replace it without changing its owner; `None` if there
/// can't be one: the folder doesn't let us create files, or the owner would change (see the
/// module comment).
async fn create_beside(
    sftp: &SftpSession,
    path: &str,
    original: &FileAttributes,
    permissions: Option<u32>,
) -> Result<Option<(File, String)>, SftpError> {
    let denied = |e: &SftpError| matches!(e, SftpError::Status(Status { status_code: StatusCode::PermissionDenied, .. }));
    let flags = OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE;
    let attributes = FileAttributes { permissions, ..FileAttributes::empty() };
    let open = |temporary: String| async {
        let file = sftp.open_with_flags_and_attributes(&temporary, flags, attributes.clone()).await?;
        Ok::<_, SftpError>((file, temporary))
    };
    let named = temporary_name(file_name(path));
    let (file, temporary) = match open(join(parent(path), &named)).await {
        Ok(created) => created,
        Err(e) if denied(&e) => return Ok(None),
        // Servers that report every failure alike (`SSH_FX_FAILURE`), or that take shorter
        // names than we guessed: a short name once more.
        Err(e) => {
            let short = short_temporary_name(&named[named.len() - 8..]);
            if short == named {
                return Err(e);
            }
            match open(join(parent(path), &short)).await {
                Ok(created) => created,
                Err(e) if denied(&e) => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    };
    let ours = match file.metadata().await {
        Ok(ours) => ours,
        Err(e) => {
            drop(file);
            let _ = sftp.remove_file(&temporary).await;
            return Err(e);
        }
    };
    let same = |a: Option<u32>, b: Option<u32>| a.is_none() || b.is_none() || a == b;
    if !(same(original.uid, ours.uid) && same(original.gid, ours.gid)) {
        drop(file);
        let _ = sftp.remove_file(&temporary).await;
        return Ok(None);
    }
    Ok(Some((file, temporary)))
}

/// A new file beside local `path`; `None` if the folder doesn't let us create files.
async fn create_new_beside(path: &Path) -> std::io::Result<Option<(tokio::fs::File, PathBuf)>> {
    let open = |temporary: PathBuf| async {
        let file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).await?;
        Ok::<_, std::io::Error>((file, temporary))
    };
    let named = temporary_name(&path.file_name().unwrap_or_default().to_string_lossy());
    let result = match open(path.with_file_name(&named)).await {
        // A file system that takes shorter names than we guessed.
        Err(e) if e.kind() != std::io::ErrorKind::PermissionDenied => {
            let short = short_temporary_name(&named[named.len() - 8..]);
            if short == named {
                Err(e)
            } else {
                open(path.with_file_name(short)).await
            }
        }
        result => result,
    };
    match result {
        Ok(created) => Ok(Some(created)),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Ok(None),
        Err(e) => Err(e),
    }
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
            if let Some((file, temporary)) = create_new_beside(path).await.map_err(created)? {
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

    /// OpenSSH's `sftp-server` on this computer, if any, as an SFTP session on its stdio.
    #[cfg(unix)]
    async fn local_sftp_server() -> Option<SftpSession> {
        let program = ["/usr/libexec/sftp-server", "/usr/lib/openssh/sftp-server", "/usr/lib/ssh/sftp-server"]
            .into_iter()
            .find(|p| Path::new(p).exists())?;
        let mut child = tokio::process::Command::new(program)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .ok()?;
        let stream = tokio::io::join(child.stdout.take()?, child.stdin.take()?);
        // Ends with the session's stream.
        tokio::spawn(async move { child.wait().await });
        SftpSession::new(stream).await.ok()
    }

    /// Replacing a remote file: never removed before the new one is complete, and nothing
    /// left beside it afterwards. Skipped without `sftp-server` (Windows).
    #[cfg(unix)]
    #[tokio::test]
    async fn remote_files_are_replaced_once_complete() {
        use std::os::unix::fs::PermissionsExt;
        let Some(sftp) = local_sftp_server().await else { return };
        let dir = std::env::temp_dir().join(format!("zshell-replace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.txt");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let remote = path.to_str().unwrap();

        let mut target = RemoteTarget::create(&sftp, remote).await.unwrap();
        assert!(target.temporary.is_some());
        target.file.write_all(b"new").await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        target.finish(&sftp).await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o640);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

        // Abandoned: the original stays.
        let mut target = RemoteTarget::create(&sftp, remote).await.unwrap();
        target.file.write_all(b"half").await.unwrap();
        target.abandon(&sftp).await;
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A name too long to have the usual temporary name beside it still gets one; a folder we
    /// may not create files in is written in place. Skipped without `sftp-server` (Windows).
    #[cfg(unix)]
    #[tokio::test]
    async fn remote_long_names_and_read_only_folders() {
        use std::os::unix::fs::PermissionsExt;
        let Some(sftp) = local_sftp_server().await else { return };
        let dir = std::env::temp_dir().join(format!("zshell-replace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("n".repeat(250));
        std::fs::write(&path, "old").unwrap();
        let remote = path.to_str().unwrap();

        let mut target = RemoteTarget::create(&sftp, remote).await.unwrap();
        assert!(target.temporary.is_some());
        target.file.write_all(b"new").await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        target.finish(&sftp).await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let mut target = RemoteTarget::create(&sftp, remote).await.unwrap();
        assert!(target.temporary.is_none());
        target.file.write_all(b"in place").await.unwrap();
        target.finish(&sftp).await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "in place");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_long_names_and_read_only_folders() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("zshell-replace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("n".repeat(250));
        std::fs::write(&path, "old").unwrap();

        let mut target = LocalTarget::create(&path).await.unwrap();
        assert!(target.temporary.is_some());
        target.file.write_all(b"new").await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        target.finish().await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let mut target = LocalTarget::create(&path).await.unwrap();
        assert!(target.temporary.is_none());
        target.file.write_all(b"in place").await.unwrap();
        target.finish().await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "in place");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
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
