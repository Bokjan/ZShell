//! Non-blocking reads and writes on Unix descriptors, for the sessions whose task drives a
//! terminal or a serial line through tokio's `AsyncFd` (`pty::unix`, `serial::unix`).

use std::io::{self, ErrorKind};
use std::os::fd::RawFd;

/// Makes reads and writes on `fd` return `WouldBlock` rather than wait. The flag is the open
/// file's, so every descriptor duplicated from it (before or after) shares it.
pub fn set_non_blocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: plain syscall; an invalid `fd` only makes it fail.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    // SAFETY: as above.
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Reads on while more is ready, up to the buffer's size: a read from a terminal returns at
/// most about 1 KiB on macOS, a serial adapter hands over a few dozen bytes at a time, and
/// each read becomes a message to the web view, which takes the main thread (`cat` of a
/// large file sent a thousand of them per MiB). Fails with `WouldBlock` only when nothing
/// was ready.
pub fn read_on(fd: RawFd, buffer: &mut [u8]) -> io::Result<usize> {
    let mut read = 0;
    while read < buffer.len() {
        match read_fd(fd, &mut buffer[read..]) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) if read == 0 => return Err(e),
            // What was read is passed on now; a failure (EIO at the end) comes again next time.
            Err(_) => break,
        }
    }
    Ok(read)
}

fn read_fd(fd: RawFd, buffer: &mut [u8]) -> io::Result<usize> {
    // SAFETY: `buffer` is valid for writes of its length.
    let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
    usize::try_from(read).map_err(|_| io::Error::last_os_error())
}

pub fn write_fd(fd: RawFd, data: &[u8]) -> io::Result<usize> {
    // SAFETY: `data` is valid for reads of its length.
    let written = unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) };
    usize::try_from(written).map_err(|_| io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    use super::*;

    /// What is ready to read comes in one read, though each read gives only part of it: a
    /// terminal about 1 KiB on macOS, a datagram socket (here) one datagram. A terminal can't
    /// be filled ahead (it holds 1 KiB on macOS), so it is read on as the shell writes.
    #[test]
    fn reads_on_while_more_is_ready() {
        use std::io::Write;
        let mut fds = [0; 2];
        // SAFETY: plain syscall, into two descriptors that are then ours alone.
        assert_eq!(unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_DGRAM, 0, fds.as_mut_ptr()) }, 0);
        // SAFETY: just opened, and owned by nothing else.
        let (reader, write_end) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        let mut writer = std::fs::File::from(write_end);
        for _ in 0..6 {
            writer.write_all(&[b'x'; 500]).unwrap();
        }
        set_non_blocking(reader.as_raw_fd()).unwrap();
        let mut buffer = vec![0; 16 << 10];
        assert_eq!(read_on(reader.as_raw_fd(), &mut buffer).unwrap(), 3000);
        assert_eq!(read_on(reader.as_raw_fd(), &mut buffer).unwrap_err().kind(), ErrorKind::WouldBlock);
    }
}
