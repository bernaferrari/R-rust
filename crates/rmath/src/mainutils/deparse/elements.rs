#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use super::*;
use crate::mainutils::r_format::{r_snprintf, r_sprintf, CArg};

// ---------------------------------------------------------------------------
// Local formatting helpers
// ---------------------------------------------------------------------------

/// C `%a` (hex float, default precision) as libc `printf` produces it:
/// shortest exact hex mantissa — `0x1.91eb851eb851fp+1`, `0x1p+0`,
/// `0x0p+0` for zero. The typed printf subset does not cover `%a`, and the
/// HEXNUMERIC deparse output must stay byte-identical to C.
fn format_hexfloat(v: f64) -> String {
    let bits = v.to_bits();
    let sign = if bits >> 63 != 0 { "-" } else { "" };
    let biased_exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & 0x000f_ffff_ffff_ffff;
    let (mut mantissa, exp) = if biased_exp == 0 {
        if frac == 0 {
            return format!("{sign}0x0p+0");
        }
        // subnormal: renormalize so the top mantissa bit is bit 52
        let shift = frac.leading_zeros() as i32 - 11;
        (frac << shift, -1022 - shift)
    } else {
        (frac | (1u64 << 52), biased_exp - 1023)
    };
    // default precision strips trailing zero hex digits (mantissa is
    // nonzero here: zero returns early above)
    mantissa >>= 4 * (mantissa.trailing_zeros() / 4);
    let digits = format!("{mantissa:x}");
    let (first, rest) = digits.split_at(1);
    if rest.is_empty() {
        format!("{sign}0x{first}p{exp:+}")
    } else {
        format!("{sign}0x{first}.{rest}p{exp:+}")
    }
}

// deparse2buf_name — deparse a vector element name to buffer
// ---------------------------------------------------------------------------

/// Deparse a name from a names vector to the buffer, with quoting as needed.
pub fn deparse2buf_name(nv: SEXP, i: c_int, d: &mut LocalParseData) {
    if isNull(nv) {
        return;
    }
    let elt = STRING_ELT(nv, i as R_xlen_t);
    if isNull(elt) {
        return;
    }
    let Some(name) = char_cstr_bytes_opt(elt) else {
        return;
    };
    if name.is_empty() {
        return;
    } // length test

    if is_valid_r_name_bytes(&name) {
        deparse2buff(elt, d);
    } else if d.backtick != 0 {
        append_line_bytes(b"`", d);
        deparse2buff(elt, d);
        append_line_bytes(b"`", d);
    } else {
        append_line_bytes(b"\"", d);
        deparse2buff(elt, d);
        append_line_bytes(b"\"", d);
    }
    append_line_bytes(b" = ", d);
}

// ---------------------------------------------------------------------------
// Owned element appenders — replace the session scratch-buffer formatters
// ---------------------------------------------------------------------------

fn append_snprintf<const N: usize>(fmt: &[u8], args: &[CArg<'_>], d: &mut LocalParseData) {
    let mut buf = [0; N];
    let len = r_snprintf(&mut buf, fmt, args).min(N - 1);
    append_line_bytes(&buf[..len], d);
}

fn append_int_element(val: c_int, d: &mut LocalParseData) {
    if val == NA_INTEGER {
        append_line_bytes(b"NA", d);
    } else {
        append_snprintf::<32>(b"%d", &[val.into()], d);
    }
}

// GNU default deparse uses formatReal + EncodeReal0, not %.17g.
fn real_element_bytes(val: f64) -> Vec<u8> {
    if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
        return b"NA".to_vec();
    }
    if val.is_nan() {
        return b"NaN".to_vec();
    }
    if val.is_infinite() {
        return if val.is_sign_negative() {
            b"-Inf".to_vec()
        } else {
            b"Inf".to_vec()
        };
    }
    let fmt = crate::mainutils::format::real_field(1, 0, |_| val);
    let mut text = crate::mainutils::printutils::encode_real0_text(val, fmt.w, fmt.d, fmt.e, b".");
    text.truncate(63);
    text
}

