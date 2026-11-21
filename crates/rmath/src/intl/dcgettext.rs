#![warn(unused_unsafe)]
//! Port of dcgettext.c -- Domain/charset translation wrapper.
//!
//! Look up MSGID in the DOMAINNAME message catalog for the current CATEGORY
//! locale. This is the core translation function; all other non-plural
//! wrappers delegate to it.

#![allow(non_snake_case)]

use std::os::raw::c_char;

use crate::intl::types;

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn libintl_dcgettext(
    domainname: *const c_char,
    msgid: *const c_char,
    category: types::c_int,
) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `libintl_dcigettext`'s contract per this function's own `# Safety` requirements.
    unsafe {
        crate::intl::dcigettext::libintl_dcigettext(
            domainname,
            msgid,
            std::ptr::null(),
            0,
            category,
        )
    }
}

/// Alias for `libintl_dcgettext` (unprefixed, for compatibility).
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn dcgettext(
    domainname: *const c_char,
    msgid: *const c_char,
    category: types::c_int,
) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `libintl_dcgettext`'s contract per this function's own `# Safety` requirements.
    unsafe { libintl_dcgettext(domainname, msgid, category) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_returns_msgid() {
        let _session = crate::sexp::session::RSession::new();

        let msgid = b"hello\0" as *const u8 as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result = unsafe { libintl_dcgettext(std::ptr::null(), msgid, types::LC_MESSAGES) };
        assert_eq!(result, msgid as *mut c_char);
    }

    #[test]
    fn test_with_null_domain() {
        let _session = crate::sexp::session::RSession::new();

        let msgid = b"world\0" as *const u8 as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result = unsafe { libintl_dcgettext(std::ptr::null(), msgid, types::LC_MESSAGES) };
        assert_eq!(result, msgid as *mut c_char);
    }
}
