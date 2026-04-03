#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/strdup.c
//!
//! Return a newly allocated copy of a string, or null if out of memory.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr;

/// Return a newly allocated copy of `str`, or a null pointer if out of memory.
///
/// This is the standard `strdup` implementation from R's src/main/strdup.c.
/// The caller is responsible for freeing the returned pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strdup(str: *const c_char) -> *mut c_char {
    unsafe {
        if str.is_null() {
            return ptr::null_mut();
        }
        let cstr = CStr::from_ptr(str);
        let bytes = cstr.to_bytes_with_nul();
        let len = bytes.len();

        let layout = std::alloc::Layout::from_size_align(len, 1).unwrap();
        let newstr = std::alloc::alloc(layout) as *mut c_char;
        if newstr.is_null() {
            return ptr::null_mut();
        }
        ptr::copy_nonoverlapping(str, newstr, len);
        newstr
    }
}
