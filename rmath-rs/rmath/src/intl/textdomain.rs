//! Port of textdomain.c -- Set/get text domain.
//!
//! Set the current default message catalog to DOMAINNAME.
//! If DOMAINNAME is null, return the current default.
//! If DOMAINNAME is "", reset to the default of "messages".

#![allow(non_snake_case)]

use std::os::raw::c_char;
use std::ptr;

use crate::intl::types::{self, c_free, c_strdup};

/// Set the current default message catalog to DOMAINNAME.
///
/// - If DOMAINNAME is null, return the current default domain.
/// - If DOMAINNAME is "", reset to the default of "messages".
/// - Otherwise, set the domain to the given name (duplicated).
///
/// # Safety
/// `domainname` must be a valid pointer to a NUL-terminated C string, or NULL.
pub unsafe fn libintl_textdomain(domainname: *const c_char) -> *mut c_char {
    unsafe {
        // A NULL pointer requests the current setting.
        if domainname.is_null() {
            return types::with_intl_runtime(|intl| intl.current_default_domain) as *mut c_char;
        }

        let old_domain = types::with_intl_runtime(|intl| intl.current_default_domain);

        // If domain name is the null string, set to default domain "messages".
        if *domainname == 0 || ptr::eq(domainname, types::_nl_default_default_domain.as_ptr()) {
            types::with_intl_runtime(|intl| {
                intl.current_default_domain = types::_nl_default_default_domain.as_ptr();
            });
            let new_domain =
                types::with_intl_runtime(|intl| intl.current_default_domain) as *mut c_char;

            // Signal a change of the loaded catalogs.
            types::with_intl_runtime(|intl| intl.msg_cat_cntr += 1);

            return new_domain;
        }

        // Check if the new domain is the same as the old one.
        let old_domain_cstr = if old_domain.is_null() {
            None
        } else {
            Some(std::ffi::CStr::from_ptr(old_domain))
        };
        let new_domain_cstr = std::ffi::CStr::from_ptr(domainname);

        let is_same = match old_domain_cstr {
            Some(old) => old == new_domain_cstr,
            None => false,
        };

        let new_domain: *mut c_char;

        if is_same {
            // Same domain, no change needed, but still signal a change.
            new_domain = old_domain as *mut c_char;
        } else {
            // Duplicate the domain name.
            new_domain = c_strdup(domainname);

            if !new_domain.is_null() {
                types::with_intl_runtime(|intl| intl.current_default_domain = new_domain);
            }
        }

        // Signal a change of the loaded catalogs if the call was successful.
        if !new_domain.is_null() {
            types::with_intl_runtime(|intl| intl.msg_cat_cntr += 1);

            // Free old domain if it was dynamically allocated.
            if old_domain != new_domain as *const c_char
                && !old_domain.is_null()
                && !ptr::eq(old_domain, types::_nl_default_default_domain.as_ptr())
            {
                c_free(old_domain as *mut c_char);
            }
        }

        new_domain
    }
}

/// Alias for `libintl_textdomain` (unprefixed, for compatibility).
pub unsafe fn textdomain(domainname: *const c_char) -> *mut c_char {
    unsafe { libintl_textdomain(domainname) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_null_returns_current() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Reset to default first.
            let _ = libintl_textdomain(std::ptr::null());

            let result = libintl_textdomain(std::ptr::null());
            assert!(!result.is_null());
            let s = std::ffi::CStr::from_ptr(result);
            assert_eq!(s.to_str().unwrap_or(""), "messages");
        }
    }

    #[test]
    fn test_empty_string_resets_to_default() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Set a custom domain first.
            let custom = b"myapp\0" as *const u8 as *const c_char;
            let _ = libintl_textdomain(custom);

            // Reset with empty string.
            let empty = b"\0" as *const u8 as *const c_char;
            let result = libintl_textdomain(empty);
            assert!(!result.is_null());
            let s = std::ffi::CStr::from_ptr(result);
            assert_eq!(s.to_str().unwrap_or(""), "messages");
        }
    }

    #[test]
    fn test_set_custom_domain() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Reset to known state.
            let empty = b"\0" as *const u8 as *const c_char;
            let _ = libintl_textdomain(empty);

            let custom = b"testdomain\0" as *const u8 as *const c_char;
            let result = libintl_textdomain(custom);
            assert!(!result.is_null());
            let s = std::ffi::CStr::from_ptr(result);
            assert_eq!(s.to_str().unwrap_or(""), "testdomain");

            // Clean up: reset to default.
            let _ = libintl_textdomain(empty);
        }
    }

    #[test]
    fn test_counter_increments() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let empty = b"\0" as *const u8 as *const c_char;
            let _ = libintl_textdomain(empty);

            let before = types::with_intl_runtime(|intl| intl.msg_cat_cntr);
            let custom = b"counter_test\0" as *const u8 as *const c_char;
            let _ = libintl_textdomain(custom);
            assert!(types::with_intl_runtime(|intl| intl.msg_cat_cntr) > before);

            // Clean up.
            let _ = libintl_textdomain(empty);
        }
    }

    #[test]
    fn test_textdomain_state_is_session_local() {
        use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};

        unsafe {
            let mut first = RInstance::new();
            set_current_instance(&mut first);
            let custom = b"first_domain\0" as *const u8 as *const c_char;
            let first_result = libintl_textdomain(custom);
            assert_eq!(
                std::ffi::CStr::from_ptr(first_result)
                    .to_str()
                    .unwrap_or(""),
                "first_domain"
            );

            let mut second = RInstance::new();
            set_current_instance(&mut second);
            let second_result = libintl_textdomain(std::ptr::null());
            assert_eq!(
                std::ffi::CStr::from_ptr(second_result)
                    .to_str()
                    .unwrap_or(""),
                "messages"
            );

            set_current_instance(&mut first);
            let first_again = libintl_textdomain(std::ptr::null());
            assert_eq!(
                std::ffi::CStr::from_ptr(first_again).to_str().unwrap_or(""),
                "first_domain"
            );
            clear_current_instance();
        }
    }
}
