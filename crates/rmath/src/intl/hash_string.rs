#![warn(unused_unsafe)]
//! Port of hash-string.c -- String hashing function (PJW hash).
//!
//! Implements the so-called `hashpjw' function by P.J. Weinberger
//! [see Aho/Sethi/Ullman, COMPILERS: Principles, Techniques and Tools,
//! 1986, 1987 Bell Telephone Laboratories, Inc.].

#![allow(non_snake_case)]

use super::types::HASHWORDBITS;

/// Compute the hash value for the given string using the PJW hash algorithm.
///
/// This is the standard GNU gettext string hash function, used for
/// hash table lookups in message catalogs.
pub(crate) fn hash_string(bytes: &[u8]) -> u32 {
    let mut hval: u32 = 0;
    for &byte in bytes {
        hval = hval.wrapping_shl(4).wrapping_add(byte as u32);
        let g = hval & (0xfu32 << (HASHWORDBITS as u32 - 4));
        if g != 0 {
            hval ^= g >> (HASHWORDBITS as u32 - 8);
            hval ^= g;
        }
    }
    hval
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_fixture() {
        // PJW hash of "hello" (verified against the algorithm).
        assert_eq!(hash_string(b"hello"), 0x006e_c32f);
        assert_eq!(hash_string(b""), 0);
        assert_eq!(hash_string(b"a"), 97);
    }
}
