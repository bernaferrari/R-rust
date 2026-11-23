#![warn(unused_unsafe)]
//! Compatibility entry point for untranslated gettext messages.

use std::os::raw::{c_char, c_int, c_ulong};

/// Pass through the input message pointer. Runtime catalog lookup is disabled.
/// The returned pointer borrows the input; this function does not read either pointer.
pub fn libintl_dcigettext(
    _domainname: *const c_char,
    msgid: *const c_char,
    _msgid_plural: *const c_char,
    _n: c_ulong,
    _category: c_int,
) -> *mut c_char {
    msgid.cast_mut()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dcigettext_passthrough() {
        for id in [std::ptr::null(), c"".as_ptr(), c"file".as_ptr()] {
            assert_eq!(
                libintl_dcigettext(
                    c"missing".as_ptr(),
                    id,
                    c"files".as_ptr(),
                    2,
                    super::super::types::LC_MESSAGES,
                ),
                id.cast_mut()
            );
        }
    }
}
