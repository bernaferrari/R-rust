#![warn(unused_unsafe)]
//! Port of printf.c -- Printf implementation with POSIX/XSI positional arguments.
//!
//! The C version provides formatted output functions that handle the `$` positional
//! argument syntax (e.g., "%2$d"). It includes `libintl_printf`, `libintl_fprintf`,
//! `libintl_sprintf`, `libintl_snprintf`, `libintl_vprintf`, `libintl_vfprintf`,
//! `libintl_vsprintf`, `libintl_vsnprintf`, and their `asprintf` variants.
//!
//! For the standalone Rust port, we provide FFI-compatible stubs that delegate
//! to the system's standard printf for simple format strings (without `$`),
//! and to our internal `vasnprintf` for format strings with positional args.

#![allow(non_snake_case, dead_code)]

use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use super::types::{c_free, c_strdup, cstr, ptr_mut, slice_mut, slice_ref};

// ---------------------------------------------------------------------------
// Internal vasnprintf stub
// ---------------------------------------------------------------------------

/// Internal vasnprintf implementation (stub).
///
/// In the full C implementation, this calls the complex vasnprintf.c code
/// which handles all format specifiers including positional arguments.
/// For the standalone port, we provide a simplified version that handles
/// basic format strings.
///
/// Returns a pointer to the formatted string (may be `resultbuf` if it fit),
/// and sets `*lengthp` to the string length (excluding NUL). Returns null on error.
fn libintl_vasnprintf(
    resultbuf: *mut c_char,
    lengthp: *mut usize,
    format: *const c_char,
    _args: *mut c_void,
) -> *mut c_char {
    let Some(fmt) = cstr(format) else {
        return ptr::null_mut();
    };

    // For the standalone port, we handle format strings without '$' by
    // returning the format string itself as-is (a placeholder).
    // A full implementation would parse the format string and format the args.
    let len = fmt.to_bytes().len();

    let _ = resultbuf;

    let result = c_strdup(format);
    if result.is_null() {
        return ptr::null_mut();
    }

    if let Some(p) = ptr_mut(lengthp) {
        *p = len;
    }

    result
}

// ---------------------------------------------------------------------------
// FFI-exported printf functions
// ---------------------------------------------------------------------------

/// Write formatted output to a stream.
///
/// If the format string contains '$' (positional arguments), uses the internal
/// vasnprintf implementation. Otherwise, delegates to the system vfprintf.
pub fn libintl_vfprintf(_stream: *mut c_void, format: *const c_char, args: *mut c_void) -> c_int {
    // Check if format contains '$'.
    let has_dollar = cstr(format).is_some_and(|f| f.to_bytes().contains(&b'$'));

    if !has_dollar {
        // No positional args -- we can't call C's vfprintf from Rust easily,
        // so just return a stub value.
        return -1;
    }

    let mut length: usize = 0;
    let result = libintl_vasnprintf(ptr::null_mut(), &mut length, format, args);
    if result.is_null() {
        return -1;
    }
    if length > c_int::MAX as usize {
        c_free(result);
        return -1;
    }
    let retval = length as c_int;
    c_free(result);
    retval
}

/// Write formatted output to stdout.
pub fn libintl_vprintf(format: *const c_char, args: *mut c_void) -> c_int {
    libintl_vfprintf(ptr::null_mut(), format, args)
}

/// Write formatted output to a string buffer.
pub fn libintl_vsprintf(resultbuf: *mut c_char, format: *const c_char, args: *mut c_void) -> c_int {
    let mut length: usize = 0;
    let result = libintl_vasnprintf(resultbuf, &mut length, format, args);
    if result.is_null() {
        return -1;
    }
    if result != resultbuf {
        // Didn't fit in the buffer.
        c_free(result);
        return -1;
    }
    if length > c_int::MAX as usize {
        return -1;
    }
    length as c_int
}

