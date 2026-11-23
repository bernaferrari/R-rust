#![warn(unused_unsafe)]
//! Compatibility names for the owned domain binding functions.

#![allow(non_snake_case)]

#[allow(unused_imports)]
pub(crate) use super::bindtextdom::{
    bind_codeset as bind_textdomain_codeset, bind_domain as bindtextdomain,
};

#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use crate::sexp::session::RSession;

    use super::*;

    #[test]
    fn bindtextdomain_alias_returns_the_owned_directory() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let bound = bindtextdomain(Some(c"test"), Some(c"/usr/share/locale"));
            assert_eq!(bound, Some(CString::new("/usr/share/locale").unwrap()));
            assert_eq!(
                bindtextdomain(Some(c"test"), None),
                Some(CString::new("/usr/share/locale").unwrap())
            );
        });
    }

    #[test]
    fn bind_textdomain_codeset_alias_keeps_owned_snapshots() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            assert_eq!(bind_textdomain_codeset(Some(c"test"), None), None);
            let utf8 = bind_textdomain_codeset(Some(c"test"), Some(c"UTF-8"));
            assert_eq!(utf8, Some(CString::new("UTF-8").unwrap()));
            assert_eq!(
                bind_textdomain_codeset(Some(c"test"), Some(c"ASCII")),
                Some(CString::new("ASCII").unwrap())
            );
            // The earlier snapshot stays valid and unchanged by the replace.
            assert_eq!(utf8, Some(CString::new("UTF-8").unwrap()));
        });
    }
}
