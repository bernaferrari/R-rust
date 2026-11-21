#![allow(unused_variables)]
#![allow(unused_assignments)]
/*!
 * Port of R's triostr.c - String utility functions.
 *
 * Original copyright (C) 2001 Bjorn Reese and Daniel Stenberg.
 * BSD-style license.
 *
 * This module provides string functions used by the trio printf/scanf
 * implementation. Functions take CStr/slice boundaries instead of raw
 * C-string pointers: every buffer's extent is explicit, so the bodies
 * are entirely safe Rust.
 */

use std::ffi::CStr;
use std::os::raw::{c_int, c_long, c_ulong};

/// Count the number of characters in a string.
pub fn trio_length(string: &CStr) -> usize {
    string.to_bytes().len()
}

/// Count at most `max` characters in a string.
pub fn trio_length_max(string: &CStr, max: usize) -> usize {
    string.to_bytes().len().min(max)
}

/// Append `source` at the end of `target` (buffer holds a C string).
/// Appends bytes then the NUL terminator; returns 1 (or 0 if the existing
/// buffer contains no NUL, i.e. no C string to append to).
pub fn trio_append(target: &mut Vec<u8>, source: &CStr) -> c_int {
    // Locate the NUL that terminates the current contents, if any.
    let end = match target.iter().position(|&b| b == 0) {
        Some(i) => i,
        None => return 0,
    };
    target.truncate(end);
    target.extend_from_slice(source.to_bytes_with_nul());
    1
}

/// Append at most `max` characters (incl. NUL budget) of `source` to `target`.
pub fn trio_append_max(target: &mut Vec<u8>, max: usize, source: &CStr) -> c_int {
    let end = match target.iter().position(|&b| b == 0) {
        Some(i) => i,
        None => target.len(),
    };
    if max > end {
        let remaining = max - end - 1;
        let bytes = source.to_bytes();
        let copy_len = bytes.len().min(remaining);
        target.truncate(end);
        target.extend_from_slice(&bytes[..copy_len]);
        target.push(0);
    }
    1
}

/// Determine if a string contains a substring.
pub fn trio_contains(string: &CStr, substring: &CStr) -> c_int {
    let s = string.to_bytes();
    let sub = substring.to_bytes();
    if sub.is_empty() {
        return 1;
    }
    if s.len() < sub.len() {
        return 0;
    }
    for i in 0..=(s.len() - sub.len()) {
        if s[i..].starts_with(sub) {
            return 1;
        }
    }
    0
}

/// Copy `source` into `target`, replacing its contents (NUL-terminated).
pub fn trio_copy(target: &mut Vec<u8>, source: &CStr) -> c_int {
    target.clear();
    target.extend_from_slice(source.to_bytes_with_nul());
    1
}

/// Copy at most `max` characters (incl. NUL) of `source` to `target`.
pub fn trio_copy_max(target: &mut Vec<u8>, max: usize, source: &CStr) -> c_int {
    target.clear();
    if max > 1 {
        let bytes = source.to_bytes();
        let copy_len = bytes.len().min(max - 1);
        target.extend_from_slice(&bytes[..copy_len]);
        target.push(0);
    } else if max == 1 {
        target.push(0);
    }
    1
}

/// Duplicate a string (returned buffer is NUL-terminated).
pub fn trio_duplicate(source: &CStr) -> Vec<u8> {
    source.to_bytes_with_nul().to_vec()
}

/// Duplicate at most `max` characters of `source` (NUL-terminated result).
pub fn trio_duplicate_max(source: &CStr, max: usize) -> Vec<u8> {
    let bytes = source.to_bytes();
    let copy_len = bytes.len().min(max);
    let mut buf = Vec::with_capacity(copy_len + 1);
    buf.extend_from_slice(&bytes[..copy_len]);
    buf.push(0);
    buf
}

/// Compare two strings for equality (case-insensitive).
pub fn trio_equal(first: &CStr, second: &CStr) -> c_int {
    if first.to_bytes().eq_ignore_ascii_case(second.to_bytes()) {
        1
    } else {
        0
    }
}

/// Compare two strings for equality (case-sensitive).
pub fn trio_equal_case(first: &CStr, second: &CStr) -> c_int {
    if first.to_bytes() == second.to_bytes() {
        1
    } else {
        0
    }
}

