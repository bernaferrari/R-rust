//! Port of dgettext.c -- Domain-specific translation wrapper.
//!
//! Look up MSGID in the DOMAINNAME message catalog of the current LC_MESSAGES locale.

#![allow(non_snake_case)]

use std::os::raw::c_char;

use crate::support::intl::types;

/// Look up MSGID in the DOMAINNAME message catalog of the current LC_MESSAGES locale.
///
/// Delegates to `libintl_dcgettext(domainname, msgid, LC_MESSAGES)`.
///
/// # Safety
/// All string pointers must be valid NUL-terminated C strings (or NULL for domainname).
pub unsafe fn libintl_dgettext(
    domainname: *const c_char,
    msgid: *const c_char,
) -> *mut c_char {
    unsafe {
        crate::support::intl::dcgettext::libintl_dcgettext(domainname, msgid, types::LC_MESSAGES)
    }
}

/// Alias for `libintl_dgettext` (unprefixed, for compatibility).
pub unsafe fn dgettext(domainname: *const c_char, msgid: *const c_char) -> *mut c_char {
    unsafe { libintl_dgettext(domainname, msgid) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_returns_msgid() {
        unsafe {
            let msgid = b"hello\0" as *const u8 as *const c_char;
            let result = libintl_dgettext(std::ptr::null(), msgid);
            assert_eq!(result, msgid as *mut c_char);
        }
    }

    #[test]
    fn test_with_domain() {
        unsafe {
            let domain = b"mydomain\0" as *const u8 as *const c_char;
            let msgid = b"test\0" as *const u8 as *const c_char;
            let result = libintl_dgettext(domain, msgid);
            assert_eq!(result, msgid as *mut c_char);
        }
    }
}
