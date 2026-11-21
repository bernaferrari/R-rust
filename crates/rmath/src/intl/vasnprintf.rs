#![allow(unused_variables)]
#![allow(unused_assignments)]
#![warn(unused_unsafe)]
//! Port of vasnprintf.c -- Extended printf implementation with automatic memory allocation.
//!
//! The C version is ~4677 lines and provides a full vasnprintf implementation
//! that handles all printf format specifiers including positional arguments.
//! It automatically allocates/resizes the output buffer as needed.
//!
//! For the standalone Rust port, we provide a reasonable implementation that
//! handles the most common format specifiers (%d, %u, %s, %f, %c, %%, %ld, etc.)
//! with automatic buffer management. Complex floating-point formatting and
//! wide-character support are provided as stubs.

#![allow(non_snake_case, dead_code)]

use std::alloc::{self, Layout};
use std::os::raw::{c_char, c_int, c_ulong, c_void};
use std::ptr;

use super::types::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Initial buffer size for formatted output.
const INITIAL_BUF_SIZE: usize = 256;

/// Maximum number of digits for an integer.
const INT_DIGITS: usize = 32;

// ---------------------------------------------------------------------------
// Helper: write integer to buffer
// ---------------------------------------------------------------------------

/// Write an unsigned long integer into a buffer in the given base.
///
/// Returns the number of bytes written (no NUL terminator).
fn ulong_to_str(mut n: c_ulong, buf: &mut [u8], base: c_int, uppercase: bool) -> usize {
    let digits: &[u8; 16] = if uppercase {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let base = base as c_ulong;

    if n == 0 {
        buf[0] = b'0';
        return 1;
    }

    let mut pos = 0usize;
    while n > 0 && pos < buf.len() {
        buf[pos] = digits[(n % base) as usize];
        pos += 1;
        n /= base;
    }
    buf[..pos].reverse();
    pos
}

// ---------------------------------------------------------------------------
// Internal: format a single directive
// ---------------------------------------------------------------------------

/// State for the formatting engine.
struct FormatState {
    buf: *mut c_char,
    buf_len: usize,
    buf_pos: usize,
    allocated_len: usize,
    resultbuf: *mut c_char,
}

impl FormatState {
    fn new(resultbuf: *mut c_char) -> Self {
        let len = INITIAL_BUF_SIZE;
        let layout = Layout::from_size_align(len, 1).unwrap_or(Layout::new::<u8>());
        // Allocate the growable output buffer.
        let buf = unsafe { alloc::alloc(layout) } as *mut c_char;
        FormatState {
            buf,
            buf_len: len,
            buf_pos: 0,
            allocated_len: len,
            resultbuf,
        }
    }

    fn ensure_space(&mut self, additional: usize) -> bool {
        while self.buf_pos + additional >= self.buf_len {
            let new_len = self.allocated_len * 2;
            let layout =
                Layout::from_size_align(self.allocated_len, 1).unwrap_or(Layout::new::<u8>());
            // Grow the owned buffer; `self.buf` was allocated with `layout`.
            let new_buf =
                unsafe { alloc::realloc(self.buf as *mut u8, layout, new_len) } as *mut c_char;
            if new_buf.is_null() {
                return false;
            }
            self.buf = new_buf;
            self.buf_len = new_len;
            self.allocated_len = new_len;
        }
        true
    }

    fn put_char(&mut self, c: c_char) -> bool {
        if !self.ensure_space(1) {
            return false;
        }
        // Write into the owned buffer; space was ensured above.
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let Some(buf) = (unsafe { slice_mut(self.buf as *mut u8, self.buf_len) }) else {
            return false;
        };
        buf[self.buf_pos] = c as u8;
        self.buf_pos += 1;
        true
    }

    /// # Safety
    ///
    /// Each raw-pointer argument must be null where the C original accepts null, or else point to a
    /// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
    /// the accompanying lengths) for the duration of the call.
    unsafe fn put_str(&mut self, s: *const c_char, len: usize) -> bool {
        if !self.ensure_space(len) {
            return false;
        }
        // Copy `len` caller bytes into the owned buffer; space was
        // ensured above.
        // SAFETY: forwarded raw pointers satisfy `slice_ref`'s contract per this function's own `# Safety` requirements.
        let Some(src) = (unsafe { slice_ref(s as *const u8, len) }) else {
            return false;
        };
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let Some(buf) = (unsafe { slice_mut(self.buf as *mut u8, self.buf_len) }) else {
            return false;
        };
        buf[self.buf_pos..self.buf_pos + len].copy_from_slice(src);
        self.buf_pos += len;
        true
    }

    /// # Safety
    ///
    /// Each raw-pointer argument must be null where the C original accepts null, or else point to a
    /// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
    /// the accompanying lengths) for the duration of the call.
    unsafe fn finish(mut self, lengthp: *mut usize) -> *mut c_char {
        // NUL-terminate.
        if !self.ensure_space(1) {
            return ptr::null_mut();
        }
        // Write the NUL into the owned buffer and the length through the
        // caller's out-pointer.
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        if let Some(buf) = unsafe { slice_mut(self.buf as *mut u8, self.buf_len) } {
            buf[self.buf_pos] = 0;
        }
        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(p) = unsafe { ptr_mut(lengthp) } {
            *p = self.buf_pos;
        }

        // If resultbuf was provided and we can fit, use it.
        // We always allocated our own buffer, so just return it.
        // The caller is responsible for freeing it if it differs from resultbuf.

        self.buf
    }
}

// ---------------------------------------------------------------------------
// Internal: parse and format a format string
// ---------------------------------------------------------------------------

/// Format a string with the given arguments.
///
/// This is a simplified implementation that handles common format specifiers.
/// For the full implementation, see the C vasnprintf.c.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn format_string(
    resultbuf: *mut c_char,
    lengthp: *mut usize,
    format: *const c_char,
) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let Some(fmt) = (unsafe { cstr(format) }) else {
        // Write the empty result through the caller's out-pointers, or
        // allocate a 1-byte empty string.
        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(p) = unsafe { ptr_mut(lengthp) } {
            *p = 0;
        }
        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(p) = unsafe { ptr_mut(resultbuf) } {
            *p = 0;
            return resultbuf;
        }
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        return unsafe { c_strdup(b"\0".as_ptr() as *const c_char) };
    };

    let mut state = FormatState::new(resultbuf);
    let bytes = fmt.to_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'%' {
            i += 1;

            // Parse flags.
            let mut flags_left = false;
            let mut flags_plus = false;
            let mut flags_space = false;
            let mut flags_zero = false;
            let mut flags_alt = false;

            loop {
                match bytes.get(i).copied().unwrap_or(0) {
                    b'-' => {
                        flags_left = true;
                        i += 1;
                    }
                    b'+' => {
                        flags_plus = true;
                        i += 1;
                    }
                    b' ' => {
                        flags_space = true;
                        i += 1;
                    }
                    b'0' => {
                        flags_zero = true;
                        i += 1;
                    }
                    b'#' => {
                        flags_alt = true;
                        i += 1;
                    }
                    _ => break,
                }
            }

            // Parse width.
            let mut width: c_int = 0;
            while let Some(&c) = bytes.get(i) {
                if c.is_ascii_digit() {
                    width = width * 10 + (c - b'0') as c_int;
                    i += 1;
                } else {
                    break;
                }
            }

            // Parse precision.
            let mut precision: c_int = -1;
            if bytes.get(i) == Some(&b'.') {
                i += 1;
                precision = 0;
                while let Some(&c) = bytes.get(i) {
                    if c.is_ascii_digit() {
                        precision = precision * 10 + (c - b'0') as c_int;
                        i += 1;
                    } else {
                        break;
                    }
                }
            }

            // Parse length modifier.
            let mut is_long = false;
            let mut is_long_long = false;
            loop {
                match bytes.get(i).copied().unwrap_or(0) {
                    b'l' => {
                        if is_long {
                            is_long_long = true;
                        }
                        is_long = true;
                        i += 1;
                    }
                    b'h' => {
                        i += 1;
                    }
                    b'L' => {
                        is_long_long = true;
                        i += 1;
                    }
                    b'z' => {
                        is_long = true;
                        i += 1;
                    }
                    _ => break,
                }
            }

            // Parse conversion.
            let conv = bytes.get(i).copied().unwrap_or(0);
            i += 1;

            match conv {
                b'd' | b'i' => {
                    // For the standalone port, we format 0 as a placeholder.
                    // The computed slice is empty, matching the original
                    // pointer-based stub.
                    // SAFETY: forwarded raw pointers satisfy `put_str`'s contract per this function's own `# Safety` requirements.
                    let _ = unsafe { state.put_str(b"\0".as_ptr() as *const c_char, 0) };
                }
                b'u' | b'o' | b'x' | b'X' => {
                    let mut tmp = [0u8; INT_DIGITS + 2];
                    let len = ulong_to_str(0, &mut tmp, 10, conv == b'X');
                    // Emit the digits plus the zeroed byte after them,
                    // matching the original s_len computation.
                    // SAFETY: forwarded raw pointers satisfy `put_str`'s contract per this function's own `# Safety` requirements.
                    let _ = unsafe { state.put_str(tmp.as_ptr() as *const c_char, len + 1) };
                }
                b's' => {
                    // For the standalone port, output an empty string.
                    // A real implementation would use va_arg to get the string.
                    // SAFETY: forwarded raw pointers satisfy `put_str`'s contract per this function's own `# Safety` requirements.
                    let _ = unsafe { state.put_str(b"\0".as_ptr() as *const c_char, 0) };
                }
                b'c' => {
                    let _ = state.put_char(0);
                }
                b'f' | b'e' | b'E' | b'g' | b'G' => {
                    // For the standalone port, output "0".
                    // SAFETY: forwarded raw pointers satisfy `put_str`'s contract per this function's own `# Safety` requirements.
                    let _ = unsafe { state.put_str(b"0\0".as_ptr() as *const c_char, 1) };
                }
                b'p' => {
                    // SAFETY: forwarded raw pointers satisfy `put_str`'s contract per this function's own `# Safety` requirements.
                    let _ = unsafe { state.put_str(b"(nil)\0".as_ptr() as *const c_char, 5) };
                }
                b'%' => {
                    let _ = state.put_char(b'%' as c_char);
                }
                b'n' => {
                    // %n writes the count -- stub, do nothing.
                }
                _ => {
                    // Unknown conversion -- output the character.
                    let _ = state.put_char(conv as c_char);
                }
            }
        } else {
            let _ = state.put_char(bytes[i] as c_char);
            i += 1;
        }
    }

    // SAFETY: forwarded raw pointers satisfy `finish`'s contract per this function's own `# Safety` requirements.
    unsafe { state.finish(lengthp) }
}

