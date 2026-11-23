#![warn(unused_unsafe)]
//! Port of textdomain.c -- Set/get text domain.
//!
//! Set the current default message catalog to DOMAINNAME.
//! If DOMAINNAME is null, return the current default.
//! If DOMAINNAME is "", reset to the default of "messages".

#![allow(non_snake_case)]

use std::ffi::{CStr, CString};

use super::types::{DEFAULT_TEXT_DOMAIN, with_intl_runtime};

/// Query the default text domain, set it, or reset it with an empty name.
/// The returned value does not borrow session state.
pub fn textdomain(domain: Option<&CStr>) -> CString {
    with_intl_runtime(|state| {
        if let Some(domain) = domain {
            let domain = if domain.is_empty() {
                DEFAULT_TEXT_DOMAIN
            } else {
                domain
            };
            if state.current_default_domain.as_c_str() != domain {
                domain.clone_into(&mut state.current_default_domain);
            }
            state.msg_cat_cntr = state.msg_cat_cntr.wrapping_add(1);
        }
        state.current_default_domain.clone()
    })
}

#[cfg(test)]
mod tests {
    use crate::sexp::session::RSession;

    use super::*;

    #[test]
    fn textdomain_query_set_and_reset_update_the_catalog_counter() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let before = with_intl_runtime(|s| s.msg_cat_cntr);
            assert_eq!(textdomain(None).as_c_str(), c"messages");
            assert_eq!(with_intl_runtime(|s| s.msg_cat_cntr), before);
            assert_eq!(textdomain(Some(c"test-domain")).as_c_str(), c"test-domain");
            assert_eq!(textdomain(None).as_c_str(), c"test-domain");
            assert_eq!(textdomain(Some(c"")).as_c_str(), c"messages");
            assert_eq!(with_intl_runtime(|s| s.msg_cat_cntr), before + 2);
        });
    }

    #[test]
    fn textdomain_is_session_local_and_returns_an_owned_snapshot() {
        let first = RSession::new_for_gc_tests();
        let second = RSession::new_for_gc_tests();

        first.with_active(|| {
            textdomain(Some(c"first"));
        });
        second.with_active(|| {
            assert_eq!(textdomain(None).as_c_str(), c"messages");
        });

        let held = first.with_active(|| {
            let _ = textdomain(Some(c"first"));
            textdomain(None)
        });
        drop(first);
        assert_eq!(held.as_c_str(), c"first");
    }
}
