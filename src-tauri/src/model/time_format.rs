//! The time that starts each line of a session log, and each line of our own messages in the
//! terminal (`logs.timeFormat`): strftime's numeric specifiers only, which come out the same
//! in every language, with any other text as it is.
//!
//! Formats are parsed and checked here only (the one in effect is `logging::time_format`'s).
//! The frontend is given the parsed parts
//! ([`logs_time_format`](crate::commands::logs::logs_time_format) for the settings, and
//! `DEFAULT_TIME_FORMAT_PARTS` before the first answer) and writes its own messages' times from
//! them without parsing anything.

use chrono::{Datelike, NaiveDateTime, Timelike};
use serde::Serialize;
use ts_rs::TS;

use crate::error::{Error, Result};

pub const DEFAULT_TIME_FORMAT: &str = "[%Y-%m-%d %H:%M:%S]";
/// The longest format, in characters: the time goes before every line.
pub const TIME_FORMAT_MAX: usize = 64;

/// A part of a parsed time format.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TimePart {
    Text {
        text: String,
    },
    /// `%Y`: four digits.
    Year,
    /// `%y`: the last two digits of the year.
    ShortYear,
    /// `%m`, `%d`, `%H`, `%M`, `%S`: two digits each.
    Month,
    Day,
    Hour,
    Minute,
    Second,
    /// `%3f`: three digits.
    Millisecond,
}

/// A parsed time format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeFormat(pub Vec<TimePart>);

impl TimeFormat {
    pub fn parse(format: &str) -> Result<Self> {
        if format.chars().count() > TIME_FORMAT_MAX {
            return Err(Error::new("log.timeFormatTooLong").param("max", TIME_FORMAT_MAX));
        }
        if format.chars().any(char::is_control) {
            return Err(Error::new("log.timeFormatControl"));
        }
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut chars = format.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                text.push(c);
                continue;
            }
            let rest = chars.as_str();
            let (part, len) = match rest.chars().next() {
                Some('%') => {
                    text.push('%');
                    chars.next();
                    continue;
                }
                Some('Y') => (TimePart::Year, 1),
                Some('y') => (TimePart::ShortYear, 1),
                Some('m') => (TimePart::Month, 1),
                Some('d') => (TimePart::Day, 1),
                Some('H') => (TimePart::Hour, 1),
                Some('M') => (TimePart::Minute, 1),
                Some('S') => (TimePart::Second, 1),
                _ if rest.starts_with("3f") => (TimePart::Millisecond, 2),
                next => {
                    let spec: String = std::iter::once('%').chain(next).collect();
                    return Err(Error::new("log.timeFormatUnknown").param("spec", spec));
                }
            };
            if !text.is_empty() {
                parts.push(TimePart::Text { text: std::mem::take(&mut text) });
            }
            parts.push(part);
            chars = rest[len..].chars();
        }
        if !text.is_empty() {
            parts.push(TimePart::Text { text });
        }
        Ok(Self(parts))
    }

    pub fn format(&self, at: NaiveDateTime) -> String {
        let mut out = String::new();
        for part in &self.0 {
            match part {
                TimePart::Text { text } => out.push_str(text),
                TimePart::Year => out.push_str(&format!("{:04}", at.year())),
                TimePart::ShortYear => out.push_str(&format!("{:02}", at.year().rem_euclid(100))),
                TimePart::Month => out.push_str(&format!("{:02}", at.month())),
                TimePart::Day => out.push_str(&format!("{:02}", at.day())),
                TimePart::Hour => out.push_str(&format!("{:02}", at.hour())),
                TimePart::Minute => out.push_str(&format!("{:02}", at.minute())),
                // A leap second counts as the one before.
                TimePart::Second => out.push_str(&format!("{:02}", at.second())),
                TimePart::Millisecond => out.push_str(&format!("{:03}", at.nanosecond() % 1_000_000_000 / 1_000_000)),
            }
        }
        out
    }
}

impl Default for TimeFormat {
    fn default() -> Self {
        Self::parse(DEFAULT_TIME_FORMAT).expect("the default time format is valid")
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn at() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 3, 5).unwrap().and_hms_milli_opt(7, 8, 9, 45).unwrap()
    }

    fn formatted(format: &str) -> String {
        TimeFormat::parse(format).unwrap().format(at())
    }

    /// The same examples as `src/lib/terminalNotice.test.ts`, which formats the parts.
    #[test]
    fn formats_each_specifier() {
        assert_eq!(formatted(DEFAULT_TIME_FORMAT), "[2026-03-05 07:08:09]");
        assert_eq!(formatted("%y/%m/%d %H:%M:%S.%3f"), "26/03/05 07:08:09.045");
        assert_eq!(formatted("100%% 时间 %H"), "100% 时间 07");
        assert_eq!(formatted(""), "");
    }

    #[test]
    fn keeps_text_between_specifiers_in_one_part() {
        let parts = TimeFormat::parse("<%H>%%%M").unwrap().0;
        let text = |text: &str| TimePart::Text { text: text.to_owned() };
        assert_eq!(parts, [text("<"), TimePart::Hour, text(">%"), TimePart::Minute]);
    }

    #[test]
    fn refuses_what_it_cannot_write_the_same_everywhere() {
        let code = |format: &str| TimeFormat::parse(format).unwrap_err().code();
        assert_eq!(code("%A %H"), "log.timeFormatUnknown");
        assert_eq!(code("%H:%"), "log.timeFormatUnknown");
        assert_eq!(code("%2f"), "log.timeFormatUnknown");
        assert_eq!(code("%H\n"), "log.timeFormatControl");
        assert_eq!(code("\x1b[31m%H"), "log.timeFormatControl");
        assert_eq!(code(&"x".repeat(TIME_FORMAT_MAX + 1)), "log.timeFormatTooLong");
        assert!(TimeFormat::parse(&"时".repeat(TIME_FORMAT_MAX)).is_ok());
        let unknown = TimeFormat::parse("%é").unwrap_err().to_string();
        assert!(unknown.contains("%é"), "{unknown}");
    }
}
