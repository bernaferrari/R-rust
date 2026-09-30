//! Bounded files supplied by an embedding host.

use crate::sexp::instance::with_current_instance;
use std::collections::HashMap;
use std::io;
#[cfg(not(target_arch = "wasm32"))]
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

/// Unix seconds, including a fraction.
///
/// `SystemTime::now` panics on bare `wasm32-unknown-unknown` before
/// `unwrap_or` can run, so automatic stamps there are the Unix epoch.
/// An explicit `Sys.setFileTime` value is still stored as given.
fn unix_now() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        0.0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs_f64())
            .unwrap_or(0.0)
    }
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
    with_current_instance(|instance| unsafe { (*instance).browser_files.set_file_time(path, secs) })
        .unwrap_or(false)
}

pub fn write_current(path: &str, bytes: &[u8]) -> Result<(), String> {
    with_current_instance(|instance| unsafe { (*instance).browser_files.put(path, bytes) })
        .ok_or_else(|| "no active R session".to_string())?
}

pub fn remove_current(path: &str) -> bool {
    with_current_instance(|instance| unsafe { (*instance).browser_files.remove(path) })
        .unwrap_or(false)
}

pub fn list_current() -> Vec<String> {
    with_current_instance(|instance| unsafe { (*instance).browser_files.names() })
        .unwrap_or_default()
}

/// Copy one flat key onto another. A missing source is false and does not
/// create `to` or read the host.
pub fn copy_current(from: &str, to: &str) -> bool {
    with_current_instance(|instance| unsafe { (*instance).browser_files.copy(from, to) })
        .unwrap_or(false)
}

pub fn write_text_or_host(path: &str, bytes: &[u8]) -> std::io::Result<()> {
    let enabled = with_current_instance(|instance| unsafe { (*instance).browser_files_enabled })
        .unwrap_or(false);
    if contains_current(path) || enabled {
        return write_current(path, bytes).map_err(std::io::Error::other);
    }
    std::fs::write(path, bytes)
}

/// Text whose native allocation stays admitted until the caller finishes
/// parsing or evaluating it. The token does not borrow the arena across reentry.
pub(crate) struct FileText {
    text: String,
    _reservation: Option<crate::sexp::memory::TransientReservation>,
}

impl std::ops::Deref for FileText {
    type Target = str;
    fn deref(&self) -> &str {
        &self.text
    }
}

fn memory_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::OutOfMemory,
        "file text exceeds session memory limit",
    )
}

fn reserve(bytes: usize) -> io::Result<Option<crate::sexp::memory::TransientReservation>> {
    with_current_instance(|instance| unsafe {
        (*instance)
            .arena
            .try_reserve_transient(bytes)
            .ok_or_else(memory_error)
            .map(Some)
    })
    .unwrap_or(Ok(None))
}

/// Admit an already owned text buffer (e.g. a connection snapshot) for its
/// remaining lifetime. Newly read buffers use `TextBuffer` admission first.
pub(crate) fn admit_text(text: String) -> io::Result<FileText> {
    let reservation = reserve(text.capacity())?;
    Ok(FileText {
        text,
        _reservation: reservation,
    })
}

struct TextBuffer {
    bytes: Vec<u8>,
    reservation: Option<crate::sexp::memory::TransientReservation>,
    limit: usize,
}

