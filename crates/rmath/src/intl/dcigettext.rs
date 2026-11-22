#![warn(unused_unsafe)]
//! Compatibility entry point for untranslated gettext messages.

use std::os::raw::{c_char, c_int, c_ulong};

/// Return the original message. Runtime catalog loading is not enabled.
///
/// # Safety
/// A non-null `msgid` must point to a NUL-terminated string for callers that
/// read the returned pointer. The pointer is borrowed from the caller.
pub unsafe fn libintl_dcigettext(
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
            // SAFETY: `id` is null or a NUL-terminated literal; the returned
            // pointer aliases the caller-provided input.
            assert_eq!(
                unsafe {
                    libintl_dcigettext(
                        c"missing".as_ptr(),
                        id,
                        c"files".as_ptr(),
                        2,
                        super::super::types::LC_MESSAGES,
                    )
                },
                id.cast_mut()
            );
        }
    }
}
