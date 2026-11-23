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
//! permissions; a new file gets those it is created with (the file copied's, as `sftp put`
//! and `get` give them). Hard links to the original keep its old contents.
//!
//! SFTP v3's rename doesn't replace an existing file, and OpenSSH's `posix-rename@openssh.com`
//! (`SftpSession::posix_rename` since russh-sftp 4) isn't used yet, so the original is first
//! renamed out of the way (to another
//! `.<name>.zshell-<random>`), then removed once the new file is in place. A connection lost
//! in between leaves both: the error names the files they are in.
//!
//! A new file is written under a temporary name too when asked to (`aside`: a large one a
//! transfer may go on with), so that one cut short doesn't pass for complete. A failed transfer
//! then `suspend`s it, keeping the temporary file, and `resume` writes on from where it says
//! (see `transfer::resume`).

use std::path::{Path, PathBuf};

use anyhow::Result;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::fs::File;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags, Status, StatusCode};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

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

/// A remote file a failed transfer left written in part, under its temporary name (see
/// [`RemoteTarget::suspend`]).
#[derive(Clone, Debug)]
pub struct RemotePartial {
    pub path: String,
    temporary: String,
    existed: bool,
    permissions: Option<u32>,
}

impl RemotePartial {
    /// Removes what was written, when the transfer won't go on with it.
    pub async fn remove(self, sftp: &SftpSession) {
        let _ = sftp.remove_file(&self.temporary).await;
    }
}

/// Creates (or truncates) remote `path`; a new file gets the permission bits `mode`, if given,
/// less the server's umask.
async fn create_file(sftp: &SftpSession, path: &str, mode: Option<u32>) -> Result<File, SftpError> {
    let attributes = FileAttributes { permissions: mode, ..FileAttributes::empty() };
    sftp.open_with_flags_and_attributes(path, OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE, attributes).await
}

impl RemoteTarget {
    /// Creates `path` (with the permission bits `mode`, if given, when there is no file), or the
    /// temporary file that is to replace it; when there is no file too, if `aside`.
    pub async fn create(sftp: &SftpSession, path: &str, mode: Option<u32>, aside: bool) -> Result<Self> {
        let created = |e| anyhow::Error::from(e).context(Error::new("transfer.createFailed").param("path", path));
        let original = match sftp.symlink_metadata(path).await {
            Ok(original) => original,
            Err(SftpError::Status(status)) if status.status_code == StatusCode::NoSuchFile => {
                return Self::create_new(sftp, path, mode, aside).await;
            }
            // Not known to be new (a timeout, a server that says "failure" of a path that
            // isn't there): written beside it where the folder lets us, and never taken for
            // one this created, which a failed copy, or deleting what a transfer left, removes.
            Err(_) => return Ok(Self { existed: true, ..Self::create_new(sftp, path, mode, true).await? }),
        };
        let permissions = original.permissions.map(|mode| mode & 0o7777);
        if original.file_type().is_file() {
            if let Some((file, temporary)) = create_beside(sftp, path, Some(&original), permissions).await.map_err(created)? {
                return Ok(Self { file, path: path.to_owned(), temporary: Some(temporary), existed: true, permissions });
            }
        }
        let file = create_file(sftp, path, mode).await.map_err(created)?;
        Ok(Self { file, path: path.to_owned(), temporary: None, existed: true, permissions: None })
    }

    /// Creates `path`, where nothing is (in a folder just created), with the permission bits
    /// `mode` if given: under a temporary name first if `aside`, where the folder lets us.
    pub async fn create_new(sftp: &SftpSession, path: &str, mode: Option<u32>, aside: bool) -> Result<Self> {
        let created = |e| anyhow::Error::from(e).context(Error::new("transfer.createFailed").param("path", path));
        if aside {
            if let Some((file, temporary)) = create_beside(sftp, path, None, mode).await.map_err(created)? {
                return Ok(Self { file, path: path.to_owned(), temporary: Some(temporary), existed: false, permissions: None });
            }
        }
        let file = create_file(sftp, path, mode).await.map_err(created)?;
        Ok(Self { file, path: path.to_owned(), temporary: None, existed: false, permissions: None })
    }

    /// Goes on writing the temporary file of `partial`, from where `from` says given how long
    /// it is now; `None` when it says to start over, or the file is gone (removed then).
    pub async fn resume(sftp: &SftpSession, partial: RemotePartial, from: impl FnOnce(u64) -> Option<u64>) -> Result<Option<(Self, u64)>> {
        let RemotePartial { path, temporary, existed, permissions } = partial;
        let Ok(mut file) = sftp.open_with_flags(&temporary, OpenFlags::WRITE).await else { return Ok(None) };
        let length = file.metadata().await.map_err(anyhow::Error::from)?.size;
        let Some(from) = length.and_then(from) else {
            let _ = file.shutdown().await;
            let _ = sftp.remove_file(&temporary).await;
            return Ok(None);
        };
        file.seek(std::io::SeekFrom::Start(from)).await?;
        Ok(Some((Self { file, path, temporary: Some(temporary), existed, permissions }, from)))
    }

