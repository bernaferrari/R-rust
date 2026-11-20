#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

//! String utility functions for GraphApp.
//!
//! Ported from strings.c - provides safe string manipulation
//! with null-safety guarantees.

use std::cmp::Ordering;
use std::os::raw::{c_char, c_int, c_long};
use std::ptr;

use super::memory;
use super::types::cstr;

/// Create and return a newly allocated copy of a string.
/// Null strings are converted into empty strings.
pub fn new_string(src: *const c_char) -> *mut c_char {
    match cstr(src) {
        Some(s) => new_string_from_bytes(s.to_bytes()),
        None => {
            // Allocate empty string
            let p = memory::memalloc(1);
            if !p.is_null() {
                // Terminate the fresh 1-byte allocation.
                unsafe { *p = 0 };
            }
            p as *mut c_char
        }
    }
}

/// Delete a previously allocated string.
pub fn del_string(str: *const c_char) {
    if !str.is_null() {
        memory::memfree(str as *mut u8);
    }
}

/// String length. Returns 0 for null strings.
pub fn string_length(s: *const c_char) -> c_long {
    cstr(s).map_or(0, |s| s.to_bytes().len() as c_long)
}

/// Copy a string. Avoids doing anything to null strings.
pub fn copy_string(dest: *mut c_char, src: *const c_char) {
    if dest.is_null() || src.is_null() {
        return;
    }
    // Copy `src` byte by byte up to and including its NUL; callers
    // guarantee `dest` has room for the whole string.
    unsafe {
        let mut i: c_long = 0;
        while *src.add(i as usize) != 0 {
            *dest.add(i as usize) = *src.add(i as usize);
            i += 1;
        }
        *dest.add(i as usize) = 0;
    }
}

/// String comparison. Null == null, null == "", "" == null.
pub fn compare_strings(s1: *const c_char, s2: *const c_char) -> c_int {
    if s1 == s2 {
        return 0;
    }
    match (cstr(s1), cstr(s2)) {
        (None, None) => 0,
        (None, Some(b)) => {
            if b.to_bytes().is_empty() {
                0
            } else {
                -1
            }
        }
        (Some(a), None) => {
            if a.to_bytes().is_empty() {
                0
            } else {
                1
            }
        }
        (Some(a), Some(b)) => {
            let a = a.to_bytes();
            let b = b.to_bytes();
            for (&x, &y) in a.iter().zip(b) {
                // Reinterpret as c_char to preserve the original signed
                // comparison semantics.
                let diff = (x as c_char) as c_int - (y as c_char) as c_int;
                if diff != 0 {
                    return diff;
                }
            }
            // Equal up to the shorter length; compare the longer
            // string's next byte against the shorter one's NUL.
            match a.len().cmp(&b.len()) {
                Ordering::Greater => (a[b.len()] as c_char) as c_int,
                Ordering::Less => -((b[a.len()] as c_char) as c_int),
                Ordering::Equal => 0,
            }
        }
    }
}

fn new_string_from_bytes(bytes: &[u8]) -> *mut c_char {
    let len = bytes.len().min(c_long::MAX as usize);
    let out = memory::memalloc(len as c_long + 1) as *mut c_char;
    if out.is_null() {
        return ptr::null_mut();
    }
    // Fill the fresh `len + 1`-byte allocation, NUL included.
    unsafe {
        for (idx, byte) in bytes.iter().take(len).enumerate() {
            *out.add(idx) = *byte as c_char;
        }
        *out.add(len) = 0;
    }
    out
}

/// Append two strings. The returned string is newly allocated and caller-owned.
pub fn add_strings(s1: *const c_char, s2: *const c_char) -> *mut c_char {
    if s1.is_null() {
        return new_string(s2);
    }
    if s2.is_null() {
        return new_string(s1);
    }

    let len1 = string_length(s1);
    let len2 = string_length(s2);
    let buf = memory::memalloc(len1 + len2 + 1) as *mut c_char;
    if !buf.is_null() {
        copy_string(buf, s1);
        // Move `len1` bytes into the fresh `len1 + len2 + 1`-byte buffer.
        copy_string(unsafe { buf.add(len1 as usize) }, s2);
    }
    buf
}

