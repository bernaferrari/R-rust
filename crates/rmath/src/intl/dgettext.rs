#![warn(unused_unsafe)]
//! Port of dgettext.c -- Domain-specific translation wrapper.
//!
//! Look up MSGID in the DOMAINNAME message catalog of the current LC_MESSAGES locale.

#![allow(non_snake_case)]

use std::os::raw::c_char;

use crate::intl::types;

/// Pass through the input message pointer. Runtime catalog lookup is disabled.
/// The returned pointer borrows the input; this function does not read either pointer.
///
/// Delegates to `libintl_dcgettext(domainname, msgid, LC_MESSAGES)`.
pub fn libintl_dgettext(domainname: *const c_char, msgid: *const c_char) -> *mut c_char {
    crate::intl::dcgettext::libintl_dcgettext(domainname, msgid, types::LC_MESSAGES)
}

/// Alias for `libintl_dgettext` (unprefixed, for compatibility).
#[allow(unused_imports)]
pub use self::libintl_dgettext as dgettext;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_returns_msgid() {
        let _session = crate::sexp::session::RSession::new();

        let msgid = c"hello".as_ptr();
        assert_eq!(libintl_dgettext(std::ptr::null(), msgid), msgid.cast_mut());
    }

    #[test]
    fn test_with_domain() {
        let _session = crate::sexp::session::RSession::new();

        let domain = c"mydomain".as_ptr();
        let msgid = c"test".as_ptr();
        assert_eq!(libintl_dgettext(domain, msgid), msgid.cast_mut());
    }
}
