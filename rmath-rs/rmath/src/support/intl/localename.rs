//! Port of localename.c -- Locale name handling.
//!
//! Determines the name of the currently selected locale for a given category.
//! On Unix systems this uses `setlocale()`. On macOS it may also query
//! CoreFoundation. On Windows it uses the Win32 API.
//!
//! For the standalone Rust port, we provide a simplified implementation
//! that queries the environment and provides reasonable defaults.

#![allow(non_snake_case, dead_code)]

use std::env;
use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::ptr;

use super::types::*;

// ---------------------------------------------------------------------------
// Internal helper: get locale from environment
// ---------------------------------------------------------------------------

unsafe fn alloc_env_var(name: &str) -> Option<*mut c_char> {
    unsafe {
        let val = env::var(name).ok()?;
        let cstr = CString::new(val.as_str()).ok()?;
        let layout = std::alloc::Layout::from_size_align(cstr.as_bytes_with_nul().len(), 1).ok()?;
        let out = std::alloc::alloc(layout) as *mut c_char;
        if out.is_null() {
            return None;
        }
        ptr::copy_nonoverlapping(
            cstr.as_ptr(),
            out as *mut libc::c_char,
            cstr.as_bytes_with_nul().len(),
        );
        Some(out)
    }
}

fn category_env_name(category: c_int) -> &'static str {
    match category {
        0 => "LC_CTYPE",
        1 => "LC_NUMERIC",
        2 => "LC_TIME",
        3 => "LC_COLLATE",
        4 => "LC_MONETARY",
        5 => "LC_MESSAGES",
        6 => "LC_ALL",
        _ => "LC_ALL",
    }
}

/// Get the locale name from the environment for the given category.
///
/// Follows the POSIX precedence: LC_ALL, LC_xxx, LANG.
unsafe fn get_locale_from_env(category: c_int) -> *const c_char {
    unsafe {
        if let Some(locale) = alloc_env_var("LC_ALL") {
            return locale;
        }
        if let Some(locale) = alloc_env_var(category_env_name(category)) {
            return locale;
        }
        if let Some(locale) = alloc_env_var("LANG") {
            return locale;
        }

        c_strdup(b"C\0".as_ptr() as *const c_char)
    }
}

// ---------------------------------------------------------------------------
// macOS-specific: get locale from CFLocale
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
unsafe fn get_locale_from_cf(category: c_int) -> *const c_char {
    unsafe {
        // On macOS, we could query CFLocaleCopyCurrent() for the locale.
        // For the standalone port, we fall through to the environment.
        get_locale_from_env(category)
    }
}

#[cfg(not(target_os = "macos"))]
unsafe fn get_locale_from_cf(category: c_int) -> *const c_char {
    get_locale_from_env(category)
}

// ---------------------------------------------------------------------------
// Public API: _nl_locale_name
// ---------------------------------------------------------------------------

/// Determine the name of the currently selected locale for the given category.
///
/// Returns a newly allocated NUL-terminated locale string.
///
/// # Safety
/// The caller owns the returned pointer and must release it with `c_free`.
pub unsafe fn _nl_locale_name(category: c_int) -> *const c_char {
    unsafe { get_locale_from_cf(category) }
}

/// Canonicalize a locale name.
///
/// Normalizes the locale name to a standard form. This is used by langprefs.c
/// to normalize macOS locale names.
///
/// # Safety
/// `name` must be a valid pointer to a mutable NUL-terminated C string buffer.
pub unsafe fn _nl_locale_name_canonicalize(name: *mut c_char) {
    unsafe {
        if name.is_null() {
            return;
        }

        // Normalize: convert language part to lowercase, territory to uppercase.
        let mut p = name;
        // Lowercase the language part.
        while *p != 0 && *p != b'_' as c_char && *p != b'.' as c_char && *p != b'@' as c_char {
            *p = (*p as u8).to_ascii_lowercase() as c_char;
            p = p.add(1);
        }

        if *p == b'_' as c_char {
            p = p.add(1);
            // Uppercase the territory part.
            while *p != 0 && *p != b'.' as c_char && *p != b'@' as c_char {
                *p = (*p as u8).to_ascii_uppercase() as c_char;
                p = p.add(1);
            }
        }

        // Leave codeset and modifier as-is.
    }
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
    fn test_locale_name_returns() {
        unsafe {
            let result = _nl_locale_name(LC_MESSAGES);
            // Should return either a valid string or "C".
            if !result.is_null() {
                let s = CStr::from_ptr(result).to_str().unwrap_or("");
                assert!(!s.is_empty());
                c_free(result as *mut c_char);
            }
        }
    }

    #[test]
    fn test_canonicalize_simple() {
        unsafe {
            let mut buf = b"en_us\0".to_vec();
            _nl_locale_name_canonicalize(buf.as_mut_ptr() as *mut c_char);
            let s = CStr::from_ptr(buf.as_ptr() as *const c_char)
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "en_US");
        }
    }

    #[test]
    fn test_canonicalize_with_codeset() {
        unsafe {
            let mut buf = b"en_us.utf-8\0".to_vec();
            _nl_locale_name_canonicalize(buf.as_mut_ptr() as *mut c_char);
            let s = CStr::from_ptr(buf.as_ptr() as *const c_char)
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "en_US.utf-8");
        }
    }

    #[test]
    fn test_canonicalize_null() {
        unsafe {
            _nl_locale_name_canonicalize(ptr::null_mut());
            // Should not crash.
        }
    }

    #[test]
    fn test_canonicalize_with_modifier() {
        unsafe {
            let mut buf = b"de_de.ISO-8859-1@euro\0".to_vec();
            _nl_locale_name_canonicalize(buf.as_mut_ptr() as *mut c_char);
            let s = CStr::from_ptr(buf.as_ptr() as *const c_char)
                .to_str()
                .unwrap_or("");
            assert_eq!(s, "de_DE.ISO-8859-1@euro");
        }
    }
}