impl TextBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            reservation: None,
            limit,
        }
    }

    fn into_text(self) -> io::Result<FileText> {
        let text = String::from_utf8(self.bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(FileText {
            text,
            _reservation: self.reservation,
        })
    }

    fn read_from(&mut self, mut reader: impl io::Read) -> io::Result<()> {
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => return Ok(()),
                Ok(n) => {
                    io::Write::write_all(self, &chunk[..n])?;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
    }
}

impl io::Write for TextBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let len = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .ok_or_else(memory_error)?;
        if len > self.limit {
            return Err(memory_error());
        }
        if len > self.bytes.capacity() {
            let capacity = len
                .checked_next_power_of_two()
                .unwrap_or(len)
                .min(self.limit);
            // Both allocations coexist while copying. Keep the old admission
            // until its buffer is freed; charge the full new capacity first.
            let reservation = reserve(capacity)?;
            let mut replacement = Vec::new();
            replacement
                .try_reserve_exact(capacity)
                .map_err(|_| memory_error())?;
            replacement.extend_from_slice(&self.bytes);
            self.bytes = replacement;
            self.reservation = reservation;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn read_text_or_host(path: &str) -> io::Result<FileText> {
    let browser = enabled();
    let limit = if browser {
        MAX_SESSION_BYTES
    } else {
        usize::MAX
    };
    let mut input = TextBuffer::new(limit);
    let stored = with_current_instance(|instance| unsafe {
        let Some(len) = (*instance).browser_files.read(path).map(<[u8]>::len) else {
            return Ok::<bool, io::Error>(false);
        };
        // Do not retain a file-store borrow while admission touches the arena.
        let reservation = reserve(len)?;
        input
            .bytes
            .try_reserve_exact(len)
            .map_err(|_| memory_error())?;
        input
            .bytes
            .extend_from_slice((*instance).browser_files.read(path).unwrap());
        input.reservation = reservation;
        Ok(true)
    })
    .unwrap_or(Ok(false))?;
    if !stored {
        if browser {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "browser file not found",
            ));
        }
        input.read_from(std::fs::File::open(path)?)?;
    }
    let bytes = &input.bytes;
    let mut output = TextBuffer::new(limit);
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let _workspace = reserve(128 * 1024)?;
        output.read_from(flate2::read::GzDecoder::new(&bytes[..]))?;
    } else if bytes.starts_with(b"BZh") {
        // bzip2's largest block is 900k; charge its decoder tables too.
        let _workspace = reserve(4 * 1024 * 1024)?;
        output.read_from(bzip2::read::BzDecoder::new(&bytes[..]))?;
    } else if bytes.starts_with(&[0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00]) {
        // lzma-rs accumulates an entire decoded block before writing it.
        // A streaming reader with a decoder limit also bounds that workspace.
        const WORKSPACE: usize = 16 * 1024 * 1024;
        let _workspace = reserve(WORKSPACE)?;
        output.read_from(lzma_rust2::XzReader::new_mem_limit(
            &bytes[..],
            true,
            (WORKSPACE / 1024) as u32,
        ))?;
    } else {
        return input.into_text();
    }
    output.into_text()
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

    /// Duplicate bytes under a new flat key. Missing `from` is false.
    /// A rejected `to` leaves both keys as they were.
    pub fn copy(&mut self, from: &str, to: &str) -> bool {
        let Some(bytes) = self.files.get(from).map(|file| file.bytes.clone()) else {
            return false;
        };
        self.put(to, &bytes).is_ok()
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
    use crate::sexp::{memory::ArenaBudget, session::RSession};
    use std::io::Write;

    #[test]
    fn text_admission_lasts_until_drop_and_releases_on_errors() {
        let mut session = RSession::new_without_default_packages();
        session.enable_browser_files();
        session.put_browser_file("note.txt", b"hello").unwrap();
        session.put_browser_file("bad.txt", &[0xff]).unwrap();
        let baseline = session
            .with_arena(|arena| arena.total_bytes_allocated())
            .unwrap();
        session.set_arena_budget(ArenaBudget::new(baseline + 5, 0));
        session.with_active(|| {
            let text = read_text_or_host("note.txt").unwrap();
            assert_eq!(&*text, "hello");
            assert!(
                reserve(1).is_err(),
                "text must remain admitted in the caller"
            );
            drop(text);
            assert!(read_text_or_host("bad.txt").is_err());
            assert_eq!(&*read_text_or_host("note.txt").unwrap(), "hello");
            assert!(read_text_or_host("missing").is_err());
            assert!(reserve(5).is_ok(), "all error paths release reservations");
        });
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn compressed_text_is_bounded_and_session_recovers() {
        let mut session = RSession::new_without_default_packages();
        session.enable_browser_files();
        session
            .put_browser_file("small.gz", &gzip(b"answer <- 42\n"))
            .unwrap();
        let expanded = vec![b'x'; MAX_SESSION_BYTES + 1];
        let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        bz.write_all(&expanded).unwrap();
        let mut xz = Vec::new();
        lzma_rs::xz_compress(&mut &b"answer <- 42\n"[..], &mut xz).unwrap();
        session
            .put_browser_file("bomb.gz", &gzip(&expanded))
            .unwrap();
        session
            .put_browser_file("bomb.bz2", &bz.finish().unwrap())
            .unwrap();
        session.put_browser_file("small.xz", &xz).unwrap();
        // Python lzma.compress(b'x' * (8 * 1024 * 1024 + 1)), default preset.
        session
            .put_browser_file(
                "bomb.xz",
                include_bytes!("../../tests/fixtures/browser-expansion.xz"),
            )
            .unwrap();
        session.with_active(|| {
            for path in ["bomb.gz", "bomb.bz2", "bomb.xz"] {
                assert!(
                    read_text_or_host(path).is_err(),
                    "{path} must exceed the expansion cap"
                );
                assert_eq!(&*read_text_or_host("small.gz").unwrap(), "answer <- 42\n");
            }
            assert_eq!(&*read_text_or_host("small.xz").unwrap(), "answer <- 42\n");
        });
        let baseline = session
            .with_arena(|arena| arena.total_bytes_allocated())
            .unwrap();
        session.set_arena_budget(ArenaBudget::new(baseline + 256 * 1024, 0));
        session.with_active(|| {
            assert!(read_text_or_host("bomb.gz").is_err());
            assert!(
                read_text_or_host("small.xz").is_err(),
                "decoder workspace needs admission"
            );
            assert_eq!(&*read_text_or_host("small.gz").unwrap(), "answer <- 42\n");
        });
    }

    #[test]
    fn admitted_files_preserve_scan_windows_and_compressed_source() {
        let mut session = RSession::new_without_default_packages();
        session.enable_browser_files();
        session
            .put_browser_file("words.txt", "header\nα β\nγ δ\n".as_bytes())
            .unwrap();
        session
            .put_browser_file("code.gz", &gzip(b"answer <- 42\n"))
            .unwrap();
        let (result, _, _) = session.eval_script_with_output_capture(
            "stopifnot(identical(scan('words.txt', what='', skip=1, nlines=1, quiet=TRUE), c('α', 'β'))); source('code.gz'); stopifnot(answer == 42)",
        );
        assert!(result.is_ok(), "file evaluation failed: {result:?}");
    }

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

    /// Bare wasm has no wall clock. Creation stamps and the ctime written by
    /// `set_file_time` stay at the epoch, and the caller-supplied time is kept.
    #[cfg(target_arch = "wasm32")]
    #[test]
    fn wasm_automatic_stamps_stay_at_the_epoch() {
        let mut store = BrowserFileStore::default();
        store.put("note.txt", b"hello").unwrap();
        let created = store.info("note.txt").expect("stored file");
        assert_eq!(created.mtime, 0.0);
        assert_eq!(created.atime, 0.0);
        assert_eq!(created.ctime, 0.0);
        assert!(store.set_file_time("note.txt", 1_700_000_000.25));
        let updated = store.info("note.txt").expect("stored file");
        assert_eq!(updated.mtime, 1_700_000_000.25);
        assert_eq!(updated.atime, 1_700_000_000.25);
        assert_eq!(updated.ctime, 0.0);
    }

    #[test]
    fn copy_duplicates_bytes_and_a_missing_source_writes_nothing() {
        let mut store = BrowserFileStore::default();
        store.put("note.txt", b"hello").unwrap();
        assert!(!store.copy("missing.txt", "other.txt"));
        assert!(store.read("other.txt").is_none());
        assert!(store.copy("note.txt", "other.txt"));
        assert_eq!(store.read("other.txt"), Some(b"hello".as_slice()));
        assert_eq!(store.read("note.txt"), Some(b"hello".as_slice()));
        assert!(store.remove("note.txt"));
        assert!(!store.remove("note.txt"));
        assert_eq!(store.names(), vec!["other.txt".to_string()]);
    }
}
