#![warn(unused_unsafe)]
//! Port of dgettext.c -- Domain-specific translation wrapper.
//!
//! Look up MSGID in the DOMAINNAME message catalog of the current LC_MESSAGES locale.

#![allow(non_snake_case)]

use std::os::raw::c_char;

use crate::intl::types;

/// Look up MSGID in the DOMAINNAME message catalog of the current LC_MESSAGES locale.
///
/// Delegates to `libintl_dcgettext(domainname, msgid, LC_MESSAGES)`.
///
/// # Safety
/// All string pointers must be valid NUL-terminated C strings (or NULL for domainname).
pub unsafe fn libintl_dgettext(domainname: *const c_char, msgid: *const c_char) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `libintl_dcgettext`'s contract per this function's own `# Safety` requirements.
    unsafe { crate::intl::dcgettext::libintl_dcgettext(domainname, msgid, types::LC_MESSAGES) }
}

/// Alias for `libintl_dgettext` (unprefixed, for compatibility).
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn dgettext(domainname: *const c_char, msgid: *const c_char) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `libintl_dgettext`'s contract per this function's own `# Safety` requirements.
    unsafe { libintl_dgettext(domainname, msgid) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_returns_msgid() {
        let _session = crate::sexp::session::RSession::new();

        let msgid = b"hello\0" as *const u8 as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result = unsafe { libintl_dgettext(std::ptr::null(), msgid) };
        assert_eq!(result, msgid as *mut c_char);
    }

    #[test]
    fn test_with_domain() {
        let _session = crate::sexp::session::RSession::new();

        let domain = b"mydomain\0" as *const u8 as *const c_char;
        let msgid = b"test\0" as *const u8 as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result = unsafe { libintl_dgettext(domain, msgid) };
        assert_eq!(result, msgid as *mut c_char);
    }
}
