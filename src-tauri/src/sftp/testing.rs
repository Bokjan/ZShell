//! An SFTP server in the test process, serving a folder of this computer, for the tests of
//! transfers on every platform (OpenSSH's `sftp-server` is there on Unix only). Remote paths
//! are `/`-separated paths under the folder. It answers as OpenSSH's does where transfers
//! tell the difference: a rename doesn't replace a file, and creating a folder that is there
//! fails. It counts the bytes it reads and writes, which tells how much a transfer copied.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use russh::server::{ChannelOpenHandle, Msg, Session};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version};

use crate::ssh::testing::Stall;

/// An SSH server that lets anyone in and runs the SFTP subsystem on [`Fs`].
pub struct SftpServer {
    root: PathBuf,
    counts: Counts,
    channels: HashMap<ChannelId, Channel<Msg>>,
}

/// The bytes read from and written to the server's files, and when to stall the connection.
#[derive(Clone, Default)]
pub struct Counts {
    read: Arc<AtomicU64>,
    written: Arc<AtomicU64>,
    stall: Arc<Mutex<Option<(u64, Stall)>>>,
}

impl Counts {
    pub fn read(&self) -> u64 {
        self.read.load(Ordering::Relaxed)
    }

    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Relaxed)
    }

    /// Stalls a connection to the server (see `ssh::testing::connect_stalling`) once it has
    /// read or written `bytes` more, as one lost midway through a transfer.
    pub fn stall_after(&self, bytes: u64, stall: &Stall) {
        *self.stall.lock().unwrap() = Some((self.read() + self.written() + bytes, stall.clone()));
    }

    fn count(&self, counter: &AtomicU64, bytes: usize) {
        counter.fetch_add(bytes as u64, Ordering::Relaxed);
        let mut stall = self.stall.lock().unwrap();
        if stall.as_ref().is_some_and(|(at, _)| self.read() + self.written() >= *at) {
            stall.take().unwrap().1.now();
        }
    }
}

impl SftpServer {
    /// Serves `root`, counting into `counts`.
    pub fn new(root: &Path, counts: &Counts) -> Self {
        Self { root: root.to_owned(), counts: counts.clone(), channels: HashMap::new() }
    }
}

impl russh::server::Handler for SftpServer {
    type Error = russh::Error;

    async fn auth_none(&mut self, _user: &str) -> Result<russh::server::Auth, Self::Error> {
        Ok(russh::server::Auth::Accept)
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn subsystem_request(&mut self, channel: ChannelId, name: &str, session: &mut Session) -> Result<(), Self::Error> {
        match (name, self.channels.remove(&channel)) {
            ("sftp", Some(opened)) => {
                session.channel_success(channel)?;
                let fs = Fs { root: self.root.clone(), counts: self.counts.clone(), open: HashMap::new(), next: 0 };
                russh_sftp::server::run(opened.into_stream(), fs).await;
            }
            _ => session.channel_failure(channel)?,
        }
        Ok(())
    }
}

/// The SFTP side: the folder served, and the files and folders open.
struct Fs {
    root: PathBuf,
    counts: Counts,
    open: HashMap<String, Open>,
    next: u64,
}

enum Open {
    File(std::fs::File),
    /// A folder's entries, until they are read.
    Dir(Option<Vec<File>>),
}

fn status(id: u32, code: StatusCode) -> Status {
    Status { id, status_code: code, error_message: code.to_string(), language_tag: "en-US".into() }
}

fn ok(id: u32) -> Status {
    status(id, StatusCode::Ok)
}

fn code(e: std::io::Error) -> StatusCode {
    match e.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        std::io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}

impl Fs {
    /// The local path of remote `path`; none for one that leads out of the folder.
    fn local(&self, path: &str) -> Result<PathBuf, StatusCode> {
        let mut local = self.root.clone();
        for part in path.split('/').filter(|part| !part.is_empty() && *part != ".") {
            if part == ".." {
                return Err(StatusCode::PermissionDenied);
            }
            local.push(part);
        }
        Ok(local)
    }

    fn add(&mut self, open: Open) -> String {
        self.next += 1;
        let handle = self.next.to_string();
        self.open.insert(handle.clone(), open);
        handle
    }

