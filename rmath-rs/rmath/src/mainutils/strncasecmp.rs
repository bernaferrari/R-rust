#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/strncasecmp.c
//!
//! Case-insensitive string comparison (locale-specific case folding).

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

/// Case-insensitive comparison of at most `n` characters of `s1` and `s2`.
///
/// Returns:
/// - negative if `s1` < `s2` (case-insensitive)
/// - 0 if they are equal
/// - positive if `s1` > `s2`
///
/// This uses ASCII-only case folding, matching the original R implementation
/// which uses `isupper`/`tolower` from ctype.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncasecmp(s1: *const c_char, s2: *const c_char, n: usize) -> c_int {
    unsafe {
        if s1.is_null() || s2.is_null() {
            if s1.is_null() && s2.is_null() {
                return 0;
            }
            return if s1.is_null() { -1 } else { 1 };
        }

        for i in 0..n {
            let c1 = *s1.add(i);
            let c2 = *s2.add(i);

            let c1 = if (c1 as u8).is_ascii_uppercase() {
                (c1 as u8).to_ascii_lowercase() as c_char
            } else {
                c1
            };
            let c2 = if (c2 as u8).is_ascii_uppercase() {
                (c2 as u8).to_ascii_lowercase() as c_char
            } else {
                c2
            };

            if c1 == 0 {
                return if c2 == 0 { 0 } else { -1 };
            }
            if c2 == 0 {
                return 1;
            }
            if (c1 as u8) < (c2 as u8) {
                return -1;
            }
            if (c1 as u8) > (c2 as u8) {
                return 1;
            }
        }
        0
    }
}
