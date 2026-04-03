#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/version.c
//!
//! R version string constants and helpers.
//! The actual version numbers come from Rversion.h; here we provide
//! FFI-compatible stubs that can be overridden at link time.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;

/// R major version number (e.g., "4")
pub const R_MAJOR: &[u8] = b"4\0";

/// R minor version number (e.g., "3.0")
pub const R_MINOR: &[u8] = b"3.0\0";

/// R development status (e.g., "Under development (unstable)" or "")
pub const R_STATUS: &[u8] = b"Under development (unstable)\0";

/// R release year
pub const R_YEAR: &[u8] = b"2024\0";

/// R release month
pub const R_MONTH: &[u8] = b"04\0";

/// R release day
pub const R_DAY: &[u8] = b"24\0";

/// R SVN revision number (0 if not from SVN)
pub const R_SVN_REVISION: c_int = 0;

/// R nickname
pub const R_NICK: &[u8] = b"Something for Everyone\0";

/// R platform string
pub const R_PLATFORM: &[u8] = b"x86_64-apple-darwin\0";

/// R CPU architecture
pub const R_CPU: &[u8] = b"x86_64\0";

/// R OS name
pub const R_OS: &[u8] = b"darwin\0";

/// R internals UUID
pub const R_INTERNALS_UUID: &[u8] = b"unset\0";

/// Print the version string into the provided buffer.
///
/// # Safety
/// `buf` must point to a buffer of at least `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_version(buf: *mut c_char, len: usize) -> c_int {
    unsafe {
        if buf.is_null() || len == 0 {
            return -1;
        }

        let version_str = CStr::from_ptr(R_version_string());
        let vbytes = version_str.to_bytes_with_nul();
        let copy_len = vbytes.len().min(len - 1);
        ptr::copy_nonoverlapping(vbytes.as_ptr(), buf as *mut u8, copy_len);
        *buf.add(copy_len) = 0;
        copy_len as c_int
    }
}

/// Return a pointer to the static R version string.
///
/// Format: "R version MAJOR.MINOR STATUS (YEAR-MONTH-DAY)"
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_version_string() -> *const c_char {
    // We build a static string. Since we can't use format! at const time,
    // we use a pre-built static.
    // For simplicity, use a reasonable default.
    static VERSION: &[u8] = b"R version 4.3.0 Under development (unstable) (2024-04-24)\0";
    VERSION.as_ptr() as *const c_char
}

/// Return R_MAJOR
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_get_major() -> *const c_char {
    R_MAJOR.as_ptr() as *const c_char
}

/// Return R_MINOR
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_get_minor() -> *const c_char {
    R_MINOR.as_ptr() as *const c_char
}

/// Return R_YEAR
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_get_year() -> *const c_char {
    R_YEAR.as_ptr() as *const c_char
}

/// Return R_MONTH
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_get_month() -> *const c_char {
    R_MONTH.as_ptr() as *const c_char
}

/// Return R_DAY
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_get_day() -> *const c_char {
    R_DAY.as_ptr() as *const c_char
}

/// Return R_NICK
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_get_nick() -> *const c_char {
    R_NICK.as_ptr() as *const c_char
}

/// Return R_PLATFORM
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_get_platform() -> *const c_char {
    R_PLATFORM.as_ptr() as *const c_char
}

/// Return R_STATUS
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_get_status() -> *const c_char {
    R_STATUS.as_ptr() as *const c_char
}
