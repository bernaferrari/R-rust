//! Bounded files supplied by an embedding host.

use crate::sexp::instance::with_current_instance;
use std::collections::HashMap;
use std::io;

pub const MAX_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_SESSION_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FILE_COUNT: usize = 128;

#[derive(Default)]
pub struct BrowserFileStore {
    files: HashMap<String, Vec<u8>>,
    total_bytes: usize,
}

pub fn enabled() -> bool {
    with_current_instance(|instance| unsafe { (*instance).browser_files_enabled }).unwrap_or(false)
}

pub fn read_bytes_or_host(path: &str) -> io::Result<Vec<u8>> {
    if let Some(bytes) = read_current(path) {
        return Ok(bytes);
    }
    if enabled() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "browser file not found",
        ));
    }
    std::fs::read(path)
}

pub fn read_current(path: &str) -> Option<Vec<u8>> {
    with_current_instance(|instance| unsafe {
        (*instance).browser_files.read(path).map(<[u8]>::to_vec)
    })
    .flatten()
}

pub fn contains_current(path: &str) -> bool {
    with_current_instance(|instance| unsafe { (*instance).browser_files.contains(path) })
        .unwrap_or(false)
}

pub fn write_current(path: &str, bytes: &[u8]) -> Result<(), String> {
    with_current_instance(|instance| unsafe { (*instance).browser_files.put(path, bytes) })
        .ok_or_else(|| "no active R session".to_string())?
}

pub fn write_text_or_host(path: &str, bytes: &[u8]) -> std::io::Result<()> {
    let enabled = with_current_instance(|instance| unsafe { (*instance).browser_files_enabled })
        .unwrap_or(false);
    if contains_current(path) || enabled {
        return write_current(path, bytes).map_err(std::io::Error::other);
    }
    std::fs::write(path, bytes)
}

pub fn read_text_or_host(path: &str) -> io::Result<String> {
    String::from_utf8(read_bytes_or_host(path)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

impl BrowserFileStore {
    pub fn validate_path(path: &str) -> Result<(), String> {
        if path.is_empty()
            || path.len() > 255
            || path.starts_with('/')
            || path.contains('\\')
            || path.contains(':')
            || path.bytes().any(|byte| byte == 0 || byte < 0x20)
        {
            return Err("browser file paths must be non-empty relative paths".into());
        }
        if path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err("browser file paths cannot contain empty, '.' or '..' components".into());
        }
        Ok(())
    }

    pub fn contains(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn read(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    pub fn put(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        Self::validate_path(path)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(format!(
                "browser file exceeds {} byte limit",
                MAX_FILE_BYTES
            ));
        }
        let old = self.files.get(path).map_or(0, Vec::len);
        if old == 0 && !self.files.contains_key(path) && self.files.len() >= MAX_FILE_COUNT {
            return Err(format!(
                "browser file store exceeds {} file limit",
                MAX_FILE_COUNT
            ));
        }
        let total = self
            .total_bytes
            .saturating_sub(old)
            .saturating_add(bytes.len());
        if total > MAX_SESSION_BYTES {
            return Err(format!(
                "browser file store exceeds {} byte limit",
                MAX_SESSION_BYTES
            ));
        }
        self.files.insert(path.to_owned(), bytes.to_vec());
        self.total_bytes = total;
        Ok(())
    }

    pub fn clear(&mut self) {
        self.files.clear();
        self.total_bytes = 0;
    }

    pub fn remove(&mut self, path: &str) -> bool {
        let Some(bytes) = self.files.remove(path) else {
            return false;
        };
        self.total_bytes = self.total_bytes.saturating_sub(bytes.len());
        true
    }

    pub fn names(&self) -> Vec<String> {
        let mut names = self.files.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_reject_atomically_and_replacement_reclaims_capacity() {
        let mut store = BrowserFileStore::default();
        let full = vec![7; MAX_FILE_BYTES];
        for i in 0..8 {
            store.put(&format!("{i}"), &full).unwrap();
        }
        assert!(store.put("extra", &[1]).is_err());
        assert!(store.put("0", &vec![2; MAX_FILE_BYTES + 1]).is_err());
        assert_eq!(store.read("0"), Some(full.as_slice()));
        store.put("0", b"short").unwrap();
        store.put("extra", &[1]).unwrap();
        for invalid in ["../escape", "/etc/hosts", "a/../b", "a//b", "C:x", "a\\b"] {
            assert!(store.put(invalid, b"x").is_err());
        }
        assert!(store.remove("1"));
        store.put("replacement", &full).unwrap();
        store.clear();
        for i in 0..MAX_FILE_COUNT {
            store.put(&format!("{i}"), b"").unwrap();
        }
        assert!(store.put("overflow", b"").is_err());
        store.put("0", b"replacement").unwrap();
    }
}
