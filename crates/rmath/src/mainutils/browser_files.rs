//! Bounded files supplied by an embedding host.

use crate::sexp::instance::with_current_instance;
use std::collections::HashMap;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_SESSION_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FILE_COUNT: usize = 128;

struct StoredFile {
    bytes: Vec<u8>,
    mtime: f64,
    atime: f64,
    ctime: f64,
}

/// Timestamps are Unix seconds, including a fractional part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrowserFileInfo {
    pub size: u64,
    pub mtime: f64,
    pub atime: f64,
    pub ctime: f64,
}

#[derive(Default)]
pub struct BrowserFileStore {
    files: HashMap<String, StoredFile>,
    total_bytes: usize,
}

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
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

pub fn info_current(path: &str) -> Option<BrowserFileInfo> {
    with_current_instance(|instance| unsafe { (*instance).browser_files.info(path) }).flatten()
}

pub fn set_time_current(path: &str, secs: f64) -> bool {
    with_current_instance(|instance| unsafe {
        (*instance).browser_files.set_file_time(path, secs)
    })
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
    use std::io::Read;
    let bytes = read_bytes_or_host(path)?;
    let text = if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&bytes[..]).read_to_end(&mut out)?;
        out
    } else if bytes.len() >= 3 && bytes.starts_with(b"BZh") {
        let mut out = Vec::new();
        bzip2::read::BzDecoder::new(&bytes[..]).read_to_end(&mut out)?;
        out
    } else if bytes.len() >= 6 && bytes.starts_with(&[0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00]) {
        let mut out = Vec::new();
        lzma_rs::xz_decompress(&mut &bytes[..], &mut out)
            .map_err(|err| io::Error::other(err.to_string()))?;
        out
    } else {
        bytes
    };
    String::from_utf8(text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
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
        self.files.get(path).map(|file| file.bytes.as_slice())
    }

    pub fn info(&self, path: &str) -> Option<BrowserFileInfo> {
        self.files.get(path).map(|file| BrowserFileInfo {
            size: file.bytes.len() as u64,
            mtime: file.mtime,
            atime: file.atime,
            ctime: file.ctime,
        })
    }

    /// GNU `Sys.setFileTime` updates modification and access time together.
    /// A non-finite time is rejected and the stored times stay unchanged.
    pub fn set_file_time(&mut self, path: &str, secs: f64) -> bool {
        if !secs.is_finite() {
            return false;
        }
        let Some(file) = self.files.get_mut(path) else {
            return false;
        };
        file.mtime = secs;
        file.atime = secs;
        file.ctime = unix_now();
        true
    }

    pub fn put(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        Self::validate_path(path)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(format!(
                "browser file exceeds {} byte limit",
                MAX_FILE_BYTES
            ));
        }
        let old = self.files.get(path).map_or(0, |file| file.bytes.len());
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
        let now = unix_now();
        let ctime = self.files.get(path).map(|file| file.ctime).unwrap_or(now);
        self.files.insert(
            path.to_owned(),
            StoredFile {
                bytes: bytes.to_vec(),
                mtime: now,
                atime: now,
                ctime,
            },
        );
        self.total_bytes = total;
        Ok(())
    }

    pub fn clear(&mut self) {
        self.files.clear();
        self.total_bytes = 0;
    }

    pub fn remove(&mut self, path: &str) -> bool {
        let Some(file) = self.files.remove(path) else {
            return false;
        };
        self.total_bytes = self.total_bytes.saturating_sub(file.bytes.len());
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

    #[test]
    fn set_file_time_roundtrips_and_rejects_a_missing_or_nonfinite_time() {
        let mut store = BrowserFileStore::default();
        store.put("note.txt", b"hello").unwrap();
        let created = store.info("note.txt").expect("stored file");
        assert_eq!(created.size, 5);
        assert!(created.mtime.is_finite() && created.atime.is_finite());
        assert!(store.set_file_time("note.txt", 1_700_000_000.25));
        let updated = store.info("note.txt").expect("stored file");
        assert_eq!(updated.size, 5);
        assert_eq!(updated.mtime, 1_700_000_000.25);
        assert_eq!(updated.atime, 1_700_000_000.25);
        assert!(updated.ctime.is_finite());
        assert!(!store.set_file_time("missing.txt", 10.0));
        assert!(store.info("missing.txt").is_none());
        assert!(!store.set_file_time("note.txt", f64::NAN));
        assert_eq!(store.info("note.txt").unwrap().mtime, 1_700_000_000.25);
    }
}
