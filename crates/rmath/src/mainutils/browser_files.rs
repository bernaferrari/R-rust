//! Bounded files supplied by an embedding host.

use crate::sexp::memory::TransientReservation;
use crate::sexp::owner::OwnerToken;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::io;
use std::rc::{Rc, Weak};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

mod paths;

pub const MAX_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_FILE_PATH_BYTES: usize = 255;
pub const MAX_SESSION_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FILE_COUNT: usize = 128;
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
const GZIP_WORKSPACE_BYTES: usize = 128 * 1024;

struct StoredFile {
    bytes: Rc<FileBytes>,
    mtime: f64,
    atime: f64,
    ctime: f64,
    mode: u32,
}

/// Timestamps are Unix seconds, including a fractional part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrowserFileInfo {
    pub size: u64,
    pub mtime: f64,
    pub atime: f64,
    pub ctime: f64,
    pub mode: u32,
}

#[derive(Default)]
pub struct BrowserFileStore {
    files: HashMap<String, StoredFile>,
    directories: HashSet<String>,
    cwd: String,
    total_bytes: usize,
    #[cfg(test)]
    test_clock: Option<f64>,
}

#[derive(Default)]
pub(crate) struct BrowserFileState {
    enabled: Cell<bool>,
    store: RefCell<BrowserFileStore>,
}

fn current_state() -> Option<Rc<BrowserFileState>> {
    OwnerToken::with_current(|owner| owner.browser_file_state())
        .ok()?
        .ok()
}

fn with_store<T>(operation: impl FnOnce(&BrowserFileStore) -> T) -> Option<T> {
    current_state().map(|state| operation(&state.store.borrow()))
}

fn with_store_mut<T>(operation: impl FnOnce(&mut BrowserFileStore) -> T) -> Option<T> {
    current_state().map(|state| operation(&mut state.store.borrow_mut()))
}

