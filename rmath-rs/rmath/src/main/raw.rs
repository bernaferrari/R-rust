#![allow(
    unsafe_op_in_unsafe_fn,
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types
)]

//! Port of R's src/main/raw.c
//!
//! The original C implementation provides:
//!   - do_charToRaw()   -- charToRaw()
//!   - do_rawToChar()   -- rawToChar()
//!   - do_rawShift()    -- rawShift()
//!   - do_rawToBits()   -- rawToBits()
//!   - do_intToBits()   -- intToBits()
//!   - do_numToInts()   -- numToInts()
//!   - do_numToBits()   -- numToBits()
//!   - do_packBits()    -- packBits()
//!   - mbrtoint()       -- UTF-8 decoder (single codepoint)
//!   - inttomb()        -- UTF-8 encoder (single codepoint)
//!   - do_utf8ToInt()   -- utf8ToInt()
//!   - do_intToUtf8()   -- intToUtf8()
//!
//! Ported from r-source/src/main/raw.c

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::main::coerce::coerceVector;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::SEXP;
use crate::sexp::ffi::*;
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::*;

// ---------------------------------------------------------------------------
// Local helpers for type checking
// ---------------------------------------------------------------------------

/// Check if x is a character (STRSXP) vector.
unsafe fn isString(x: SEXP) -> bool {
    !x.is_null() && TYPEOF(x) == SEXPTYPE::STRSXP.0
}

/// Check if x is an integer (INTSXP) vector.
unsafe fn isInteger(x: SEXP) -> bool {
    !x.is_null() && TYPEOF(x) == SEXPTYPE::INTSXP.0
}

// ---------------------------------------------------------------------------
// UTF-8 tables (from PCRE)
// ---------------------------------------------------------------------------

/// Lookup table: maximum codepoint value encodable in i bytes (i = 1..4).
/// Based on PCRE, but current Unicode only needs 4 bytes with maximum 0x10ffff.
static utf8_table1: [c_int; 4] = [0x7f, 0x7ff, 0xffff, 0x1fffff];

/// Lookup table: leading byte mask for i-byte sequences (i = 1..4).
static utf8_table2: [c_int; 4] = [0, 0xc0, 0xe0, 0xf0];

// ---------------------------------------------------------------------------
// SEXPTYPE constants now imported from crate::sexp::ffi::SEXPTYPE
// ---------------------------------------------------------------------------

const CE_NATIVE: c_int = 0;
const CE_UTF8: c_int = 1;

// ---------------------------------------------------------------------------
// Local helpers for encoding-aware character creation
// ---------------------------------------------------------------------------

/// Create an R character scalar with the given encoding.
/// Stub: ignores encoding and calls Rf_mkChar().
unsafe fn mkCharCE(s: *const c_char, _enc: c_int) -> SEXP {
    Rf_mkChar(s)
}

/// Create an R character scalar with the given length and encoding.
/// Stub: ignores encoding and calls Rf_mkCharLen().
unsafe fn mkCharLenCE(s: *const c_char, len: c_int, _enc: c_int) -> SEXP {
    Rf_mkCharLen(s, len)
}

// ---------------------------------------------------------------------------
// mbrtoint -- decode one UTF-8 character
// ---------------------------------------------------------------------------

/// Simplified version for RFC 3629 definition of UTF-8.
///
/// Decodes one multi-byte UTF-8 character starting at `s` and writes
/// the resulting codepoint into `w`.
///
/// Returns the number of bytes consumed (1-4), 0 for a null terminator,
/// -1 for an invalid sequence, or -2 for an incomplete (truncated) sequence.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbrtoint(w: *mut c_int, s: *const c_char) -> c_int {
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

// ---------------------------------------------------------------------------
// inttomb -- encode one codepoint as UTF-8
// ---------------------------------------------------------------------------

/// Encodes a single codepoint `wc` as UTF-8 into the buffer pointed to by `s`.
///
/// If `s` is null, no bytes are written (but the length is still computed).
///
/// Returns the number of bytes written (1-4), or 0 for a null codepoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn inttomb(s: *mut c_char, wc: c_int) -> usize {
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
    let mut bp = b.offset(i as isize);
    while j > 0 {
        j -= 1;
        *bp = (0x80 | (cvalue & 0x3F)) as c_char;
        bp = bp.offset(-1);
        cvalue >>= 6;
    }
    *bp = (utf8_table2[i] as u32 | cvalue) as c_char;
    i + 1
}

