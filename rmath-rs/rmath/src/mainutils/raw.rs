// UTF-8 encoding/decoding functions ported from R source:
//   src/main/raw.c - mbrtoint, inttomb, utf8_table1, utf8_table2

#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

use std::os::raw::{c_char, c_int};

/// Lookup table: maximum codepoint value encodable in i bytes (i = 1..4).
/// Based on PCRE, but current Unicode only needs 4 bytes with maximum 0x10ffff.
static utf8_table1: [c_int; 4] = [0x7f, 0x7ff, 0xffff, 0x1fffff];
/// Lookup table: leading byte mask for i-byte sequences (i = 1..4).
static utf8_table2: [c_int; 4] = [0, 0xc0, 0xe0, 0xf0];

/// Simplified version for RFC 3629 definition of UTF-8.
///
/// Decodes one multi-byte UTF-8 character starting at `s` and writes
/// the resulting codepoint into `w`.
///
/// Returns the number of bytes consumed (1-4), 0 for a null terminator,
/// -1 for an invalid sequence, or -2 for an incomplete (truncated) sequence.
pub unsafe fn mbrtoint(w: *mut c_int, s: *const c_char) -> c_int {
    unsafe {
        let byte = *s as u8 as u32;

        if byte == 0 {
            *w = 0;
            return 0;
        } else if byte < 0xC0 {
            *w = byte as c_int;
            return 1;
        } else if byte < 0xE0 {
            if *s.add(1) == 0 {
                return -2;
            }
            if ((*s.add(1) as u8) & 0xC0) == 0x80 {
                *w = (((byte & 0x1F) << 6) | ((*s.add(1) as u8 as u32) & 0x3F)) as c_int;
                return 2;
            } else {
                return -1;
            }
        } else if byte < 0xF0 {
            if *s.add(1) == 0 || *s.add(2) == 0 {
                return -2;
            }
            if ((*s.add(1) as u8) & 0xC0) == 0x80 && ((*s.add(2) as u8) & 0xC0) == 0x80 {
                *w = (((byte & 0x0F) << 12)
                    | (((*s.add(1) as u8 as u32) & 0x3F) << 6)
                    | ((*s.add(2) as u8 as u32) & 0x3F)) as c_int;
                let b = *w as u32;
                if b >= 0xD800 && b <= 0xDFFF {
                    return -1; /* surrogate */
                }
                // Following Corrigendum 9, these are valid in UTF-8
                // if (b == 0xFFFE || b == 0xFFFF) return -1;
                return 3;
            } else {
                return -1;
            }
        } else if byte <= 0xF4 {
            // for RFC3629
            if *s.add(1) == 0 || *s.add(2) == 0 || *s.add(3) == 0 {
                return -2;
            }
            if ((*s.add(1) as u8) & 0xC0) == 0x80
                && ((*s.add(2) as u8) & 0xC0) == 0x80
                && ((*s.add(3) as u8) & 0xC0) == 0x80
            {
                *w = (((byte & 0x07) << 18)
                    | (((*s.add(1) as u8 as u32) & 0x3F) << 12)
                    | (((*s.add(2) as u8 as u32) & 0x3F) << 6)
                    | ((*s.add(3) as u8 as u32) & 0x3F)) as c_int;
                let b = *w as u32;
                if b <= 0x10FFFF {
                    return 4;
                } else {
                    return -1;
                }
            } else {
                return -1;
            }
        } else {
            return -1;
        }
    }
}

/// Encodes a single codepoint `wc` as UTF-8 into the buffer pointed to by `s`.
///
/// If `s` is null, no bytes are written (but the length is still computed).
///
/// Returns the number of bytes written (1-4), or 0 for a null codepoint.
pub unsafe fn inttomb(s: *mut c_char, wc: c_int) -> usize {
    unsafe {
        let mut cvalue: u32 = wc as u32;
        let mut buf: [c_char; 10] = [0; 10];
        let b = if !s.is_null() { s } else { buf.as_mut_ptr() };

        if cvalue == 0 {
            *b = 0;
            return 0;
        }

        let mut i: usize = 0;
        while i < utf8_table1.len() && cvalue > utf8_table1[i] as u32 {
            i += 1;
        }

        let mut j = i as isize;
        let mut bp = b.add(i);
        while j > 0 {
            j -= 1;
            *bp = (0x80 | (cvalue & 0x3F)) as c_char;
            bp = bp.sub(1);
            cvalue >>= 6;
        }
        *bp = (utf8_table2[i] as u32 | cvalue) as c_char;
        i + 1
    }
}
