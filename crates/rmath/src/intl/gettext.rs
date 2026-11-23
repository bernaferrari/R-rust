#![warn(unused_unsafe)]
//! Port of gettext.c -- Translation wrapper.
//!
//! Look up MSGID in the current default message catalog for the current
//! LC_MESSAGES locale. If not found, returns MSGID itself (the default text).

#![allow(non_snake_case)]

use std::os::raw::c_char;

use crate::intl::types;

/// Pass through the input message pointer. Runtime catalog lookup is disabled.
/// The returned pointer borrows the input; this function does not read either pointer.
///
/// Delegates to `libintl_dcgettext(NULL, msgid, LC_MESSAGES)`.
pub fn libintl_gettext(msgid: *const c_char) -> *mut c_char {
    crate::intl::dcgettext::libintl_dcgettext(std::ptr::null(), msgid, types::LC_MESSAGES)
}

/// Alias for `libintl_gettext` (unprefixed, for compatibility).
#[allow(unused_imports)]
pub use self::libintl_gettext as gettext;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_returns_msgid() {
        let _session = crate::sexp::session::RSession::new();

        let msgid = c"hello world".as_ptr();
        assert_eq!(libintl_gettext(msgid), msgid.cast_mut());
    }
}
