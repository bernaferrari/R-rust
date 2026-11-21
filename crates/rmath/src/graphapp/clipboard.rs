#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

//! Clipboard functions for GraphApp.

use super::runtime::with_graphapp_runtime;
use super::types::*;
use std::os::raw::c_int;

fn set_clipboard_bytes(bytes: &[u8]) {
    with_graphapp_runtime(|runtime| {
        runtime.clipboard_text.clear();
        runtime.clipboard_text.extend_from_slice(bytes);
    });
}

pub fn copytoclipboard(_src: drawing) {}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn copystringtoclipboard(str_: *const std::os::raw::c_char) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let Some(s) = (unsafe { cstr(str_) }) else {
        set_clipboard_bytes(&[]);
        return 0;
    };
    let bytes = s.to_bytes();
    set_clipboard_bytes(bytes);
    bytes.len() as c_int
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getstringfromclipboard(str_: *mut std::os::raw::c_char, n: c_int) -> c_int {
    if str_.is_null() || n <= 0 {
        return 0;
    }

    with_graphapp_runtime(|runtime| {
        let count = runtime.clipboard_text.len().min((n - 1) as usize);
        // Write up to `n - 1` bytes plus NUL into the caller's `n`-byte
        // buffer.
        // SAFETY: forwarded raw pointers satisfy `slice_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(out) = unsafe { slice_mut(str_ as *mut u8, n as usize) } {
            out[..count].copy_from_slice(&runtime.clipboard_text[..count]);
            out[count] = 0;
        }
        count as c_int
    })
}

pub fn clipboardhastext() -> c_int {
    with_graphapp_runtime(|runtime| (!runtime.clipboard_text.is_empty()) as c_int)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};

    #[test]
    fn clipboard_roundtrips_text() {
        let _session = crate::sexp::session::RSession::new_for_gc_tests();
        unsafe {
            let text = CString::new("hello").unwrap();
            assert_eq!(copystringtoclipboard(text.as_ptr()), 5);
            assert_eq!(clipboardhastext(), 1);

            let mut buf = [0i8; 16];
            assert_eq!(
                getstringfromclipboard(buf.as_mut_ptr(), buf.len() as c_int),
                5
            );
            assert_eq!(CStr::from_ptr(buf.as_ptr()).to_str().unwrap(), "hello");
        }
    }

    #[test]
    fn clipboard_state_is_session_local() {
        use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};

        unsafe {
            let mut first = RInstance::new();
            set_current_instance(&mut first);
            let text = CString::new("first").unwrap();
            assert_eq!(copystringtoclipboard(text.as_ptr()), 5);
            assert_eq!(clipboardhastext(), 1);

            let mut second = RInstance::new();
            set_current_instance(&mut second);
            assert_eq!(clipboardhastext(), 0);

            set_current_instance(&mut first);
            let mut buf = [0i8; 16];
            assert_eq!(
                getstringfromclipboard(buf.as_mut_ptr(), buf.len() as c_int),
                5
            );
            assert_eq!(CStr::from_ptr(buf.as_ptr()).to_str().unwrap(), "first");
            clear_current_instance();
        }
    }
}