fn append_complex_element(x: Rcomplex, all_na: bool, d: &mut LocalParseData) {
    if crate::sexp::ffi::R_IsNA(x.r) && crate::sexp::ffi::R_IsNA(x.i) {
        append_line_bytes(if all_na { b"NA_complex_" } else { b"NA" }, d);
    } else if ISNAN(x.r) || !R_FINITE(x.i) {
        let part = |v: f64| -> String {
            if R_FINITE(v) {
                r_sprintf("%.17g", &[v.into()])
            } else if crate::sexp::ffi::R_IsNA(v) {
                "NA".to_string()
            } else if v.is_nan() {
                "NaN".to_string()
            } else if v.is_sign_negative() {
                "-Inf".to_string()
            } else {
                "Inf".to_string()
            }
        };
        let re = part(x.r);
        let im = part(x.i);
        append_line_bytes(
            format!("complex(real={re}, imaginary={im})").as_bytes(),
            d,
        );
    } else if d.opts & HEXNUMERIC != 0 && R_FINITE(x.r) && R_FINITE(x.i) {
        let text = format!("{} + {}i", format_hexfloat(x.r), format_hexfloat(x.i));
        append_line_bytes(text.as_bytes(), d);
    } else if d.opts & DIGITS17 != 0 && R_FINITE(x.r) && R_FINITE(x.i) {
        append_snprintf::<128>(b"%.17g%+.17gi", &[x.r.into(), x.i.into()], d);
    } else {
        let re = real_element_bytes(x.r);
        let im = real_element_bytes(x.i);
        let sign: &[u8] = if x.i >= 0.0 { b"+" } else { b"" };
        append_snprintf::<256>(
            b"%s%s%si",
            &[re.as_slice().into(), sign.into(), im.as_slice().into()],
            d,
        );
    }
}

fn append_string_element(s: SEXP, all_na: bool, d: &mut LocalParseData) {
    if isNull(s) || s == R_NaString() {
        append_line_bytes(if all_na { b"NA_character_" } else { b"NA" }, d);
        return;
    }
    let Some(bytes) = char_cstr_bytes_opt(s) else {
        append_line_bytes(b"NA", d);
        return;
    };
    let mut buf = [0; 2048];
    buf[0] = b'"';
    let mut pos = 1;
    for byte in bytes {
        if pos + 2 >= 2046 {
            break;
        }
        let escape = match byte {
            b'"' | b'\\' => Some(byte),
            b'\n' => Some(b'n'),
            b'\r' => Some(b'r'),
            b'\t' => Some(b't'),
            _ => None,
        };
        if let Some(escape) = escape {
            buf[pos] = b'\\';
            buf[pos + 1] = escape;
            pos += 2;
        } else {
            buf[pos] = byte;
            pos += 1;
        }
    }
    buf[pos] = b'"';
    append_line_bytes(&buf[..=pos], d);
}

// ---------------------------------------------------------------------------
// vector2buff — deparse atomic vectors to buffer
// ---------------------------------------------------------------------------

