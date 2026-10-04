#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/version.c
//!
//! R version string constants and helpers.
//! C projections of the same pinned target used by R introspection and streams.

use std::os::raw::{c_char, c_int};

/// R major version number (e.g., "4")
pub const R_MAJOR: &[u8] = crate::mainutils::compatibility_target::MAJOR_C;

/// R minor version number (e.g., "3.0")
pub const R_MINOR: &[u8] = crate::mainutils::compatibility_target::MINOR_C;

/// R development status (e.g., "Under development (unstable)" or "")
pub const R_STATUS: &[u8] = crate::mainutils::compatibility_target::STATUS_C;

/// R release year
pub const R_YEAR: &[u8] = crate::mainutils::compatibility_target::YEAR_C;

/// R release month
pub const R_MONTH: &[u8] = crate::mainutils::compatibility_target::MONTH_C;

/// R release day
pub const R_DAY: &[u8] = crate::mainutils::compatibility_target::DAY_C;

/// R SVN revision number (0 if not from SVN)
pub const R_SVN_REVISION: c_int = crate::mainutils::compatibility_target::REVISION;

/// R nickname
pub const R_NICK: &[u8] = crate::mainutils::compatibility_target::NICKNAME_C;

/// R platform string
pub const R_PLATFORM: &[u8] = &crate::mainutils::compatibility_target::PLATFORM_C;

/// R CPU architecture
pub const R_CPU: &[u8] = &crate::mainutils::compatibility_target::ARCH_C;

/// R OS name
pub const R_OS: &[u8] = &crate::mainutils::compatibility_target::OS_C;

/// R internals UUID
pub const R_INTERNALS_UUID: &[u8] = b"unset\0";

/// Print the version string into the provided buffer.
///
/// # Safety
/// `buf` must point to a buffer of at least `len` bytes.
pub unsafe fn R_version(buf: *mut c_char, len: usize) -> c_int {
    unsafe {
        if buf.is_null() || len == 0 {
            return -1;
        }

        // SAFETY: the compatibility caller owns these `len` writable bytes.
        let output = std::slice::from_raw_parts_mut(buf.cast::<u8>(), len);
        crate::mainutils::compatibility_target::write_version(output).unwrap() as c_int
    }
}

/// Return a pointer to the static R version string.
///
/// Identifies the Rust port and its pinned GNU compatibility target.
pub fn R_version_string() -> *const c_char {
    crate::mainutils::compatibility_target::VERSION_STRING_C
        .as_ptr()
        .cast()
}

/// Return R_MAJOR
pub fn R_get_major() -> *const c_char {
    R_MAJOR.as_ptr() as *const c_char
}

/// Return R_MINOR
pub fn R_get_minor() -> *const c_char {
    R_MINOR.as_ptr() as *const c_char
}

/// Return R_YEAR
pub fn R_get_year() -> *const c_char {
    R_YEAR.as_ptr() as *const c_char
}

/// Return R_MONTH
pub fn R_get_month() -> *const c_char {
    R_MONTH.as_ptr() as *const c_char
}

/// Return R_DAY
pub fn R_get_day() -> *const c_char {
    R_DAY.as_ptr() as *const c_char
}

/// Return R_NICK
pub fn R_get_nick() -> *const c_char {
    R_NICK.as_ptr() as *const c_char
}

/// Return R_PLATFORM
pub fn R_get_platform() -> *const c_char {
    R_PLATFORM.as_ptr() as *const c_char
}

/// Return R_STATUS
pub fn R_get_status() -> *const c_char {
    R_STATUS.as_ptr() as *const c_char
}
