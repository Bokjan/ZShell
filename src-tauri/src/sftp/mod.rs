//! Remote file management over SFTP. Remote paths are POSIX-style strings.

pub mod drag;
pub mod edit;
pub mod names;
pub mod transfer;

use anyhow::Result;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileAttributes;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    /// True for directories and for symlinks pointing at directories.
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub modified: Option<u32>,
    pub permissions: Option<u32>,
}

#[derive(Serialize)]
pub struct Listing {
    /// Canonical path of the listed directory.
    pub path: String,
    pub entries: Vec<FileEntry>,
}

pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

pub fn file_name(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path)
}

pub async fn list(sftp: &SftpSession, path: &str) -> Result<Listing> {
    let path = sftp.canonicalize(path).await?;
    let mut entries = Vec::new();
    for entry in sftp.read_dir(&path).await? {
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let metadata = entry.metadata();
        let full = join(&path, &name);
        let is_symlink = metadata.file_type().is_symlink();
        let is_dir = if is_symlink {
            sftp.metadata(&full).await.is_ok_and(|m| m.file_type().is_dir())
        } else {
            metadata.file_type().is_dir()
        };
        entries.push(FileEntry {
            name,
            path: full,
            is_dir,
            is_symlink,
            size: metadata.size.unwrap_or(0),
            modified: metadata.mtime,
            permissions: metadata.permissions,
        });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(Listing { path, entries })
}

pub async fn chmod(sftp: &SftpSession, path: &str, mode: u32) -> Result<()> {
    let attrs = FileAttributes { permissions: Some(mode & 0o7777), ..FileAttributes::empty() };
    sftp.set_metadata(path, attrs).await?;
    Ok(())
}

/// Removes a file, symlink, or directory tree (symlinks are never followed).
pub async fn remove(sftp: &SftpSession, path: &str) -> Result<()> {
    if !sftp.symlink_metadata(path).await?.file_type().is_dir() {
        sftp.remove_file(path).await?;
        return Ok(());
    }
    // Delete files breadth-first while collecting directories, then remove the
    // directories deepest-first.
    let mut dirs = vec![path.to_owned()];
    let mut next = 0;
    while next < dirs.len() {
        let dir = dirs[next].clone();
        next += 1;
        for entry in sftp.read_dir(&dir).await? {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let full = join(&dir, &name);
            if entry.file_type().is_dir() {
                dirs.push(full);
            } else {
                sftp.remove_file(&full).await?;
            }
        }
    }
    for dir in dirs.iter().rev() {
        sftp.remove_dir(dir).await?;
    }
    Ok(())
}
