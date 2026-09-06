//! libc-free stdio-shaped file layer.
//!
//! `RFile` replaces C's `FILE*` for all engine-internal file I/O. The
//! `r_*` helpers mirror the libc stdio API shape — including sticky
//! EOF/error flags, NUL-terminated `fgets`, one-byte `ungetc` pushback,
//! and `SEEK_*` whence constants — so translated C call sites convert
//! mechanically while doing all real work through `std::fs`.
//!
//! Ownership: `r_fopen` returns a leaked `Box<RFile>` (raw pointer);
//! `r_fclose` reconstructs and drops it. Never double-close. Every
//! helper is NULL-tolerant.
//!
//! wasm32: every `std::fs` API compiles on wasm32-unknown-unknown and
//! fails cleanly at runtime — same clean-error policy as the rest of
//! the engine. No platform gating is needed.

use core::ffi::{c_char, c_int, c_void};
use std::ffi::CStr;
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

/// End-of-file / error sentinel matching C's `EOF`.
pub const EOF: c_int = -1;

/// Whence constants matching C's `SEEK_SET` / `SEEK_CUR` / `SEEK_END`.
pub const SEEK_SET: c_int = 0;
pub const SEEK_CUR: c_int = 1;
pub const SEEK_END: c_int = 2;

/// libc-free replacement for C's `FILE` stream.
pub struct RFile {
    inner: File,
    /// Sticky end-of-file flag (set once a read hits EOF, cleared by
    /// successful `r_fseek` / `r_rewind` / `r_ungetc`).
    eof: bool,
    /// Sticky error flag (set on any I/O error).
    error: bool,
    /// One-byte pushback for `r_ungetc` (C guarantees one level).
    pushback: Option<u8>,
}

// ---------------------------------------------------------------------------
// Open / close
// ---------------------------------------------------------------------------