    fn file(&mut self, handle: &str) -> Result<&mut std::fs::File, StatusCode> {
        match self.open.get_mut(handle) {
            Some(Open::File(file)) => Ok(file),
            _ => Err(StatusCode::Failure),
        }
    }
}

impl russh_sftp::server::Handler for Fs {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(&mut self, _version: u32, _extensions: HashMap<String, String>) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn open(&mut self, id: u32, filename: String, pflags: OpenFlags, attrs: FileAttributes) -> Result<Handle, Self::Error> {
        let path = self.local(&filename)?;
        let mut options = std::fs::OpenOptions::new();
        options.read(pflags.contains(OpenFlags::READ)).write(pflags.contains(OpenFlags::WRITE)).append(pflags.contains(OpenFlags::APPEND));
        if pflags.contains(OpenFlags::CREATE | OpenFlags::EXCLUDE) {
            options.create_new(true);
        } else {
            options.create(pflags.contains(OpenFlags::CREATE)).truncate(pflags.contains(OpenFlags::TRUNCATE));
        }
        #[cfg(unix)]
        if let Some(mode) = attrs.permissions {
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, mode & 0o7777);
        }
        #[cfg(not(unix))]
        let _ = attrs;
        let file = options.open(path).map_err(code)?;
        Ok(Handle { id, handle: self.add(Open::File(file)) })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        self.open.remove(&handle).ok_or(StatusCode::Failure)?;
        Ok(ok(id))
    }

    async fn read(&mut self, id: u32, handle: String, offset: u64, len: u32) -> Result<Data, Self::Error> {
        let file = self.file(&handle)?;
        file.seek(SeekFrom::Start(offset)).map_err(code)?;
        let mut data = vec![0; len as usize];
        let n = file.read(&mut data).map_err(code)?;
        if n == 0 {
            return Err(StatusCode::Eof);
        }
        data.truncate(n);
        self.counts.count(&self.counts.read, n);
        Ok(Data { id, data })
    }

    async fn write(&mut self, id: u32, handle: String, offset: u64, data: Vec<u8>) -> Result<Status, Self::Error> {
        let file = self.file(&handle)?;
        file.seek(SeekFrom::Start(offset)).map_err(code)?;
        file.write_all(&data).map_err(code)?;
        self.counts.count(&self.counts.written, data.len());
        Ok(ok(id))
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let metadata = std::fs::symlink_metadata(self.local(&path)?).map_err(code)?;
        Ok(Attrs { id, attrs: FileAttributes::from(&metadata) })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let metadata = std::fs::metadata(self.local(&path)?).map_err(code)?;
        Ok(Attrs { id, attrs: FileAttributes::from(&metadata) })
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        let metadata = self.file(&handle)?.metadata().map_err(code)?;
        Ok(Attrs { id, attrs: FileAttributes::from(&metadata) })
    }

    async fn setstat(&mut self, id: u32, path: String, attrs: FileAttributes) -> Result<Status, Self::Error> {
        let path = self.local(&path)?;
        if let Some(size) = attrs.size {
            std::fs::OpenOptions::new().write(true).open(&path).and_then(|file| file.set_len(size)).map_err(code)?;
        }
        #[cfg(unix)]
        if let Some(mode) = attrs.permissions {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & 0o7777)).map_err(code)?;
        }
        Ok(ok(id))
    }

    async fn fsetstat(&mut self, id: u32, handle: String, attrs: FileAttributes) -> Result<Status, Self::Error> {
        if let Some(size) = attrs.size {
            self.file(&handle)?.set_len(size).map_err(code)?;
        }
        Ok(ok(id))
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(self.local(&path)?).map_err(code)? {
            let entry = entry.map_err(code)?;
            let metadata = entry.metadata().map_err(code)?;
            entries.push(File::new(entry.file_name().to_string_lossy(), FileAttributes::from(&metadata)));
        }
        Ok(Handle { id, handle: self.add(Open::Dir(Some(entries))) })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        match self.open.get_mut(&handle) {
            Some(Open::Dir(entries)) => match entries.take() {
                Some(files) if !files.is_empty() => Ok(Name { id, files }),
                _ => Err(StatusCode::Eof),
            },
            _ => Err(StatusCode::Failure),
        }
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        std::fs::remove_file(self.local(&filename)?).map_err(code)?;
        Ok(ok(id))
    }

    async fn mkdir(&mut self, id: u32, path: String, _attrs: FileAttributes) -> Result<Status, Self::Error> {
        std::fs::create_dir(self.local(&path)?).map_err(code)?;
        Ok(ok(id))
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        std::fs::remove_dir(self.local(&path)?).map_err(code)?;
        Ok(ok(id))
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty() && *part != ".").collect();
        Ok(Name { id, files: vec![File::dummy(format!("/{}", parts.join("/")))] })
    }

    async fn rename(&mut self, id: u32, oldpath: String, newpath: String) -> Result<Status, Self::Error> {
        let (from, to) = (self.local(&oldpath)?, self.local(&newpath)?);
        // As OpenSSH's, which links the new name first.
        if std::fs::symlink_metadata(&to).is_ok() {
            return Err(StatusCode::Failure);
        }
        std::fs::rename(from, to).map_err(code)?;
        Ok(ok(id))
    }
}
