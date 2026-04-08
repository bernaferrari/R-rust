//! Port of dcngettext.c -- Domain/charset-specific plural translation wrapper.
//!
//! Look up MSGID in the DOMAINNAME message catalog for the current CATEGORY
//! locale. This is the core plural translation function; all other plural
//! wrappers delegate to it.

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_ulong};

use crate::intl::types;

pub unsafe fn libintl_dcngettext(
    domainname: *const c_char,
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
    category: types::c_int,
) -> *mut c_char {
    unsafe { crate::intl::dcigettext::libintl_dcigettext(domainname, msgid1, msgid2, n, category) }
}

/// Alias for `libintl_dcngettext` (unprefixed, for compatibility).
pub unsafe fn dcngettext(
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
    fn test_plural_no_catalog_returns_msgid1() {
        unsafe {
            let msgid1 = b"file\0" as *const u8 as *const c_char;
            let msgid2 = b"files\0" as *const u8 as *const c_char;
            let result =
                libintl_dcngettext(std::ptr::null(), msgid1, msgid2, 5, types::LC_MESSAGES);
            assert_eq!(result, msgid1 as *mut c_char);
        }
    }
}
