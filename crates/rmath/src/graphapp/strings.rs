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
use super::types::{cstr, ptr_mut, slice_mut};

/// Create and return a newly allocated copy of a string.
/// Null strings are converted into empty strings.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn new_string(src: *const c_char) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    match unsafe { cstr(src) } {
        Some(s) => new_string_from_bytes(s.to_bytes()),
        None => {
            // Allocate empty string
            let p = memory::memalloc(1);
            // Terminate the fresh 1-byte allocation.
            // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
            if let Some(b) = unsafe { ptr_mut(p) } {
                *b = 0;
            }
            p as *mut c_char
        }
    }
}

/// Delete a previously allocated string.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn del_string(str: *const c_char) {
    if !str.is_null() {
        // SAFETY: forwarded raw pointers satisfy `memfree`'s contract per this function's own `# Safety` requirements.
        unsafe { memory::memfree(str as *mut u8) };
    }
}

/// String length. Returns 0 for null strings.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn string_length(s: *const c_char) -> c_long {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    (unsafe { cstr(s) }).map_or(0, |s| s.to_bytes().len() as c_long)
}

/// Copy a string. Avoids doing anything to null strings.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn copy_string(dest: *mut c_char, src: *const c_char) {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let Some(s) = (unsafe { cstr(src) }) else {
        return;
    };
    // Copy `src` up to and including its NUL; callers guarantee `dest`
    // has room for the whole string.
    let bytes = s.to_bytes_with_nul();
    // SAFETY: forwarded raw pointers satisfy `slice_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(d) = unsafe { slice_mut(dest as *mut u8, bytes.len()) } {
        d.copy_from_slice(bytes);
    }
}

/// String comparison. Null == null, null == "", "" == null.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn compare_strings(s1: *const c_char, s2: *const c_char) -> c_int {
    if s1 == s2 {
        return 0;
    }
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    match (unsafe { cstr(s1) }, unsafe { cstr(s2) }) {
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
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    if let Some(buf) = unsafe { slice_mut(out as *mut u8, len + 1) } {
        for (d, &b) in buf.iter_mut().zip(bytes.iter().take(len)) {
            *d = b;
        }
        buf[len] = 0;
    }
    out
}

/// Append two strings. The returned string is newly allocated and caller-owned.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn add_strings(s1: *const c_char, s2: *const c_char) -> *mut c_char {
    if s1.is_null() {
        // SAFETY: forwarded raw pointers satisfy `new_string`'s contract per this function's own `# Safety` requirements.
        return unsafe { new_string(s2) };
    }
    if s2.is_null() {
        // SAFETY: forwarded raw pointers satisfy `new_string`'s contract per this function's own `# Safety` requirements.
        return unsafe { new_string(s1) };
    }

    // SAFETY: forwarded raw pointers satisfy `string_length`'s contract per this function's own `# Safety` requirements.
    let len1 = unsafe { string_length(s1) };
    // SAFETY: forwarded raw pointers satisfy `string_length`'s contract per this function's own `# Safety` requirements.
    let len2 = unsafe { string_length(s2) };
    let buf = memory::memalloc(len1 + len2 + 1) as *mut c_char;
    if !buf.is_null() {
        // SAFETY: forwarded raw pointers satisfy `copy_string`'s contract per this function's own `# Safety` requirements.
        unsafe { copy_string(buf, s1) };
        // Move `len1` bytes into the fresh `len1 + len2 + 1`-byte buffer.
        // SAFETY: forwarded raw pointers satisfy `slice_mut`'s contract per this function's own `# Safety` requirements.
        let tail = unsafe { slice_mut(buf as *mut u8, (len1 + len2 + 1) as usize) }
            .and_then(|b| b.get_mut(len1 as usize..))
            .map_or(ptr::null_mut(), |t| t.as_mut_ptr() as *mut c_char);
        // SAFETY: forwarded raw pointers satisfy `copy_string`'s contract per this function's own `# Safety` requirements.
        unsafe { copy_string(tail, s2) };
    }
    buf
}

/// Convert a char to a newly allocated string.
pub fn char_to_string(ch: c_char) -> *mut c_char {
    let out = memory::memalloc(2) as *mut c_char;
    // Fill the fresh 2-byte allocation.
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    if let Some(b) = unsafe { slice_mut(out, 2) } {
        b[0] = ch;
        b[1] = 0;
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
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn string_diff(s: *const c_char, t: *const c_char) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let a = unsafe { cstr(s) }.map_or(&[][..], |s| s.to_bytes());
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let b = unsafe { cstr(t) }.map_or(&[][..], |s| s.to_bytes());
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
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn to_dos_string(text: *const c_char) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let Some(text) = (unsafe { cstr(text) }) else {
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
    // SAFETY: forwarded raw pointers satisfy `slice_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(out) = unsafe { slice_mut(newstr as *mut u8, length + 1) } {
        let mut w = 0usize;
        prev = 0;
        for &b in bytes {
            if b == b'\n' && prev != b'\r' {
                out[w] = b'\r';
                w += 1;
            }
            out[w] = b;
            prev = b;
            w += 1;
        }
        out[w] = 0;
    }

    newstr
}

/// Strip carriage returns. The returned string must be freed.
///
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn to_c_string(text: *const c_char) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let Some(text) = (unsafe { cstr(text) }) else {
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
    // SAFETY: forwarded raw pointers satisfy `slice_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(out) = unsafe { slice_mut(newstr as *mut u8, length + 1) } {
        let mut w = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                i += 1; // skip CR
            }
            out[w] = bytes[i];
            w += 1;
            i += 1;
        }
        out[w] = 0;
    }

    newstr
}
