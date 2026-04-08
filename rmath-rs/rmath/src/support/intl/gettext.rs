//! Port of gettext.c -- Translation wrapper.
//!
//! Look up MSGID in the current default message catalog for the current
//! LC_MESSAGES locale. If not found, returns MSGID itself (the default text).

#![allow(non_snake_case)]

use std::os::raw::c_char;

use crate::support::intl::types;

/// Look up MSGID in the current default message catalog for the current
/// LC_MESSAGES locale. If not found, returns MSGID itself.
///
/// Delegates to `libintl_dcgettext(NULL, msgid, LC_MESSAGES)`.
///
/// # Safety
/// `msgid` must be a valid pointer to a NUL-terminated C string.
pub unsafe fn libintl_gettext(msgid: *const c_char) -> *mut c_char {
    unsafe {
        crate::support::intl::dcgettext::libintl_dcgettext(
            std::ptr::null(),
            msgid,
            types::LC_MESSAGES,
        )
    }
}

/// Alias for `libintl_gettext` (unprefixed, for compatibility).
pub unsafe fn gettext(msgid: *const c_char) -> *mut c_char {
    unsafe { libintl_gettext(msgid) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_returns_msgid() {
        unsafe {
            let msgid = b"hello world\0" as *const u8 as *const c_char;
            let result = libintl_gettext(msgid);
            assert_eq!(result, msgid as *mut c_char);
        }
    }
}