// ---------------------------------------------------------------------------
// do_charToRaw -- charToRaw()
// ---------------------------------------------------------------------------

/// Convert a character string to a raw vector (byte level, ignores encoding).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_charToRaw(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let x = CAR(args);
    if !isString(x) || LENGTH(x) == 0 {
        // error: argument must be a character vector of length 1
        return R_NilValue();
    }
    if LENGTH(x) > 1 {
        // warning: argument should be a character vector of length 1
    }
    let nc = LENGTH(STRING_ELT(x, 0));
    let ans = Rf_allocVector(SEXPTYPE::RAWSXP.0, nc);
    if nc > 0 {
        ptr::copy_nonoverlapping(
            CHAR(STRING_ELT(x, 0)) as *const u8,
            RAW(ans) as *mut u8,
            nc as usize,
        );
    }
    ans
}

// ---------------------------------------------------------------------------
// do_rawToChar -- rawToChar()
// ---------------------------------------------------------------------------

/// Convert a raw vector to a character string.
/// If multiple=TRUE, returns a character vector with one element per byte.
/// Otherwise, returns a single string (stripping trailing NULs).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_rawToChar(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let x = CAR(args);
    if TYPEOF(x) != SEXPTYPE::RAWSXP.0 {
        return R_NilValue();
    }
    let multiple = crate::main::coerce::asLogical(CADR(args));
    let ans: SEXP;
    if multiple != 0 {
        let nc = XLENGTH(x);
        let mut buf = [0i8 as c_char; 2];
        ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, nc as c_int));
        let mut i: R_xlen_t = 0;
        while i < nc {
            buf[0] = *RAW(x).add(i as usize) as c_char;
            buf[1] = 0;
            SET_STRING_ELT(ans, i, Rf_mkChar(buf.as_ptr()));
            i += 1;
        }
        Rf_unprotect(1);
        return ans;
    } else {
        let nc = LENGTH(x);
        // Strip trailing NULs
        let mut j: c_int = -1;
        let mut i: c_int = 0;
        while i < nc {
            if *RAW(x).add(i as usize) != 0 {
                j = i;
            }
            i += 1;
        }
        let new_nc = j + 1;
        ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, 1));
        if new_nc > 0 {
            SET_STRING_ELT(
                ans,
                0,
                mkCharLenCE(RAW(x) as *const c_char, new_nc, CE_NATIVE),
            );
        } else {
            SET_STRING_ELT(ans, 0, Rf_mkChar(c"".as_ptr()));
        }
        Rf_unprotect(1);
        return ans;
    }
}

// ---------------------------------------------------------------------------
// do_rawShift -- rawShift()
// ---------------------------------------------------------------------------

/// Shift raw vector elements left or right by n bits.
/// Positive n shifts left, negative n shifts right.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_rawShift(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let x = CAR(args);
    let shift = crate::main::coerce::asInteger(CADR(args));

    if TYPEOF(x) != SEXPTYPE::RAWSXP.0 {
        return R_NilValue();
    }
    if shift == NA_INTEGER || shift < -8 || shift > 8 {
        return R_NilValue();
    }
    let ans = Rf_protect(crate::main::duplicate::Rf_duplicate(x));
    let n = XLENGTH(x);
    let mut i: R_xlen_t = 0;
    if shift > 0 {
        while i < n {
            *RAW(ans).add(i as usize) <<= shift;
            i += 1;
        }
    } else {
        let abs_shift = (-shift) as u8;
        while i < n {
            *RAW(ans).add(i as usize) >>= abs_shift;
            i += 1;
        }
    }
    Rf_unprotect(1);
    ans
}

// ---------------------------------------------------------------------------
// do_rawToBits -- rawToBits()
// ---------------------------------------------------------------------------

/// Expand each byte of a raw vector into 8 individual bits.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_rawToBits(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let x = CAR(args);
    if TYPEOF(x) != SEXPTYPE::RAWSXP.0 {
        return R_NilValue();
    }
    let n = XLENGTH(x);
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::RAWSXP.0, 8 * n as c_int));
    let mut j: R_xlen_t = 0;
    let mut i: R_xlen_t = 0;
    while i < n {
        let mut tmp: u32 = *RAW(x).add(i as usize) as u32;
        let mut k: c_int = 0;
        while k < 8 {
            *RAW(ans).add(j as usize) = (tmp & 0x1) as u8;
            tmp >>= 1;
            j += 1;
            k += 1;
        }
        i += 1;
    }
    Rf_unprotect(1);
    ans
}

