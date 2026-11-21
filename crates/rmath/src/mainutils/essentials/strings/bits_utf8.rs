//! Raw/bit/UTF-8 conversions: intToBits, packBits, utf8ToInt, validity checks

use super::*;

/// GNU `intToBits(x)`: 32 LSB-first raw bits per integer.
pub fn do_intToBits(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let n = if x.is_null() || x == R_NilValue() {
        0
    } else {
        XLENGTH(x)
    };
    let out = Rf_allocVector3(SEXPTYPE::RAWSXP, n * 32);
    let _o = protect(out);
    for i in 0..n {
        let v = if TYPEOF(x) == SEXPTYPE::INTSXP {
            integer_elt_checked(x, (i as usize) as R_xlen_t) as u32
        } else if TYPEOF(x) == SEXPTYPE::REALSXP {
            real_elt_checked(x, (i as usize) as R_xlen_t) as i32 as u32
        } else if TYPEOF(x) == SEXPTYPE::LGLSXP {
            logical_elt_checked(x, (i as usize) as R_xlen_t) as u32
        } else {
            0
        };
        for b in 0..32 {
            set_raw_checked(
                out,
                ((i as usize) as R_xlen_t) * 32 + b,
                ((v >> b) & 1) as u8,
            );
        }
    }
    out
}

/// GNU `rawToBits(x)`: 8 LSB-first raw bits per byte.
pub fn do_rawToBits(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let n = if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::RAWSXP {
        0
    } else {
        XLENGTH(x)
    };
    let out = Rf_allocVector3(SEXPTYPE::RAWSXP, n * 8);
    let _o = protect(out);
    for i in 0..n {
        let v = raw_elt_checked(x, (i as usize) as R_xlen_t);
        for b in 0..8 {
            set_raw_checked(out, ((i as usize) as R_xlen_t) * 8 + b, (v >> b) & 1);
        }
    }
    out
}

/// GNU `rawShift(x, n)`.
pub fn do_rawShift(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::RAWSXP {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "argument 'x' must be a raw vector",
        );
    }
    let mut shift = 0i32;
    let rest = CDR(args);
    if !rest.is_null() && rest != R_NilValue() {
        let n = CAR(rest);
        if TYPEOF(n) == SEXPTYPE::INTSXP && XLENGTH(n) > 0 {
            shift = integer_elt_checked(n, 0);
        } else if TYPEOF(n) == SEXPTYPE::REALSXP && XLENGTH(n) > 0 {
            shift = real_elt_checked(n, 0) as i32;
        }
    }
    if shift < -8 || shift > 8 {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "argument 'n' must be a small integer",
        );
    }
    let n = XLENGTH(x);
    let out = Rf_allocVector3(SEXPTYPE::RAWSXP, n);
    let _o = protect(out);
    for i in 0..n as usize {
        let v = raw_elt_checked(x, (i) as R_xlen_t) as u32;
        set_raw_checked(
            out,
            (i) as R_xlen_t,
            if shift > 0 {
                (v << shift) as u8
            } else if shift < 0 {
                (v >> (-shift)) as u8
            } else {
                v as u8
            },
        );
    }
    out
}

/// GNU `packBits(x, type)`.
pub fn do_packBits(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::RAWSXP, 0);
    }
    let mut type_raw = true;
    let mut type_int = false;
    let rest = CDR(args);
    if !rest.is_null() && rest != R_NilValue() {
        let t = CAR(rest);
        if TYPEOF(t) == SEXPTYPE::STRSXP && XLENGTH(t) > 0 {
            let ch = STRING_ELT(t, 0);
            if !ch.is_null() {
                let s_cstr = charsxp_cstring(ch);
                let s = s_cstr.to_string_lossy();
                if s == "integer" {
                    type_raw = false;
                    type_int = true;
                } else if s == "double" {
                    type_raw = false;
                    type_int = false;
                }
            }
        }
    }
    let len = XLENGTH(x);
    let fac = if type_raw {
        8
    } else if type_int {
        32
    } else {
        64
    };
    if len % fac != 0 {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "argument 'x' must be a multiple of 8 long",
        );
    }
    let slen = len / fac;
    if type_raw {
        let out = Rf_allocVector3(SEXPTYPE::RAWSXP, slen);
        let _o = protect(out);
        for i in 0..slen {
            let mut btmp: u8 = 0;
            for k in (0..8).rev() {
                btmp <<= 1;
                let bit = packbits_bit(x, 8 * i + k);
                btmp |= bit;
            }
            set_raw_checked(out, (i as usize) as R_xlen_t, btmp);
        }
        out
    } else if type_int {
        let out = Rf_allocVector3(SEXPTYPE::INTSXP, slen);
        let _o = protect(out);
        for i in 0..slen {
            let mut itmp: u32 = 0;
            for k in (0..32).rev() {
                itmp <<= 1;
                itmp |= packbits_bit(x, 32 * i + k) as u32;
            }
            set_integer_checked(out, (i as usize) as R_xlen_t, itmp as i32);
        }
        out
    } else {
        let out = Rf_allocVector3(SEXPTYPE::REALSXP, slen);
        let _o = protect(out);
        for i in 0..slen {
            let mut bits: u64 = 0;
            for b in 0..64 {
                bits |= (packbits_bit(x, 64 * i + b) as u64) << b;
            }
            set_real_checked(out, (i as usize) as R_xlen_t, f64::from_bits(bits));
        }
        out
    }
}

