//! What a panic leaves behind. A release build ends at once on a panic (`panic = "abort"` in
//! `Cargo.toml`): unwinding would end only the task that panicked, and the locks it held,
//! poisoned, would take the next tasks that use them down one after another, leaving the app
//! half broken rather than gone. Without a console (Windows) nothing would show why it ended,
//! so each panic is noted in `crash.log` in the configuration folder first. The hook runs
//! before the process aborts.

use std::fs::OpenOptions;
use std::io::Write;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};

/// Past this size, the next panic starts the file again.
const MAX_SIZE: u64 = 64 << 10;

/// Notes each panic in `path` before the default hook prints it.
pub fn record_panics(path: PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = append(&path, &describe(info));
        default(info);
    }));
}

/// One line: when, which version on which system, the thread, where and the message.
fn describe(info: &PanicHookInfo) -> String {
    let time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%:z");
    let thread = std::thread::current();
    let message = info.payload_as_str().unwrap_or("(no message)");
    let place = info.location().map(|at| format!("{}:{}:{}", at.file(), at.line(), at.column())).unwrap_or_default();
    format!(
        "{time} ZShell {} ({} {}): thread '{}' panicked at {place}: {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        thread.name().unwrap_or("unnamed"),
        message.replace('\n', " "),
    )
}

fn append(path: &Path, line: &str) -> std::io::Result<()> {
    let full = std::fs::metadata(path).is_ok_and(|metadata| metadata.len() >= MAX_SIZE);
    let mut options = OpenOptions::new();
    match full {
        true => options.write(true).truncate(true),
        false => options.append(true),
    };
    options.create(true).open(path)?.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_starts_again_once_full() {
        let path = std::env::temp_dir().join(format!("zshell-crash-{}.log", uuid::Uuid::new_v4()));
        append(&path, "one\n").unwrap();
        append(&path, "two\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        std::fs::write(&path, vec![b'x'; MAX_SIZE as usize]).unwrap();
        append(&path, "three\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "three\n");
        std::fs::remove_file(&path).unwrap();
    }
}
