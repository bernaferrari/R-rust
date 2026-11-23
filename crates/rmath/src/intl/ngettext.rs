#![warn(unused_unsafe)]
//! Untranslated fallback for plural lookup in the default domain.

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_ulong};

use crate::intl::types;

/// Return `msgid1` without reading the input pointers or consulting a catalog.
pub fn libintl_ngettext(
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
) -> *mut c_char {
    crate::intl::dcngettext::libintl_dcngettext(
        std::ptr::null(),
        msgid1,
        msgid2,
        n,
        types::LC_MESSAGES,
    )
}

/// Alias for `libintl_ngettext` (unprefixed, for compatibility).
#[allow(unused_imports)]
pub use self::libintl_ngettext as ngettext;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singular_form() {
        let _session = crate::sexp::session::RSession::new();

        let s1 = c"cat".as_ptr();
        let s2 = c"cats".as_ptr();
        assert_eq!(libintl_ngettext(s1, s2, 1), s1.cast_mut());
    }

    #[test]
    fn test_plural_no_catalog() {
        let _session = crate::sexp::session::RSession::new();

        let s1 = c"cat".as_ptr();
        let s2 = c"cats".as_ptr();
        assert_eq!(libintl_ngettext(s1, s2, 5), s1.cast_mut());
    }
}
