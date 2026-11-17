//! Going on with a transfer that failed, as OpenSSH's `sftp` does with `reget` and `reput`:
//! the files copied whole are skipped, and a large file cut short goes on from what its
//! temporary file holds (see `crate::sftp::replace`). After a lost connection, once the tab is
//! connected again, taking the files copied whole as they were left; after another failure,
//! when the user retries, copying again those no longer there as they were left (by their
//! size: one deleted or changed since). A file of the same name a transfer replaced is not
//! asked about again.
//!
//! What it takes to go on is kept in memory by [`super::Transfers`] for as long as the transfer
//! is listed, and only for this run of the app: the plan, and how far each file got. A file
//! smaller than [`RESUMABLE`] is copied again from the start, which costs less than looking up
//! where it got. A file is copied again from the start, too, when it changed since the plan was
//! made (its size or modification time), or when its temporary file is gone or longer than it.

use std::time::Duration;

use russh_sftp::client::error::Error as SftpError;
use serde::Serialize;
use ts_rs::TS;

use crate::error::Error;
use crate::sftp::channels::TRANSFER_WRITES_IN_FLIGHT;
use crate::ssh;

/// Files larger than this are written under a temporary name, and gone on with after a
/// failure; smaller ones are copied again.
pub(super) const RESUMABLE: u64 = 8 << 20;

/// How long a transfer that failed in a way a lost connection would make it fail waits for
/// the connection to be found lost (see [`ended_by_loss`]): a check of the connection, which
/// this starts, takes up to 10 seconds (see `ssh::liveness`). A tenth in tests, as the checks.
const LOSS_WAIT: Duration = Duration::from_millis(if cfg!(test) { 1500 } else { 15000 });

/// Whether a file of `size` is written so that it can be gone on with.
pub(super) fn resumable(size: u64) -> bool {
    size > RESUMABLE
}

/// How far a file of a transfer has been copied.
#[derive(Debug)]
pub(super) enum Copied<D, T> {
    /// Not at all, or to be copied again from the start.
    No,
    /// Whole, to where it is now (a download may have taken another name for it).
    Done(D),
    /// In part: where its temporary file is.
    Partly(T),
}

/// How a failed transfer can be gone on with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "TransferResume")]
pub enum Resume {
    /// The connection was lost: once the tab is connected again.
    Lost,
    /// Something else went wrong: when the user asks.
    Retry,
}

/// Where a download cut short goes on, given how much of the file is `written` here: right
/// after it, as a download is written in order. `None` to start over: the file there is longer
/// than the one copied (`size`).
pub(super) fn download_from(written: u64, size: u64) -> Option<u64> {
    (written <= size).then_some(written)
}

/// Where an upload cut short goes on, given how long the file on the server is (`written`): the
/// server may not have written the last writes in flight in order, leaving holes before where
/// the file ends, so they are written again (the same data, which is no harm). `None` to start
/// over, as for [`download_from`].
pub(super) fn upload_from(written: u64, size: u64) -> Option<u64> {
    (written <= size).then(|| written.saturating_sub(TRANSFER_WRITES_IN_FLIGHT))
}

/// Whether `e` may come from losing the connection: an SFTP request with no answer (it timed
/// out, or the channel closed under it), or the channel failing to open. Not when the server
/// answered with an error, or this computer's disk failed.
pub(super) fn may_be_lost(e: &anyhow::Error) -> bool {
    let sftp = |e: &SftpError| !matches!(e, SftpError::Status(_));
    e.chain().any(|cause| {
        if let Some(e) = cause.downcast_ref::<SftpError>() {
            return sftp(e);
        }
        if let Some(e) = cause.downcast_ref::<std::io::Error>() {
            // russh-sftp's files turn its errors into these.
            return e.get_ref().and_then(|inner| inner.downcast_ref::<SftpError>()).is_some_and(sftp);
        }
        cause.downcast_ref::<Error>().is_some_and(|e| matches!(e.code(), "net.connectionLost" | "sftp.openTimeout" | "sftp.openFailed"))
    })
}

/// Whether a transfer through `connection` failed with `e` because the connection was lost:
/// it has ended, or it ends soon after a failure that losing it would cause, as the check this
/// starts finds it gone (see [`may_be_lost`]).
pub async fn ended_by_loss(connection: &ssh::Connection, e: &anyhow::Error) -> bool {
    let mut ended = connection.disconnect_reason();
    if ended.borrow().is_some() {
        return true;
    }
    if !may_be_lost(e) {
        return false;
    }
    connection.liveness().expect_reply();
    tokio::time::timeout(LOSS_WAIT, ended.wait_for(Option::is_some)).await.is_ok_and(|ended| ended.is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn where_a_file_cut_short_goes_on() {
        let window = TRANSFER_WRITES_IN_FLIGHT;
        assert_eq!(download_from(0, 100), Some(0));
        assert_eq!(download_from(40, 100), Some(40));
        assert_eq!(download_from(100, 100), Some(100));
        assert_eq!(download_from(101, 100), None);

        // An upload writes again what may have been in flight.
        let size = 10 * window;
        assert_eq!(upload_from(0, size), Some(0));
        assert_eq!(upload_from(window / 2, size), Some(0));
        assert_eq!(upload_from(3 * window, size), Some(2 * window));
        assert_eq!(upload_from(size, size), Some(9 * window));
        assert_eq!(upload_from(size + 1, size), None);
    }

    #[test]
    fn failures_a_lost_connection_may_cause() {
        use russh_sftp::protocol::{Status, StatusCode};
        let status = || {
            SftpError::Status(Status {
                id: 1,
                status_code: StatusCode::PermissionDenied,
                error_message: "".into(),
                language_tag: "".into(),
            })
        };
        let in_file = |e: SftpError| anyhow::Error::from(std::io::Error::from(e));
        let with_context = |e: anyhow::Error| e.context(Error::new("transfer.uploadFailed").param("path", "a"));
        assert!(may_be_lost(&with_context(SftpError::Timeout.into())));
        assert!(may_be_lost(&with_context(in_file(SftpError::UnexpectedBehavior("session closed".into())))));
        assert!(may_be_lost(&with_context(Error::new("sftp.openTimeout").into())));
        assert!(!may_be_lost(&with_context(status().into())));
        assert!(!may_be_lost(&with_context(in_file(status()))));
        assert!(!may_be_lost(&with_context(std::io::Error::from(std::io::ErrorKind::StorageFull).into())));
        assert!(!may_be_lost(&with_context(std::io::Error::other("Permission denied").into())));
    }
}