// File writes need quota admission for this same session. No R callbacks run
// while the store is borrowed.
fn with_write_store<T>(
    operation: impl FnOnce(&mut BrowserFileStore, OwnerToken<'_>) -> T,
) -> Option<T> {
    OwnerToken::with_current(|owner| {
        let state = owner.browser_file_state().ok()?;
        Some(operation(&mut state.store.borrow_mut(), owner))
    })
    .ok()
    .flatten()
}

fn admit_file_write(
    owner: OwnerToken<'_>,
    len: usize,
) -> Result<Option<TransientReservation>, String> {
    owner
        .reserve_file_bytes(len)
        .map(Some)
        .ok_or_else(|| "browser file exceeds session memory limit".to_string())
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
    current_state().is_some_and(|state| state.enabled.get())
}

pub(crate) fn enable_current() {
    if let Some(state) = current_state() {
        state.enabled.set(true);
    }
}

pub(crate) fn read_import(path: &str) -> Option<Vec<u8>> {
    with_store(|store| store.read(path).map(<[u8]>::to_vec)).flatten()
}

pub(crate) fn remove_import(path: &str) -> bool {
    with_store_mut(|store| store.remove(path)).unwrap_or(false)
}

#[cfg(test)]
pub(crate) fn set_test_clock_current(seconds: f64) {
    with_store_mut(|store| store.set_test_clock(seconds)).expect("active browser file store");
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
    with_store(|store| {
        store
            .resolve(path)
            .ok()
            .and_then(|key| store.read(&key).map(<[u8]>::to_vec))
    })
    .flatten()
}

pub fn contains_current(path: &str) -> bool {
    with_store(|store| store.resolve(path).is_ok_and(|key| store.contains(&key))).unwrap_or(false)
}

pub fn info_current(path: &str) -> Option<BrowserFileInfo> {
    with_store(|store| store.info(&store.resolve(path).ok()?)).flatten()
}

pub fn set_time_current(path: &str, secs: f64) -> bool {
    with_store_mut(|store| {
        store
            .resolve(path)
            .is_ok_and(|key| store.set_file_time(&key, secs))
    })
    .unwrap_or(false)
}

pub fn write_current(path: &str, bytes: &[u8]) -> Result<(), String> {
    write_snapshot_current(path, &[bytes]).map(drop)
}

/// Write and return one admitted file version shared by the store and caller.
pub(crate) fn write_snapshot_current(path: &str, parts: &[&[u8]]) -> Result<Rc<FileBytes>, String> {
    with_write_store(|store, owner| {
        let key = store.resolve(path)?;
        store.require_parent(&key)?;
        store.put_with_admission(&key, parts, |len| admit_file_write(owner, len))
    })
    .ok_or_else(|| "no active R session".to_string())?
}

/// Stage Rust-generated bytes under the active session's memory ledger.
#[cfg(feature = "renderplot-device")]
pub(crate) fn generate_admitted_bytes(
    limit: usize,
    generate: impl FnOnce(&mut dyn io::Write) -> io::Result<()>,
) -> io::Result<FileBytes> {
    let mut output = FileBuffer::new(limit);
    generate(&mut output)?;
    Ok(output.into_bytes())
}

/// Generate a bounded file without replacing the destination on failure.
#[cfg(feature = "renderplot-device")]
pub(crate) fn publish_generated_current(
    path: &str,
    generate: impl FnOnce(&mut dyn io::Write) -> io::Result<()>,
) -> io::Result<()> {
    let owner = current_state().ok_or_else(|| io::Error::other("no active R session"))?;
    let key = {
        let store = owner.store.borrow();
        let key = store.resolve(path).map_err(io::Error::other)?;
        store.require_parent(&key).map_err(io::Error::other)?;
        key.into_owned()
    };
    let original = Rc::downgrade(&owner);
    drop(owner);
    // Generated output calls only the Rust serializer. It cannot evaluate R.
    let output = generate_admitted_bytes(MAX_FILE_BYTES, generate)?;
    let current = current_state().ok_or_else(|| io::Error::other("no active R session"))?;
    if !Weak::ptr_eq(&original, &Rc::downgrade(&current)) {
        return Err(io::Error::other(
            "original file session is no longer active",
        ));
    }
    let mut store = current.store.borrow_mut();
    let total = store
        .total_after_write(&key, output.len())
        .map_err(io::Error::other)?;
    store
        .publish(&key, Rc::new(output), total)
        .map_err(io::Error::other)
}

/// Import keys are rooted at the embedding boundary, independently of R's cwd.
pub(crate) fn import_current(path: &str, bytes: &[u8]) -> Result<(), String> {
    BrowserFileStore::validate_path(path)?;
    with_write_store(|store, owner| {
        store.put_import(path, bytes, |len| admit_file_write(owner, len))
    })
    .ok_or_else(|| "no active R session".to_string())?
}

pub(crate) fn absolute_current(path: &str) -> Result<String, String> {
    with_store(|store| store.resolve(path).map(|key| format!("/{key}")))
        .ok_or_else(|| "no active R session".to_string())?
}

pub(crate) enum BrowserOpenMode {
    Read,
    Truncate,
    Append,
}

pub(crate) fn open_snapshot_current(
    path: &str,
    mode: BrowserOpenMode,
) -> Result<Rc<FileBytes>, String> {
    with_write_store(|store, owner| {
        let key = store.resolve(path)?;
        let path = key.as_ref();
        match mode {
            BrowserOpenMode::Read => store
                .files
                .get(path)
                .map(|file| Rc::clone(&file.bytes))
                .ok_or_else(|| "browser file not found".to_string()),
            BrowserOpenMode::Append | BrowserOpenMode::Truncate => {
                store.require_parent(path)?;
                if matches!(mode, BrowserOpenMode::Append) {
                    let now = store.now();
                    if let Some(file) = store.files.get_mut(path) {
                        file.mtime = now;
                        file.atime = now;
                        return Ok(Rc::clone(&file.bytes));
                    }
                }
                store.put_with_admission(path, &[], |len| admit_file_write(owner, len))
            }
        }
    })
    .ok_or_else(|| "no active R session".to_string())?
}

/// Decode all gzip members into a connection-owned, admitted snapshot.
/// Plain files keep their original snapshot without allocating another copy.
pub(crate) fn gunzip_snapshot(source: Rc<FileBytes>) -> io::Result<Rc<FileBytes>> {
    if !source.starts_with(&GZIP_MAGIC) {
        return Ok(source);
    }
    // The extra field can allocate 64 KiB before any data is read. The
    // filename and comment are individually limited to 64 KiB by flate2;
    // also cover their old and new capacities during growth.
    let header_workspace = 64 * 1024 + source.len().saturating_mul(3).min(192 * 1024);
    let _workspace = reserve(GZIP_WORKSPACE_BYTES + header_workspace)?;
    let mut decoded = FileBuffer::new(MAX_SESSION_BYTES);
    decoded.read_from(flate2::bufread::MultiGzDecoder::new(&**source))?;
    Ok(Rc::new(decoded.into_bytes()))
}

pub fn remove_current(path: &str) -> bool {
    with_store_mut(|store| store.resolve(path).is_ok_and(|key| store.remove(&key))).unwrap_or(false)
}

pub fn list_current() -> Vec<String> {
    with_store(|store| store.names()).unwrap_or_default()
}

pub(crate) fn append_current(destination: &str, source: &str) -> bool {
    with_write_store(|store, owner| {
        let Ok(from) = store.resolve(source) else {
            return false;
        };
        let Ok(to) = store.resolve(destination) else {
            return false;
        };
        let Some(input) = store
            .files
            .get(from.as_ref())
            .map(|file| Rc::clone(&file.bytes))
        else {
            return false;
        };
        let saved = store
            .files
            .get(to.as_ref())
            .map(|file| Rc::clone(&file.bytes));
        store.require_parent(&to).is_ok()
            && store
                .put_with_admission(
                    &to,
                    &[saved.as_deref().map_or(&[][..], |bytes| bytes), &input],
                    |len| admit_file_write(owner, len),
                )
                .is_ok()
    })
    .unwrap_or(false)
}

pub(crate) fn set_mode_current(path: &str, mode: u32) -> bool {
    with_store_mut(|store| {
        let Ok(key) = store.resolve(path) else {
            return false;
        };
        let Some(file) = store.files.get_mut(key.as_ref()) else {
            return false;
        };
        file.mode = mode & 0o777;
        true
    })
    .unwrap_or(false)
}

pub(crate) fn dir_exists_current(path: &str) -> bool {
    enabled()
        && with_store(|store| store.resolve(path).is_ok_and(|key| store.directory(&key)))
            .unwrap_or(false)
}

pub(crate) fn mkdir_current(path: &str, recursive: bool) -> Result<(), String> {
    with_store_mut(|store| store.create_directory(path, recursive))
        .ok_or_else(|| "no active R session".to_string())?
}

pub(crate) fn getcwd_current() -> String {
    with_store(|store| store.working_directory()).unwrap_or_default()
}

pub(crate) fn chdir_current(path: &str) -> Result<String, String> {
    with_store_mut(|store| store.change_directory(path))
        .ok_or_else(|| "no active R session".to_string())?
}

/// Restore the cwd in the original live session after R evaluation unwinds.
/// The weak owner restores the original session's store even while another
/// session is active, without extending that session's file lifetime.
pub(crate) struct BrowserDirectoryGuard {
    owner: Weak<BrowserFileState>,
    previous: String,
}

pub(crate) fn scoped_directory(path: &str) -> Result<BrowserDirectoryGuard, String> {
    let owner = current_state().ok_or_else(|| "no active R session".to_string())?;
    let mut store = owner.store.borrow_mut();
    let previous = store.cwd.clone();
    store.change_directory(path)?;
    Ok(BrowserDirectoryGuard {
        owner: Rc::downgrade(&owner),
        previous,
    })
}

impl Drop for BrowserDirectoryGuard {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            let mut store = owner.store.borrow_mut();
            store.cwd = if store.directory(&self.previous) {
                std::mem::take(&mut self.previous)
            } else {
                String::new()
            };
        }
    }
}