// ---------------------------------------------------------------------------
// do_intToBits -- intToBits()
// ---------------------------------------------------------------------------

/// Expand each integer into 32 individual bits.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_intToBits(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let x = Rf_protect(coerceVector(CAR(args), SEXPTYPE::INTSXP.0));
    if !isInteger(x) {
        Rf_unprotect(1);
        return R_NilValue();
    }
    let n = XLENGTH(x);
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::RAWSXP.0, 32 * n as c_int));
    let mut j: R_xlen_t = 0;
    let mut i: R_xlen_t = 0;
    while i < n {
        let mut tmp: u32 = *INTEGER(x).add(i as usize) as u32;
        let mut k: c_int = 0;
        while k < 32 {
            *RAW(ans).add(j as usize) = (tmp & 0x1) as u8;
            tmp >>= 1;
            j += 1;
            k += 1;
        }
        i += 1;
    }
    Rf_unprotect(2);
    ans
}

// ---------------------------------------------------------------------------
// do_numToInts -- numToInts()
// ---------------------------------------------------------------------------

/// Split each double into two 32-bit integers.
/// Returns an integer vector of length 2 * length(x).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_numToInts(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let x = Rf_protect(coerceVector(CAR(args), SEXPTYPE::REALSXP.0));
    let n = XLENGTH(x);
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 2 * n as c_int));
    let mut j: R_xlen_t = 0;
    let mut i: R_xlen_t = 0;
    while i < n {
        let d = *REAL(x).add(i as usize);
        // Reinterpret double as two 32-bit integers via union
        let bytes = d.to_bits().to_ne_bytes();
        // Low word and high word depend on endianness
        #[cfg(target_endian = "little")]
        {
            let lo = u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let hi = u32::from_ne_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
            *INTEGER(ans).add(j as usize) = lo as c_int;
            *INTEGER(ans).add((j + 1) as usize) = hi as c_int;
        }
        #[cfg(target_endian = "big")]
        {
            let hi = u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let lo = u32::from_ne_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
            *INTEGER(ans).add(j as usize) = lo as c_int;
            *INTEGER(ans).add((j + 1) as usize) = hi as c_int;
        }
        j += 2;
        i += 1;
    }
    Rf_unprotect(2);
    ans
}

// ---------------------------------------------------------------------------
// do_numToBits -- numToBits()
// ---------------------------------------------------------------------------

/// Split each double into 64 individual bits.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_numToBits(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let x = Rf_protect(coerceVector(CAR(args), SEXPTYPE::REALSXP.0));
    let n = XLENGTH(x);
    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::RAWSXP.0, 64 * n as c_int));
    let mut j: R_xlen_t = 0;
    let mut i: R_xlen_t = 0;
    while i < n {
        let mut tmp: u64 = (*REAL(x).add(i as usize)).to_bits();
        let mut k: c_int = 0;
        while k < 64 {
            *RAW(ans).add(j as usize) = (tmp & 0x1) as u8;
            tmp >>= 1;
            j += 1;
            k += 1;
        }
        i += 1;
    }
    Rf_unprotect(2);
    ans
}

// ---------------------------------------------------------------------------
// do_packBits -- packBits()
// ---------------------------------------------------------------------------