/// Open `path` using a libc-style mode string: `r`/`w`/`a` base, optional
/// `+` (read+write), optional `b` (ignored, byte mode is the only mode).
/// Returns NULL on failure. On `w`/`a` the file is created if missing;
/// `w` also truncates.
pub unsafe fn r_fopen(path: *const c_char, mode: *const c_char) -> *mut RFile {
    unsafe {
        if path.is_null() || mode.is_null() || *path == 0 || *mode == 0 {
            return std::ptr::null_mut();
        }
        let mut plus = false;
        let mut m = mode;
        loop {
            let ch = *m as u8;
            if ch == 0 {
                break;
            }
            if ch == b'+' {
                plus = true;
            }
            m = m.add(1);
        }
        let mut opts = OpenOptions::new();
        match *mode as u8 {
            b'r' => {
                opts.read(true);
                if plus {
                    opts.write(true);
                }
            }
            b'w' => {
                opts.write(true);
                if plus {
                    opts.read(true);
                }
                opts.create(true).truncate(true);
            }
            b'a' => {
                opts.write(true);
                if plus {
                    opts.read(true);
                }
                opts.create(true).append(true);
            }
            _ => return std::ptr::null_mut(),
        }
        let bytes = CStr::from_ptr(path).to_bytes();
        let pathbuf = PathBuf::from(String::from_utf8_lossy(bytes).into_owned());
        match opts.open(&pathbuf) {
            Ok(inner) => Box::into_raw(Box::new(RFile {
                inner,
                eof: false,
                error: false,
                pushback: None,
            })),
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Close the stream and release its Box. NULL-tolerant; returns 0.
pub unsafe fn r_fclose(f: *mut RFile) -> c_int {
    unsafe {
        if f.is_null() {
            return 0;
        }
        drop(Box::from_raw(f));
        0
    }
}

// ---------------------------------------------------------------------------
// Read
// ---------------------------------------------------------------------------

/// Read up to `size * n` bytes into `ptr`, looping like libc `fread`
/// until the request is satisfied, EOF, or error. Returns the number of
/// complete items read (`total_bytes / size`). Sets the sticky EOF flag
/// when the stream ends, the error flag on I/O failure.
pub unsafe fn r_fread(ptr: *mut c_void, size: usize, n: usize, f: *mut RFile) -> usize {
    unsafe {
        if f.is_null() || size == 0 || n == 0 {
            return 0;
        }
        let total = size.saturating_mul(n);
        if ptr.is_null() || total == 0 {
            return 0;
        }
        let dst = ptr as *mut u8;
        let mut got: usize = 0;
        while got < total {
            let buf = std::slice::from_raw_parts_mut(dst.add(got), total - got);
            match (*f).inner.read(buf) {
                Ok(0) => {
                    (*f).eof = true;
                    break;
                }
                Ok(k) => got += k,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => {
                    (*f).error = true;
                    break;
                }
            }
        }
        got / size
    }
}

/// Read one line (like libc `fgets`): stops after a newline (kept) or
/// after `n - 1` bytes, always NUL-terminates. Returns `buf`, or NULL
/// when the stream is at EOF and no characters were read.
pub unsafe fn r_fgets(buf: *mut c_char, n: c_int, f: *mut RFile) -> *mut c_char {
    unsafe {
        if f.is_null() || buf.is_null() || n <= 0 {
            return std::ptr::null_mut();
        }
        let cap = n as usize - 1;
        let mut count: usize = 0;
        while count < cap {
            let c = r_fgetc(f);
            if c == EOF {
                break;
            }
            *buf.add(count) = c as c_char;
            count += 1;
            if c as u8 == b'\n' {
                break;
            }
        }
        if count == 0 {
            if (*f).eof || (*f).error {
                return std::ptr::null_mut();
            }
            // n == 1 and not at EOF: store just the NUL, like C fgets.
            *buf = 0;
            return buf;
        }
        *buf.add(count) = 0;
        buf
    }
}

/// Read one byte, returning it as `c_int`, or EOF at end of file.
pub unsafe fn r_fgetc(f: *mut RFile) -> c_int {
    unsafe {
        if f.is_null() {
            return EOF;
        }
        if let Some(b) = (*f).pushback.take() {
            return b as c_int;
        }
        let mut one = [0u8; 1];
        loop {
            match (*f).inner.read(&mut one) {
                Ok(0) => {
                    (*f).eof = true;
                    return EOF;
                }
                Ok(_) => return one[0] as c_int,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => {
                    (*f).error = true;
                    return EOF;
                }
            }
        }
    }
}

/// Push one byte back (like libc `ungetc`; only one level is kept).
/// `r_ungetc(f, EOF)` is a no-op returning EOF. A successful pushback
/// clears the EOF flag.
pub unsafe fn r_ungetc(c: c_int, f: *mut RFile) -> c_int {
    unsafe {
        if f.is_null() || c == EOF {
            return EOF;
        }
        if (*f).pushback.is_none() {
            (*f).pushback = Some(c as u8);
            (*f).eof = false;
            c
        } else {
            EOF
        }
    }
}

// ---------------------------------------------------------------------------
// Write
// ---------------------------------------------------------------------------

/// Write `size * n` bytes from `ptr`, looping until all are written or
/// an error occurs. Returns complete items written. Sets the error flag
/// on failure.
pub unsafe fn r_fwrite(ptr: *const c_void, size: usize, n: usize, f: *mut RFile) -> usize {
    unsafe {
        if f.is_null() || size == 0 || n == 0 {
            return 0;
        }
        let total = size.saturating_mul(n);
        if ptr.is_null() || total == 0 {
            return 0;
        }
        let src = ptr as *const u8;
        let mut put: usize = 0;
        while put < total {
            let buf = std::slice::from_raw_parts(src.add(put), total - put);
            match (*f).inner.write(buf) {
                Ok(0) => {
                    (*f).error = true;
                    break;
                }
                Ok(k) => put += k,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => {
                    (*f).error = true;
                    break;
                }
            }
        }
        put / size
    }
}

/// Write the NUL-terminated string `s` (excluding the NUL). Returns the
/// number of bytes written, or EOF on failure.
pub unsafe fn r_fputs(s: *const c_char, f: *mut RFile) -> c_int {
    unsafe {
        if f.is_null() || s.is_null() {
            return EOF;
        }
        let bytes = CStr::from_ptr(s).to_bytes();
        let wrote = r_fwrite(bytes.as_ptr() as *const c_void, 1, bytes.len(), f);
        if wrote != bytes.len() {
            return EOF;
        }
        wrote as c_int
    }
}

/// Write the single byte `c`. Returns `c` on success, EOF on failure.
pub unsafe fn r_fputc(c: c_int, f: *mut RFile) -> c_int {
    unsafe {
        if f.is_null() {
            return EOF;
        }
        let one = [c as u8];
        match (*f).inner.write_all(&one) {
            Ok(()) => c,
            Err(_) => {
                (*f).error = true;
                EOF
            }
        }
    }
}

/// Flush the stream. Returns 0 on success, EOF on failure. (The layer
/// keeps no write buffer, so this only reports the underlying flush.)
pub unsafe fn r_fflush(f: *mut RFile) -> c_int {
    unsafe {
        if f.is_null() {
            return 0;
        }
        match (*f).inner.flush() {
            Ok(()) => 0,
            Err(_) => {
                (*f).error = true;
                EOF
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Position
// ---------------------------------------------------------------------------

/// Reposition the stream (like libc `fseek`). `whence` is one of
/// `SEEK_SET` / `SEEK_CUR` / `SEEK_END`. Returns 0 on success, -1 on
/// failure. A successful seek clears the sticky EOF flag and discards
/// any `r_ungetc` pushback (adjusting `SEEK_CUR` for it first).
pub unsafe fn r_fseek(f: *mut RFile, off: i64, whence: c_int) -> c_int {
    unsafe {
        if f.is_null() {
            return -1;
        }
        let target = match whence {
            SEEK_SET => SeekFrom::Start(off as u64),
            SEEK_CUR => {
                let adj = if (*f).pushback.is_some() { -1 } else { 0 };
                SeekFrom::Current(off + adj)
            }
            SEEK_END => SeekFrom::End(off),
            _ => return -1,
        };
        (*f).pushback = None;
        match (*f).inner.seek(target) {
            Ok(_) => {
                (*f).eof = false;
                0
            }
            Err(_) => {
                (*f).error = true;
                -1
            }
        }
    }
}

/// Return the current logical position (accounting for `r_ungetc`
/// pushback), or -1 on failure.
pub unsafe fn r_ftell(f: *mut RFile) -> i64 {
    unsafe {
        if f.is_null() {
            return -1;
        }
        match (*f).inner.stream_position() {
            Ok(pos) => {
                let pb = if (*f).pushback.is_some() { 1 } else { 0 };
                pos as i64 - pb
            }
            Err(_) => {
                (*f).error = true;
                -1
            }
        }
    }
}

/// Rewind to the start and clear both the EOF and error flags.
pub unsafe fn r_rewind(f: *mut RFile) {
    unsafe {
        if f.is_null() {
            return;
        }
        (*f).pushback = None;
        let _ = (*f).inner.seek(SeekFrom::Start(0));
        (*f).eof = false;
        (*f).error = false;
    }
}

// ---------------------------------------------------------------------------
// Status flags
// ---------------------------------------------------------------------------

/// Non-zero when the sticky end-of-file indicator is set.
pub unsafe fn r_feof(f: *mut RFile) -> c_int {
    unsafe {
        if f.is_null() {
            return 0;
        }
        (*f).eof as c_int
    }
}

/// Non-zero when the sticky error indicator is set.
pub unsafe fn r_ferror(f: *mut RFile) -> c_int {
    unsafe {
        if f.is_null() {
            return 0;
        }
        (*f).error as c_int
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rport-rfile-test-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn fopen_rejects_bad_mode_and_missing_file() {
        let p = temp_path("nomode");
        let cpath = CString::new(p.to_string_lossy().as_bytes()).unwrap();
        unsafe {
            assert!(r_fopen(cpath.as_ptr(), b"q\0".as_ptr() as *const c_char).is_null());
            assert!(r_fopen(cpath.as_ptr(), b"r\0".as_ptr() as *const c_char).is_null());
            assert!(r_fopen(std::ptr::null(), b"r\0".as_ptr() as *const c_char).is_null());
        }
    }

    #[test]
    fn write_read_seek_round_trip() {
        let p = temp_path("rw");
        let cpath = CString::new(p.to_string_lossy().as_bytes()).unwrap();
        unsafe {
            let f = r_fopen(cpath.as_ptr(), b"wb+\0".as_ptr() as *const c_char);
            assert!(!f.is_null());
            let wrote = r_fwrite(b"hello\0".as_ptr() as *const c_void, 1, 6, f);
            assert_eq!(wrote, 6);
            assert_eq!(r_fflush(f), 0);
            assert_eq!(r_ftell(f), 6);
            r_rewind(f);
            assert_eq!(r_ftell(f), 0);
            let mut buf = [0u8; 6];
            assert_eq!(r_fread(buf.as_mut_ptr() as *mut c_void, 1, 6, f), 6);
            assert_eq!(&buf, b"hello\0");
            // EOF is sticky after a read that hits the end.
            assert_eq!(r_fgetc(f), EOF);
            assert_eq!(r_feof(f), 1);
            // fseek clears EOF.
            assert_eq!(r_fseek(f, 1, SEEK_SET), 0);
            assert_eq!(r_feof(f), 0);
            assert_eq!(r_fgetc(f), b'e' as c_int);
            assert_eq!(r_fclose(f), 0);
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn fgets_stops_at_newline_and_reports_eof() {
        let p = temp_path("fgets");
        std::fs::write(&p, b"ab\ncd").unwrap();
        let cpath = CString::new(p.to_string_lossy().as_bytes()).unwrap();
        unsafe {
            let f = r_fopen(cpath.as_ptr(), b"r\0".as_ptr() as *const c_char);
            assert!(!f.is_null());
            let mut line = [0 as c_char; 16];
            let r = r_fgets(line.as_mut_ptr(), 16, f);
            assert!(!r.is_null());
            assert_eq!(CStr::from_ptr(r).to_bytes(), b"ab\n");
            let r = r_fgets(line.as_mut_ptr(), 16, f);
            assert!(!r.is_null());
            assert_eq!(CStr::from_ptr(r).to_bytes(), b"cd");
            // EOF with no data: NULL, and feof set.
            assert!(r_fgets(line.as_mut_ptr(), 16, f).is_null());
            assert_eq!(r_feof(f), 1);
            // Long line: NUL-terminated at n-1 bytes, newline kept if it fits.
            r_rewind(f);
            let mut small = [0 as c_char; 3];
            let r = r_fgets(small.as_mut_ptr(), 3, f);
            assert_eq!(CStr::from_ptr(r).to_bytes(), b"ab");
            r_fclose(f);
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn ungetc_pushes_one_byte_back() {
        let p = temp_path("ungetc");
        std::fs::write(&p, b"xy").unwrap();
        let cpath = CString::new(p.to_string_lossy().as_bytes()).unwrap();
        unsafe {
            let f = r_fopen(cpath.as_ptr(), b"r\0".as_ptr() as *const c_char);
            assert_eq!(r_fgetc(f), b'x' as c_int);
            assert_eq!(r_ungetc(b'x' as c_int, f), b'x' as c_int);
            assert_eq!(r_fgetc(f), b'x' as c_int);
            // ungetc(EOF) is a no-op.
            assert_eq!(r_ungetc(EOF, f), EOF);
            assert_eq!(r_fgetc(f), b'y' as c_int);
            assert_eq!(r_fgetc(f), EOF);
            r_fclose(f);
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn append_mode_writes_at_end() {
        let p = temp_path("append");
        std::fs::write(&p, b"one").unwrap();
        let cpath = CString::new(p.to_string_lossy().as_bytes()).unwrap();
        unsafe {
            let f = r_fopen(cpath.as_ptr(), b"a\0".as_ptr() as *const c_char);
            assert!(!f.is_null());
            assert_eq!(r_fwrite(b"two".as_ptr() as *const c_void, 1, 3, f), 3);
            r_fclose(f);
        }
        assert_eq!(std::fs::read(&p).unwrap(), b"onetwo");
        let _ = std::fs::remove_file(&p);
    }
}
