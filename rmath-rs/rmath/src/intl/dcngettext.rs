//! Port of dcngettext.c -- Domain/charset-specific plural translation wrapper.
//!
//! Look up MSGID in the DOMAINNAME message catalog for the current CATEGORY
//! locale. This is the core plural translation function; all other plural
//! wrappers delegate to it.

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_ulong};

use crate::intl::types;

/// Look up MSGID1/MSGID2 in the DOMAINNAME message catalog for the given CATEGORY,
/// selecting the plural form based on N.
///
/// This is a thin wrapper that delegates to `libintl_dcigettext`. In this stub
/// implementation, it returns MSGID1 (the singular form) as a fallback.
///
/// # Safety
/// All string pointers must be valid NUL-terminated C strings (or NULL for domainname).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn libintl_dcngettext(
    _domainname: *const c_char,
    msgid1: *const c_char,
    _msgid2: *const c_char,
    _n: c_ulong,
    _category: types::c_int,
) -> *mut c_char {
    // Stub: return msgid1 as-is (cast away const for C compatibility).
    // In the full implementation, this would call libintl_dcigettext().
    if _n == 1 {
        msgid1 as *mut c_char
    } else {
        // For n != 1, return msgid2 if provided, else msgid1
        if !_msgid2.is_null() {
            _msgid2 as *mut c_char
        } else {
            msgid1 as *mut c_char
        }
    }
}

/// Alias for `libintl_dcngettext` (unprefixed, for compatibility).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dcngettext(
    domainname: *const c_char,
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
    category: types::c_int,
) -> *mut c_char {
    unsafe { libintl_dcngettext(domainname, msgid1, msgid2, n, category) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singular_returns_msgid1() {
        unsafe {
            let msgid1 = b"file\0" as *const u8 as *const c_char;
            let msgid2 = b"files\0" as *const u8 as *const c_char;
            let result =
                libintl_dcngettext(std::ptr::null(), msgid1, msgid2, 1, types::LC_MESSAGES);
            assert_eq!(result, msgid1 as *mut c_char);
        }
    }

    #[test]
    fn test_plural_returns_msgid2() {
        unsafe {
            let msgid1 = b"file\0" as *const u8 as *const c_char;
            let msgid2 = b"files\0" as *const u8 as *const c_char;
            let result =
                libintl_dcngettext(std::ptr::null(), msgid1, msgid2, 5, types::LC_MESSAGES);
            assert_eq!(result, msgid2 as *mut c_char);
        }
    }
}