/// Convert a char to a newly allocated string.
pub fn char_to_string(ch: c_char) -> *mut c_char {
    let out = memory::memalloc(2) as *mut c_char;
    if !out.is_null() {
        // Fill the fresh 2-byte allocation.
        unsafe {
            *out = ch;
            *out.add(1) = 0;
        }
    }
    out
}

/// Convert an integer to a newly allocated string.
pub fn int_to_string(i: c_long) -> *mut c_char {
    let str_val = format!("{}", i);
    new_string_from_bytes(str_val.as_bytes())
}

/// Convert a float to a newly allocated string.
pub fn float_to_string(f: f32) -> *mut c_char {
    let str_val = format!("{}", f);
    new_string_from_bytes(str_val.as_bytes())
}

/// Case-insensitive string comparison.
pub fn string_diff(s: *const c_char, t: *const c_char) -> c_int {
    let a = cstr(s).map_or(&[][..], |s| s.to_bytes());
    let b = cstr(t).map_or(&[][..], |s| s.to_bytes());
    let mut diff: c_int = 0;
    let mut i = 0usize;

    while diff == 0 && (i < a.len() || i < b.len()) {
        // Reinterpret as c_char to preserve the original signed
        // comparison semantics; positions past the NUL read as 0.
        let mut ch1 = a.get(i).copied().unwrap_or(0) as c_char as c_int;
        let mut ch2 = b.get(i).copied().unwrap_or(0) as c_char as c_int;

        if ch1 >= ('A' as c_int) && ch1 <= ('Z' as c_int) {
            ch1 = ch1 - 'A' as c_int + 'a' as c_int;
        }
        if ch2 >= ('A' as c_int) && ch2 <= ('Z' as c_int) {
            ch2 = ch2 - 'A' as c_int + 'a' as c_int;
        }
        diff = ch1 - ch2;
        i += 1;
    }
    diff
}

/// Convert \n to \r\n. The returned string must be freed.
pub fn to_dos_string(text: *const c_char) -> *mut c_char {
    let Some(text) = cstr(text) else {
        return ptr::null_mut();
    };
    let bytes = text.to_bytes();

    // First pass: count length
    let mut length = bytes.len();
    let mut prev = 0u8;
    for &b in bytes {
        if b == b'\n' && prev != b'\r' {
            length += 1;
        }
        prev = b;
    }

    let newstr = memory::memalloc(length as c_long + 1) as *mut c_char;
    if newstr.is_null() {
        return ptr::null_mut();
    }

    // Write the converted bytes into the fresh `length + 1`-byte buffer.
    unsafe {
        prev = 0;
        let mut ss = newstr;
        for &b in bytes {
            if b == b'\n' && prev != b'\r' {
                *ss = b'\r' as c_char;
                ss = ss.add(1);
            }
            *ss = b as c_char;
            prev = b;
            ss = ss.add(1);
        }
        *ss = 0;
    }

    newstr
}

/// Strip carriage returns. The returned string must be freed.
pub fn to_c_string(text: *const c_char) -> *mut c_char {
    let Some(text) = cstr(text) else {
        return ptr::null_mut();
    };
    let bytes = text.to_bytes();

    // First pass: count length
    let mut length = bytes.len();
    for pair in bytes.windows(2) {
        if pair == b"\r\n" {
            length -= 1;
        }
    }

    let newstr = memory::memalloc(length as c_long + 1) as *mut c_char;
    if newstr.is_null() {
        return ptr::null_mut();
    }

    // Write the converted bytes into the fresh `length + 1`-byte buffer.
    unsafe {
        let mut ss = newstr;
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                i += 1; // skip CR
            }
            *ss = bytes[i] as c_char;
            ss = ss.add(1);
            i += 1;
        }
        *ss = 0;
    }

    newstr
}