/// Pack bits into raw, integer, or double vectors.
/// type="raw" packs 8 bits per byte, type="integer" packs 32 bits per int,
/// type="double" (numeric) packs 64 bits per double.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_packBits(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let x = CAR(args);
    let stype = CADR(args);

    if TYPEOF(x) != SEXPTYPE::RAWSXP.0
        && TYPEOF(x) != SEXPTYPE::LGLSXP.0
        && TYPEOF(x) != SEXPTYPE::INTSXP.0
    {
        return R_NilValue();
    }
    if !isString(stype) || LENGTH(stype) != 1 {
        return R_NilValue();
    }

    let type_str = CStr::from_ptr(CHAR(STRING_ELT(stype, 0)))
        .to_str()
        .unwrap_or("");
    let use_raw = type_str == "raw";
    let use_int = type_str == "integer";
    let use_double = !use_raw && !use_int;

    let fac: usize = if use_raw {
        8
    } else if use_int {
        32
    } else {
        64
    };
    let len = XLENGTH(x);
    if len % (fac as R_xlen_t) != 0 {
        return R_NilValue();
    }
    let slen = len / (fac as R_xlen_t);

    let result_type = if use_raw {
        SEXPTYPE::RAWSXP.0
    } else if use_int {
        SEXPTYPE::INTSXP.0
    } else {
        SEXPTYPE::REALSXP.0
    };
    let ans = Rf_protect(Rf_allocVector(result_type, slen as c_int));

    let mut i: R_xlen_t = 0;
    while i < slen {
        if use_raw {
            let mut btmp: u8 = 0;
            let mut k: c_int = 7;
            while k >= 0 {
                btmp <<= 1;
                if TYPEOF(x) == SEXPTYPE::RAWSXP.0 {
                    btmp |= *RAW(x).add((8 * i + k as R_xlen_t) as usize) & 0x1;
                } else {
                    let val = *INTEGER(x).add((8 * i + k as R_xlen_t) as usize);
                    if val == NA_INTEGER {
                        Rf_unprotect(1);
                        return R_NilValue();
                    }
                    btmp |= (val & 0x1) as u8;
                }
                k -= 1;
            }
            *RAW(ans).add(i as usize) = btmp;
        } else if use_int {
            let mut itmp: u32 = 0;
            let mut k: c_int = 31;
            while k >= 0 {
                itmp <<= 1;
                if TYPEOF(x) == SEXPTYPE::RAWSXP.0 {
                    itmp |= *RAW(x).add((32 * i + k as R_xlen_t) as usize) as u32 & 0x1;
                } else {
                    let val = *INTEGER(x).add((32 * i + k as R_xlen_t) as usize);
                    if val == NA_INTEGER {
                        Rf_unprotect(1);
                        return R_NilValue();
                    }
                    itmp |= (val as u32) & 0x1;
                }
                k -= 1;
            }
            *INTEGER(ans).add(i as usize) = itmp as c_int;
        } else {
            // useDouble: pack 64 bits into a double via union
            let mut lo_word: u32 = 0;
            let mut hi_word: u32 = 0;
            for kk in 0..2 {
                let mut w: u32 = 0;
                for b in 0..32 {
                    let mut bit: u32 = 0;
                    if TYPEOF(x) == SEXPTYPE::RAWSXP.0 {
                        bit = *RAW(x).add((64 * i + (kk as R_xlen_t) * 32 + b as R_xlen_t) as usize)
                            as u32
                            & 0x1;
                    } else {
                        let val = *INTEGER(x)
                            .add((64 * i + (kk as R_xlen_t) * 32 + b as R_xlen_t) as usize);
                        if val == NA_INTEGER {
                            Rf_unprotect(1);
                            return R_NilValue();
                        }
                        bit = (val as u32) & 0x1;
                    }
                    w |= bit << b;
                }
                if kk == 0 {
                    lo_word = w;
                } else {
                    hi_word = w;
                }
            }
            // Reconstruct double from two 32-bit words
            #[cfg(target_endian = "little")]
            {
                let bytes = lo_word
                    .to_ne_bytes()
                    .iter()
                    .chain(hi_word.to_ne_bytes().iter())
                    .copied()
                    .collect::<Vec<u8>>();
                let bits = u64::from_ne_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
                *REAL(ans).add(i as usize) = f64::from_bits(bits);
            }
            #[cfg(target_endian = "big")]
            {
                let bytes = hi_word
                    .to_ne_bytes()
                    .iter()
                    .chain(lo_word.to_ne_bytes().iter())
                    .copied()
                    .collect::<Vec<u8>>();
                let bits = u64::from_ne_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
                *REAL(ans).add(i as usize) = f64::from_bits(bits);
            }
        }
        i += 1;
    }
    Rf_unprotect(1);
    ans
}

// ---------------------------------------------------------------------------
// do_utf8ToInt -- utf8ToInt()
// ---------------------------------------------------------------------------

/// Convert a UTF-8 string to a vector of integer codepoints.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_utf8ToInt(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let x = CAR(args);
    if !isString(x) || LENGTH(x) == 0 {
        return R_NilValue();
    }
    if LENGTH(x) > 1 {
        // warning: argument should be a character vector of length 1
    }
    if STRING_ELT(x, 0) == R_NilValue() {
        return Rf_ScalarInteger(NA_INTEGER);
    }
    let s = CHAR(STRING_ELT(x, 0));
    let nc = XLENGTH(STRING_ELT(x, 0));
    let mut ians: Vec<c_int> = vec![0; nc as usize];
    let mut j: usize = 0;
    let mut i: R_xlen_t = 0;
    let mut tmp: c_int = 0;
    let mut used: c_int = 0;
    while i < nc {
        used = mbrtoint(&mut tmp, s.add(i as usize));
        if used <= 0 {
            break;
        }
        ians[j] = tmp;
        j += 1;
        i += used as R_xlen_t;
    }
    if used < 0 {
        // error: invalid UTF-8 string
        return R_NilValue();
    }
    let ans = Rf_allocVector(SEXPTYPE::INTSXP.0, j as c_int);
    let mut k: usize = 0;
    while k < j {
        *INTEGER(ans).add(k) = ians[k];
        k += 1;
    }
    ans
}

