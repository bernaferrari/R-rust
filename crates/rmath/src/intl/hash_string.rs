#![warn(unused_unsafe)]
//! Port of hash-string.c -- String hashing function (PJW hash).
//!
//! Implements the so-called `hashpjw' function by P.J. Weinberger
//! [see Aho/Sethi/Ullman, COMPILERS: Principles, Techniques and Tools,
//! 1986, 1987 Bell Telephone Laboratories, Inc.].

#![allow(non_snake_case)]

use std::os::raw::c_char;

use crate::intl::types::{HASHWORDBITS, cstr};

/// Compute the hash value for the given string using the PJW hash algorithm.
///
/// This is the standard GNU gettext string hash function, used for
/// hash table lookups in message catalogs.
///
/// # Safety
/// `str_param` must be a valid pointer to a NUL-terminated C string.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn __hash_string(str_param: *const c_char) -> u32 {
    let mut hval: u32 = 0;
    let mut g: u32;

    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    if let Some(s) = unsafe { cstr(str_param) } {
        for &byte in s.to_bytes() {
            hval = hval.wrapping_shl(4);
            hval = hval.wrapping_add(byte as u32);
            g = hval & (0xfu32 << (HASHWORDBITS as u32 - 4));
            if g != 0 {
                hval ^= g >> (HASHWORDBITS as u32 - 8);
                hval ^= g;
            }
        }
    }
    hval
}

/// Alias for `__hash_string` (non-prefixed version).
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn hash_string(str_param: *const c_char) -> u32 {
    // SAFETY: forwarded raw pointers satisfy `__hash_string`'s contract per this function's own `# Safety` requirements.
    unsafe { __hash_string(str_param) }
}

/// Alias for `__hash_string` (libintl-prefixed version).
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn libintl_hash_string(str_param: *const c_char) -> u32 {
    // SAFETY: forwarded raw pointers satisfy `__hash_string`'s contract per this function's own `# Safety` requirements.
    unsafe { __hash_string(str_param) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_string() {
        let s = b"\0" as *const u8 as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert_eq!(unsafe { __hash_string(s) }, 0);
    }

    #[test]
    fn test_single_char() {
        let s = b"a\0" as *const u8 as *const c_char;
        // hval = 0 << 4 + 'a' = 97, no g overflow
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert_eq!(unsafe { __hash_string(s) }, 97);
    }

    #[test]
    fn test_deterministic() {
        let s = b"hello\0" as *const u8 as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let h1 = unsafe { __hash_string(s) };
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let h2 = unsafe { __hash_string(s) };
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_different_strings_different_hashes() {
        let s1 = b"hello\0" as *const u8 as *const c_char;
        let s2 = b"world\0" as *const u8 as *const c_char;
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        assert_ne!(unsafe { __hash_string(s1) }, unsafe { __hash_string(s2) });
    }
}
