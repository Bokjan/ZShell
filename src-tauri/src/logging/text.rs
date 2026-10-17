//! Plain-text logs: the line being shown, with the cursor's column, as a shell's line editing
//! leaves it (see the module docs of `logging`).

use chrono::Local;
use unicode_width::UnicodeWidthChar;

/// Columns kept for the line being edited. Output is not to be trusted with memory (a long
/// minified file, or cursor movements of up to 65535 columns each): text going past this
/// is passed on (see [`Line::spill`]) and the cursor stays within it.
pub(super) const MAX_COLUMNS: usize = 4096;
/// Bytes kept on one column, for characters of no width (combining marks).
pub(super) const MAX_CELL_LEN: usize = 32;

/// One column of the line: a character and the marks joined to it, kept inline (a log may
/// take every character of a busy terminal).
#[derive(Clone, Copy)]
pub(super) struct Cell {
    bytes: [u8; MAX_CELL_LEN],
    len: u8,
}

impl Cell {
    const BLANK: Self = Self::of(' ');
    /// A wide character's second column.
    const EMPTY: Self = Self { bytes: [0; MAX_CELL_LEN], len: 0 };

    const fn of(c: char) -> Self {
        let mut cell = Self::EMPTY;
        cell.len = c.encode_utf8(&mut cell.bytes).len() as u8;
        cell
    }

    /// Adds a mark of no width, if there is room.
    fn push(&mut self, c: char) {
        let len = self.len as usize;
        if len + c.len_utf8() <= MAX_CELL_LEN {
            self.len += c.encode_utf8(&mut self.bytes[len..]).len() as u8;
        }
    }

    pub(super) fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or_default()
    }
}

/// The line being shown, as plain text, with the cursor's column: carriage returns,
/// backspaces and the cursor movements and erasures a shell uses to edit its command line
/// are applied, so a line comes out as the screen showed it. Complete lines go to `out`.
#[derive(Default)]
pub(super) struct Line {
    /// One per column; a wide character's second column is empty, and characters of no
    /// width (combining marks) join the one before.
    pub(super) cells: Vec<Cell>,
    pub(super) cursor: usize,
    /// Part of the line has already gone to `out`.
    pub(super) spilled: bool,
    pub(super) timestamps: bool,
    /// The last timestamp written, with the second it is for.
    pub(super) stamp: Option<(i64, String)>,
    /// The cells' text, kept to be reused.
    pub(super) text: String,
    pub(super) out: Vec<u8>,
}

impl Line {
    pub(super) fn end(&mut self) {
        self.spill();
        self.out.push(b'\n');
        self.spilled = false;
    }

    /// Passes on what the line has so far, without ending it, to go on at column 0: for a
    /// line longer than `MAX_COLUMNS`, whose start is past editing. Trailing blanks are
    /// dropped, as at the end of a line: cursor movements alone (a few bytes each) would
    /// otherwise turn into whole lines of spaces in the file.
    pub(super) fn spill(&mut self) {
        self.start_out();
        self.collect_text();
        self.out.extend(self.text.trim_end().as_bytes());
        self.cells.clear();
        self.cursor = 0;
        self.spilled = true;
    }

    pub(super) fn collect_text(&mut self) {
        self.text.clear();
        self.text.extend(self.cells.iter().map(Cell::as_str));
    }

    pub(super) fn start_out(&mut self) {
        if self.timestamps && !self.spilled {
            let now = Local::now();
            let second = now.timestamp();
            if self.stamp.as_ref().is_none_or(|(at, _)| *at != second) {
                self.stamp = Some((second, now.format("[%Y-%m-%d %H:%M:%S] ").to_string()));
            }
            if let Some((_, stamp)) = &self.stamp {
                self.out.extend(stamp.as_bytes());
            }
        }
    }

    /// Ends a line left unfinished (at a note, or when the log closes).
    pub(super) fn finish(&mut self) {
        self.collect_text();
        if self.spilled || !self.text.trim().is_empty() {
            self.end();
        }
        self.cells.clear();
        self.cursor = 0;
    }

    /// Moves the cursor to column `to`, within `MAX_COLUMNS`, padding the line to reach it.
    fn move_to(&mut self, to: usize) {
        self.cursor = to.min(MAX_COLUMNS);
        self.pad(self.cursor);
    }

    /// Makes the line reach column `to`, with spaces.
    fn pad(&mut self, to: usize) {
        if self.cells.len() < to {
            self.cells.resize(to, Cell::BLANK);
        }
    }
}

impl vte::Perform for Line {
    fn print(&mut self, c: char) {
        match c.width().unwrap_or(0) {
            0 => {
                if let Some(cell) = self.cursor.checked_sub(1).and_then(|i| self.cells.get_mut(i)) {
                    cell.push(c);
                }
            }
            width => {
                if self.cursor + width > MAX_COLUMNS {
                    self.spill();
                }
                self.pad(self.cursor + width);
                self.cells[self.cursor] = Cell::of(c);
                if width == 2 {
                    self.cells[self.cursor + 1] = Cell::EMPTY;
                }
                self.cursor += width;
            }
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.end(),
            b'\r' => self.cursor = 0,
            0x08 => self.cursor = self.cursor.saturating_sub(1),
            b'\t' => self.move_to((self.cursor / 8 + 1) * 8),
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, _intermediates: &[u8], _ignore: bool, action: char) {
        let first = params.iter().next().map_or(0, |p| p[0] as usize);
        let count = first.max(1);
        match action {
            // Cursor forward, back, to a column.
            'C' => self.move_to(self.cursor + count),
            'D' => self.cursor = self.cursor.saturating_sub(count),
            'G' => self.move_to(count - 1),
            // Erase to the end of the line, from its start, or all of it.
            'K' => match first {
                0 => self.cells.truncate(self.cursor),
                1 => self.cells.iter_mut().take(self.cursor + 1).for_each(|cell| *cell = Cell::BLANK),
                _ => self.cells.clear(),
            },
            // Delete, insert or blank characters at the cursor.
            'P' => {
                let end = (self.cursor + count).min(self.cells.len());
                if self.cursor < end {
                    self.cells.drain(self.cursor..end);
                }
            }
            // What is pushed past the last column is lost, as on a screen.
            '@' => {
                self.pad(self.cursor);
                let count = count.min(MAX_COLUMNS - self.cursor);
                self.cells.splice(self.cursor..self.cursor, std::iter::repeat_n(Cell::BLANK, count));
                self.cells.truncate(MAX_COLUMNS);
            }
            'X' => {
                let end = (self.cursor + count).min(MAX_COLUMNS);
                self.pad(end);
                self.cells[self.cursor..end].fill(Cell::BLANK);
            }
            _ => {}
        }
    }
}
