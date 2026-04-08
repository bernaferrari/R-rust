//! Port of dngettext.c -- Domain-specific plural translation wrapper.
//!
//! Look up MSGID1/MSGID2 in the DOMAINNAME message catalog of the current
//! LC_MESSAGES locale and select the appropriate plural form.

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_ulong};

use crate::support::intl::types;

/// Look up MSGID1/MSGID2 in the DOMAINNAME message catalog of the current
/// LC_MESSAGES locale, selecting the plural form based on N.
///
/// Delegates to `libintl_dcngettext(domainname, msgid1, msgid2, n, LC_MESSAGES)`.
///
/// # Safety
/// All string pointers must be valid NUL-terminated C strings (or NULL for domainname).
pub unsafe fn libintl_dngettext(
    domainname: *const c_char,
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
) -> *mut c_char {
    unsafe {
        crate::support::intl::dcngettext::libintl_dcngettext(
            domainname,
            msgid1,
            msgid2,
            n,
            types::LC_MESSAGES,
        )
    }
}

/// Alias for `libintl_dngettext` (unprefixed, for compatibility).
pub unsafe fn dngettext(
    domainname: *const c_char,
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
) -> *mut c_char {
    unsafe { libintl_dngettext(domainname, msgid1, msgid2, n) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singular_with_domain() {
        unsafe {
            let domain = b"myapp\0" as *const u8 as *const c_char;
            let s1 = b"file\0" as *const u8 as *const c_char;
            let s2 = b"files\0" as *const u8 as *const c_char;
            let result = libintl_dngettext(domain, s1, s2, 1);
            assert_eq!(result, s1 as *mut c_char);
        }
    }

    #[test]
    fn test_plural_with_domain() {
        unsafe {
            let domain = b"myapp\0" as *const u8 as *const c_char;
            let s1 = b"file\0" as *const u8 as *const c_char;
            let s2 = b"files\0" as *const u8 as *const c_char;
            let result = libintl_dngettext(domain, s1, s2, 10);
            assert_eq!(result, s2 as *mut c_char);
        }
    }
}
