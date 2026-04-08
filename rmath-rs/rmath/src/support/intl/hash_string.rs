//! Port of hash-string.c -- String hashing function (PJW hash).
//!
//! Implements the so-called `hashpjw' function by P.J. Weinberger
//! [see Aho/Sethi/Ullman, COMPILERS: Principles, Techniques and Tools,
//! 1986, 1987 Bell Telephone Laboratories, Inc.].

#![allow(non_snake_case)]

use std::os::raw::c_char;

use crate::support::intl::types::HASHWORDBITS;

/// Compute the hash value for the given string using the PJW hash algorithm.
///
/// This is the standard GNU gettext string hash function, used for
/// hash table lookups in message catalogs.
///
/// # Safety
/// `str_param` must be a valid pointer to a NUL-terminated C string.
pub unsafe fn __hash_string(str_param: *const c_char) -> u32 {
    unsafe {
        let mut hval: u32 = 0;
        let mut g: u32;
        let mut str_ptr = str_param;

        while *str_ptr != 0 {
            hval = hval.wrapping_shl(4);
            hval = hval.wrapping_add(*str_ptr as u8 as u32);
            g = hval & (0xfu32 << (HASHWORDBITS as u32 - 4));
            if g != 0 {
                hval ^= g >> (HASHWORDBITS as u32 - 8);
                hval ^= g;
            }
            str_ptr = str_ptr.add(1);
        }

        hval
    }
}

/// Alias for `__hash_string` (non-prefixed version).
pub unsafe fn hash_string(str_param: *const c_char) -> u32 {
    unsafe { __hash_string(str_param) }
}

/// Alias for `__hash_string` (libintl-prefixed version).
pub unsafe fn libintl_hash_string(str_param: *const c_char) -> u32 {
    unsafe { __hash_string(str_param) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_string() {
        unsafe {
            let s = b"\0" as *const u8 as *const c_char;
            assert_eq!(__hash_string(s), 0);
        }
    }

    #[test]
    fn test_single_char() {
        unsafe {
            let s = b"a\0" as *const u8 as *const c_char;
            // hval = 0 << 4 + 'a' = 97, no g overflow
            assert_eq!(__hash_string(s), 97);
        }
    }

    #[test]
    fn test_deterministic() {
        unsafe {
            let s = b"hello\0" as *const u8 as *const c_char;
            let h1 = __hash_string(s);
            let h2 = __hash_string(s);
            assert_eq!(h1, h2);
        }
    }

    #[test]
    fn test_different_strings_different_hashes() {
        unsafe {
            let s1 = b"hello\0" as *const u8 as *const c_char;
            let s2 = b"world\0" as *const u8 as *const c_char;
            assert_ne!(__hash_string(s1), __hash_string(s2));
        }
    }
}
