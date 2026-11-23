#![warn(unused_unsafe)]
//! Untranslated fallback for domain-specific plural lookup.

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_ulong};

use crate::intl::types;

/// Return `msgid1` without reading the input pointers or consulting a catalog.
pub fn libintl_dngettext(
    domainname: *const c_char,
    msgid1: *const c_char,
    msgid2: *const c_char,
    n: c_ulong,
) -> *mut c_char {
    crate::intl::dcngettext::libintl_dcngettext(
        domainname,
        msgid1,
        msgid2,
        n,
        types::LC_MESSAGES,
    )
}

/// Alias for `libintl_dngettext` (unprefixed, for compatibility).
#[allow(unused_imports)]
pub use self::libintl_dngettext as dngettext;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singular_with_domain() {
        let _session = crate::sexp::session::RSession::new();

        let domain = c"myapp".as_ptr();
        let s1 = c"file".as_ptr();
        let s2 = c"files".as_ptr();
        assert_eq!(libintl_dngettext(domain, s1, s2, 1), s1.cast_mut());
    }

    #[test]
    fn test_plural_no_catalog() {
        let _session = crate::sexp::session::RSession::new();

        let domain = c"myapp".as_ptr();
        let s1 = c"file".as_ptr();
        let s2 = c"files".as_ptr();
        assert_eq!(libintl_dngettext(domain, s1, s2, 10), s1.cast_mut());
    }
}