    /// Closes the file, checking what the server says (some, like NFS or quotas, report a
    /// failed write only then), and puts it in place. Removes what was written if that fails.
    /// A file replacing another is synced to the server's disk first (where the server can),
    /// so that a crash of the server right after can't leave it in place incomplete; a new one
    /// isn't, as `sftp put` doesn't, which would take a round trip and a wait on every file.
    pub async fn finish(self, sftp: &SftpSession) -> Result<()> {
        self.finish_or_suspend(sftp, false).await.map_err(|(e, _)| e)
    }

    /// [`finish`](Self::finish), but if `keep`, a file that couldn't be closed or put in place
    /// (the connection lost as the last writes were acknowledged) keeps its temporary file to
    /// go on with later, given with the error (see [`suspend`](Self::suspend)).
    pub async fn finish_or_suspend(mut self, sftp: &SftpSession, keep: bool) -> Result<(), (anyhow::Error, Option<RemotePartial>)> {
        let written = match self.temporary {
            Some(_) if self.existed => self.file.flush().await,
            _ => Ok(()),
        };
        if let Err(e) = written.and(self.file.shutdown().await) {
            if keep {
                return Err((e.into(), self.suspend(sftp).await));
            }
            self.remove_incomplete(sftp).await;
            return Err((e.into(), None));
        }
        let Some(temporary) = &self.temporary else { return Ok(()) };
        if let Some(permissions) = self.permissions {
            // Created with them, less the server's umask.
            let attributes = FileAttributes { permissions: Some(permissions), ..FileAttributes::empty() };
            let _ = sftp.set_metadata(temporary, attributes).await;
        }
        // Nothing was there: put in place at once, unless something is there by now.
        if !self.existed && sftp.rename(temporary, &self.path).await.is_ok() {
            return Ok(());
        }
        let original = join(parent(&self.path), &temporary_name(file_name(&self.path)));
        if let Err(e) = sftp.rename(&self.path, &original).await {
            // The original went meanwhile (a transfer gone on with a while later).
            if self.existed && sftp.rename(temporary, &self.path).await.is_ok() {
                return Ok(());
            }
            let error = anyhow::Error::from(e).context(Error::new("transfer.createFailed").param("path", &self.path));
            if keep {
                return Err((error, self.suspend(sftp).await));
            }
            let _ = sftp.remove_file(temporary).await;
            return Err((error, None));
        }
        if let Err(e) = sftp.rename(temporary, &self.path).await {
            let error = anyhow::Error::from(e);
            // The original back where it was, if the connection still allows.
            return Err((
                match sftp.rename(&original, &self.path).await {
                    Ok(()) => error.context(Error::new("transfer.replaceFailed").param("path", &self.path).param("temporary", temporary)),
                    Err(_) => error.context(
                        Error::new("transfer.replaceInterrupted")
                            .param("path", &self.path)
                            .param("temporary", temporary)
                            .param("original", &original),
                    ),
                },
                None,
            ));
        }
        // The upload is done; a copy of the old contents left behind (the connection lost
        // just now) is a hidden file beside it.
        let _ = sftp.remove_file(&original).await;
        Ok(())
    }

    /// Whether there was a file at the path, which this one replaces.
    pub fn replaces(&self) -> bool {
        self.existed
    }

    /// Gives up on the file after a failed or cancelled transfer.
    pub async fn abandon(mut self, sftp: &SftpSession) {
        let _ = self.file.shutdown().await;
        self.remove_incomplete(sftp).await;
    }

