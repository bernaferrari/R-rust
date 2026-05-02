#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Rust-shaped approximate string matching helpers from R's `agrep.c`.
//!
//! GNU R routes approximate regex matching through TRE.  The fully vectorized
//! `.Internal(agrep)` entry point lives with the other grep primitives; this
//! helper covers the fixed-string C boundary with the same Levenshtein matcher
//! used by `grep.rs`.

use std::os::raw::{c_char, c_int};

/// Fixed-string approximate grep.
pub unsafe fn R_agrep(
    pattern: *const c_char,
    text: *const c_char,
    max_distance: c_int,
    ignore_case: c_int,
) -> c_int {
    unsafe { crate::mainutils::grep::R_agrep_fixed(pattern, text, max_distance, ignore_case) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_r_agrep_matches_within_distance() {
        unsafe {
            assert_eq!(R_agrep(c"kitten".as_ptr(), c"sitten".as_ptr(), 1, 0), 1);
            assert_eq!(R_agrep(c"kitten".as_ptr(), c"sitting".as_ptr(), 2, 0), 0);
            assert_eq!(R_agrep(c"kitten".as_ptr(), c"sitting".as_ptr(), 3, 0), 1);
        }
    }

    #[test]
    fn test_r_agrep_honors_case_policy() {
        unsafe {
            assert_eq!(R_agrep(c"Cat".as_ptr(), c"cat".as_ptr(), 0, 0), 0);
            assert_eq!(R_agrep(c"Cat".as_ptr(), c"cat".as_ptr(), 0, 1), 1);
        }
    }
}