/// Compare two strings up to `max` characters (case-sensitive).
pub fn trio_equal_case_max(first: &CStr, max: usize, second: &CStr) -> c_int {
    let f = first.to_bytes();
    let s = second.to_bytes();
    for i in 0..max {
        let fc = f.get(i).copied().unwrap_or(0);
        let sc = s.get(i).copied().unwrap_or(0);
        if fc != sc {
            return 0;
        }
        if fc == 0 {
            break;
        }
    }
    1
}

/// Compare two strings for equality using locale collation.
pub fn trio_equal_locale(first: &CStr, second: &CStr) -> c_int {
    // Simplified: fall back to byte comparison (no locale support)
    trio_equal_case(first, second)
}

/// Compare two strings up to `max` characters (case-insensitive).
pub fn trio_equal_max(first: &CStr, max: usize, second: &CStr) -> c_int {
    let f = first.to_bytes();
    let s = second.to_bytes();
    for i in 0..=max {
        let fc = f.get(i).copied().unwrap_or(0);
        let sc = s.get(i).copied().unwrap_or(0);
        let fu = if fc.is_ascii_lowercase() {
            fc - (b'a' - b'A')
        } else {
            fc
        };
        let su = if sc.is_ascii_lowercase() {
            sc - (b'a' - b'A')
        } else {
            sc
        };
        if fu != su {
            return 0;
        }
        if fc == 0 {
            return 1;
        }
    }
    1
}

/// Get textual description of an error number.
pub fn trio_error(_error_number: c_int) -> &'static CStr {
    // Simplified: return a static string
    c"Unknown error"
}

/// Find first occurrence of a character in a string; returns byte offset.
pub fn trio_index(string: &CStr, character: c_int) -> Option<usize> {
    string
        .to_bytes()
        .iter()
        .position(|&b| b as c_int == character)
}

/// Find last occurrence of a character in a string; returns byte offset.
pub fn trio_index_last(string: &CStr, character: c_int) -> Option<usize> {
    string
        .to_bytes()
        .iter()
        .rposition(|&b| b as c_int == character)
}

/// Convert a character to upper case.
pub fn trio_to_upper(source: c_int) -> c_int {
    let c = source as u8;
    if c >= b'a' && c <= b'z' {
        (c - (b'a' - b'A')) as c_int
    } else {
        source
    }
}

/// Convert the alphabetic letters in the buffer to lower-case (up to NUL).
/// Returns the number of characters processed.
pub fn trio_lower(target: &mut [u8]) -> c_int {
    let mut i = 0usize;
    for b in target.iter_mut() {
        if *b == 0 {
            break;
        }
        if b.is_ascii_uppercase() {
            *b += b'a' - b'A';
        }
        i += 1;
    }
    i as c_int
}

/// Convert a string to upper-case (up to NUL). Returns count processed.
pub fn trio_upper(target: &mut [u8]) -> c_int {
    let mut i = 0usize;
    for b in target.iter_mut() {
        if *b == 0 {
            break;
        }
        if b.is_ascii_lowercase() {
            *b -= b'a' - b'A';
        }
        i += 1;
    }
    i as c_int
}

/// Compare two strings using wildcards (case-insensitive).
///
/// Wildcards: `*` matches any number of characters, `?` matches a single character.
pub fn trio_match(string: &CStr, pattern: &CStr) -> c_int {
    if match_impl(string.to_bytes(), pattern.to_bytes()) {
        1
    } else {
        0
    }
}

fn match_impl(s: &[u8], p: &[u8]) -> bool {
    let mut si = 0usize;
    let mut pi = 0usize;
    while pi < p.len() && p[pi] != b'*' {
        if si >= s.len() {
            return pi == p.len();
        }
        let sc = s[si].to_ascii_uppercase();
        let pc = p[pi].to_ascii_uppercase();
        if sc != pc && p[pi] != b'?' {
            return false;
        }
        si += 1;
        pi += 1;
    }
    // Skip consecutive stars
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    if pi >= p.len() {
        return true;
    }
    // Try matching rest of pattern against each suffix
    let mut start = si;
    while start < s.len() {
        if match_impl(&s[start..], &p[pi..]) {
            return true;
        }
        start += 1;
    }
    match_impl(&s[s.len()..], &p[pi..])
}