pub(crate) fn entries_current(path: &str) -> Option<Vec<(String, bool)>> {
    with_store(|store| store.entries(path)).flatten()
}

pub(crate) fn unlink_current(path: &str, recursive: bool) -> bool {
    with_store_mut(|store| store.unlink(path, recursive)).unwrap_or(false)
}

pub(crate) fn copy_current(
    from: &str,
    to: &str,
    into_directory: bool,
    overwrite: bool,
    recursive: bool,
    copy_mode: bool,
    copy_date: bool,
) -> Result<(), String> {
    with_store_mut(|store| {
        store.copy(
            from,
            to,
            into_directory,
            overwrite,
            recursive,
            copy_mode,
            copy_date,
        )
    })
    .ok_or_else(|| "no active R session".to_string())?
}

/// Write binary or text data using the current session's existing file policy.
pub fn write_bytes_or_host(path: &str, bytes: &[u8]) -> std::io::Result<()> {
    if contains_current(path) || enabled() {
        return write_current(path, bytes).map_err(std::io::Error::other);
    }
    std::fs::write(path, bytes)
}

pub fn write_text_or_host(path: &str, bytes: &[u8]) -> std::io::Result<()> {
    write_bytes_or_host(path, bytes)
}

/// Hold the budget charge for the lifetime of a file buffer.
pub(crate) struct FileBytes {
    bytes: Vec<u8>,
    _reservation: Option<TransientReservation>,
}

