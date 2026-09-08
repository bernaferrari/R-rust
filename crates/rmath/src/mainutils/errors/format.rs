#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Low-level message formatting: vsnprintf binding, display-width utility,
//! buffer formatting helpers, and format argument counting.

use super::*;

// ---------------------------------------------------------------------------
// C library bindings
#[cfg(not(target_arch = "wasm32"))]
unsafe extern "C" {
    /// C's vsnprintf — format a string into a buffer with a va_list.
    /// On macOS, va_list is a pointer type.
    #[link_name = "vsnprintf"]
    pub(super) fn vsnprintf_c(
        buf: *mut c_char,
        size: usize,
        format: *const c_char,
        ap: *mut c_void,
    ) -> c_int;
}

/// wasm32 stand-in for C's vsnprintf.
///
/// Stable Rust cannot build a real `va_list`, so every caller in this port
/// passes a NULL `ap` with an already-formatted message (see
/// `format_varargs`). Mirror that contract: copy the format string as the
/// final message, honoring the size/truncation-return semantics.
#[cfg(target_arch = "wasm32")]
pub(super) unsafe fn vsnprintf_c(
    buf: *mut c_char,
    size: usize,
    format: *const c_char,
    _ap: *mut c_void,
) -> c_int {
    unsafe {
        if format.is_null() {
            return 0;
        }
        let bytes = CStr::from_ptr(format).to_bytes();
        if size > 0 && !buf.is_null() {
            let len = bytes.len().min(size - 1);
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, len);
            *buf.add(len) = 0;
        }
        bytes.len() as c_int
    }
}

// ---------------------------------------------------------------------------
// Display width utility
// ---------------------------------------------------------------------------

/// Compute the display width of a string in columns.
/// Ported from R's `wd()` function in errors.c.
pub fn wd(buf: &str) -> usize {
    buf.chars().count()
}

/// Display width from C string.
pub(super) unsafe fn wd_c(s: *const c_char) -> usize {
    unsafe {
        if s.is_null() {
            return 0;
        }
        let str = CStr::from_ptr(s).to_str().unwrap_or("");
        wd(str)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Format string helper — truncates at BUFSIZE, null-terminates.
pub(super) fn format_to_buf(buf: &mut [u8; BUFSIZE + 1], fmt: &str) -> (usize, bool) {
    let mut truncated = false;
    let bytes = fmt.as_bytes();
    if bytes.len() >= BUFSIZE {
        // Find a safe truncation point (don't split multi-byte chars)
        let mut end = BUFSIZE - 1;
        while end > 0 && (bytes[end] & 0xC0) == 0x80 {
            end -= 1;
        }
        buf[..end].copy_from_slice(&bytes[..end]);
        buf[end] = 0;
        truncated = true;
    } else {
        buf[..bytes.len()].copy_from_slice(bytes);
        buf[bytes.len()] = 0;
    }
    (bytes.len(), truncated)
}

/// Append to buf, ensuring we don't overflow and don't split multi-byte chars.
pub(super) fn bufcat(buf: &mut [u8], txt: &str) {
    let cur_len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let remaining = buf.len().saturating_sub(cur_len);
    if remaining == 0 {
        return;
    }
    let bytes = txt.as_bytes();
    let copy_len = bytes.len().min(remaining.saturating_sub(1));
    buf[cur_len..cur_len + copy_len].copy_from_slice(&bytes[..copy_len]);
    buf[cur_len + copy_len] = 0;
}

/// Append "[... truncated]" if needed.
pub(super) fn print_trunc(buf: &mut [u8; BUFSIZE + 1], truncated: bool) {
    if truncated {
        let cur_len = buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE);
        let msg = " [... truncated]";
        if cur_len + msg.len() < BUFSIZE {
            bufcat(buf, msg);
        }
    }
}

/// Format a printf-style format string with variadic arguments into a Rust String.
/// Uses C's vsnprintf via FFI. Returns the formatted string.
///
/// Note: The `ap` parameter is only meaningful when called from C code that passes
/// a real va_list. When called from Rust, ap is typically null and the format
/// string should be pre-formatted.
pub(super) unsafe fn format_varargs(format: *const c_char, ap: *mut c_void) -> String {
    unsafe {
        if format.is_null() {
            return String::new();
        }
        if ap.is_null() {
            // No va_list — format string is already the final message
            return CStr::from_ptr(format).to_str().unwrap_or("").to_string();
        }
        // First pass: determine required size
        let needed = vsnprintf_c(ptr::null_mut(), 0, format, ap);
        if needed < 0 {
            let fallback = CStr::from_ptr(format).to_str().unwrap_or("");
            return fallback.to_string();
        }
        let needed = needed as usize + 1; // +1 for null terminator
        // Second pass: format into buffer
        let mut buf = vec![0u8; needed];
        vsnprintf_c(buf.as_mut_ptr() as *mut c_char, needed, format, ap);
        // Trim trailing null
        if let Some(pos) = buf.iter().position(|&b| b == 0) {
            buf.truncate(pos);
        }
        String::from_utf8_lossy(&buf).into_owned()
    }
}

/// Format a printf-style format string with variadic arguments into a buffer.
/// Uses C's vsnprintf via FFI. Returns (formatted_string, was_truncated).
pub(super) unsafe fn format_varargs_to_buf(
    format: *const c_char,
    ap: *mut c_void,
) -> (String, bool) {
    unsafe {
        if format.is_null() {
            return (String::new(), false);
        }
        if ap.is_null() {
            let s = CStr::from_ptr(format).to_str().unwrap_or("").to_string();
            return (s, false);
        }
        let psize = std::cmp::min(BUFSIZE, r_warn_length() as usize) + 1;
        let mut buf = vec![0u8; psize];
        let pval = vsnprintf_c(buf.as_mut_ptr() as *mut c_char, psize, format, ap);
        let truncated = pval >= psize as i32;
        // Ensure null termination
        if psize > 0 {
            buf[psize - 1] = 0;
        }
        // Trim to null
        if let Some(pos) = buf.iter().position(|&b| b == 0) {
            buf.truncate(pos);
        }
        let s = String::from_utf8_lossy(&buf).into_owned();
        (s, truncated)
    }
}

// ---------------------------------------------------------------------------
// Message formatting helpers
// ---------------------------------------------------------------------------

/// Count the number of % escapes in a format string.
pub(super) fn count_format_args(s: &str) -> usize {
    let mut count = 0;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.peek() {
                Some('%') => {
                    chars.next();
                }
                Some(&c)
                    if !matches!(
                        c,
                        's' | 'd' | 'f' | 'g' | 'e' | 'i' | 'o' | 'u' | 'x' | 'X' | 'c' | 'p' | 'l'
                    ) => {}
                Some(_) => {
                    count += 1;
                }
                None => {}
            }
        }
    }
    count
}