/// Tokenize a buffer holding a C string: splits `string` at the first
/// delimiter (writes NUL in place) and returns the token's byte offset.
pub fn trio_tokenize(string: &mut [u8], delimiters: &CStr) -> Option<usize> {
    let delims = delimiters.to_bytes();
    // Skip leading delimiters
    let mut i = 0usize;
    while i < string.len() && string[i] != 0 && delims.contains(&string[i]) {
        i += 1;
    }
    if i >= string.len() || string[i] == 0 {
        return None;
    }
    let token_start = i;
    // Find end of token
    while i < string.len() && string[i] != 0 {
        if delims.contains(&string[i]) {
            string[i] = 0;
            break;
        }
        i += 1;
    }
    Some(token_start)
}

/// Convert string to double.
pub fn trio_to_double(source: &CStr) -> f64 {
    source.to_str().unwrap_or("0").parse().unwrap_or(0.0)
}

/// Convert string to float.
pub fn trio_to_float(source: &CStr) -> f32 {
    source.to_str().unwrap_or("0").parse().unwrap_or(0.0)
}

/// Convert string to long.
pub fn trio_to_long(source: &CStr, base: c_int) -> c_long {
    let s = source.to_str().unwrap_or("0");
    match base {
        10 => s.parse().unwrap_or(0),
        16 => {
            let trimmed = s.trim_start_matches("0x").trim_start_matches("0X");
            u64::from_str_radix(trimmed, 16).unwrap_or(0) as c_long
        }
        8 => u64::from_str_radix(s, 8).unwrap_or(0) as c_long,
        _ => s.parse().unwrap_or(0),
    }
}

/// Convert string to unsigned long.
pub fn trio_to_unsigned_long(source: &CStr, base: c_int) -> c_ulong {
    let s = source.to_str().unwrap_or("0");
    match base {
        10 => s.parse().unwrap_or(0),
        16 => {
            let trimmed = s.trim_start_matches("0x").trim_start_matches("0X");
            u64::from_str_radix(trimmed, 16).unwrap_or(0) as c_ulong
        }
        8 => u64::from_str_radix(s, 8).unwrap_or(0) as c_ulong,
        _ => s.parse().unwrap_or(0),
    }
}

/// Convert string to long double (returns bits of f64).
pub fn trio_to_long_double(source: &CStr) -> u64 {
    let val: f64 = source.to_str().unwrap_or("0").parse().unwrap_or(0.0);
    val.to_bits()
}

/// Apply a function to each character (up to NUL), writing results into
/// `target`; NUL-terminates. Returns the number of characters processed.
pub fn trio_span_function(target: &mut [u8], source: &CStr, function: fn(c_int) -> c_int) -> usize {
    let bytes = source.to_bytes();
    let count = bytes.len().min(target.len().saturating_sub(1));
    for i in 0..count {
        target[i] = function(bytes[i] as c_int) as u8;
    }
    if count < target.len() {
        target[count] = 0;
    }
    count
}

/// Find substring in string; returns byte offset of first match.
pub fn trio_substring(string: &CStr, substring: &CStr) -> Option<usize> {
    let s = string.to_bytes();
    let sub = substring.to_bytes();
    if sub.is_empty() {
        return Some(0);
    }
    if s.len() < sub.len() {
        return None;
    }
    (0..=(s.len() - sub.len())).find(|&i| s[i..].starts_with(sub))
}

/// Find substring within the first `max` bytes; returns byte offset.
pub fn trio_substring_max(string: &CStr, max: usize, substring: &CStr) -> Option<usize> {
    let s = string.to_bytes();
    let sub = substring.to_bytes();
    if sub.is_empty() {
        return Some(0);
    }
    if s.len() < sub.len() {
        return None;
    }
    let limit = max.min(s.len());
    if limit < sub.len() {
        return None;
    }
    (0..=(limit - sub.len())).find(|&i| s[i..i + sub.len()] == *sub)
}

/// Calculate a hash value for a string.
pub fn trio_hash(string: &CStr, _hash_type: c_int) -> c_ulong {
    let mut value: c_ulong = 0;
    for &b in string.to_bytes() {
        value = value.wrapping_mul(31);
        value += b as c_ulong;
    }
    value
}

/// Allocate a zeroed buffer of given size.
pub fn trio_create(size: usize) -> Vec<u8> {
    vec![0u8; size]
}

/// Free a string allocated by trio_duplicate/trio_create.
pub fn trio_destroy(_buf: Vec<u8>) {}