impl FileBytes {
    fn copy_parts(
        parts: &[&[u8]],
        len: usize,
        _reservation: Option<TransientReservation>,
    ) -> Result<Self, String> {
        let mut data = Vec::new();
        data.try_reserve_exact(len)
            .map_err(|_| "could not allocate browser file data".to_string())?;
        for bytes in parts {
            data.extend_from_slice(bytes);
        }
        Ok(Self {
            bytes: data,
            _reservation,
        })
    }
}

impl std::ops::Deref for FileBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.bytes
    }
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
        "file data exceeds session memory limit",
    )
}

fn reserve(bytes: usize) -> io::Result<Option<TransientReservation>> {
    OwnerToken::with_current(|owner| {
        owner
            .reserve_file_bytes(bytes)
            .map(Some)
            .ok_or_else(memory_error)
    })
    .unwrap_or(Ok(None))
}

/// Admit an already owned text buffer (e.g. a connection snapshot) for its
/// remaining lifetime. Newly read buffers use `FileBuffer` admission first.
pub(crate) fn admit_text(text: String) -> io::Result<FileText> {
    let reservation = reserve(text.capacity())?;
    Ok(FileText {
        text,
        _reservation: reservation,
    })
}

/// Decode a byte range from a trusted embedded zlib stream. The returned
/// bytes keep their arena reservation through parsing or evaluation.
pub(crate) fn read_embedded_zlib_bytes(
    compressed: &[u8],
    offset: usize,
    length: usize,
    max_end: usize,
) -> io::Result<FileBytes> {
    use io::Read;

    if offset.checked_add(length).is_none_or(|end| end > max_end) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid packed source range",
        ));
    }
    // Allow room for miniz's 32 KiB dictionary and its inflate state.
    // The input slice already implements BufRead, so no input copy is needed.
    let _decoder = reserve(64 * 1024)?;
    let reservation = reserve(length)?;
    let mut decoder = flate2::bufread::ZlibDecoder::new(compressed);
    if io::copy(&mut decoder.by_ref().take(offset as u64), &mut io::sink())? != offset as u64 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "truncated packed source",
        ));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| memory_error())?;
    bytes.resize(length, 0);
    decoder.read_exact(&mut bytes)?;
    Ok(FileBytes {
        bytes,
        _reservation: reservation,
    })
}