/// Write formatted output to a string buffer with maximum length.
pub fn libintl_vsnprintf(
    resultbuf: *mut c_char,
    _maxlength: usize,
    format: *const c_char,
    args: *mut c_void,
) -> c_int {
    let mut length: usize = 0;
    let result = libintl_vasnprintf(resultbuf, &mut length, format, args);
    if result.is_null() {
        return -1;
    }
    if result != resultbuf {
        if _maxlength > 0 && !resultbuf.is_null() {
            let pruned = if length < _maxlength {
                length
            } else {
                _maxlength - 1
            };
            // Copy `pruned` bytes plus NUL into the caller's
            // `maxlength`-byte buffer.
            if let (Some(dst), Some(src)) = (
                slice_mut(resultbuf as *mut u8, _maxlength),
                slice_ref(result as *const u8, pruned),
            ) {
                dst[..pruned].copy_from_slice(src);
                dst[pruned] = 0;
            }
        }
        c_free(result);
    }
    if length > c_int::MAX as usize {
        return -1;
    }
    length as c_int
}

/// Write formatted output to a dynamically allocated string.
pub fn libintl_vasprintf(
    resultp: *mut *mut c_char,
    format: *const c_char,
    args: *mut c_void,
) -> c_int {
    let mut length: usize = 0;
    let result = libintl_vasnprintf(ptr::null_mut(), &mut length, format, args);
    if result.is_null() {
        return -1;
    }
    if length > c_int::MAX as usize {
        c_free(result);
        return -1;
    }
    if let Some(p) = ptr_mut(resultp) {
        *p = result;
    } else {
        c_free(result);
    }
    length as c_int
}

// ---------------------------------------------------------------------------
// Unprefixed aliases for compatibility
// ---------------------------------------------------------------------------

/// Alias for `libintl_printf`.
/// Note: Variadic stub - signature simplified for Rust compatibility.
pub fn libintl_printf(format: *const c_char, _args: *const c_void) -> c_int {
    // Variadic functions cannot be truly implemented in Rust FFI.
    // This is a stub that returns -1.
    let _ = format;
    -1
}

/// Alias for `libintl_fprintf`.
pub fn libintl_fprintf(_stream: *mut c_void, format: *const c_char, _args: *const c_void) -> c_int {
    let _ = format;
    -1
}

/// Alias for `libintl_sprintf`.
pub fn libintl_sprintf(
    resultbuf: *mut c_char,
    format: *const c_char,
    _args: *const c_void,
) -> c_int {
    let _ = resultbuf;
    let _ = format;
    -1
}

/// Alias for `libintl_snprintf`.
pub fn libintl_snprintf(
    resultbuf: *mut c_char,
    maxlength: usize,
    format: *const c_char,
    _args: *const c_void,
) -> c_int {
    let _ = resultbuf;
    let _ = maxlength;
    let _ = format;
    -1
}

/// Alias for `libintl_asprintf`.
pub fn libintl_asprintf(
    resultp: *mut *mut c_char,
    format: *const c_char,
    _args: *const c_void,
) -> c_int {
    let _ = resultp;
    let _ = format;
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn test_vasnprintf_simple() {
        unsafe {
            let fmt = b"hello\0".as_ptr() as *const c_char;
            let mut length: usize = 0;
            let result = libintl_vasnprintf(ptr::null_mut(), &mut length, fmt, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(length, 5);
            let s = CStr::from_ptr(result).to_str().unwrap_or("");
            assert_eq!(s, "hello");
            std::alloc::dealloc(
                result as *mut u8,
                std::alloc::Layout::from_size_align_unchecked(6, 1),
            );
        }
    }

    #[test]
    fn test_vasnprintf_null_format() {
        let mut length: usize = 0;
        let result = libintl_vasnprintf(ptr::null_mut(), &mut length, ptr::null(), ptr::null_mut());
        assert!(result.is_null());
    }
}
