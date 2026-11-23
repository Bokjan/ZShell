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
