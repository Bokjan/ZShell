//! Input a backend holds until the remote side, the device or its writer takes it.

use std::collections::VecDeque;

/// One piece of input waiting: data, or a break that a serial line sends between data.
#[derive(Debug, PartialEq)]
pub enum Pending {
    Data(Vec<u8>),
    Break,
}

/// The input a backend's writer has no room for yet, in order, and how many bytes of data
/// that is, which [`super::TermIo::recv`] is told. Part of the first piece may have been
/// written: a pseudo terminal or a serial line takes a few KiB at a time, and moving the rest
/// of a large paste to the front after each write would copy it over and over.
#[derive(Default)]
pub struct PendingInput {
    queue: VecDeque<Pending>,
    /// How much of the first piece has been written.
    written: usize,
    bytes: usize,
}

impl PendingInput {
    pub fn push(&mut self, data: Vec<u8>) {
        self.bytes += data.len();
        self.queue.push_back(Pending::Data(data));
    }

    pub fn push_break(&mut self) {
        self.queue.push_back(Pending::Break);
    }

    /// The bytes of data waiting.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Whether a break comes next.
    pub fn break_next(&self) -> bool {
        matches!(self.queue.front(), Some(Pending::Break))
    }

    /// The data to write next, unless nothing or a break comes first.
    pub fn data(&self) -> Option<&[u8]> {
        match self.queue.front()? {
            Pending::Data(data) => Some(&data[self.written..]),
            Pending::Break => None,
        }
    }

    /// Counts `n` bytes of [`PendingInput::data`] as written.
    pub fn wrote(&mut self, n: usize) {
        let Some(Pending::Data(data)) = self.queue.front() else { return };
        self.written += n;
        self.bytes -= n;
        if self.written >= data.len() {
            self.queue.pop_front();
            self.written = 0;
        }
    }

    /// Takes the next piece whole, for a writer that takes pieces rather than bytes (a thread
    /// of its own). Not mixed with [`PendingInput::wrote`]: the piece would lose its start.
    pub fn pop(&mut self) -> Option<Pending> {
        debug_assert_eq!(self.written, 0);
        let next = self.queue.pop_front()?;
        if let Pending::Data(data) = &next {
            self.bytes -= data.len();
        }
        Some(next)
    }

    /// Drops all of it: the writer is gone.
    pub fn clear(&mut self) {
        self.queue.clear();
        (self.written, self.bytes) = (0, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_written_in_parts_is_not_moved() {
        let mut pending = PendingInput::default();
        pending.push(b"hello".to_vec());
        pending.push(b"!".to_vec());
        assert_eq!(pending.bytes(), 6);
        pending.wrote(2);
        assert_eq!((pending.data(), pending.bytes()), (Some(&b"llo"[..]), 4));
        pending.wrote(3);
        assert_eq!((pending.data(), pending.bytes()), (Some(&b"!"[..]), 1));
        pending.wrote(1);
        assert!(pending.is_empty());
        assert_eq!((pending.data(), pending.bytes()), (None, 0));
    }

    #[test]
    fn breaks_stay_between_the_data_around_them() {
        let mut pending = PendingInput::default();
        pending.push(b"a".to_vec());
        pending.push_break();
        pending.push(b"b".to_vec());
        assert!(!pending.break_next());
        pending.wrote(1);
        assert!(pending.break_next());
        assert_eq!(pending.data(), None);
        assert_eq!(pending.pop(), Some(Pending::Break));
        assert_eq!(pending.bytes(), 1);
        assert_eq!(pending.pop(), Some(Pending::Data(b"b".to_vec())));
        assert_eq!(pending.bytes(), 0);
    }

    #[test]
    fn empty_data_is_done_when_written() {
        let mut pending = PendingInput::default();
        pending.push(Vec::new());
        assert_eq!(pending.data(), Some(&b""[..]));
        pending.wrote(0);
        assert!(pending.is_empty());
    }

    #[test]
    fn clearing_forgets_a_piece_partly_written() {
        let mut pending = PendingInput::default();
        pending.push(b"abc".to_vec());
        pending.wrote(1);
        pending.clear();
        pending.push(b"de".to_vec());
        assert_eq!((pending.data(), pending.bytes()), (Some(&b"de"[..]), 2));
    }
}
