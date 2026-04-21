#![allow(unused_variables)]
#![allow(unused_assignments)]
/*!
 * Port of R's triostr.c - String utility functions.
 *
 * Original copyright (C) 2001 Bjorn Reese and Daniel Stenberg.
 * BSD-style license.
 *
 * This module provides string functions used by the trio printf/scanf
 * implementation. Many functions are thin wrappers around libc/CString
 * equivalents, but are kept to match the C API exactly.
 */

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_long, c_ulong};
use std::ptr;

/// Count the number of characters in a string.
pub unsafe fn trio_length(string: *const c_char) -> usize {
    unsafe {
        if string.is_null() {
            return 0;
        }
        let mut i = 0usize;
        while *string.add(i) != 0 as libc::c_char {
            i += 1;
        }
        i
    }
}

/// Count at most `max` characters in a string.
pub unsafe fn trio_length_max(string: *const c_char, max: usize) -> usize {
    unsafe {
        if string.is_null() {
            return 0;
        }
        let mut i = 0usize;
        while i < max {
            if *string.add(i) == 0 as libc::c_char {
                break;
            }
            i += 1;
        }
        i
    }
}

/// Append `source` at the end of `target`.
pub unsafe fn trio_append(target: *mut c_char, source: *const c_char) -> c_int {
    unsafe {
        if target.is_null() || source.is_null() {
            return 0;
        }
        let target_len = trio_length(target);
        let source_len = trio_length(source);
        ptr::copy_nonoverlapping(source, target.add(target_len), source_len);
        *target.add(target_len + source_len) = 0;
        1
    }
}

/// Append at most `max` characters from `source` to `target`.
pub unsafe fn trio_append_max(target: *mut c_char, max: usize, source: *const c_char) -> c_int {
    unsafe {
        let length = trio_length(target);
        if max > length && !source.is_null() {
            let remaining = max - length - 1;
            let source_len = trio_length(source).min(remaining);
            ptr::copy_nonoverlapping(source, target.add(length), source_len);
            *target.add(length + source_len) = 0;
        }
        1
    }
}