/// Deparse atomic vectors (LGLSXP, INTSXP, REALSXP, CPLXSXP, STRSXP, RAWSXP).
pub fn vector2buff(vector: SEXP, d: &mut LocalParseData) {
    let d_opts_in = d.opts;
    let tlen = LENGTH(vector);
    let mut surround = false;

    // Check for integer sequences (m:n)
    let mut int_seq = false;
    if TYPEOF(vector) == SEXPTYPE::INTSXP && tlen > 1 {
        let v0 = INTEGER_ELT(vector, 0);
        let v1 = INTEGER_ELT(vector, (1) as c_int);
        if v0 != NA_INTEGER && v1 != NA_INTEGER {
            let d_i = (v1 as f64) - (v0 as f64);
            if d_i.abs() == 1.0 {
                int_seq = true;
                for i in 2..tlen as usize {
                    let vi = INTEGER_ELT(vector, (i) as c_int);
                    if vi == NA_INTEGER {
                        int_seq = false;
                        break;
                    }
                    let diff = (vi as f64) - (INTEGER_ELT(vector, (i - 1) as c_int) as f64);
                    if (diff - d_i).abs() > 1e-10 {
                        int_seq = false;
                        break;
                    }
                }
            }
        }
    }

    let names_sym = crate::sexp::symbol::install_str("names");
    let mut nv = R_NilValue();
    let mut do_names = (d_opts_in & SHOW_ATTR_OR_NMS) != 0;
    if do_names {
        nv = getAttrib(vector, names_sym);
        if isNull(nv) {
            do_names = false;
        }
    }
    let _nv_guard = protect(nv);

    let mut str_names = false;
    let mut need_c = tlen > 1;
    str_names = do_names && (int_seq || tlen == 0);
    if str_names {
        d.opts &= !NICE_NAMES;
    }
    let attr = if (d_opts_in & SHOW_ATTR_OR_NMS) != 0 {
        attr1(vector, d)
    } else {
        ATTR_SIMPLE
    };
    if do_names {
        do_names = attr == ATTR_OK_NAMES || attr == ATTR_STRUC_ATTR;
    }
    if !need_c {
        need_c = do_names; // GNU: c(a = *) but not c(1)
    }

    if tlen == 0 {
        match TYPEOF(vector) {
            10 => append_line_bytes(b"logical(0)", d), // LGLSXP
            13 => append_line_bytes(b"integer(0)", d), // INTSXP
            14 => append_line_bytes(b"numeric(0)", d), // REALSXP
            15 => append_line_bytes(b"complex(0)", d), // CPLXSXP
            16 => append_line_bytes(b"character(0)", d), // STRSXP
            24 => append_line_bytes(b"raw(0)", d),     // RAWSXP
            _ => {} // intentionally unhandled: unsupported type for empty vector display
        }
    } else if TYPEOF(vector) == SEXPTYPE::INTSXP {
        if int_seq {
            append_int_element(INTEGER_ELT(vector, 0), d);
            append_line_bytes(b":", d);
            append_int_element(
                INTEGER_ELT(vector, ((tlen - 1) as usize) as c_int),
                d,
            );
        } else {
            let add_l = (d.opts & KEEPINTEGER != 0) && (d.opts & S_COMPAT == 0);
            let mut all_na = (d.opts & KEEPNA != 0) || add_l;
            for i in 0..tlen as usize {
                if INTEGER_ELT(vector, (i) as c_int) != NA_INTEGER {
                    all_na = false;
                    break;
                }
            }
            if (d.opts & KEEPINTEGER != 0) && (d.opts & S_COMPAT != 0) {
                append_line_bytes(b"as.integer(", d);
                surround = true;
            }
            all_na = all_na && (d.opts & S_COMPAT == 0);
            if need_c {
                append_line_bytes(b"c(", d);
            }
            for i in 0..tlen as usize {
                if do_names {
                    deparse2buf_name(nv, i as c_int, d);
                }
                let value = INTEGER_ELT(vector, (i) as c_int);
                if all_na && value == NA_INTEGER {
                    append_line_bytes(b"NA_integer_", d);
                } else {
                    append_int_element(value, d);
                    if add_l && value != NA_INTEGER {
                        append_line_bytes(b"L", d);
                    }
                }
                if i < (tlen as usize) - 1 {
                    append_line_bytes(b", ", d);
                }
                if tlen > 1 && d.len > d.cutoff {
                    writeline(d);
                }
                if !d.active {
                    break;
                }
            }
            if need_c {
                append_line_bytes(b")", d);
            }
            if surround {
                append_line_bytes(b")", d);
            }
        }
    } else {
        // tlen > 0; not INTSXP
        let mut all_na = d.opts & KEEPNA != 0;

        // Handle NA-heavy types
        if (d.opts & KEEPNA != 0) && TYPEOF(vector) == SEXPTYPE::REALSXP {
            for i in 0..tlen as usize {
                if !ISNAN(REAL_ELT(vector, (i) as c_int)) {
                    all_na = false;
                    break;
                }
            }
            if all_na && (d.opts & S_COMPAT != 0) {
                append_line_bytes(b"as.double(", d);
                surround = true;
            }
        } else if (d.opts & KEEPNA != 0) && TYPEOF(vector) == SEXPTYPE::CPLXSXP {
            for i in 0..tlen as usize {
                let c = COMPLEX_ELT(vector, (i) as c_int);
                if !crate::sexp::ffi::R_IsNA(c.r) && !crate::sexp::ffi::R_IsNA(c.i) {
                    all_na = false;
                    break;
                }
            }
            if all_na && (d.opts & S_COMPAT != 0) {
                append_line_bytes(b"as.complex(", d);
                surround = true;
            }
        } else if (d.opts & KEEPNA != 0) && TYPEOF(vector) == SEXPTYPE::STRSXP {
            for i in 0..tlen as usize {
                let elt = STRING_ELT(vector, i as R_xlen_t);
                if !elt.is_null() && elt != R_NilValue() && elt != R_NaString() {
                    all_na = false;
                    break;
                }
            }
            if all_na && (d.opts & S_COMPAT != 0) {
                append_line_bytes(b"as.character(", d);
                surround = true;
            }
        } else if TYPEOF(vector) == SEXPTYPE::RAWSXP {
            append_line_bytes(b"as.raw(", d);
            surround = true;
        }

        if need_c {
            append_line_bytes(b"c(", d);
        }
        all_na = all_na && (d.opts & S_COMPAT == 0);

        for i in 0..tlen as usize {
            if do_names {
                deparse2buf_name(nv, i as c_int, d);
            }

            match TYPEOF(vector) {
                10 => {
                    // LGLSXP
                    let value = LOGICAL_ELT(vector, (i) as c_int);
                    let text: &[u8] = match value {
                        NA_INTEGER => b"NA",
                        0 => b"FALSE",
                        _ => b"TRUE",
                    };
                    append_line_bytes(text, d);
                }
                14 => {
                    // REALSXP
                    let v = REAL_ELT(vector, (i) as c_int);
                    if all_na && v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        append_line_bytes(b"NA_real_", d);
                    } else if (d.opts & HEXNUMERIC != 0) && R_FINITE(v) {
                        append_line_bytes(format_hexfloat(v).as_bytes(), d);
                    } else if (d.opts & DIGITS17 != 0) && R_FINITE(v) {
                        append_snprintf::<64>(b"%.17g", &[v.into()], d);
                    } else {
                        append_line_bytes(&real_element_bytes(v), d);
                    }
                }
                15 => append_complex_element(COMPLEX_ELT(vector, (i) as c_int), all_na, d),
                16 => append_string_element(STRING_ELT(vector, i as R_xlen_t), all_na, d),
                24 => append_snprintf::<8>(
                    b"0x%02x",
                    &[(RAW_ELT(vector, (i) as c_int) as c_uint).into()],
                    d,
                ),
                _ => {} // intentionally unhandled: unsupported SEXPTYPE for element formatting
            }
            if i < (tlen as usize) - 1 {
                append_line_bytes(b", ", d);
            }
            if tlen > 1 && d.len > d.cutoff {
                writeline(d);
            }
            if !d.active {
                break;
            }
        }

        if need_c {
            append_line_bytes(b")", d);
        }
        if surround {
            append_line_bytes(b")", d);
        }
    }
    if attr >= ATTR_STRUC_ATTR {
        attr2(vector, d, attr == ATTR_STRUC_ATTR);
    }
    if str_names {
        d.opts = d_opts_in;
    }
}
// ---------------------------------------------------------------------------
// vec2buff — deparse list/expression vectors to buffer
// ---------------------------------------------------------------------------

