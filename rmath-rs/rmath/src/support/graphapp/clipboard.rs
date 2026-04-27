#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Clipboard functions for GraphApp.

use super::types::*;
use super::runtime::with_graphapp_runtime;
use std::os::raw::c_int;
use std::ptr;

fn set_clipboard_bytes(bytes: &[u8]) {
    with_graphapp_runtime(|runtime| {
        runtime.clipboard_text.clear();
        runtime.clipboard_text.extend_from_slice(bytes);
    });
}

pub unsafe fn copytoclipboard(_src: drawing) {}

pub unsafe fn copystringtoclipboard(str_: *const std::os::raw::c_char) -> c_int {
    unsafe {
        if str_.is_null() {
            set_clipboard_bytes(&[]);
            return 0;
        }

        let mut len = 0usize;
        while *str_.add(len) != 0 {
            len += 1;
        }
        let bytes = std::slice::from_raw_parts(str_ as *const u8, len);
        set_clipboard_bytes(bytes);
        len as c_int
    }
}

pub unsafe fn getstringfromclipboard(str_: *mut std::os::raw::c_char, n: c_int) -> c_int {
    if str_.is_null() || n <= 0 {
        return 0;
    }

    with_graphapp_runtime(|runtime| {
        let count = runtime.clipboard_text.len().min((n - 1) as usize);
        if count > 0 {
            unsafe {
                ptr::copy_nonoverlapping(runtime.clipboard_text.as_ptr(), str_ as *mut u8, count);
            }
        }
        unsafe {
            *str_.add(count) = 0;
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
        unsafe {
            let text = CString::new("hello").unwrap();
            assert_eq!(copystringtoclipboard(text.as_ptr()), 5);
            assert_eq!(clipboardhastext(), 1);

            let mut buf = [0i8; 16];
            assert_eq!(getstringfromclipboard(buf.as_mut_ptr(), buf.len() as c_int), 5);
            assert_eq!(CStr::from_ptr(buf.as_ptr()).to_str().unwrap(), "hello");
        }
    }
}
