//! Remote file management over SFTP. Remote paths are POSIX-style strings.

mod batch;
pub mod channels;
pub mod drag;
pub mod edit;
pub mod names;
pub mod replace;
pub mod shell_dir;
pub mod transfer;

use std::sync::Arc;

use anyhow::Result;
use futures_util::{StreamExt, TryStreamExt};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileAttributes;
use serde::Serialize;
use ts_rs::TS;

#[derive(Serialize, TS)]
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
    /// Of the entry itself: for a symlink, its own (usually `0777` on Linux).
    pub permissions: Option<u32>,
    /// For a symlink, those of what it points to, which changing its permissions changes
    /// (SFTP's `SETSTAT` follows links); unknown for a broken one.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub target_permissions: Option<u32>,
}

#[derive(Serialize, TS)]
pub struct Listing {
    /// Canonical path of the listed directory.
    pub path: String,
    /// In the server's order: the file panel sorts them as the user chose.
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

/// Whether `name`, from a directory listing, names an entry of the directory: not `.` or `..`,
/// nor what no file system has (`x/..`, a NUL), which a server could send to make a path
/// that leads elsewhere.
pub fn is_entry(name: &str) -> bool {
    !matches!(name, "" | "." | "..") && !name.contains(['/', '\0'])
}

/// Whether an id the frontend made (a UUID) can name a folder of ours: letters, digits and
/// dashes only, so that it can't lead elsewhere (`..`, or a drive such as `C:` on Windows).
pub fn is_folder_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Symlinks whose targets are looked up at once when listing, rather than one round trip
/// after another (a folder such as `/usr/lib` has hundreds).
const SYMLINK_LOOKUPS: usize = 16;

pub async fn list(sftp: &Arc<SftpSession>, path: &str) -> Result<Listing> {
    let path = sftp.canonicalize(path).await?;
    let mut entries = Vec::new();
    for entry in sftp.read_dir(&path).await? {
        let name = entry.file_name();
        if !is_entry(&name) {
            continue;
        }
        let metadata = entry.metadata();
        entries.push(FileEntry {
            path: join(&path, &name),
            name,
            is_dir: metadata.file_type().is_dir(),
            is_symlink: metadata.file_type().is_symlink(),
            size: metadata.size.unwrap_or(0),
            modified: metadata.mtime,
            permissions: metadata.permissions,
            target_permissions: None,
        });
    }
    // A symlink is shown as a folder if it leads to one, and changing its permissions changes
    // its target's.
    let symlinks: Vec<(usize, String)> =
        entries.iter().enumerate().filter(|(_, e)| e.is_symlink).map(|(i, e)| (i, e.path.clone())).collect();
    let mut symlinks = symlinks.into_iter();
    let mut lookups = tokio::task::JoinSet::new();
    loop {
        while lookups.len() < SYMLINK_LOOKUPS {
            let Some((index, target)) = symlinks.next() else { break };
            let sftp = sftp.clone();
            lookups.spawn(async move { (index, sftp.metadata(&target).await.ok()) });
        }
        match lookups.join_next().await {
            Some(Ok((index, target))) => {
                let entry = &mut entries[index];
                entry.is_dir = target.as_ref().is_some_and(|m| m.file_type().is_dir());
                entry.target_permissions = target.and_then(|m| m.permissions);
            }
            Some(Err(_)) => {}
            None => break,
        }
    }
    Ok(Listing { path, entries })
}

pub async fn chmod(sftp: &SftpSession, path: &str, mode: u32) -> Result<()> {
    let attrs = FileAttributes { permissions: Some(mode & 0o7777), ..FileAttributes::empty() };
    sftp.set_metadata(path, attrs).await?;
    Ok(())
}

/// Requests sent at once while removing a folder: one after another, removing 10,000 files
/// took over a quarter of an hour from a server 100 ms away.
const CONCURRENT_REMOVALS: usize = 16;

/// Removes a file, symlink, or directory tree (symlinks are never followed).
pub async fn remove(sftp: &SftpSession, path: &str) -> Result<()> {
    if !sftp.symlink_metadata(path).await?.file_type().is_dir() {
        sftp.remove_file(path).await?;
        return Ok(());
    }
    // A level of the tree at a time: its folders listed and their files removed, each at once;
    // then the folders, deepest first.
    let mut levels = vec![vec![path.to_owned()]];
    loop {
        let level = levels.last().unwrap();
        let listings: Vec<_> = batch::each_in_order(level, CONCURRENT_REMOVALS, |dir| sftp.read_dir(dir)).collect().await;
        let (mut files, mut dirs) = (Vec::new(), Vec::new());
        for (dir, listing) in level.iter().zip(listings) {
            for entry in listing? {
                let name = entry.file_name();
                if !is_entry(&name) {
                    continue;
                }
                let full = join(dir, &name);
                if entry.file_type().is_dir() {
                    dirs.push(full);
                } else {
                    files.push(full);
                }
            }
        }
        batch::each(&files, CONCURRENT_REMOVALS, |file| sftp.remove_file(file)).try_collect::<()>().await?;
        if dirs.is_empty() {
            break;
        }
        levels.push(dirs);
    }
    for level in levels.into_iter().rev() {
        batch::each(&level, CONCURRENT_REMOVALS, |dir| sftp.remove_dir(dir)).try_collect::<()>().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree of folders and files goes, a link in it without what it points to. Skipped
    /// without `sftp-server`.
    #[cfg(unix)]
    #[tokio::test]
    async fn removes_a_tree() {
        let Some(sftp) = replace::tests::local_sftp_server().await else { return };
        let root = std::env::temp_dir().join(format!("zshell-remove-{}", uuid::Uuid::new_v4()));
        let kept = root.join("kept");
        std::fs::create_dir_all(&kept).unwrap();
        let tree = root.join("tree");
        for dir in ["a/b/c", "a/d", "e"] {
            std::fs::create_dir_all(tree.join(dir)).unwrap();
            for n in 0..30 {
                std::fs::write(tree.join(dir).join(n.to_string()), "x").unwrap();
            }
        }
        std::os::unix::fs::symlink(&kept, tree.join("a/link")).unwrap();
        let result = remove(&sftp, tree.to_str().unwrap()).await;
        let (gone, kept) = (!tree.exists(), kept.exists());
        std::fs::remove_dir_all(&root).unwrap();
        result.unwrap();
        assert!(gone && kept);
    }

    #[test]
    fn folder_ids_are_plain() {
        assert!(is_folder_id("0b4f5e3c-7d1a-4c55-9a0e-2f6b8c1d3e4f"));
        for id in ["", ".", "..", "C:x", "a/b", "a\\b", "x.y"] {
            assert!(!is_folder_id(id), "{id:?}");
        }
    }

    #[test]
    fn listings_name_entries_of_the_directory_only() {
        assert!(is_entry("notes.txt") && is_entry(".bashrc") && is_entry("a\\b"));
        for name in ["", ".", "..", "x/..", "/etc", "a\0b"] {
            assert!(!is_entry(name), "{name:?}");
        }
    }
}
