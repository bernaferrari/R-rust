#![warn(unused_unsafe)]
//! Port of dcngettext.c -- Domain/charset-specific plural translation wrapper.
//!
//! Look up MSGID in the DOMAINNAME message catalog for the current CATEGORY
//! locale. This is the core plural translation function; all other plural
//! wrappers delegate to it.

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_ulong};

use crate::intl::types;

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn libintl_dcngettext(
    domainname: *const c_char,
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
    category: types::c_int,
) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `libintl_dcigettext`'s contract per this function's own `# Safety` requirements.
    unsafe { crate::intl::dcigettext::libintl_dcigettext(domainname, msgid1, msgid2, n, category) }
}

/// Alias for `libintl_dcngettext` (unprefixed, for compatibility).
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn dcngettext(
    domainname: *const c_char,
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
    category: types::c_int,
) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `libintl_dcngettext`'s contract per this function's own `# Safety` requirements.
    unsafe { libintl_dcngettext(domainname, msgid1, msgid2, n, category) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singular_returns_msgid1() {
        let _session = crate::sexp::session::RSession::new();

        let msgid1 = b"file\0" as *const u8 as *const c_char;
        let msgid2 = b"files\0" as *const u8 as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result =
            unsafe { libintl_dcngettext(std::ptr::null(), msgid1, msgid2, 1, types::LC_MESSAGES) };
        assert_eq!(result, msgid1 as *mut c_char);
    }

    #[test]
    fn test_plural_no_catalog_returns_msgid1() {
        let _session = crate::sexp::session::RSession::new();

        let msgid1 = b"file\0" as *const u8 as *const c_char;
        let msgid2 = b"files\0" as *const u8 as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result =
            unsafe { libintl_dcngettext(std::ptr::null(), msgid1, msgid2, 5, types::LC_MESSAGES) };
        assert_eq!(result, msgid1 as *mut c_char);
    }
}
