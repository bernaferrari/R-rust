//! Port of ngettext.c -- Plural form translation wrapper.
//!
//! Look up MSGID1/MSGID2 in the current default message catalog for the current
//! LC_MESSAGES locale. If not found, returns the appropriate form (singular or plural).

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_ulong};

use crate::intl::types;

/// Look up MSGID1/MSGID2 in the current default message catalog for the current
/// LC_MESSAGES locale, selecting the plural form based on N.
///
/// Delegates to `libintl_dcngettext(NULL, msgid1, msgid2, n, LC_MESSAGES)`.
///
/// # Safety
/// All string pointers must be valid NUL-terminated C strings.
pub unsafe fn libintl_ngettext(
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
) -> *mut c_char {
    unsafe {
        crate::intl::dcngettext::libintl_dcngettext(
            std::ptr::null(),
            msgid1,
            msgid2,
            n,
            types::LC_MESSAGES,
        )
    }
}

/// Alias for `libintl_ngettext` (unprefixed, for compatibility).
pub unsafe fn ngettext(msgid1: *const c_char, msgid2: *const c_char, n: c_ulong) -> *mut c_char {
    unsafe { libintl_ngettext(msgid1, msgid2, n) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singular_form() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s1 = b"cat\0" as *const u8 as *const c_char;
            let s2 = b"cats\0" as *const u8 as *const c_char;
            let result = libintl_ngettext(s1, s2, 1);
            assert_eq!(result, s1 as *mut c_char);
        }
    }

    #[test]
    fn test_plural_no_catalog() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s1 = b"cat\0" as *const u8 as *const c_char;
            let s2 = b"cats\0" as *const u8 as *const c_char;
            let result = libintl_ngettext(s1, s2, 5);
            assert_eq!(result, s1 as *mut c_char);
        }
    }
}
