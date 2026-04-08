//! Port of dcgettext.c -- Domain/charset translation wrapper.
//!
//! Look up MSGID in the DOMAINNAME message catalog for the current CATEGORY
//! locale. This is the core translation function; all other non-plural
//! wrappers delegate to it.

#![allow(non_snake_case)]

use std::os::raw::c_char;

use crate::intl::types;

pub unsafe fn libintl_dcgettext(
    domainname: *const c_char,
    msgid: *const c_char,
    category: types::c_int,
) -> *mut c_char {
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