    /// Leaves the file after a failed transfer, keeping its temporary file to go on with later
    /// ([`RemoteTarget::resume`]); gives it up ([`RemoteTarget::abandon`]) if it has none.
    pub async fn suspend(mut self, sftp: &SftpSession) -> Option<RemotePartial> {
        let Some(temporary) = self.temporary.take() else {
            self.abandon(sftp).await;
            return None;
        };
        // Fails at once when the connection is gone, as it most often is.
        let _ = self.file.shutdown().await;
        let path = std::mem::take(&mut self.path);
        Some(RemotePartial { path, temporary, existed: self.existed, permissions: self.permissions })
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

/// A new file beside `path` that can replace the `original` there, if any, without changing its
/// owner, created with the permission bits `permissions`; `None` if there can't be one: the
/// folder doesn't let us create files, or the owner would change (see the module comment).
async fn create_beside(
    sftp: &SftpSession,
    path: &str,
    original: Option<&FileAttributes>,
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
    let Some(original) = original else { return Ok(Some((file, temporary))) };
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

/// A new file beside local `path`, created with the permission bits `mode` (Unix: an original's,
/// so that the copy is never more open than it meanwhile, and the owner's write, as it is
/// written to); `None` if the folder doesn't let us create files.
async fn create_new_beside(path: &Path, mode: Option<u32>) -> std::io::Result<Option<(tokio::fs::File, PathBuf)>> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(mode) = mode {
        options.mode(mode & 0o777 | 0o200);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let open = |temporary: PathBuf| async {
        let file = options.open(&temporary).await?;
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

/// The permission bits of a local file, where there are any (Unix).
fn local_mode(metadata: &std::fs::Metadata) -> Option<u32> {
    #[cfg(unix)]
    return Some(std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()));
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

/// A local file being written (see the module comment).
pub struct LocalTarget {
    pub file: tokio::fs::File,
    path: PathBuf,
    temporary: Option<PathBuf>,
    existed: bool,
    permissions: Option<std::fs::Permissions>,
    /// `path` is an empty file the transfer made to hold the name (see
    /// [`LocalTarget::create_claimed`]), removed with what failed to arrive.
    claimed: bool,
}

/// A local file a failed transfer left written in part, under its temporary name (see
/// [`LocalTarget::suspend`]).
#[derive(Clone, Debug)]
pub struct LocalPartial {
    pub path: PathBuf,
    temporary: PathBuf,
    existed: bool,
    permissions: Option<std::fs::Permissions>,
    claimed: bool,
}

impl LocalPartial {
    /// Removes what was written, when the transfer won't go on with it, and the name it held.
    pub async fn remove(self) {
        let _ = tokio::fs::remove_file(&self.temporary).await;
        if self.claimed {
            let _ = tokio::fs::remove_file(&self.path).await;
        }
    }

    /// Whether the file holds a name the transfer took (see [`LocalTarget::create_claimed`]).
    pub fn claimed(&self) -> bool {
        self.claimed
    }
}

impl LocalTarget {
    /// Creates `path` (with the permission bits `mode`, if given, when there is no file; Unix,
    /// less the umask), or the temporary file that is to replace it; when there is no file
    /// too, if `aside`.
    pub async fn create(path: &Path, mode: Option<u32>, aside: bool) -> Result<Self> {
        let created = |e| anyhow::Error::from(e).context(Error::new("transfer.createFailed").param("path", path.display()));
        // Likewise only a path that isn't there is a new file.
        let original = match tokio::fs::symlink_metadata(path).await {
            Ok(original) => Some(original),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(created(e)),
        };
        let beside = match &original {
            Some(original) if original.is_file() => Some((local_mode(original), Some(original.permissions()))),
            None if aside => Some((mode, None)),
            _ => None,
        };
        if let Some((mode, permissions)) = beside {
            if let Some((file, temporary)) = create_new_beside(path, mode).await.map_err(created)? {
                let existed = original.is_some();
                return Ok(Self { file, path: path.to_owned(), temporary: Some(temporary), existed, permissions, claimed: false });
            }
        }
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        if let Some(mode) = mode {
            options.mode(mode);
        }
        let file = options.open(path).await.map_err(created)?;
        Ok(Self { file, path: path.to_owned(), temporary: None, existed: original.is_some(), permissions: None, claimed: false })
    }

    /// [`create`](Self::create) for `path`, an empty file this transfer made to take a free
    /// name (`name (1)`): it replaces nothing, and is removed with what fails to arrive.
    pub async fn create_claimed(path: &Path, mode: Option<u32>, aside: bool) -> Result<Self> {
        let target = Self::create(path, mode, aside).await;
        if target.is_err() {
            let _ = tokio::fs::remove_file(path).await;
        }
        Ok(Self { existed: false, claimed: true, ..target? })
    }

    /// Goes on writing the temporary file of `partial`, from where `from` says given how long
    /// it is now; `None` when it says to start over, or the file is gone (removed then).
    pub async fn resume(partial: LocalPartial, from: impl FnOnce(u64) -> Option<u64>) -> Result<Option<(Self, u64)>> {
        let LocalPartial { path, temporary, existed, permissions, claimed } = partial;
        let Ok(mut file) = tokio::fs::OpenOptions::new().write(true).open(&temporary).await else { return Ok(None) };
        let length = file.metadata().await?.len();
        let Some(from) = from(length) else {
            drop(file);
            let _ = tokio::fs::remove_file(&temporary).await;
            return Ok(None);
        };
        file.seek(std::io::SeekFrom::Start(from)).await?;
        Ok(Some((Self { file, path, temporary: Some(temporary), existed, permissions, claimed }, from)))
    }

    /// Whether there was a file at the path, which this one replaces.
    pub fn replaces(&self) -> bool {
        self.existed
    }

    /// Flushes the file and puts it in place. Removes what was written if that fails.
    pub async fn finish(mut self) -> Result<()> {
        if let Err(e) = self.file.flush().await {
            self.remove_incomplete().await;
            return Err(e.into());
        }
        let Some(temporary) = &self.temporary else { return Ok(()) };
        // On the disk before it replaces the original: a power cut right after the rename
        // would otherwise leave a file that was never written in place of a complete one. A new
        // file (written aside to be gone on with) isn't, as `sftp get` doesn't: it can only lose
        // itself, and syncing each large file of a folder would hold up the download.
        if self.existed {
            if let Err(e) = self.file.sync_all().await {
                self.remove_incomplete().await;
                return Err(anyhow::Error::from(e).context(Error::new("transfer.createFailed").param("path", self.path.display())));
            }
        }
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

    /// Leaves the file after a failed transfer, keeping its temporary file to go on with later
    /// ([`LocalTarget::resume`]), all that was written to it on the disk; gives it up
    /// ([`LocalTarget::abandon`]) if it has none.
    pub async fn suspend(mut self) -> Option<LocalPartial> {
        let Some(temporary) = self.temporary.take() else {
            self.abandon().await;
            return None;
        };
        // What tokio has yet to write: the file is written in order, and its length is then
        // where to go on from.
        if self.file.flush().await.is_err() {
            drop(self.file);
            let _ = tokio::fs::remove_file(&temporary).await;
            if self.claimed {
                let _ = tokio::fs::remove_file(&self.path).await;
            }
            return None;
        }
        Some(LocalPartial { path: self.path, temporary, existed: self.existed, permissions: self.permissions, claimed: self.claimed })
    }

    async fn remove_incomplete(&self) {
        match (&self.temporary, self.existed) {
            (Some(temporary), _) => drop(tokio::fs::remove_file(temporary).await),
            (None, false) => drop(tokio::fs::remove_file(&self.path).await),
            (None, true) => {}
        }
        if self.claimed && self.temporary.is_some() {
            let _ = tokio::fs::remove_file(&self.path).await;
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    #[tokio::test]
    async fn local_files_are_replaced_once_complete() {
        let dir = std::env::temp_dir().join(format!("zshell-replace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.txt");
        std::fs::write(&path, "old").unwrap();

        // Abandoned: the original stays, and nothing else is left.
        let mut target = LocalTarget::create(&path, None, false).await.unwrap();
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
        let mut target = LocalTarget::create(&path, None, false).await.unwrap();
        // Never more open than the original, even before it is complete.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let temporary = target.temporary.as_ref().unwrap();
            assert_eq!(std::fs::metadata(temporary).unwrap().permissions().mode() & 0o777, 0o640);
        }
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
        let target = LocalTarget::create(&new, None, false).await.unwrap();
        target.abandon().await;
        assert!(!new.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// OpenSSH's `sftp-server` on this computer, if any, as an SFTP session on its stdio.
    #[cfg(unix)]
    pub(in crate::sftp) async fn local_sftp_server() -> Option<SftpSession> {
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

        let mut target = RemoteTarget::create(&sftp, remote, None, false).await.unwrap();
        assert!(target.temporary.is_some());
        target.file.write_all(b"new").await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        target.finish(&sftp).await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o640);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

        // Abandoned: the original stays.
        let mut target = RemoteTarget::create(&sftp, remote, None, false).await.unwrap();
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

        let mut target = RemoteTarget::create(&sftp, remote, None, false).await.unwrap();
        assert!(target.temporary.is_some());
        target.file.write_all(b"new").await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        target.finish(&sftp).await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let mut target = RemoteTarget::create(&sftp, remote, None, false).await.unwrap();
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

        let mut target = LocalTarget::create(&path, None, false).await.unwrap();
        assert!(target.temporary.is_some());
        target.file.write_all(b"new").await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        target.finish().await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let mut target = LocalTarget::create(&path, None, false).await.unwrap();
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
        let mut target = LocalTarget::create(&dir.join("link.txt"), None, false).await.unwrap();
        target.file.write_all(b"new").await.unwrap();
        target.finish().await.unwrap();
        assert!(std::fs::symlink_metadata(dir.join("link.txt")).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(dir.join("real.txt")).unwrap(), "new");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
