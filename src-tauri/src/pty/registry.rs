//! Reading the registry, for the shells installed here (see `discover.rs`): strings, numbers
//! and the names of a key's subkeys. Missing keys and values, and values of another type, read
//! as nothing.

use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, KEY_ENUMERATE_SUB_KEYS, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// The string `value` of key `path` under `root`.
pub fn string(root: HKEY, path: &str, value: &str) -> Option<String> {
    let (path, value) = (wide(path), wide(value));
    let mut buffer = vec![0u16; 260];
    loop {
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: NUL-terminated wide strings; `buffer` holds `size` bytes, and RRF_RT_REG_SZ lets
        // only a string through, NUL-terminated within them.
        let status = unsafe {
            RegGetValueW(root, path.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buffer.as_mut_ptr().cast(), &mut size)
        };
        match status {
            ERROR_SUCCESS => {
                let length = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
                return String::from_utf16(&buffer[..length]).ok();
            }
            // Changed in between, or longer than the buffer: `size` is what it takes now.
            ERROR_MORE_DATA if buffer.len() < (size as usize).div_ceil(2) + 1 => buffer.resize((size as usize).div_ceil(2) + 1, 0),
            _ => return None,
        }
    }
}

/// The number (`REG_DWORD`) `value` of key `path` under `root`.
pub fn dword(root: HKEY, path: &str, value: &str) -> Option<u32> {
    let (path, value) = (wide(path), wide(value));
    let mut number = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: NUL-terminated wide strings; RRF_RT_REG_DWORD lets only four bytes through.
    let status = unsafe {
        RegGetValueW(root, path.as_ptr(), value.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), (&raw mut number).cast(), &mut size)
    };
    (status == ERROR_SUCCESS).then_some(number)
}

/// The names of the subkeys of key `path` under `root`.
pub fn subkeys(root: HKEY, path: &str) -> Vec<String> {
    let path = wide(path);
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: a NUL-terminated wide string; `key` is closed below.
    if unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, KEY_ENUMERATE_SUB_KEYS, &mut key) } != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut names = Vec::new();
    // Key names are at most 255 characters.
    let mut buffer = [0u16; 256];
    for index in 0.. {
        let mut length = buffer.len() as u32;
        // SAFETY: `buffer` holds `length` characters; the other outputs are not asked for.
        let status = unsafe {
            RegEnumKeyExW(
                key,
                index,
                buffer.as_mut_ptr(),
                &mut length,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            break;
        }
        names.push(String::from_utf16_lossy(&buffer[..length as usize]));
    }
    // SAFETY: opened above.
    unsafe { RegCloseKey(key) };
    names
}
