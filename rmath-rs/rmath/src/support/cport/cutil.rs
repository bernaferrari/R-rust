//! Port of standalone C utility functions from R's src/main/
//!
//! - strdup.c: return a newly allocated copy of a string (FSF, LGPL)
//! - strncasecmp.c: locale-specific case-insensitive string comparison

/// Return a newly allocated copy of a string, or null if out of memory.
///
/// Ported from R's src/main/strdup.c (Copyright 1990 Free Software Foundation)
pub unsafe fn R_strdup(s: *const i8) -> *mut i8 {
    unsafe {
        if s.is_null() {
            return std::ptr::null_mut();
        }
        let len = std::ffi::CStr::from_ptr(s).to_bytes().len();
        let layout = std::alloc::Layout::from_size_align(len + 1, 1).expect("unwrap on None/Err");
        let newstr = std::alloc::alloc(layout);
        if newstr.is_null() {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(s as *const u8, newstr, len);
        *newstr.add(len) = 0;
        newstr as *mut i8
    }
}

/// Locale-specific case-insensitive string comparison.
///
/// Ported from R's src/main/strncasecmp.c
pub unsafe fn R_strncasecmp(s1: *const i8, s2: *const i8, n: usize) -> i32 {
    unsafe {
        let s1 = std::ffi::CStr::from_ptr(s1).to_bytes();
        let s2 = std::ffi::CStr::from_ptr(s2).to_bytes();
        let n = n.min(s1.len()).min(s2.len());

        for i in 0..n {
            let c1 = s1[i] as char;
            let c2 = s2[i] as char;
            let c1 = if c1.is_ascii_uppercase() {
                c1.to_ascii_lowercase()
            } else {
                c1
            };
            let c2 = if c2.is_ascii_uppercase() {
                c2.to_ascii_lowercase()
            } else {
                c2
            };
            if c1 == '\0' {
                return if c2 == '\0' { 0 } else { -1 };
            }
            if c2 == '\0' {
                return 1;
            }
            if c1 < c2 {
                return -1;
            }
            if c1 > c2 {
                return 1;
            }
        }
        0
    }
}
