//! Port of intl-compat.c -- Compatibility wrappers for gettext functions.
//!
//! This file provides stub implementations for gettext functions that are
//! not defined in the individual module files. In the C implementation,
//! this file redirects unprefixed functions to libintl_-prefixed ones.
//!
//! The real implementations of bindtextdomain and bind_textdomain_codeset
//! live in bindtextdom.rs, so we only provide the unprefixed aliases here.

#![allow(non_snake_case)]

use std::os::raw::c_char;

/// Alias for `bindtextdomain()` that calls the libintl_ version.
pub unsafe fn bindtextdomain(domainname: *const c_char, dirname: *const c_char) -> *mut c_char {
    unsafe { crate::intl::bindtextdom::libintl_bindtextdomain(domainname, dirname) }
}

/// Alias for `bind_textdomain_codeset()` that calls the libintl_ version.
pub unsafe fn bind_textdomain_codeset(
    domainname: *const c_char,
    codeset: *const c_char,
) -> *mut c_char {
    unsafe { crate::intl::bindtextdom::libintl_bind_textdomain_codeset(domainname, codeset) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bindtextdomain_alias() {
        unsafe {
            let result = bindtextdomain(
                b"test\0" as *const u8 as *const c_char,
                b"/usr/share/locale\0" as *const u8 as *const c_char,
            );
            assert!(!result.is_null());
        }
    }

    #[test]
    fn test_bind_textdomain_codeset_alias() {
        unsafe {
            let result = bind_textdomain_codeset(
                b"test\0" as *const u8 as *const c_char,
                b"UTF-8\0" as *const u8 as *const c_char,
            );
            assert!(!result.is_null());
        }
    }
}
