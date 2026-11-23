#![warn(unused_unsafe)]
//! Port of dcgettext.c -- Domain/charset translation wrapper.
//!
//! Look up MSGID in the DOMAINNAME message catalog for the current CATEGORY
//! locale. This is the core translation function; all other non-plural
//! wrappers delegate to it.

#![allow(non_snake_case)]

use std::os::raw::c_char;

use crate::intl::types;

/// Pass through the input message pointer. Runtime catalog lookup is disabled.
/// The returned pointer borrows the input; this function does not read either pointer.
pub fn libintl_dcgettext(
    domainname: *const c_char,
    msgid: *const c_char,
    category: types::c_int,
) -> *mut c_char {
    crate::intl::dcigettext::libintl_dcigettext(
        domainname,
        msgid,
        std::ptr::null(),
        0,
        category,
    )
}

/// Alias for `libintl_dcgettext` (unprefixed, for compatibility).
#[allow(unused_imports)]
pub use self::libintl_dcgettext as dcgettext;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_returns_msgid() {
        let _session = crate::sexp::session::RSession::new();

        let msgid = c"hello".as_ptr();
        assert_eq!(
            libintl_dcgettext(std::ptr::null(), msgid, types::LC_MESSAGES),
            msgid.cast_mut()
        );
    }

    #[test]
    fn test_with_null_domain() {
        let _session = crate::sexp::session::RSession::new();

        let msgid = c"world".as_ptr();
        assert_eq!(
            libintl_dcgettext(std::ptr::null(), msgid, types::LC_MESSAGES),
            msgid.cast_mut()
        );
    }
}