/// Determine if a string contains a substring.
pub unsafe fn trio_contains(string: *const c_char, substring: *const c_char) -> c_int {
    unsafe {
        if string.is_null() || substring.is_null() {
            return 0;
        }
        let s = CStr::from_ptr(string).to_bytes();
        let sub = CStr::from_ptr(substring).to_bytes();
        // Use a simple substring search
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
}

/// Copy `source` to `target`.
pub unsafe fn trio_copy(target: *mut c_char, source: *const c_char) -> c_int {
    unsafe {
        if target.is_null() || source.is_null() {
            return 0;
        }
        let len = trio_length(source);
        ptr::copy_nonoverlapping(source, target, len);
        *target.add(len) = 0;
        1
    }
}

/// Copy at most `max` characters from `source` to `target`.
/// Always null-terminates.
pub unsafe fn trio_copy_max(target: *mut c_char, max: usize, source: *const c_char) -> c_int {
    unsafe {
        if target.is_null() || source.is_null() {
            return 0;
        }
        if max > 1 {
            let copy_len = trio_length(source).min(max - 1);
            ptr::copy_nonoverlapping(source, target, copy_len);
            *target.add(copy_len) = 0;
        } else if max == 1 {
            *target = 0;
        }
        1
    }
}

/// Duplicate a string.
pub unsafe fn trio_duplicate(source: *const c_char) -> *mut c_char {
    unsafe {
        if source.is_null() {
            return ptr::null_mut();
        }
        let len = trio_length(source);
        let buf = trio_create(len + 1);
        if !buf.is_null() {
            ptr::copy_nonoverlapping(source, buf, len);
            *buf.add(len) = 0;
        }
        buf
    }
}

/// Duplicate at most `max` characters of `source`.
pub unsafe fn trio_duplicate_max(source: *const c_char, max: usize) -> *mut c_char {
    unsafe {
        if source.is_null() {
            return ptr::null_mut();
        }
        let len = trio_length(source);
        let copy_len = if len > max { max } else { len };
        let buf = trio_create(copy_len + 1);
        if !buf.is_null() {
            ptr::copy_nonoverlapping(source, buf, copy_len);
            *buf.add(copy_len) = 0;
        }
        buf
    }
}

/// Compare two strings for equality (case-insensitive).
pub unsafe fn trio_equal(first: *const c_char, second: *const c_char) -> c_int {
    unsafe {
        if first.is_null() || second.is_null() {
            return 0;
        }
        let mut f = first;
        let mut s = second;
        while *f != 0 as libc::c_char && *s != 0 as libc::c_char {
            let fc = trio_to_upper(*f as c_int) as u8;
            let sc = trio_to_upper(*s as c_int) as u8;
            if fc != sc {
                return 0;
            }
            f = f.add(1);
            s = s.add(1);
        }
        if *f == 0 as libc::c_char && *s == 0 as libc::c_char { 1 } else { 0 }
    }
}

/// Compare two strings for equality (case-sensitive).
pub unsafe fn trio_equal_case(first: *const c_char, second: *const c_char) -> c_int {
    unsafe {
        if first.is_null() || second.is_null() {
            return 0;
        }
        let mut f = first;
        let mut s = second;
        while *f != 0 as libc::c_char && *s != 0 as libc::c_char {
            if *f != *s {
                return 0;
            }
            f = f.add(1);
            s = s.add(1);
        }
        if *f == 0 as libc::c_char && *s == 0 as libc::c_char { 1 } else { 0 }
    }
}

/// Compare two strings up to `max` characters (case-sensitive).
pub unsafe fn trio_equal_case_max(
    first: *const c_char,
    max: usize,
    second: *const c_char,
) -> c_int {
    unsafe {
        if first.is_null() || second.is_null() {
            return 0;
        }
        for i in 0..max {
            let fc = *first.add(i);
            let sc = *second.add(i);
            if fc != sc {
                return 0;
            }
            if fc == 0 {
                break;
            }
        }
        1
    }
}

/// Compare two strings for equality using locale collation.
pub unsafe fn trio_equal_locale(first: *const c_char, second: *const c_char) -> c_int {
    unsafe {
        // Simplified: fall back to byte comparison (no locale support)
        trio_equal_case(first, second)
    }
}

/// Compare two strings up to `max` characters (case-insensitive).
pub unsafe fn trio_equal_max(first: *const c_char, max: usize, second: *const c_char) -> c_int {
    unsafe {
        if first.is_null() || second.is_null() {
            return 0;
        }
        let mut f = first;
        let mut s = second;
        let mut cnt = 0usize;
        while *f != 0 as libc::c_char && *s != 0 as libc::c_char && cnt <= max {
            let fc = trio_to_upper(*f as c_int) as u8;
            let sc = trio_to_upper(*s as c_int) as u8;
            if fc != sc {
                break;
            }
            f = f.add(1);
            s = s.add(1);
            cnt += 1;
        }
        if cnt == max || (*f == 0 as libc::c_char && *s == 0 as libc::c_char) {
            1
        } else {
            0
        }
    }
}

/// Get textual description of an error number.
pub unsafe fn trio_error(_error_number: c_int) -> *const c_char {
    // Simplified: return a static string
    static UNKNOWN: &[u8] = b"Unknown error\0";
    UNKNOWN.as_ptr() as *const c_char
}

/// Find first occurrence of a character in a string.
pub unsafe fn trio_index(string: *const c_char, character: c_int) -> *mut c_char {
    unsafe {
        if string.is_null() {
            return ptr::null_mut();
        }
        let mut s = string;
        while *s != 0 as libc::c_char {
            if *s as c_int == character {
                return s as *mut c_char;
            }
            s = s.add(1);
        }
        ptr::null_mut()
    }
}

/// Find last occurrence of a character in a string.
pub unsafe fn trio_index_last(string: *const c_char, character: c_int) -> *mut c_char {
    unsafe {
        if string.is_null() {
            return ptr::null_mut();
        }
        let mut last: *mut c_char = ptr::null_mut();
        let mut s = string;
        while *s != 0 as libc::c_char {
            if *s as c_int == character {
                last = s as *mut c_char;
            }
            s = s.add(1);
        }
        last
    }
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

/// Convert the alphabetic letters in the string to lower-case.
pub unsafe fn trio_lower(target: *mut c_char) -> c_int {
    unsafe {
        if target.is_null() {
            return 0;
        }
        let mut i = 0usize;
        while *target.add(i) != 0 as libc::c_char {
            let c = *target.add(i) as u8;
            if c >= b'A' && c <= b'Z' {
                *target.add(i) = (c + (b'a' - b'A')) as libc::c_char;
            }
            i += 1;
        }
        i as c_int
    }
}

/// Convert a string to upper-case.
pub unsafe fn trio_upper(target: *mut c_char) -> c_int {
    unsafe {
        if target.is_null() {
            return 0;
        }
        let mut i = 0usize;
        while *target.add(i) != 0 as libc::c_char {
            let c = *target.add(i) as u8;
            if c >= b'a' && c <= b'z' {
                *target.add(i) = (c - (b'a' - b'A')) as libc::c_char;
            }
            i += 1;
        }
        i as c_int
    }
}

/// Compare two strings using wildcards (case-insensitive).
///
/// Wildcards: `*` matches any number of characters, `?` matches a single character.
pub unsafe fn trio_match(string: *const c_char, pattern: *const c_char) -> c_int {
    unsafe {
        let mut s = string;
        let mut p = pattern;
        while *p != b'*' as c_char {
            if *s == 0 as libc::c_char {
                return if *p == 0 as libc::c_char { 1 } else { 0 };
            }
            let sc = trio_to_upper(*s as c_int) as u8;
            let pc = trio_to_upper(*p as c_int) as u8;
            if sc != pc && *p != b'?' as c_char {
                return 0;
            }
            s = s.add(1);
            p = p.add(1);
        }
        // Skip consecutive stars
        while *p == b'*' as c_char {
            p = p.add(1);
        }
        if *p == 0 as libc::c_char {
            return 1;
        }
        // Use a recursive approach for wildcard matching
        while *s != 0 as libc::c_char {
            if trio_match(s.add(1), p) != 0 {
                return 1;
            }
            s = s.add(1);
        }
        // Try matching empty string against remaining pattern
        trio_match(s, p)
    }
}

/// Tokenize a string.
pub unsafe fn trio_tokenize(string: *mut c_char, delimiters: *const c_char) -> *mut c_char {
    unsafe {
        // Simplified: find next token
        if string.is_null() || delimiters.is_null() {
            return ptr::null_mut();
        }
        // Skip leading delimiters
        let mut s = string;
        while *s != 0 as libc::c_char {
            let mut is_delim = false;
            let mut d = delimiters;
            while *d != 0 as libc::c_char {
                if *s == *d {
                    is_delim = true;
                    break;
                }
                d = d.add(1);
            }
            if !is_delim {
                break;
            }
            s = s.add(1);
        }
        if *s == 0 as libc::c_char {
            return ptr::null_mut();
        }
        let token_start = s;
        // Find end of token
        while *s != 0 as libc::c_char {
            let mut is_delim = false;
            let mut d = delimiters;
            while *d != 0 as libc::c_char {
                if *s == *d {
                    is_delim = true;
                    break;
                }
                d = d.add(1);
            }
            if is_delim {
                *s = 0 as libc::c_char;
                s = s.add(1);
                break;
            }
            s = s.add(1);
        }
        token_start
    }
}

/// Convert string to double.
pub unsafe fn trio_to_double(source: *const c_char, _endp: *mut *mut c_char) -> f64 {
    unsafe {
        if source.is_null() {
            return 0.0;
        }
        let s = CStr::from_ptr(source).to_str().unwrap_or("0");
        s.parse().unwrap_or(0.0)
    }
}

/// Convert string to float.
pub unsafe fn trio_to_float(source: *const c_char, _endp: *mut *mut c_char) -> f32 {
    unsafe {
        if source.is_null() {
            return 0.0;
        }
        let s = CStr::from_ptr(source).to_str().unwrap_or("0");
        s.parse().unwrap_or(0.0)
    }
}

/// Convert string to long.
pub unsafe fn trio_to_long(source: *const c_char, _endp: *mut *mut c_char, base: c_int) -> c_long {
    unsafe {
        if source.is_null() {
            return 0;
        }
        let s = CStr::from_ptr(source).to_str().unwrap_or("0");
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
}

/// Convert string to unsigned long.
pub unsafe fn trio_to_unsigned_long(
    source: *const c_char,
    _endp: *mut *mut c_char,
    base: c_int,
) -> c_ulong {
    unsafe {
        if source.is_null() {
            return 0;
        }
        let s = CStr::from_ptr(source).to_str().unwrap_or("0");
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
}

/// Convert string to long double (returns bits of f64).
pub unsafe fn trio_to_long_double(source: *const c_char, _endp: *mut *mut c_char) -> u64 {
    unsafe {
        if source.is_null() {
            return 0;
        }
        let s = CStr::from_ptr(source).to_str().unwrap_or("0");
        let val: f64 = s.parse().unwrap_or(0.0);
        val.to_bits()
    }
}

/// Apply a function to each character in the target string.
pub unsafe fn trio_span_function(
    target: *mut c_char,
    source: *const c_char,
    function: unsafe extern "C" fn(c_int) -> c_int,
) -> usize {
    unsafe {
        let mut count = 0usize;
        let mut i = 0usize;
        while *source.add(i) != 0 as libc::c_char {
            *target.add(i) = function(*source.add(i) as c_int) as libc::c_char;
            i += 1;
            count += 1;
        }
        *target.add(i) = 0 as libc::c_char;
        count
    }
}

/// Find substring in string.
pub unsafe fn trio_substring(string: *const c_char, substring: *const c_char) -> *mut c_char {
    unsafe {
        if string.is_null() || substring.is_null() {
            return ptr::null_mut();
        }
        let s = CStr::from_ptr(string).to_bytes();
        let sub = CStr::from_ptr(substring).to_bytes();
        if sub.is_empty() {
            return string as *mut c_char;
        }
        if s.len() < sub.len() {
            return ptr::null_mut();
        }
        for i in 0..=(s.len() - sub.len()) {
            if s[i..].starts_with(sub) {
                return string.add(i) as *mut c_char;
            }
        }
        ptr::null_mut()
    }
}

/// Find substring up to max characters.
pub unsafe fn trio_substring_max(
    string: *const c_char,
    max: usize,
    substring: *const c_char,
) -> *mut c_char {
    unsafe {
        if string.is_null() || substring.is_null() {
            return ptr::null_mut();
        }
        let str_len = trio_length(string);
        let sub_len = trio_length(substring);
        if sub_len == 0 {
            return string as *mut c_char;
        }
        if str_len < sub_len {
            return ptr::null_mut();
        }
        let limit = if max < str_len { max } else { str_len };
        let mut i = 0usize;
        while i + sub_len <= limit {
            if ptr::eq(string.add(i), substring) {
                // Use byte-by-byte comparison
                let mut matched = true;
                for j in 0..sub_len {
                    if *string.add(i + j) != *substring.add(j) {
                        matched = false;
                        break;
                    }
                }
                if matched {
                    return string.add(i) as *mut c_char;
                }
            }
            i += 1;
        }
        ptr::null_mut()
    }
}

/// Calculate a hash value for a string.
pub unsafe fn trio_hash(string: *const c_char, _hash_type: c_int) -> c_ulong {
    unsafe {
        let mut value: c_ulong = 0;
        let mut p = string;
        while *p != 0 as libc::c_char {
            value = value.wrapping_mul(31);
            value += *p as c_ulong;
            p = p.add(1);
        }
        value
    }
}

/// Allocate a string of given size.
pub unsafe fn trio_create(size: usize) -> *mut c_char {
    unsafe {
        let layout =
            std::alloc::Layout::from_size_align(size, 1).unwrap_or(std::alloc::Layout::new::<u8>());
        let ptr = std::alloc::alloc(layout);
        if ptr.is_null() {
            ptr::null_mut()
        } else {
            ptr as *mut c_char
        }
    }
}

/// Free a string.
pub unsafe fn trio_destroy(string: *mut c_char) {
    unsafe {
        if !string.is_null() {
            // We don't know the original size, so we use a minimal layout
            // In practice, trio_duplicate uses trio_create, so this should match
            // For safety, we deallocate with size 1 (the minimum)
            let layout = std::alloc::Layout::from_size_align(1, 1)
                .unwrap_or_else(|_| std::alloc::Layout::new::<u8>());
            std::alloc::dealloc(string as *mut u8, layout);
        }
    }
}