/// Deparse vectors of S-expressions (list() and expression() objects).
pub fn vec2buff(v: SEXP, d: &mut LocalParseData, do_names: bool) {
    let mut lbreak = false;
    let n = LENGTH(v);
    let names_sym = crate::sexp::symbol::install_str("names");
    let srcref_sym = crate::sexp::symbol::install_str("srcref");
    let mut nv = R_NilValue();
    let mut do_names = do_names;
    if do_names {
        nv = getAttrib(v, names_sym);
        if isNull(nv) {
            do_names = false;
        }
    }
    let _nv_guard = protect(nv);

    let mut sv = R_NilValue();
    if (d.opts & USESOURCE) != 0 {
        sv = getAttrib(v, srcref_sym);
        if TYPEOF(sv) != SEXPTYPE::VECSXP {
            sv = R_NilValue();
        }
    }

    for i in 0..n as usize {
        if i > 0 {
            append_line_bytes(b", ", d);
        }
        linebreak(&mut lbreak, d);
        if do_names {
            deparse2buf_name(nv, i as c_int, d);
        }
        if !src2buff(sv, i as c_int, d) {
            deparse2buff(VECTOR_ELT(v, i as R_xlen_t), d);
        }
    }
    if lbreak {
        d.indent -= 1;
    }
}

// ---------------------------------------------------------------------------
// args2buff — deparse argument list to buffer
// ---------------------------------------------------------------------------

/// Deparse an argument list (pairlist) to the buffer.
///
/// Handles named and unnamed arguments, default values for formals, and
/// line breaking for long argument lists.
pub fn args2buff(arglist: SEXP, _lineb: c_int, formals: c_int, d: &mut LocalParseData) {
    let mut lbreak = false;
    let mut cur = arglist;

    while !isNull(cur) {
        if TYPEOF(cur) != SEXPTYPE::LISTSXP && TYPEOF(cur) != SEXPTYPE::LANGSXP {
            break;
        }
        if !isNull(TAG(cur)) {
            let s = TAG(cur);
            if s == R_DotsSymbol() {
                append_char_name(PRINTNAME(s), d);
            } else if d.backtick != 0 {
                append_quoted_name(PRINTNAME(s), b'`', d);
            } else {
                append_quoted_name(PRINTNAME(s), b'"', d);
            }
            if formals != 0 {
                // GNU: only R_MissingArg means no default. `x = NULL` must
                // print as `x = NULL`, not as a missing formal.
                if CAR(cur) != R_MissingArg() {
                    append_line_bytes(b" = ", d);
                    d.fnarg = true;
                    deparse2buff(CAR(cur), d);
                }
            } else {
                append_line_bytes(b" = ", d);
                if CAR(cur) != R_MissingArg() {
                    d.fnarg = true;
                    deparse2buff(CAR(cur), d);
                }
            }
        } else {
            d.fnarg = true;
            deparse2buff(CAR(cur), d);
        }
        cur = CDR(cur);
        if !isNull(cur) {
            append_line_bytes(b", ", d);
            linebreak(&mut lbreak, d);
        }
    }
    if lbreak {
        d.indent -= 1;
    }
}

// ---------------------------------------------------------------------------
