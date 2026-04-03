#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! String utility functions for GraphApp.
//!
//! Ported from strings.c - provides safe string manipulation
//! with null-safety guarantees.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_long};
use std::ptr;

use super::memory;

/// Create and return a newly allocated copy of a string.
/// Null strings are converted into empty strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn new_string(src: *const c_char) -> *mut c_char {
    unsafe {
        if src.is_null() {
            // Allocate empty string
            let p = memory::memalloc(1);
            if !p.is_null() {
                *p = 0;
            }
            return p as *mut c_char;
        }
        let len = string_length(src);
        let str = memory::memalloc(len + 1) as *mut c_char;
        if !str.is_null() {
            copy_string(str, src);
        }
        str
    }
}

/// Delete a previously allocated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn del_string(str: *const c_char) {
    unsafe {
        if !str.is_null() {
            memory::memfree(str as *mut u8);
        }
    }
}

/// String length. Returns 0 for null strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn string_length(s: *const c_char) -> c_long {
    unsafe {
        if s.is_null() {
            return 0;
        }
        let mut len: c_long = 0;
        while *s.add(len as usize) != 0 {
            len += 1;
        }
        len
    }
}

/// Copy a string. Avoids doing anything to null strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn copy_string(dest: *mut c_char, src: *const c_char) {
    unsafe {
        if dest.is_null() || src.is_null() {
            return;
        }
        let mut i: c_long = 0;
        while *src.add(i as usize) != 0 {
            *dest.add(i as usize) = *src.add(i as usize);
            i += 1;
        }
        *dest.add(i as usize) = 0;
    }
}

/// String comparison. Null == null, null == "", "" == null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn compare_strings(s1: *const c_char, s2: *const c_char) -> c_int {
    unsafe {
        if s1 == s2 {
            return 0;
        } else if s1.is_null() {
            if *s2 == 0 { 0 } else { -1 }
        } else if s2.is_null() {
            if *s1 == 0 { 0 } else { 1 }
        } else {
            let mut i: c_long = 0;
            loop {
                let c1 = *s1.add(i as usize);
                let c2 = *s2.add(i as usize);
                let diff = c1 as c_int - c2 as c_int;
                if diff != 0 {
                    return diff;
                }
                if c1 == 0 {
                    break;
                }
                i += 1;
            }
            0
        }
    }
}

/// Append one string to another. Returns the result in a static buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn add_strings(s1: *const c_char, s2: *const c_char) -> *const c_char {
    unsafe {
        static mut BUFFER: *mut c_char = ptr::null_mut();

        if s1.is_null() {
            return s2;
        }
        if s2.is_null() {
            return s1;
        }

        let len1 = string_length(s1);
        let len2 = string_length(s2);

        let prev = BUFFER;
        BUFFER = memory::memalloc(len1 + len2 + 1) as *mut c_char;

        if !BUFFER.is_null() {
            copy_string(BUFFER, s1);
            copy_string(BUFFER.add(len1 as usize), s2);
        }

        if !prev.is_null() {
            memory::memfree(prev as *mut u8);
        }

        BUFFER as *const c_char
    }
}

/// Convert a char to a string, return in a static buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn char_to_string(ch: c_char) -> *mut c_char {
    unsafe {
        static mut STR: [c_char; 2] = [0; 2];
        STR[0] = ch;
        STR[1] = 0;
        std::ptr::addr_of_mut!(STR) as *mut c_char
    }
}

/// Convert an integer to a string, return in a static buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn int_to_string(i: c_long) -> *mut c_char {
    unsafe {
        static mut STR: [c_char; 40] = [0; 40];
        let s = format!("{}", i);
        let bytes = s.as_bytes();
        let len = bytes.len().min(39);
        for j in 0..len {
            STR[j] = bytes[j] as c_char;
        }
        STR[len] = 0;
        std::ptr::addr_of_mut!(STR) as *mut c_char
    }
}

/// Convert a float to a string, return in a static buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn float_to_string(f: f32) -> *mut c_char {
    unsafe {
        static mut STR: [c_char; 40] = [0; 40];
        let s = format!("{}", f);
        let bytes = s.as_bytes();
        let len = bytes.len().min(39);
        for j in 0..len {
            STR[j] = bytes[j] as c_char;
        }
        STR[len] = 0;
        std::ptr::addr_of_mut!(STR) as *mut c_char
    }
}

/// Case-insensitive string comparison.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn string_diff(s: *const c_char, t: *const c_char) -> c_int {
    unsafe {
        let mut diff: c_int = 0;
        let mut si: isize = 0;
        let mut ti: isize = 0;

        while diff == 0 && (*s.add(si as usize) != 0 || *t.add(ti as usize) != 0) {
            let mut ch1 = *s.add(si as usize) as c_int;
            let mut ch2 = *t.add(ti as usize) as c_int;

            if ch1 >= ('A' as c_int) && ch1 <= ('Z' as c_int) {
                ch1 = ch1 - 'A' as c_int + 'a' as c_int;
            }
            if ch2 >= ('A' as c_int) && ch2 <= ('Z' as c_int) {
                ch2 = ch2 - 'A' as c_int + 'a' as c_int;
            }
            diff = ch1 - ch2;
            si += 1;
            ti += 1;
        }
        diff
    }
}

/// Convert \n to \r\n. The returned string must be freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn to_dos_string(text: *const c_char) -> *mut c_char {
    unsafe {
        if text.is_null() {
            return ptr::null_mut();
        }

        // First pass: count length
        let mut length: c_long = 0;
        let mut prev: c_char = 0;
        let mut s = text;
        while *s != 0 {
            length += 1;
            if *s == ('\n' as c_char) && prev != ('\r' as c_char) {
                length += 1;
            }
            prev = *s;
            s = s.add(1);
        }

        let newstr = memory::memalloc(length + 1) as *mut c_char;
        if newstr.is_null() {
            return ptr::null_mut();
        }

        prev = 0;
        let mut ss = newstr;
        let mut t = text;
        while *t != 0 {
            if *t == ('\n' as c_char) && prev != ('\r' as c_char) {
                *ss = '\r' as c_char;
                ss = ss.add(1);
            }
            *ss = *t;
            prev = *ss;
            ss = ss.add(1);
            t = t.add(1);
        }
        *ss = 0;

        newstr
    }
}

/// Strip carriage returns. The returned string must be freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn to_c_string(text: *const c_char) -> *mut c_char {
    unsafe {
        if text.is_null() {
            return ptr::null_mut();
        }

        // First pass: count length
        let mut length: c_long = 0;
        let mut s = text;
        while *s != 0 {
            length += 1;
            if *s == ('\r' as c_char) && *s.add(1) == ('\n' as c_char) {
                length -= 1;
            }
            s = s.add(1);
        }

        let newstr = memory::memalloc(length + 1) as *mut c_char;
        if newstr.is_null() {
            return ptr::null_mut();
        }

        let mut ss = newstr;
        let mut t = text;
        while *t != 0 {
            if *t == ('\r' as c_char) && *t.add(1) == ('\n' as c_char) {
                t = t.add(1); // skip CR
            }
            *ss = *t;
            ss = ss.add(1);
            t = t.add(1);
        }
        *ss = 0;

        newstr
    }
}