fn packbits_bit(x: SEXP, idx: i64) -> u8 {
    match TYPEOF(x) {
        t if t == SEXPTYPE::RAWSXP => raw_elt_checked(x, (idx as usize) as R_xlen_t) & 1,
        t if t == SEXPTYPE::LGLSXP => {
            (logical_elt_checked(x, (idx as usize) as R_xlen_t) & 1) as u8
        }
        t if t == SEXPTYPE::INTSXP => {
            (integer_elt_checked(x, (idx as usize) as R_xlen_t) & 1) as u8
        }
        _ => 0,
    }
}

/// GNU `numToInts(x)`: two little-endian ints per double.
pub fn do_numToInts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let n = if x.is_null() || x == R_NilValue() {
        0
    } else {
        XLENGTH(x)
    };
    let out = Rf_allocVector3(SEXPTYPE::INTSXP, n * 2);
    let _o = protect(out);
    for i in 0..n {
        let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
            real_elt_checked(x, (i as usize) as R_xlen_t)
        } else if TYPEOF(x) == SEXPTYPE::INTSXP {
            let iv = integer_elt_checked(x, (i as usize) as R_xlen_t);
            if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
        } else {
            0.0
        };
        let bits = v.to_bits();
        set_integer_checked(out, ((i as usize) * 2) as R_xlen_t, bits as u32 as i32);
        set_integer_checked(
            out,
            ((i as usize) * 2 + 1) as R_xlen_t,
            (bits >> 32) as u32 as i32,
        );
    }
    out
}

/// GNU `numToBits(x)`: 64 LSB-first raw bits per double.
pub fn do_numToBits(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let n = if x.is_null() || x == R_NilValue() {
        0
    } else {
        XLENGTH(x)
    };
    let out = Rf_allocVector3(SEXPTYPE::RAWSXP, n * 64);
    let _o = protect(out);
    for i in 0..n {
        let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
            real_elt_checked(x, (i as usize) as R_xlen_t)
        } else if TYPEOF(x) == SEXPTYPE::INTSXP {
            let iv = integer_elt_checked(x, (i as usize) as R_xlen_t);
            if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
        } else {
            0.0
        };
        let mut bits = v.to_bits();
        for b in 0..64 {
            set_raw_checked(out, ((i as usize) as R_xlen_t) * 64 + b, (bits & 1) as u8);
            bits >>= 1;
        }
    }
    out
}

/// GNU `utf8ToInt(x)`: UTF-8 code points of a length-1 string.
pub fn do_utf8ToInt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP || XLENGTH(x) == 0 {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "argument must be a character vector of length 1",
        );
    }
    let _x_root = protect(x);
    if XLENGTH(x) > 1 {
        crate::mainutils::errors::Rf_warning_cstr(
            c"argument should be a character vector of length 1\nall but the first element will be ignored",
        );
    }
    let ch = STRING_ELT(x, 0);
    if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
        return Rf_ScalarInteger(NA_INTEGER);
    }
    // GNU raw.c: `if (!utf8Valid(s)) return ScalarInteger(NA_INTEGER)`.
    let n = XLENGTH(ch);
    let bytes = if n <= 0 {
        Vec::new()
    } else {
        crate::mainutils::printutils::charsxp_bytes(ch)
    };
    let Ok(s) = std::str::from_utf8(&bytes) else {
        return Rf_ScalarInteger(NA_INTEGER);
    };
    let cps: Vec<i32> = s.chars().map(|c| c as u32 as i32).collect();
    let out = Rf_allocVector3(SEXPTYPE::INTSXP, cps.len() as i64);
    let _o = protect(out);
    for (i, cp) in cps.iter().enumerate() {
        set_integer_checked(out, (i) as R_xlen_t, *cp);
    }
    out
}

