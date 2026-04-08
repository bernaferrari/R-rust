//! Port of dcgettext.c -- Domain/charset translation wrapper.
//!
//! Look up MSGID in the DOMAINNAME message catalog for the current CATEGORY
//! locale. This is the core translation function; all other non-plural
//! wrappers delegate to it.

#![allow(non_snake_case)]

use std::os::raw::c_char;

use crate::support::intl::types;

/// Look up MSGID in the DOMAINNAME message catalog for the given CATEGORY.
///
/// This is a thin wrapper that delegates to `libintl_dcigettext`. In this stub
/// implementation, it returns MSGID as-is (the untranslated string).
///
/// # Safety
/// All string pointers must be valid NUL-terminated C strings (or NULL for domainname).
pub unsafe fn libintl_dcgettext(
    _domainname: *const c_char,
    msgid: *const c_char,
    _category: types::c_int,
) -> *mut c_char {
    // Stub: return msgid as-is (cast away const for C compatibility).
    // In the full implementation, this would call libintl_dcigettext()
    // with (domainname, msgid, NULL, 0, 0, category).
    msgid as *mut c_char
}

/// Alias for `libintl_dcgettext` (unprefixed, for compatibility).
pub unsafe fn dcgettext(
    domainname: *const c_char,
    msgid: *const c_char,
    category: types::c_int,
) -> *mut c_char {
    unsafe { libintl_dcgettext(domainname, msgid, category) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_returns_msgid() {
        unsafe {
            let msgid = b"hello\0" as *const u8 as *const c_char;
            let result = libintl_dcgettext(std::ptr::null(), msgid, types::LC_MESSAGES);
            assert_eq!(result, msgid as *mut c_char);
        }
    }

    #[test]
    fn test_with_null_domain() {
        unsafe {
            let msgid = b"world\0" as *const u8 as *const c_char;
            let result = libintl_dcgettext(std::ptr::null(), msgid, types::LC_MESSAGES);
            assert_eq!(result, msgid as *mut c_char);
        }
    }
}