// ---------------------------------------------------------------------------
// do_intToUtf8 -- intToUtf8()
// ---------------------------------------------------------------------------

/// Convert integer codepoints to a UTF-8 string.
/// If multiple=TRUE, returns a character vector with one string per codepoint.
/// If allow_surrogate_pairs=TRUE, handles UTF-16 surrogate pairs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_intToUtf8(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let x = Rf_protect(coerceVector(CAR(args), SEXPTYPE::INTSXP.0));
    if !isInteger(x) {
        Rf_unprotect(1);
        return R_NilValue();
    }
    let multiple = crate::main::coerce::asLogical(CADR(args));
    let s_pair = crate::main::coerce::asLogical(CADDR(args));
    let ans: SEXP;

    if multiple != 0 {
        let nc = XLENGTH(x);
        ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, nc as c_int));
        let mut i: R_xlen_t = 0;
        while i < nc {
            let this = *INTEGER(x).add(i as usize);
            if this == NA_INTEGER || (this >= 0xD800 && this <= 0xDFFF) || this > 0x10FFFF {
                SET_STRING_ELT(ans, i, R_NilValue());
            } else {
                let mut buf = [0i8; 10];
                let used = inttomb(buf.as_mut_ptr(), this);
                buf[used] = 0;
                SET_STRING_ELT(ans, i, mkCharCE(buf.as_ptr(), CE_UTF8));
            }
            i += 1;
        }
        Rf_unprotect(2);
        return ans;
    } else {
        let nc = LENGTH(x);
        let mut have_na = false;
        // First pass: compute total length and validate
        let mut total_len: usize = 0;
        let mut i: c_int = 0;
        while i < nc {
            let this = *INTEGER(x).add(i as usize);
            if this == NA_INTEGER || (this >= 0xDC00 && this <= 0xDFFF) || this > 0x10FFFF {
                have_na = true;
                break;
            } else if this >= 0xD800 && this <= 0xDBFF {
                if s_pair == 0 || i >= nc - 1 {
                    have_na = true;
                    break;
                }
                let next = *INTEGER(x).add((i + 1) as usize);
                if next >= 0xDC00 && next <= 0xDFFF {
                    i += 1;
                    total_len += 4; // all points not in basic plane have length 4
                } else {
                    have_na = true;
                    break;
                }
            } else {
                total_len += inttomb(ptr::null_mut(), this);
            }
            i += 1;
        }
        if have_na {
            ans = Rf_allocVector(SEXPTYPE::STRSXP.0, 1);
            SET_STRING_ELT(ans, 0, R_NilValue());
            Rf_unprotect(2);
            return ans;
        }
        // Second pass: build the string
        let mut tmp = vec![0u8; total_len + 1];
        let mut len: usize = 0;
        let mut i: c_int = 0;
        while i < nc {
            let mut this = *INTEGER(x).add(i as usize);
            if s_pair != 0 && (this >= 0xD800 && this <= 0xDBFF) {
                // Surrogate pair handling
                i += 1;
                let next = *INTEGER(x).add(i as usize);
                let hi = (this - 0xD800) as u32;
                let lo = (next - 0xDC00) as u32;
                this = (0x10000 + (hi << 10) + lo) as c_int;
            }
            let mut buf = [0i8; 10];
            let used = inttomb(buf.as_mut_ptr(), this);
            if used > 0 {
                ptr::copy_nonoverlapping(
                    buf.as_ptr() as *const u8,
                    tmp.as_mut_ptr().add(len),
                    used,
                );
                len += used;
            }
            i += 1;
        }
        ans = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, 1));
        SET_STRING_ELT(
            ans,
            0,
            mkCharLenCE(tmp.as_ptr() as *const c_char, len as c_int, CE_UTF8),
        );
        Rf_unprotect(3);
        return ans;
    }
}
