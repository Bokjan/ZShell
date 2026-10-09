//! Names chosen by the other side (an SFTP server's listing, a ZMODEM sender) for files we
//! create locally. A name must stay a single file name inside the folder it is saved to:
//! `Path::join` replaces the folder when given an absolute path (or on Windows, a drive
//! prefix such as `C:x`), and `..` walks out of it.
//!
//! Files we name ourselves next to existing ones (downloads, received files, logs) get a
//! free name with `create_unique`.

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};

/// Characters Windows does not allow in file names, besides control characters.
const WINDOWS_RESERVED_CHARS: &str = r#"<>:"\|?*"#;

/// `name` as the name of a file or folder to create locally, or `None` if it isn't a single
/// name (empty, `.`, `..`, or containing `/` or NUL).
///
/// On Windows, names that are valid on the server but not here are adjusted rather than
/// refused: reserved characters (including `\`, a path separator, and `:`, which would name
/// an alternate data stream) and trailing dots and spaces (which Windows drops) become `_`,
/// and device names such as `CON` or `com1.txt` get a `_` in front.
pub fn local_file_name(name: &str) -> Option<String> {
    file_name_for(name, cfg!(windows))
}

fn file_name_for(name: &str, windows: bool) -> Option<String> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
        return None;
    }
    let name = if windows { windows_name(name) } else { name.to_owned() };
    // Whatever the rules above miss, the result must be exactly one ordinary component.
    let mut components = Path::new(&name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(component)), None) if component == OsStr::new(&name) => Some(name),
        _ => None,
    }
}

fn windows_name(name: &str) -> String {
    let mut name: String =
        name.chars().map(|c| if u32::from(c) < 0x20 || WINDOWS_RESERVED_CHARS.contains(c) { '_' } else { c }).collect();
    let kept = name.trim_end_matches(['.', ' ']).len();
    let dropped = name.len() - kept;
    name.replace_range(kept.., &"_".repeat(dropped));
    let stem = name.split('.').next().unwrap_or_default().trim_end_matches(' ');
    if is_device_name(stem) {
        name.insert(0, '_');
    }
    name
}

/// Windows device names, which open the device instead of a file whatever the extension.
fn is_device_name(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    match upper.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        _ => ["COM", "LPT"].iter().any(|prefix| {
            upper.strip_prefix(prefix).is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"))
        }),
    }
}

/// Creates `path`, or `name (1).ext`, `name (2).ext`, … when it is taken, with `create`,
/// which must fail for a name that exists (`create_new`, `create_dir`). The name is chosen
/// and taken in one step, so two downloads started together, or `README` and `readme` on a
/// file system that ignores case, get a name each.
pub fn create_unique<T>(path: &Path, create: impl Fn(&Path) -> io::Result<T>) -> io::Result<(PathBuf, T)> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 0u32.. {
        let candidate = if n == 0 { path.to_path_buf() } else { path.with_file_name(format!("{stem} ({n}){ext}")) };
        match create(&candidate) {
            Ok(made) => return Ok((candidate, made)),
            // Windows reports a folder in the way of a new file as access denied.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists || candidate.symlink_metadata().is_ok() => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

/// A new, empty file at `path` or a free name next to it.
pub fn create_unique_file(path: &Path) -> io::Result<(PathBuf, File)> {
    create_unique(path, |path| OpenOptions::new().write(true).create_new(true).open(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_unique_names() {
        let dir = std::env::temp_dir().join(format!("zshell-unique-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let names: Vec<_> = (0..3).map(|_| create_unique_file(&dir.join("a.txt")).unwrap().0).collect();
        assert_eq!(names, [dir.join("a.txt"), dir.join("a (1).txt"), dir.join("a (2).txt")]);
        let (folder, ()) = create_unique(&dir.join("a.txt"), |path| std::fs::create_dir(path)).unwrap();
        assert_eq!(folder, dir.join("a (3).txt"));
        assert!(folder.is_dir());
        // A folder in the way of a file.
        std::fs::create_dir(dir.join("b")).unwrap();
        assert_eq!(create_unique_file(&dir.join("b")).unwrap().0, dir.join("b (1)"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refuses_anything_but_one_name() {
        for windows in [false, true] {
            for name in ["", ".", "..", "../x", "a/b", "/etc/passwd", "a\0b"] {
                assert_eq!(file_name_for(name, windows), None, "{name:?} (windows: {windows})");
            }
        }
    }

    #[test]
    fn keeps_valid_names() {
        for name in ["a.txt", "..hidden", "a b", "日本語.txt", "report (1).pdf"] {
            assert_eq!(local_file_name(name).as_deref(), Some(name));
        }
    }

    #[cfg(unix)]
    #[test]
    fn keeps_names_only_windows_rejects() {
        for name in ["a:b", "x\\..\\y", "CON", "trailing."] {
            assert_eq!(local_file_name(name).as_deref(), Some(name));
        }
    }

    #[test]
    fn adjusts_names_windows_cannot_create() {
        let cases = [
            (r"..\..\Startup\x.bat", ".._.._Startup_x.bat"),
            ("C:evil.dll", "C_evil.dll"),
            ("a:b", "a_b"),
            ("what?*.txt", "what__.txt"),
            ("tab\there", "tab_here"),
            ("name. ", "name__"),
            ("...", "___"),
            ("CON", "_CON"),
            ("com1.txt", "_com1.txt"),
            ("LPT¹", "_LPT¹"),
            ("nul .tar.gz", "_nul .tar.gz"),
            ("CONSOLE", "CONSOLE"),
            ("COM10", "COM10"),
        ];
        for (name, expected) in cases {
            assert_eq!(file_name_for(name, true).as_deref(), Some(expected), "{name:?}");
        }
    }
}