/// GNU `intToUtf8(x, multiple=FALSE, allow_surrogate_pairs=FALSE)`.
pub fn do_intToUtf8(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let matched = match_formals(
        crate::mainutils::errors::R_getCurrentCall(),
        args,
        &["x", "multiple", "allow_surrogate_pairs"],
    );
    let x = matched[0];
    let m = matched[1];
    let p = matched[2];
    if x == crate::sexp::globals::R_MissingArg() {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "argument \"x\" is missing, with no default",
        );
    }
    let logical_flag = |value: SEXP, name: &str| -> bool {
        if value == crate::sexp::globals::R_MissingArg() {
            return false;
        }
        if value.is_null() || value == R_NilValue() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                if name == "multiple" {
                    "argument 'multiple' must be TRUE or FALSE"
                } else {
                    "argument 'allow_surrogate_pairs' must be TRUE or FALSE"
                },
            );
        }
        let flag = crate::mainutils::coerce::asLogical(value);
        if flag == crate::sexp::ffi::NA_LOGICAL {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                if name == "multiple" {
                    "argument 'multiple' must be TRUE or FALSE"
                } else {
                    "argument 'allow_surrogate_pairs' must be TRUE or FALSE"
                },
            );
        }
        flag == TRUE
    };
    let multiple = logical_flag(m, "multiple");
    let s_pair = logical_flag(p, "allow_surrogate_pairs");
    if is_factor(x) != 0 {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "argument 'x' must be an integer vector",
        );
    }
    let int_x = if TYPEOF(x) == SEXPTYPE::INTSXP {
        x
    } else {
        crate::mainutils::coerce::coerceVector(x, SEXPTYPE::INTSXP.into())
    };
    let _int_x_root = protect(int_x);
    let n = if int_x.is_null() || int_x == R_NilValue() {
        0
    } else {
        XLENGTH(int_x)
    };
    let code = |i: i64| -> i32 {
        if TYPEOF(int_x) == SEXPTYPE::INTSXP {
            integer_elt_checked(int_x, (i as usize) as R_xlen_t)
        } else {
            NA_INTEGER
        }
    };
    let valid_scalar = |cp: i32| -> Option<char> {
        if cp == NA_INTEGER || cp < 0 || (0xD800..=0xDFFF).contains(&cp) || cp > 0x10FFFF {
            None
        } else if cp == 0 {
            Some('\0')
        } else {
            char::from_u32(cp as u32)
        }
    };
    if multiple {
        if s_pair {
            crate::main::errors::Rf_warning_cstr(
                    c"allow_surrogate_pairs = TRUE is incompatible with multiple = TRUE and will be ignored",
                );
        }
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for i in 0..n {
            match valid_scalar(code(i)) {
                Some('\0') => {
                    SET_STRING_ELT(out, i, mk_char_cstr(c""));
                }
                Some(ch) => {
                    let mut buf = [0u8; 8];
                    let encoded = ch.encode_utf8(&mut buf);
                    let c = CString::new(encoded.as_bytes())
                        .unwrap_or_else(|_| CString::new("").unwrap());
                    SET_STRING_ELT(out, i, mk_char_cstr(&c));
                }
                None => {
                    SET_STRING_ELT(out, i, crate::sexp::globals::R_NaString());
                }
            }
        }
        out
    } else {
        // GNU raw.c: high surrogates pair with the next low surrogate when
        // allow_surrogate_pairs=TRUE; unpaired surrogates / NA / out of
        // range yield NA_STRING.
        let mut s = String::new();
        let mut have_na = false;
        let mut i = 0i64;
        while i < n {
            let this = code(i);
            if this == NA_INTEGER
                || this < 0
                || (0xDC00..=0xDFFF).contains(&this)
                || this > 0x10FFFF
            {
                have_na = true;
                break;
            } else if (0xD800..=0xDBFF).contains(&this) {
                if !s_pair || i + 1 >= n {
                    have_na = true;
                    break;
                }
                let next = code(i + 1);
                if !(0xDC00..=0xDFFF).contains(&next) {
                    have_na = true;
                    break;
                }
                let hi = (this as u32) - 0xD800;
                let lo = (next as u32) - 0xDC00;
                let cp = 0x10000 + (hi << 10) + lo;
                if let Some(ch) = char::from_u32(cp) {
                    s.push(ch);
                } else {
                    have_na = true;
                    break;
                }
                i += 2;
            } else if this == 0 {
                i += 1;
            } else if let Some(ch) = char::from_u32(this as u32) {
                s.push(ch);
                i += 1;
            } else {
                have_na = true;
                break;
            }
        }
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _o = protect(out);
        if have_na {
            SET_STRING_ELT(out, 0, crate::sexp::globals::R_NaString());
        } else {
            SET_STRING_ELT(out, 0, mk_char_len(s.as_ref(), s.len() as c_int));
        }
        out
    }
}

fn utf8_bytes_valid(s: SEXP) -> bool {
    if s.is_null() {
        return true;
    }
    let p = charsxp_text_of(s);
    if p.is_null() {
        return true;
    }
    let bytes = p.as_cstr().to_bytes();
    std::str::from_utf8(bytes).is_ok()
}

/// GNU `validUTF8(x)`.
pub fn do_validUTF8(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "invalid 'x' argument",
        );
    }
    let n = XLENGTH(x);
    let out = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    let _o = protect(out);
    for i in 0..n {
        set_logical_checked(
            out,
            (i as usize) as R_xlen_t,
            if utf8_bytes_valid(STRING_ELT(x, i)) {
                TRUE
            } else {
                FALSE
            },
        );
    }
    out
}

/// GNU `validEnc(x)`.
pub fn do_validEnc(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "invalid 'x' argument",
        );
    }
    let n = XLENGTH(x);
    let out = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    let _o = protect(out);
    for i in 0..n {
        let p = STRING_ELT(x, i);
        let ok = if crate::sexp::accessors::IS_BYTES(p) != 0
            || crate::sexp::accessors::IS_LATIN1(p) != 0
        {
            true
        } else {
            utf8_bytes_valid(p)
        };
        set_logical_checked(out, (i as usize) as R_xlen_t, if ok { TRUE } else { FALSE });
    }
    out
}
