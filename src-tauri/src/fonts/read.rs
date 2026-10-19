//! Big-endian OpenType data, read without trusting it: every read is checked, and an offset
//! that points outside its table reads as missing.

/// Part of a table, from where its offsets count.
#[derive(Clone, Copy)]
pub struct Data<'a>(pub &'a [u8]);

impl<'a> Data<'a> {
    pub fn u16(self, at: usize) -> Option<u16> {
        let bytes = self.0.get(at..at.checked_add(2)?)?;
        Some(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    pub fn u32(self, at: usize) -> Option<u32> {
        let bytes = self.0.get(at..at.checked_add(4)?)?;
        Some(u32::from_be_bytes(bytes.try_into().ok()?))
    }

    pub fn tag(self, at: usize) -> Option<[u8; 4]> {
        self.0.get(at..at.checked_add(4)?)?.try_into().ok()
    }

    /// The data at the 16-bit offset stored at `at`; `None` for a null offset.
    pub fn offset16(self, at: usize) -> Option<Data<'a>> {
        match usize::from(self.u16(at)?) {
            0 => None,
            offset => self.0.get(offset..).map(Data),
        }
    }

    /// The data at the 32-bit offset stored at `at`; `None` for a null offset.
    pub fn offset32(self, at: usize) -> Option<Data<'a>> {
        match usize::try_from(self.u32(at)?).ok()? {
            0 => None,
            offset => self.0.get(offset..).map(Data),
        }
    }

    /// `count` 16-bit values from `at`.
    pub fn u16s(self, at: usize, count: usize) -> Option<Vec<u16>> {
        (0..count).map(|i| self.u16(at + i * 2)).collect()
    }

    /// A count at `at` and the values after it.
    pub fn u16_array(self, at: usize) -> Option<Vec<u16>> {
        self.u16s(at + 2, usize::from(self.u16(at)?))
    }
}