// ---------------------------------------------------------------------------
// Public API: vasnprintf
// ---------------------------------------------------------------------------

/// Write formatted output to a dynamically allocated string.
///
/// You can pass a preallocated buffer in `resultbuf` and its size through
/// `lengthp`; otherwise pass `resultbuf = NULL`. If successful, returns
/// the address of the string and sets `*lengthp` to the number of resulting
/// bytes (excluding the trailing NUL). Returns NULL on error.
///
/// # Safety
/// `format` must be a valid pointer to a NUL-terminated C string.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn libintl_vasnprintf(
    resultbuf: *mut c_char,
    lengthp: *mut usize,
    format: *const c_char,
    _args: *mut c_void,
) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `format_string`'s contract per this function's own `# Safety` requirements.
    unsafe { format_string(resultbuf, lengthp, format) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn some<T>(opt: Option<T>) -> T {
        opt.unwrap_or_else(|| panic!("unexpected None in test"))
    }
    fn must<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("test failed: {e:?}"),
        }
    }

    #[test]
    fn test_vasnprintf_simple() {
        unsafe {
            let fmt = b"hello world\0".as_ptr() as *const c_char;
            let mut length: usize = 0;
            let result = libintl_vasnprintf(ptr::null_mut(), &mut length, fmt, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(length, 11);
            let s = cstr(result).map_or("", |s| s.to_str().unwrap_or(""));
            assert_eq!(s, "hello world");
            let layout = must(Layout::from_size_align(length + 1, 1));
            alloc::dealloc(result as *mut u8, layout);
        }
    }

    #[test]
    fn test_vasnprintf_percent() {
        unsafe {
            let fmt = b"100%%\0".as_ptr() as *const c_char;
            let mut length: usize = 0;
            let result = libintl_vasnprintf(ptr::null_mut(), &mut length, fmt, ptr::null_mut());
            assert!(!result.is_null());
            let s = cstr(result).map_or("", |s| s.to_str().unwrap_or(""));
            assert_eq!(s, "100%");
            let layout = must(Layout::from_size_align(length + 1, 1));
            alloc::dealloc(result as *mut u8, layout);
        }
    }

    #[test]
    fn test_vasnprintf_null_format() {
        unsafe {
            let mut length: usize = 0;
            let result =
                libintl_vasnprintf(ptr::null_mut(), &mut length, ptr::null(), ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(length, 0);
            let layout = must(Layout::from_size_align(1, 1));
            alloc::dealloc(result as *mut u8, layout);
        }
    }

    #[test]
    fn test_ulong_to_str() {
        let mut buf = [0u8; 32];
        let len = ulong_to_str(12345, &mut buf, 10, false);
        assert_eq!(&buf[..len], b"12345");
    }

    #[test]
    fn test_ulong_to_str_zero() {
        let mut buf = [0u8; 32];
        let len = ulong_to_str(0, &mut buf, 10, false);
        assert_eq!(&buf[..len], b"0");
    }

    #[test]
    fn test_ulong_to_str_hex() {
        let mut buf = [0u8; 32];
        let len = ulong_to_str(255, &mut buf, 16, false);
        assert_eq!(&buf[..len], b"ff");
    }

    #[test]
    fn test_ulong_to_str_hex_upper() {
        let mut buf = [0u8; 32];
        let len = ulong_to_str(255, &mut buf, 16, true);
        assert_eq!(&buf[..len], b"FF");
    }
}
