//! Our own messages in the terminal (connecting, how a session ended, transfers...), each line
//! starting with the time it is printed, in the settings' format (see
//! `logging/time_format.rs`). The login dialogue (host keys, passwords), echoed input and other
//! programs' output are printed as they are ([`SessionSink::print`]).
//!
//! The frontend writes its own messages the same way (`src/lib/terminalNotice.ts`); the
//! tests of both check the same example.
//!
//! [`SessionSink::print`]: super::SessionSink::print

/// How a message's text is shown; the time before it is always dim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Dim,
    Warning,
    Error,
}

impl Tone {
    fn sgr(self) -> &'static str {
        match self {
            Tone::Plain => "",
            Tone::Dim => "\x1b[2m",
            Tone::Warning => "\x1b[33m",
            Tone::Error => "\x1b[31m",
        }
    }
}

/// A message as written to the terminal, and without the times, for a log that starts its
/// lines with the time itself.
pub(super) struct Notice {
    pub shown: String,
    pub unstamped: String,
}

/// `text`, each line starting with `time`. Ends with a line break, unless `end_line` is false
/// (a progress line, which the next message replaces).
pub(super) fn render(tone: Tone, text: &str, time: &str, end_line: bool) -> Notice {
    let stamp = format!("\x1b[2m{time}\x1b[0m ");
    let mut notice = Notice { shown: String::new(), unstamped: String::new() };
    let text = text.trim_end_matches(['\r', '\n']);
    let lines = text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line));
    for (i, line) in lines.enumerate() {
        if i > 0 {
            notice.shown.push_str("\r\n");
            notice.unstamped.push_str("\r\n");
        }
        let line = match tone {
            Tone::Plain => line.to_owned(),
            _ => format!("{}{line}\x1b[0m", tone.sgr()),
        };
        notice.shown.push_str(&stamp);
        notice.shown.push_str(&line);
        notice.unstamped.push_str(&line);
    }
    if end_line {
        notice.shown.push_str("\r\n");
        notice.unstamped.push_str("\r\n");
    }
    notice
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: &str = "[2026-03-05 07:08:09]";

    /// The same example as `src/lib/notice.test.ts`.
    #[test]
    fn starts_each_line_with_the_time() {
        let notice = render(Tone::Error, "The connection was lost\nPress Enter\n", AT, true);
        assert_eq!(
            notice.shown,
            "\x1b[2m[2026-03-05 07:08:09]\x1b[0m \x1b[31mThe connection was lost\x1b[0m\r\n\x1b[2m[2026-03-05 07:08:09]\x1b[0m \x1b[31mPress Enter\x1b[0m\r\n"
        );
        assert_eq!(notice.unstamped, "\x1b[31mThe connection was lost\x1b[0m\r\n\x1b[31mPress Enter\x1b[0m\r\n");
    }

    #[test]
    fn leaves_a_progress_line_open() {
        let notice = render(Tone::Plain, "Sending a.txt\r\n", AT, false);
        assert_eq!(notice.shown, "\x1b[2m[2026-03-05 07:08:09]\x1b[0m Sending a.txt");
        assert_eq!(notice.unstamped, "Sending a.txt");
    }
}