pub(crate) fn read_embedded_zlib_text(
    compressed: &[u8],
    offset: usize,
    length: usize,
    max_end: usize,
) -> io::Result<FileText> {
    let FileBytes {
        bytes,
        _reservation,
    } = read_embedded_zlib_bytes(compressed, offset, length, max_end)?;
    let text = String::from_utf8(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(FileText { text, _reservation })
}

struct FileBuffer {
    bytes: Vec<u8>,
    reservation: Option<crate::sexp::memory::TransientReservation>,
    limit: usize,
}

impl FileBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            reservation: None,
            limit,
        }
    }

    fn into_bytes(self) -> FileBytes {
        FileBytes {
            bytes: self.bytes,
            _reservation: self.reservation,
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

impl io::Write for FileBuffer {
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

fn read_buffer_or_host(path: &str) -> io::Result<FileBuffer> {
    let browser = enabled();
    let limit = if browser {
        MAX_SESSION_BYTES
    } else {
        usize::MAX
    };
    let mut input = FileBuffer::new(limit);
    let snapshot = with_store(|store| {
        let key = store.resolve(path).ok()?;
        store
            .files
            .get(key.as_ref())
            .map(|file| Rc::clone(&file.bytes))
    })
    .flatten();
    if let Some(snapshot) = snapshot {
        let reservation = reserve(snapshot.len())?;
        input
            .bytes
            .try_reserve_exact(snapshot.len())
            .map_err(|_| memory_error())?;
        input.bytes.extend_from_slice(&snapshot);
        input.reservation = reservation;
    } else {
        if browser {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "browser file not found",
            ));
        }
        input.read_from(std::fs::File::open(path)?)?;
    }
    Ok(input)
}

pub(crate) fn read_admitted_bytes_or_host(path: &str) -> io::Result<FileBytes> {
    Ok(read_buffer_or_host(path)?.into_bytes())
}

pub(crate) fn read_text_or_host(path: &str) -> io::Result<FileText> {
    let input = read_buffer_or_host(path)?;
    let limit = input.limit;
    let bytes = &input.bytes;
    let mut output = FileBuffer::new(limit);
    if bytes.starts_with(&GZIP_MAGIC) {
        let _workspace = reserve(GZIP_WORKSPACE_BYTES)?;
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
    /// Keep real in-memory file operations deterministic under isolated tests.
    #[cfg(test)]
    pub(crate) fn set_test_clock(&mut self, seconds: f64) {
        self.test_clock = Some(seconds);
    }
    fn now(&self) -> f64 {
        #[cfg(test)]
        if let Some(seconds) = self.test_clock {
            return seconds;
        }
        unix_now()
    }

    pub fn validate_path(path: &str) -> Result<(), String> {
        if path.is_empty()
            || path.len() > MAX_FILE_PATH_BYTES
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
        self.files.get(path).map(|file| &**file.bytes)
    }

    pub fn info(&self, path: &str) -> Option<BrowserFileInfo> {
        self.files.get(path).map(|file| BrowserFileInfo {
            size: file.bytes.len() as u64,
            mtime: file.mtime,
            atime: file.atime,
            ctime: file.ctime,
            mode: file.mode,
        })
    }

    /// GNU `Sys.setFileTime` updates modification and access time together.
    /// A non-finite time is rejected and the stored times stay unchanged.
    pub fn set_file_time(&mut self, path: &str, secs: f64) -> bool {
        if !secs.is_finite() {
            return false;
        }
        if !self.files.contains_key(path) {
            return false;
        }
        let now = self.now();
        let file = self.files.get_mut(path).expect("existing file timestamp");
        file.mtime = secs;
        file.atime = secs;
        file.ctime = now;
        true
    }

    fn validate_file(path: &str, length: usize) -> Result<(), String> {
        Self::validate_path(path)?;
        if length > MAX_FILE_BYTES {
            return Err(file_size_error());
        }
        Ok(())
    }

    fn total_after_write(&self, path: &str, size: usize) -> Result<usize, String> {
        Self::validate_file(path, size)?;
        if self.directory(path) {
            return Err(format!("'{path}' is a browser directory"));
        }
        let existing = self.files.get(path);
        if existing.is_none() && self.files.len() >= MAX_FILE_COUNT {
            return Err(format!(
                "browser file store exceeds {} file limit",
                MAX_FILE_COUNT
            ));
        }
        let old = existing.map_or(0, |file| file.bytes.len());
        let total = self.total_bytes - old + size;
        if total > MAX_SESSION_BYTES {
            return Err(format!(
                "browser file store exceeds {} byte limit",
                MAX_SESSION_BYTES
            ));
        }
        Ok(total)
    }

    fn publish(&mut self, path: &str, bytes: Rc<FileBytes>, total: usize) -> Result<(), String> {
        if !self.files.contains_key(path) {
            self.files
                .try_reserve(1)
                .map_err(|_| "could not allocate browser file entry".to_string())?;
        }
        let mut key = String::new();
        key.try_reserve_exact(path.len())
            .map_err(|_| "could not allocate browser file path".to_string())?;
        key.push_str(path);
        let now = self.now();
        let (ctime, mode) = self
            .files
            .get(path)
            .map(|file| (file.ctime, file.mode))
            .unwrap_or((now, 0o644));
        self.files.insert(
            key,
            StoredFile {
                bytes,
                mtime: now,
                atime: now,
                ctime,
                mode,
            },
        );
        self.total_bytes = total;
        Ok(())
    }

    pub fn put(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.put_import(path, bytes, |_| Ok(None))
    }

    fn put_import(
        &mut self,
        path: &str,
        bytes: &[u8],
        admit: impl FnOnce(usize) -> Result<Option<TransientReservation>, String>,
    ) -> Result<(), String> {
        Self::validate_path(path)?;
        let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        let missing = self.missing_directories(parent)?;
        self.reserve_directories(missing.len())?;
        self.put_with_admission(path, &[bytes], admit)?;
        self.directories.extend(missing);
        Ok(())
    }

    fn put_with_admission(
        &mut self,
        path: &str,
        parts: &[&[u8]],
        admit: impl FnOnce(usize) -> Result<Option<TransientReservation>, String>,
    ) -> Result<Rc<FileBytes>, String> {
        let len = parts
            .iter()
            .try_fold(0usize, |len, bytes| len.checked_add(bytes.len()))
            .ok_or_else(file_size_error)?;
        let total = self.total_after_write(path, len)?;
        // The old version stays charged for as long as the store or an open
        // connection holds it. Failed admission leaves all versions intact.
        let data = Rc::new(FileBytes::copy_parts(parts, len, admit(len)?)?);
        self.publish(path, Rc::clone(&data), total)?;
        Ok(data)
    }

    pub fn clear(&mut self) {
        self.files.clear();
        self.directories.clear();
        self.cwd.clear();
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

fn file_size_error() -> String {
    format!("browser file exceeds {MAX_FILE_BYTES} byte limit")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{memory::ArenaBudget, session::RSession};

    fn admitted_bytes(session: &mut RSession) -> usize {
        session
            .with_arena(|arena| arena.total_bytes_allocated() + arena.workspace_bytes())
            .unwrap()
    }

    #[test]
    fn stored_files_charge_the_session_through_copy_replace_and_remove() {
        let mut session = RSession::new_for_gc_tests();
        let baseline = admitted_bytes(&mut session);
        session.set_arena_budget(ArenaBudget::new(baseline + 6, 0));
        session.put_browser_file("source.csv", b"1,2\n").unwrap();
        assert_eq!(admitted_bytes(&mut session), baseline + 4);

        session.with_active(|| {
            mkdir_current("copies", false).unwrap();
            copy_current("source.csv", "copies", true, false, false, true, false).unwrap();
            assert!(write_current("source.csv", b"123").is_err());
            assert_eq!(read_current("source.csv").unwrap(), b"1,2\n");
            write_current("source.csv", b"3\n").unwrap();
        });
        assert_eq!(admitted_bytes(&mut session), baseline + 6);
        assert!(session.remove_browser_file("source.csv"));
        assert_eq!(admitted_bytes(&mut session), baseline + 4);
        assert_eq!(
            session.get_browser_file("copies/source.csv").unwrap(),
            b"1,2\n"
        );
        assert!(session.remove_browser_file("copies/source.csv"));
        assert_eq!(admitted_bytes(&mut session), baseline);
    }

    #[test]
    fn file_store_explicit_clock_preserves_creation_and_touch_semantics() {
        let mut store = BrowserFileStore::default();
        store.set_test_clock(11.0);
        store.put("clock.txt", b"a").unwrap();
        let initial = store.info("clock.txt").unwrap();
        assert_eq!(
            (initial.mtime, initial.atime, initial.ctime),
            (11.0, 11.0, 11.0)
        );
        store.set_test_clock(22.0);
        store.put("clock.txt", b"second").unwrap();
        let changed = store.info("clock.txt").unwrap();
        assert_eq!(
            (changed.size, changed.mtime, changed.atime, changed.ctime),
            (6, 22.0, 22.0, 11.0)
        );
        assert!(store.set_file_time("clock.txt", 33.0));
        let touched = store.info("clock.txt").unwrap();
        assert_eq!(
            (touched.mtime, touched.atime, touched.ctime),
            (33.0, 33.0, 22.0)
        );
        assert!(!store.set_file_time("missing.txt", 0.0));
        assert_eq!(store.info("clock.txt"), Some(touched));
    }

    #[test]
    fn binary_file_bytes_remain_admitted_and_rejected_replacement_preserves_store() {
        let mut session = RSession::new_for_gc_tests();
        session.enable_browser_files();
        session.put_browser_file("saved.rds", &[1, 2, 3]).unwrap();
        let baseline = admitted_bytes(&mut session);
        session.set_arena_budget(ArenaBudget::new(baseline + 3, 0));
        session.with_active(|| {
            let bytes = read_admitted_bytes_or_host("saved.rds").unwrap();
            assert_eq!(&*bytes, &[1, 2, 3]);
            assert!(reserve(1).is_err());
            drop(bytes);
            assert!(write_bytes_or_host("saved.rds", &[4, 5, 6, 7]).is_err());
            assert_eq!(read_current("saved.rds").unwrap(), [1, 2, 3]);
            write_bytes_or_host("saved.rds", &[7, 8]).unwrap();
            assert_eq!(&*read_admitted_bytes_or_host("saved.rds").unwrap(), &[7, 8]);
        });
    }
    use std::io::Write;

    #[test]
    fn text_admission_lasts_until_drop_and_releases_on_errors() {
        let mut session = RSession::new_without_default_packages();
        session.enable_browser_files();
        session.put_browser_file("note.txt", b"hello").unwrap();
        session.put_browser_file("bad.txt", &[0xff]).unwrap();
        let baseline = admitted_bytes(&mut session);
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
        let baseline = admitted_bytes(&mut session);
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
    fn directory_copy_admits_all_payloads_before_publishing() {
        let mut store = BrowserFileStore::default();
        store.create_directory("target", false).unwrap();
        let full = vec![7; MAX_FILE_BYTES];
        for i in 0..5 {
            store.put(&format!("source/{i}"), &full).unwrap();
        }
        assert!(
            store
                .copy("source", "target", true, true, true, true, false)
                .is_err()
        );
        assert_eq!(store.names().len(), 5);
        assert_eq!(store.entries("target"), Some(Vec::new()));
        assert_eq!(store.read("source/0"), Some(full.as_slice()));
        assert!(store.put("source", b"would replace directory").is_err());
        assert!(store.put("source/0/child", b"would replace file").is_err());
    }

    #[test]
    fn browser_directory_guard_restores_original_session_on_unwind() {
        let original = RSession::new_for_gc_tests();
        let replacement = RSession::new_for_gc_tests();
        let guard = original.with_active(|| {
            mkdir_current("home", false).unwrap();
            mkdir_current("temp", false).unwrap();
            chdir_current("home").unwrap();
            scoped_directory("/temp").unwrap()
        });
        replacement.with_active(|| {
            mkdir_current("other", false).unwrap();
            chdir_current("other").unwrap();
            let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let _guard = guard;
                panic!("source interrupted");
            }));
            assert!(unwound.is_err());
            assert_eq!(getcwd_current(), "/other");
        });
        original.with_active(|| {
            assert_eq!(getcwd_current(), "/home");
        });
    }

    #[test]
    fn browser_write_refuses_lent_arena_without_replacing_file() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            set_test_clock_current(1.0);
            write_current("scope.txt", b"original").unwrap();
            OwnerToken::with_current(|owner| {
                owner.with_arena(|_| {
                    assert!(
                        write_current("scope.txt", b"replacement")
                            .unwrap_err()
                            .contains("session memory limit")
                    );
                })
            })
            .unwrap()
            .unwrap();
            assert_eq!(read_current("scope.txt").unwrap(), b"original");
        });
    }
}
