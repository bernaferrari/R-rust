#![warn(unused_unsafe)]
//! Untranslated fallback for domain- and category-specific plural lookup.

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_ulong};

use crate::intl::types;

/// Return `msgid1` without reading the input pointers or consulting a catalog.
pub fn libintl_dcngettext(
    domainname: *const c_char,
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
    category: types::c_int,
) -> *mut c_char {
    crate::intl::dcigettext::libintl_dcigettext(domainname, msgid1, msgid2, n, category)
}

/// Alias for `libintl_dcngettext` (unprefixed, for compatibility).
#[allow(unused_imports)]
pub use self::libintl_dcngettext as dcngettext;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singular_returns_msgid1() {
        let _session = crate::sexp::session::RSession::new();

        let msgid1 = c"file".as_ptr();
        let msgid2 = c"files".as_ptr();
        assert_eq!(
            libintl_dcngettext(std::ptr::null(), msgid1, msgid2, 1, types::LC_MESSAGES),
            msgid1.cast_mut()
        );
    }

    #[test]
    fn test_plural_no_catalog_returns_msgid1() {
        let _session = crate::sexp::session::RSession::new();

        let msgid1 = c"file".as_ptr();
        let msgid2 = c"files".as_ptr();
        assert_eq!(
            libintl_dcngettext(std::ptr::null(), msgid1, msgid2, 5, types::LC_MESSAGES),
            msgid1.cast_mut()
        );
    }
}
