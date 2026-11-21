#![warn(unused_unsafe)]
//! Port of gettext.c -- Translation wrapper.
//!
//! Look up MSGID in the current default message catalog for the current
//! LC_MESSAGES locale. If not found, returns MSGID itself (the default text).

#![allow(non_snake_case)]

use std::os::raw::c_char;

use crate::intl::types;

/// Look up MSGID in the current default message catalog for the current
/// LC_MESSAGES locale. If not found, returns MSGID itself.
///
/// Delegates to `libintl_dcgettext(NULL, msgid, LC_MESSAGES)`.
///
/// # Safety
/// `msgid` must be a valid pointer to a NUL-terminated C string.
pub unsafe fn libintl_gettext(msgid: *const c_char) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `libintl_dcgettext`'s contract per this function's own `# Safety` requirements.
    unsafe {
        crate::intl::dcgettext::libintl_dcgettext(std::ptr::null(), msgid, types::LC_MESSAGES)
    }
}

/// Alias for `libintl_gettext` (unprefixed, for compatibility).
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn gettext(msgid: *const c_char) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `libintl_gettext`'s contract per this function's own `# Safety` requirements.
    unsafe { libintl_gettext(msgid) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_returns_msgid() {
        let _session = crate::sexp::session::RSession::new();

        let msgid = b"hello world\0" as *const u8 as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let result = unsafe { libintl_gettext(msgid) };
        assert_eq!(result, msgid as *mut c_char);
    }
}
