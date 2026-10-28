//! format/formatC/prettyNum, hexmode/octmode class wrappers, and calendar labels

use super::*;

fn as_mode_integer(x: SEXP, base: i32) -> Option<SEXP> {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::INTSXP {
            return Some(x);
        }
        if TYPEOF(x) == SEXPTYPE::REALSXP {
            let n = XLENGTH(x);
            let out = Rf_allocVector3(SEXPTYPE::INTSXP, n);
            let _o = protect(out);
            for i in 0..n {
                let v = *REAL(x).add(i as usize);
                if v.is_nan() {
                    *INTEGER(out).add(i as usize) = NA_INTEGER;
                } else if v == v.trunc() && v >= i32::MIN as f64 && v <= i32::MAX as f64 {
                    *INTEGER(out).add(i as usize) = v as i32;
                } else {
                    return None;
                }
            }
            return Some(out);
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP {
            let n = XLENGTH(x);
            let out = Rf_allocVector3(SEXPTYPE::INTSXP, n);
            let _o = protect(out);
            for i in 0..n {
                let ch = STRING_ELT(x, i);
                if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
                    *INTEGER(out).add(i as usize) = NA_INTEGER;
                    continue;
                }
                let s = std::ffi::CStr::from_ptr(CHAR(ch)).to_string_lossy();
                match i32::from_str_radix(s.trim(), base as u32) {
                    Ok(v) if v >= 0 => *INTEGER(out).add(i as usize) = v,
                    _ => return None,
                }
            }
            return Some(out);
        }
        None
    }
}

unsafe fn set_mode_class(x: SEXP, class: &str) -> SEXP {
    unsafe {
        let dup = crate::mainutils::duplicate::Rf_duplicate(x);
        let _d = protect(dup);
        let cstr = CString::new(class).unwrap_or_else(|_| CString::new("").unwrap());
        crate::sexp::attrib_core::setAttrib(
            dup,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(cstr.as_ptr()),
        );
        dup
    }
}

/// GNU `as.hexmode(x)`.
pub unsafe fn do_as_hexmode(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if crate::mainutils::objects::inherits2(x, c"hexmode".as_ptr()) != FALSE {
            return x;
        }
        match as_mode_integer(x, 16) {
            Some(v) => set_mode_class(v, "hexmode"),
            None => crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'x' cannot be coerced to class \"hexmode\"",
            ),
        }
    }
}

/// GNU `as.octmode(x)`.
pub unsafe fn do_as_octmode(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if crate::mainutils::objects::inherits2(x, c"octmode".as_ptr()) != FALSE {
            return x;
        }
        match as_mode_integer(x, 8) {
            Some(v) => set_mode_class(v, "octmode"),
            None => crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'x' cannot be coerced to class \"octmode\"",
            ),
        }
    }
}

fn format_mode_ints(x: SEXP, hex: bool) -> SEXP {
    unsafe {
        let n = if TYPEOF(x) == SEXPTYPE::INTSXP {
            XLENGTH(x)
        } else {
            0
        };
        let mut texts: Vec<Option<String>> = Vec::with_capacity(n as usize);
        for i in 0..n {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER {
                texts.push(None);
            } else if hex {
                texts.push(Some(format!("{v:x}")));
            } else {
                texts.push(Some(format!("{v:o}")));
            }
        }
        if n > 1 {
            let width = texts.iter().flatten().map(|s| s.len()).max().unwrap_or(0);
            for text in texts.iter_mut().flatten() {
                *text = format!("{text:0>width$}");
            }
        }
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for (i, s) in texts.iter().enumerate() {
            let ch = if let Some(text) = s {
                let c = CString::new(text.as_str()).unwrap_or_else(|_| CString::new("").unwrap());
                Rf_mkChar(c.as_ptr())
            } else {
                crate::sexp::globals::R_NaString()
            };
            SET_STRING_ELT(out, i as i64, ch);
        }
        copy_mode_structure(out, x);
        out
    }
}

fn copy_mode_structure(out: SEXP, x: SEXP) {
    unsafe {
        use crate::sexp::attrib_core::{
            R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib,
        };
        let dim = getAttrib(x, R_DimSymbol());
        if !dim.is_null() && dim != R_NilValue() {
            setAttrib(out, R_DimSymbol(), dim);
        }
        let dn = getAttrib(x, R_DimNamesSymbol());
        if !dn.is_null() && dn != R_NilValue() {
            setAttrib(out, R_DimNamesSymbol(), dn);
        }
        let nm = getAttrib(x, R_NamesSymbol());
        if !nm.is_null() && nm != R_NilValue() {
            setAttrib(out, R_NamesSymbol(), nm);
        }
    }
}

pub(crate) fn as_character_mode(x: SEXP, hex: bool, keep_str: bool) -> SEXP {
    unsafe {
        let n = if TYPEOF(x) == SEXPTYPE::INTSXP {
            XLENGTH(x)
        } else {
            0
        };
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for i in 0..n {
            let v = *INTEGER(x).add(i as usize);
            let ch = if v == NA_INTEGER {
                crate::sexp::globals::R_NaString()
            } else {
                let s = if hex {
                    format!("{v:x}")
                } else {
                    format!("{v:o}")
                };
                let c = CString::new(s).unwrap_or_else(|_| CString::new("").unwrap());
                Rf_mkChar(c.as_ptr())
            };
            SET_STRING_ELT(out, i as i64, ch);
        }
        if keep_str {
            copy_mode_structure(out, x);
        }
        out
    }
}

pub(crate) fn mode_keep_str(args: SEXP) -> bool {
    unsafe {
        let rest = CDR(args);
        if rest.is_null() || rest == R_NilValue() {
            return false;
        }
        crate::mainutils::coerce::asLogical(CAR(rest)) == 1
    }
}

/// GNU `as.character.hexmode`: unpadded `%x`; copy dim only if `keepStr`.
pub unsafe fn do_as_character_hexmode(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { as_character_mode(CAR(args), true, mode_keep_str(args)) }
}

/// GNU `format.hexmode`: zero-pad to the widest value.
pub unsafe fn do_format_hexmode(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { format_mode_ints(CAR(args), true) }
}

/// GNU `as.character.octmode`: unpadded `%o`; copy dim only if `keepStr`.
pub unsafe fn do_as_character_octmode(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { as_character_mode(CAR(args), false, mode_keep_str(args)) }
}

/// GNU `format.octmode`: zero-pad to the widest value.
pub unsafe fn do_format_octmode(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { format_mode_ints(CAR(args), false) }
}

fn formatc_exp(v: f64, digits: usize, upper: bool) -> String {
    let s = if upper {
        format!("{v:.digits$E}")
    } else {
        format!("{v:.digits$e}")
    };
    // GNU uses at least two exponent digits and a sign: e+00
    if let Some(idx) = s.rfind(['e', 'E']) {
        let (head, exp) = s.split_at(idx);
        let mut chars = exp.chars();
        let mark = chars.next().unwrap();
        let rest: String = chars.collect();
        let (sign, digits_part) = if let Some(stripped) = rest.strip_prefix('+') {
            ('+', stripped)
        } else if let Some(stripped) = rest.strip_prefix('-') {
            ('-', stripped)
        } else {
            ('+', rest.as_str())
        };
        format!("{head}{mark}{sign}{digits_part:0>2}")
    } else {
        s
    }
}

fn formatc_round_significant(v: f64, digits: usize) -> f64 {
    if v == 0.0 {
        return 0.0;
    }
    let exponent = v.abs().log10().floor() as i32;
    let scale = 10f64.powi(digits as i32 - 1 - exponent);
    (v * scale).round() / scale
}

fn formatc_g_style(v: f64, digits: usize, alt: bool) -> String {
    let digits = digits.max(1);
    let rounded = formatc_round_significant(v, digits);
    let exponent = rounded.abs().log10().floor() as i32;
    if exponent < -4 || exponent >= digits as i32 {
        let mut text = formatc_exp(rounded, digits.saturating_sub(1), false);
        if let Some(index) = text.rfind('e') {
            let (head, tail) = text.split_at(index);
            let head = if alt {
                if head.contains('.') {
                    head.to_string()
                } else {
                    format!("{head}.")
                }
            } else {
                head.trim_end_matches('0').trim_end_matches('.').to_string()
            };
            text = format!("{head}{tail}");
        }
        text
    } else {
        let decimals = (digits as i32 - exponent - 1).max(0) as usize;
        let text = format!("{rounded:.decimals$}");
        if alt {
            if text.contains('.') {
                text
            } else {
                format!("{text}.")
            }
        } else if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            text
        }
    }
}

fn formatc_one(v: f64, digits: i32, format: &str, alt: bool) -> String {
    // GNU util.c str_signif: dig = abs(digits); a negative `digits` is the
    // R-level flag ("#") signal to keep trailing zeros (digits >= 0 strips).
    let d = digits.unsigned_abs() as usize;
    match format {
        "f" => format!("{v:.d$}"),
        "e" => formatc_exp(v, d, false),
        "E" => formatc_exp(v, d, true),
        "g" | "G" => {
            if !v.is_finite() {
                return if v.is_nan() {
                    "NA".to_string()
                } else if v.is_sign_negative() {
                    "-Inf".to_string()
                } else {
                    "Inf".to_string()
                };
            }
            if v == 0.0 || (d == 0 && !matches!(format, "fg" | "fG")) {
                return "0".to_string();
            }
            let exp = v.abs().log10().floor() as i32;
            let upper = format == "G";
            if exp < -4 || exp >= d as i32 {
                let s = formatc_exp(v, d.saturating_sub(1), upper);
                if alt {
                    s
                } else if let Some(idx) = s.rfind(['e', 'E']) {
                    let (head, tail) = s.split_at(idx);
                    let head = head.trim_end_matches('0').trim_end_matches('.');
                    format!("{head}{tail}")
                } else {
                    s
                }
            } else {
                let decimals = (d as i32 - exp - 1).max(0) as usize;
                let s = format!("{v:.decimals$}");
                if !alt && s.contains('.') {
                    s.trim_end_matches('0').trim_end_matches('.').to_string()
                } else {
                    s
                }
            }
        }
        "fg" | "fG" => {
            if !v.is_finite() {
                return if v.is_nan() {
                    "NA".to_string()
                } else if v.is_sign_negative() {
                    "-Inf".to_string()
                } else {
                    "Inf".to_string()
                };
            }
            if v == 0.0 {
                return "0".to_string();
            }
            // Mirror GNU str_signif's "smart f" path: round the absolute
            // mantissa, carry a decade only for positive exponents, then use
            // significant-digit g formatting unless the value is below 1e-4.
            let mut exponent = (v.abs().log10() + 1e-12).floor() as i32;
            let scale = 10f64.powi(d as i32 - 1);
            let mantissa = v.abs() / 10f64.powi(exponent) + 1e-12;
            let rounded_mantissa = (mantissa * scale).round() / scale;
            let mut rounded = v;
            if exponent > 0 && rounded_mantissa >= 10.0 {
                // GNU's source assigns the positive absolute mantissa here.
                rounded = rounded_mantissa * 10f64.powi(exponent);
                exponent += 1;
            }
            if exponent == -4 && rounded.abs() < 1e-4 {
                exponent = -5;
            }

            if exponent < -4 {
                let decimals = (d as i32 - 1 - exponent).max(0) as usize;
                let text = format!("{rounded:.decimals$}");
                if digits >= 0 {
                    text.trim_end_matches('0').trim_end_matches('.').to_string()
                } else if text.contains('.') {
                    text
                } else {
                    format!("{text}.")
                }
            } else {
                let precision = if exponent >= d as i32 {
                    exponent as usize + 1
                } else {
                    d
                };
                formatc_g_style(rounded, precision, alt)
            }
        }
        "d" => format!("{}", v as i64),
        _ => format!("{v:.d$}"),
    }
}

/// GNU `formatC(x, digits, width, format)`.
pub unsafe fn do_formatC(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // GNU util.c do_formatC: positional (x, type, width, digits,
        // format, flag, i.strlen). The R-level formatC closure in
        // base/format.R performs argument checking, character padding,
        // zero.print splicing and prettyNum post-processing.
        let mut cell = args;
        let mut next = || -> Option<SEXP> {
            if cell.is_null() || cell == R_NilValue() {
                None
            } else {
                let v = CAR(cell);
                cell = CDR(cell);
                Some(v)
            }
        };
        let x = next().unwrap_or(R_NilValue());
        let type_arg = next().unwrap_or(R_NilValue());
        let width_arg = next().unwrap_or(R_NilValue());
        let digits_arg = next().unwrap_or(R_NilValue());
        let format_arg = next().unwrap_or(R_NilValue());
        let flag_arg = next().unwrap_or(R_NilValue());
        let i_strlen_arg = next().unwrap_or(R_NilValue());

        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let _x_guard = protect(x);
        let _type_guard = protect(type_arg);
        let _width_guard = protect(width_arg);
        let _digits_guard = protect(digits_arg);
        let _format_guard = protect(format_arg);
        let _flag_guard = protect(flag_arg);
        let _i_strlen_source_guard = protect(i_strlen_arg);
        // GNU do_formatC() coerces i.strlen to integer before allocating its
        // output buffers. Its R-level pmax can produce a real vector, so use
        // the shared R coercion path rather than an unchecked Rust cast.
        let i_strlen_arg =
            crate::mainutils::coerce::coerceVector(i_strlen_arg, SEXPTYPE::INTSXP.as_c_int());
        let _i_strlen_guard = protect(i_strlen_arg);
        let type_str = string_elt_or(&type_arg, "double");
        let mut width = int_or_na(&width_arg);
        let digits = int_or_na(&digits_arg);
        let mut format = string_elt_or(&format_arg, "");
        if format.is_empty() {
            format = if TYPEOF(x) == SEXPTYPE::INTSXP {
                "d".to_string()
            } else {
                "g".to_string()
            };
        }
        let flag = string_elt_or(&flag_arg, "");
        let alt = flag.contains('#');
        if width == NA_INTEGER {
            width = 0;
        }

        let n = XLENGTH(x);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for i in 0..n {
            let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                REAL_ELT(x, i as i32)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                let iv = INTEGER_ELT(x, i as i32);
                if iv == NA_INTEGER {
                    f64::NAN
                } else {
                    iv as f64
                }
            } else {
                0.0
            };
            let s = if v.is_nan() {
                "NA".to_string()
            } else if format == "d" {
                // %*d with the flag modifiers; width < 0 left-justifies.
                let w = width.unsigned_abs() as usize;
                let body = format!("{}", v as i64);
                c_pad(body, w, &flag, width < 0, flag.contains('0'))
            } else {
                let body = formatc_one(v, digits, &format, alt);
                c_pad(
                    body,
                    width.unsigned_abs() as usize,
                    &flag,
                    width < 0 || flag.contains('-'),
                    flag.contains('0') && !flag.contains('-'),
                )
            };
            // The C buffers hold i.strlen + 2 characters plus NUL; longer
            // formatted strings are truncated to that cap.
            let cap = if XLENGTH(i_strlen_arg) > i {
                (INTEGER_ELT(i_strlen_arg, i as i32) as usize).saturating_add(2)
            } else {
                usize::MAX
            };
            let s = if s.chars().count() > cap {
                s.chars().take(cap).collect()
            } else {
                s
            };
            let c = CString::new(s).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(out, i, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}

/// Read element 0 of a string argument with a fallback.
unsafe fn string_elt_or(x: &SEXP, fallback: &str) -> String {
    unsafe {
        if !x.is_null() && *x != R_NilValue() && TYPEOF(*x) == SEXPTYPE::STRSXP && XLENGTH(*x) > 0 {
            let ch = STRING_ELT(*x, 0);
            if !ch.is_null() {
                return std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned();
            }
        }
        fallback.to_string()
    }
}

/// `asInteger` for the internal numeric arguments.
unsafe fn int_or_na(x: &SEXP) -> i32 {
    unsafe {
        if !x.is_null() && *x != R_NilValue() {
            if TYPEOF(*x) == SEXPTYPE::INTSXP && XLENGTH(*x) > 0 {
                return INTEGER_ELT(*x, 0);
            }
            if TYPEOF(*x) == SEXPTYPE::REALSXP && XLENGTH(*x) > 0 {
                return REAL_ELT(*x, 0) as i32;
            }
        }
        NA_INTEGER
    }
}

/// C-style width adjustment with the flag modifiers.
fn c_pad(body: String, width: usize, flag: &str, left: bool, zero: bool) -> String {
    let len = body.chars().count();
    if len >= width {
        return body;
    }
    let pad = width - len;
    if left || (zero && !body.chars().all(|c| c.is_ascii_digit())) {
        if left {
            format!("{body}{}", " ".repeat(pad))
        } else {
            format!("{}{body}", " ".repeat(pad))
        }
    } else if zero {
        // Zero padding goes after any sign.
        if let Some(rest) = body.strip_prefix('-') {
            format!("-{}{rest}", "0".repeat(pad))
        } else if let Some(rest) = body.strip_prefix('+') {
            format!("+{}{rest}", "0".repeat(pad))
        } else {
            format!("{}{body}", "0".repeat(pad))
        }
    } else {
        format!("{}{body}", " ".repeat(pad))
    }
}

fn prettynum_group(int_part: &str, mark: &str, interval: usize) -> String {
    if mark.is_empty() || interval == 0 {
        return int_part.to_string();
    }
    let (sign, digits) = if let Some(rest) = int_part.strip_prefix('-') {
        ("-", rest)
    } else {
        ("", int_part)
    };
    let chars: Vec<char> = digits.chars().collect();
    if chars.len() <= interval {
        return format!("{sign}{digits}");
    }
    let mut out = String::new();
    let rem = chars.len() % interval;
    if rem > 0 {
        out.extend(chars[..rem].iter());
    }
    let mut i = rem;
    while i < chars.len() {
        if !out.is_empty() {
            out.push_str(mark);
        }
        out.extend(chars[i..i + interval].iter());
        i += interval;
    }
    format!("{sign}{out}")
}

fn pretty_num_split(args: SEXP) -> (SEXP, SEXP) {
    unsafe {
        let mut x = R_NilValue();
        let mut found = false;
        let mut head = R_NilValue();
        let mut tail = R_NilValue();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if !found && (name == "x" || name.is_empty()) {
                x = CAR(cell);
                found = true;
            } else {
                let next = Rf_cons(CAR(cell), R_NilValue());
                SETTAG(next, tag);
                if head == R_NilValue() {
                    head = next;
                } else {
                    SETCDR(tail, next);
                }
                tail = next;
            }
            cell = CDR(cell);
        }
        (x, head)
    }
}

/// GNU `prettyNum(x, big.mark, decimal.mark)`.
pub unsafe fn do_prettyNum(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let (x, rest) = pretty_num_split(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = XLENGTH(x);
        let mut drop0 = false;
        let mut scan = rest;
        while !scan.is_null() && scan != R_NilValue() {
            let tag = TAG(scan);
            if !tag.is_null() && tag != R_NilValue() {
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag))).to_string_lossy();
                if name == "drop0trailing" {
                    drop0 = crate::main::coerce::asLogical(CAR(scan)) != 0;
                }
            }
            scan = CDR(scan);
        }
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _out = protect(out);
        if TYPEOF(x) == SEXPTYPE::STRSXP {
            for i in 0..n {
                let s = elt_to_string(x, i);
                let s = pretty_num_one(&s, "", drop0, ".", "", 5);
                let c = std::ffi::CString::new(s).unwrap_or_default();
                SET_STRING_ELT(out, i, Rf_mkChar(c.as_ptr()));
            }
            return out;
        }
        for i in 0..n {
            let elt = if TYPEOF(x) == SEXPTYPE::INTSXP {
                Rf_ScalarInteger(*INTEGER(x).add(i as usize))
            } else if TYPEOF(x) == SEXPTYPE::REALSXP {
                Rf_ScalarReal(*REAL(x).add(i as usize))
            } else if TYPEOF(x) == SEXPTYPE::STRSXP {
                let one = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
                SET_STRING_ELT(one, 0, STRING_ELT(x, i));
                one
            } else {
                x
            };
            let _elt = protect(elt);
            let call_args = Rf_cons(elt, rest);
            let _call_args = protect(call_args);
            let formatted = format_numeric_vector(elt, 1, call_args);
            if TYPEOF(formatted) == SEXPTYPE::STRSXP && XLENGTH(formatted) > 0 {
                SET_STRING_ELT(out, i, STRING_ELT(formatted, 0));
            }
        }
        out
    }
}

/// R's `format(x, digits, nsmall)` — format numbers as strings.
pub unsafe fn do_format(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let digits_arg = CAR(CDR(args));
        let nsmall_arg = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() {
            return Rf_mkString(c"".as_ptr());
        }
        // Upstream `format` is a UseMethod generic; its .Internal core
        // (do_format) never dispatches — GNU reaches the internal only via
        // `format.default`'s .Internal(format(...)), and S3 dispatch already
        // happened at the generic. Skip closure AND builtin-class dispatch
        // when invoked from inside the `format` generic or `format.default`
        // (NextMethod frames keep the generic-name sys.call, so they match)
        // so `format.default` on a classed object cannot re-dispatch into
        // e.g. `format.AsIs` (infinite recursion) and
        // format.default(POSIXct) formats the raw number like GNU. Direct
        // Rust callers that stand in for the generic (print/output column
        // formatting) and other contexts still dispatch.
        let inside_format_default = called_from_format_generic_or_default();
        if !inside_format_default
            && let Some(result) =
                crate::mainutils::essentials::apply_s3_closure_method("format", _call, args, _rho)
        {
            return result;
        }
        if !inside_format_default
            && crate::mainutils::objects::inherits2(x, c"POSIXlt".as_ptr()) != FALSE
        {
            return crate::mainutils::datetime::do_format_POSIXlt(_call, _op, args, _rho);
        }
        if !inside_format_default
            && (crate::mainutils::objects::inherits2(x, c"POSIXct".as_ptr()) != FALSE
                || crate::mainutils::objects::inherits2(x, c"POSIXt".as_ptr()) != FALSE)
        {
            return crate::mainutils::datetime::do_format_POSIXct(_call, _op, args, _rho);
        }
        if !inside_format_default
            && crate::mainutils::objects::inherits2(x, c"Date".as_ptr()) != FALSE
        {
            return crate::mainutils::essentials::do_format_Date(_call, _op, args, _rho);
        }
        if !inside_format_default
            && crate::mainutils::objects::inherits2(x, c"data.frame".as_ptr()) != FALSE
        {
            return crate::mainutils::essentials::do_format_data_frame(_call, _op, args, _rho);
        }
        if !inside_format_default
            && crate::mainutils::objects::inherits2(x, c"hexmode".as_ptr()) != FALSE
        {
            return format_mode_ints(x, true);
        }
        if !inside_format_default
            && crate::mainutils::objects::inherits2(x, c"octmode".as_ptr()) != FALSE
        {
            return format_mode_ints(x, false);
        }
        // format.default: environments render as their display form
        // ("<environment: 0x...>"); closures deparse. These types have no
        // meaningful XLENGTH — the vector loop below must not see them.
        let x_type = TYPEOF(x);
        if x_type == SEXPTYPE::ENVSXP {
            if let Some(env_sexp) = crate::sexp::object::Sexp::from_raw(x) {
                let rendered = crate::sexp::output::format_environment_public(env_sexp);
                let cstr = CString::new(rendered).unwrap_or_default();
                return Rf_mkString(cstr.as_ptr());
            }
            return Rf_mkString(c"<environment>".as_ptr() as *const std::os::raw::c_char);
        }
        if x_type == SEXPTYPE::CLOSXP
            || x_type == SEXPTYPE::SPECIALSXP
            || x_type == SEXPTYPE::BUILTINSXP
        {
            return crate::mainutils::deparse::deparse_symbolic(x, true);
        }
        // format.default formats symbolic objects by deparse:
        // call/expression/"function"/"(" -> deparse(x, backtick=TRUE),
        // name -> deparse(x, backtick=FALSE). Reaching the vector path with
        // a LANGSXP/SYMSXP walks garbage pairlist memory (XLENGTH is only
        // meaningful for vectors).
        if x_type == SEXPTYPE::LANGSXP || x_type == SEXPTYPE::EXPRSXP || x_type == SEXPTYPE::SYMSXP
        {
            return crate::mainutils::deparse::deparse_symbolic(x, x_type != SEXPTYPE::SYMSXP);
        }
        if x_type == SEXPTYPE::BCODESXP {
            crate::mainutils::errors::errorcall_str(
                _call,
                "Found no format() method for class \"bytecode\"",
            );
        }
        let nsmall = if nsmall_arg.is_null() || nsmall_arg == R_NilValue() {
            0usize
        } else {
            real_or_default(nsmall_arg, 0.0) as usize
        };
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        // GNU format.default numeric core: compute the common field
        // parameters across the whole vector, encode each element with the
        // Encode* printers, and right-justify to the shared width.
        let x_type_now = TYPEOF(x);
        if matches!(x_type_now, 10 | 13 | 14 | 15) {
            return format_numeric_vector(x, n, args);
        }
        if x_type_now == SEXPTYPE::STRSXP {
            return format_character_vector(x, n, args);
        }
        for i in 0..n {
            let s = if TYPEOF(x) == SEXPTYPE::REALSXP {
                let v = *REAL(x).add(i as usize);
                if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    "NA".to_string()
                } else if nsmall > 0 {
                    format!("{:.*}", nsmall, v)
                } else {
                    format!("{}", v)
                }
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER {
                    "NA".to_string()
                } else {
                    format!("{}", v)
                }
            } else {
                elt_to_string(x, i)
            };
            let cstr = CString::new(s).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                crate::sexp::accessors::SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
        }
        result
    }
}

/// True when an enclosing function context is the `format` generic or
/// `format.default` (NextMethod keeps the generic-name sys.call, so method
/// frames invoked from the generic match too). GNU's .Internal(format(...))
/// invokes the C entry directly regardless of R-level bindings; S3 dispatch
/// already happened at the generic, so re-dispatching here would loop
/// format.AsIs -> format.default -> .Internal(format) -> format.AsIs.
unsafe fn called_from_format_generic_or_default() -> bool {
    unsafe {
        let mut ctx = crate::sexp::context::R_GlobalContext();
        while !ctx.is_null() {
            if (*ctx).callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION != 0 {
                let call = (*ctx).call.as_raw();
                if !call.is_null()
                    && TYPEOF(call) == SEXPTYPE::LANGSXP
                    && TYPEOF(CAR(call)) == SEXPTYPE::SYMSXP
                {
                    let pname = PRINTNAME(CAR(call));
                    if !pname.is_null() {
                        let name = std::ffi::CStr::from_ptr(CHAR(pname))
                            .to_string_lossy()
                            .into_owned();
                        if name == "format" || name == "format.default" {
                            return true;
                        }
                    }
                }
            }
            ctx = (*ctx).nextcontext;
        }
        false
    }
}

/// GNU `format.default` for atomic numeric vectors: shared width/digits
/// across the vector, honoring `digits`, `trim`, `nsmall`, `width`, and
/// `scientific` (TRUE/FALSE/NA/numeric → scipen -99/310/keep/value).
unsafe fn format_numeric_vector(x: SEXP, n: R_xlen_t, args: SEXP) -> SEXP {
    unsafe {
        let mut trim = false;
        let mut digits_opt: Option<c_int> = None;
        let mut nsmall: c_int = 0;
        let mut width: c_int = 0;
        let mut sci_opt: Option<c_int> = None;
        let mut big_mark = String::new();
        let mut drop0trailing = false;
        let mut zero_print: Option<String> = None;
        let mut decimal_mark = {
            let opt = crate::mainutils::options::GetOption(c"OutDec".as_ptr());
            if !opt.is_null()
                && opt != R_NilValue()
                && TYPEOF(opt) == SEXPTYPE::STRSXP
                && XLENGTH(opt) >= 1
            {
                elt_to_string(opt, 0)
            } else {
                ".".to_string()
            }
        };
        let mut small_mark = String::new();
        let mut small_interval: usize = 5;
        let mut positional = 0;
        let mut cell = crate::sexp::accessors::CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                crate::sexp::accessors::PRINTNAME(tag)
            } else {
                std::ptr::null_mut()
            };
            let label: Option<std::borrow::Cow<'_, str>> = if !name.is_null()
                && name != R_NilValue()
            {
                Some(std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(name)).to_string_lossy())
            } else {
                None
            };
            let slot = label
                .as_deref()
                .and_then(|label| match label {
                    "trim" => Some(0),
                    "digits" => Some(1),
                    "nsmall" => Some(2),
                    "justify" => Some(3),
                    "width" => Some(4),
                    "na.encode" => Some(5),
                    "scientific" => Some(6),
                    "big.mark" => Some(7),
                    "drop0trailing" => Some(8),
                    "zero.print" => Some(9),
                    "decimal.mark" => Some(10),
                    "small.mark" => Some(11),
                    "small.interval" => Some(12),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    // GNU .Internal(format): trim, digits, nsmall, width, justify,
                    // na.encode, scientific, decimal.mark.
                    let order = [0, 1, 2, 4, 3, 5, 6, 10];
                    let slot = order.get(positional).copied().unwrap_or(99);
                    positional += 1;
                    slot
                });
            match slot {
                0 => {
                    let flag = crate::main::coerce::asLogical(value);
                    trim = flag != 0;
                }
                1 => {
                    let d = crate::main::coerce::asInteger(value);
                    if d != NA_INTEGER {
                        digits_opt = Some(d);
                    }
                }
                2 => {
                    let v = crate::main::coerce::asInteger(value);
                    if v != NA_INTEGER && v >= 0 && v <= 20 {
                        nsmall = v;
                    }
                }
                4 => {
                    let v = crate::main::coerce::asInteger(value);
                    if v != NA_INTEGER {
                        width = v;
                    }
                }
                6 => {
                    // GNU do_format: TRUE -> scipen=-99, FALSE -> 310,
                    // NA logical leaves the option, numeric sets scipen.
                    if XLENGTH(value) != 1 {
                        crate::mainutils::errors::errorcall_str(
                            crate::mainutils::errors::R_getCurrentCall(),
                            "invalid 'scientific' argument",
                        );
                    }
                    if TYPEOF(value) == SEXPTYPE::LGLSXP {
                        let tmp = crate::main::coerce::asLogical(value);
                        if tmp != NA_LOGICAL {
                            sci_opt = Some(if tmp != 0 { -99 } else { 310 });
                        }
                    } else if matches!(TYPEOF(value), 13 | 14) {
                        sci_opt = Some(crate::main::coerce::asInteger(value));
                    } else {
                        crate::mainutils::errors::errorcall_str(
                            crate::mainutils::errors::R_getCurrentCall(),
                            "invalid 'scientific' argument",
                        );
                    }
                }
                7 => {
                    if TYPEOF(value) == SEXPTYPE::STRSXP && XLENGTH(value) >= 1 {
                        big_mark = elt_to_string(value, 0);
                    }
                }
                8 => {
                    drop0trailing = crate::main::coerce::asLogical(value) != 0;
                }
                9 => {
                    if value.is_null() || value == R_NilValue() {
                        zero_print = None;
                    } else if TYPEOF(value) == SEXPTYPE::LGLSXP {
                        let flag = crate::main::coerce::asLogical(value);
                        zero_print = Some(if flag != 0 {
                            "0".to_string()
                        } else {
                            " ".to_string()
                        });
                    } else if TYPEOF(value) == SEXPTYPE::STRSXP {
                        zero_print = Some(elt_to_string(value, 0));
                    }
                }
                10 => {
                    if TYPEOF(value) == SEXPTYPE::STRSXP && XLENGTH(value) >= 1 {
                        decimal_mark = elt_to_string(value, 0);
                    }
                }
                11 => {
                    if TYPEOF(value) == SEXPTYPE::STRSXP && XLENGTH(value) >= 1 {
                        small_mark = elt_to_string(value, 0);
                    }
                }
                12 => {
                    let v = crate::main::coerce::asInteger(value);
                    if v != NA_INTEGER && v > 0 {
                        small_interval = v as usize;
                    }
                }
                _ => {}
            }
            cell = CDR(cell);
        }
        if decimal_mark.len() != 1 {
            let which = if decimal_mark.len() > 1 {
                "more"
            } else {
                "less"
            };
            let msg = std::ffi::CString::new(format!(
                "the decimal mark is {which} than one character wide; this will become an error"
            ))
            .unwrap_or_default();
            crate::mainutils::errors::Rf_warning(msg.as_ptr());
        }

        // formatReal/formatComplex read the live digits option; honor an
        // explicit digits argument by swapping the option for this call.
        let digits = digits_opt.unwrap_or_else(|| crate::mainutils::options::GetOptionDigits());
        let digits_value = crate::sexp::constructors::Rf_ScalarInteger(digits);
        let _digits_value = protect(digits_value);
        let saved_digits = crate::mainutils::options::SetOptionByName("digits", digits_value);
        let saved_scipen = if let Some(sci) = sci_opt {
            if sci != NA_INTEGER {
                let sci_value = crate::sexp::constructors::Rf_ScalarInteger(sci);
                let _sci_value = protect(sci_value);
                Some(crate::mainutils::options::SetOptionByName(
                    "scipen", sci_value,
                ))
            } else {
                None
            }
        } else {
            None
        };
        let pinned_print =
            crate::mainutils::format::format_set_R_print(crate::mainutils::format::RPrint {
                digits,
                scipen: crate::mainutils::options::GetOptionScipen(),
                na_width: 2,
                na_width_noquote: 2,
            });

        // GNU do_format feeds decimal.mark to EncodeReal0 as OutDec.
        let outdec_owned =
            CString::new(decimal_mark.as_str()).unwrap_or_else(|_| CString::new(".").expect("dot"));
        let outdec = outdec_owned.as_ptr();
        let mut wr: c_int = 0;
        let mut dr: c_int = 0;
        let mut er: c_int = 0;
        let mut wi: c_int = 0;
        let mut di: c_int = 0;
        let mut ei: c_int = 0;
        let mut w: c_int = 0;
        match TYPEOF(x) {
            10 => {
                crate::mainutils::format::formatLogicalS(x, n, &mut w);
            }
            13 => {
                crate::mainutils::format::formatIntegerS(x, n, &mut w);
            }
            14 => {
                crate::mainutils::format::formatRealS(x, n, &mut wr, &mut dr, &mut er, nsmall);
                w = wr;
            }
            _ => {
                crate::mainutils::format::formatComplexS(
                    x, n, &mut wr, &mut dr, &mut er, &mut wi, &mut di, &mut ei, nsmall,
                );
                w = wr + wi + 2;
            }
        }
        if trim {
            w = 0;
        }
        if w < width {
            w = width;
        }

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _result_guard = protect(result);
        let mut encoded_strings = Vec::with_capacity(n as usize);
        for i in 0..n {
            let encoded = match TYPEOF(x) {
                10 => {
                    let v = crate::sexp::accessors::LOGICAL_ELT(x, i as c_int);
                    // trim=TRUE passes width 0. EncodeLogical treats that as
                    // an empty field; GNU still emits TRUE/FALSE/NA.
                    let field = if w > 0 {
                        w
                    } else if v == crate::sexp::ffi::NA_LOGICAL {
                        crate::mainutils::format::format_get_R_print().na_width
                    } else if v != 0 {
                        4
                    } else {
                        5
                    };
                    crate::mainutils::printutils::EncodeLogical(v, field)
                }
                13 => {
                    let v = crate::sexp::accessors::INTEGER_ELT(x, i as c_int);
                    crate::mainutils::printutils::EncodeInteger(v, w)
                }
                14 => {
                    let v = crate::sexp::accessors::REAL_ELT(x, i as c_int);
                    crate::mainutils::printutils::EncodeReal0(v, w, dr, er, outdec)
                }
                _ => {
                    let v = *crate::sexp::accessors::COMPLEX(x).add(i as usize);
                    // GNU encodes each part with its own width and then
                    // right-justifies to the total width; fusing that into a
                    // wider real field is equivalent only when w holds the
                    // combined width (trim=TRUE sets w = 0, so fall back to
                    // wr instead of underflowing the c_int).
                    let field_re = if w > wi + 2 { w - wi - 2 } else { wr };
                    crate::mainutils::printutils::EncodeComplex(
                        v, field_re, dr, er, wi, di, ei, outdec,
                    )
                }
            };
            encoded_strings.push(
                std::ffi::CStr::from_ptr(encoded)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        if !big_mark.is_empty()
            || drop0trailing
            || zero_print.is_some()
            || decimal_mark != "."
            || !small_mark.is_empty()
        {
            pretty_num_inplace(
                &mut encoded_strings,
                &big_mark,
                drop0trailing,
                zero_print.as_deref(),
                &decimal_mark,
                &small_mark,
                small_interval,
                trim,
            );
        }
        for (i, text) in encoded_strings.iter().enumerate() {
            let cstr = CString::new(text.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                crate::sexp::accessors::SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
        }
        copy_format_shape(x, result);

        crate::mainutils::format::format_set_R_print(pinned_print);
        crate::mainutils::options::SetOptionByName("digits", saved_digits);
        if let Some(old) = saved_scipen {
            crate::mainutils::options::SetOptionByName("scipen", old);
        }
        result
    }
}

fn pretty_num_inplace(
    strings: &mut [String],
    big_mark: &str,
    drop0trailing: bool,
    zero_print: Option<&str>,
    decimal_mark: &str,
    small_mark: &str,
    small_interval: usize,
    trim: bool,
) {
    let before: Vec<usize> = strings.iter().map(|s| s.chars().count()).collect();
    for s in strings.iter_mut() {
        if s.trim() == "NA" || s.trim() == "NaN" || s.trim() == "Inf" || s.trim() == "-Inf" {
            continue;
        }
        *s = pretty_num_one(
            s,
            big_mark,
            drop0trailing,
            decimal_mark,
            small_mark,
            small_interval,
        );
    }
    if let Some(zero) = zero_print {
        for s in strings.iter_mut() {
            if pretty_num_is_zero(s) {
                *s = format_zero_print(s, zero);
            }
        }
    }
    if !trim
        && strings
            .iter()
            .zip(&before)
            .any(|(s, old)| s.chars().count() > *old)
    {
        let max_w = strings.iter().map(|s| s.chars().count()).max().unwrap_or(0);
        for s in strings.iter_mut() {
            let len = s.chars().count();
            if len < max_w {
                *s = format!("{}{s}", " ".repeat(max_w - len));
            }
        }
    }
}

fn pretty_num_one(
    s: &str,
    big_mark: &str,
    drop0trailing: bool,
    decimal_mark: &str,
    small_mark: &str,
    small_interval: usize,
) -> String {
    let leading = s.chars().take_while(|c| *c == ' ').count();
    let body = s.trim_start();
    let (sign, rest) = if let Some(stripped) = body.strip_prefix('-') {
        ("-", stripped)
    } else if let Some(stripped) = body.strip_prefix('+') {
        ("+", stripped)
    } else {
        ("", body)
    };
    let (int_part, frac_exp) = rest
        .split_once(decimal_mark)
        .or_else(|| rest.split_once('.'))
        .map(|(int_part, rest)| (int_part, Some(rest)))
        .unwrap_or((rest, None));
    let (mut frac, exp) = match frac_exp {
        Some(rest) => match rest.find(['e', 'E']) {
            Some(at) => (rest[..at].to_string(), Some(&rest[at..])),
            None => (rest.to_string(), None),
        },
        None => match rest.find(['e', 'E']) {
            Some(at) => {
                let (int_only, exp) = rest.split_at(at);
                return pretty_num_join(
                    leading,
                    sign,
                    &insert_big_mark(int_only, big_mark),
                    None,
                    Some(exp),
                    drop0trailing,
                    decimal_mark,
                );
            }
            None => (String::new(), None),
        },
    };
    if drop0trailing {
        while frac.ends_with('0') {
            frac.pop();
        }
        if let Some(e) = exp {
            if e.bytes()
                .skip(1)
                .all(|b| b == b'+' || b == b'-' || b == b'0')
            {
                let marked = insert_small_mark(&frac, small_mark, small_interval);
                return pretty_num_join(
                    leading,
                    sign,
                    &insert_big_mark(int_part, big_mark),
                    if marked.is_empty() {
                        None
                    } else {
                        Some(&marked)
                    },
                    None,
                    drop0trailing,
                    decimal_mark,
                );
            }
        }
    }
    let marked = insert_small_mark(&frac, small_mark, small_interval);
    pretty_num_join(
        leading,
        sign,
        &insert_big_mark(int_part, big_mark),
        if marked.is_empty() {
            None
        } else {
            Some(&marked)
        },
        exp,
        drop0trailing,
        decimal_mark,
    )
}

fn pretty_num_join(
    leading: usize,
    sign: &str,
    int_part: &str,
    frac: Option<&str>,
    exp: Option<&str>,
    drop0trailing: bool,
    decimal_mark: &str,
) -> String {
    let mut out = " ".repeat(leading);
    out.push_str(sign);
    out.push_str(int_part);
    match frac {
        Some(frac) => {
            out.push_str(decimal_mark);
            out.push_str(frac);
        }
        None if !drop0trailing => {}
        None => {}
    }
    if let Some(exp) = exp {
        out.push_str(exp);
    }
    out
}

fn insert_small_mark(frac: &str, mark: &str, interval: usize) -> String {
    if mark.is_empty() || interval == 0 {
        return frac.to_string();
    }
    let digits: Vec<char> = frac.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() <= interval {
        return frac.to_string();
    }
    let mut out = String::new();
    for (i, ch) in digits.iter().enumerate() {
        out.push(*ch);
        if (i + 1) % interval == 0 && i + 1 < digits.len() {
            out.push_str(mark);
        }
    }
    out
}

fn insert_big_mark(int_part: &str, mark: &str) -> String {
    if mark.is_empty() {
        return int_part.to_string();
    }
    let digits: String = int_part.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() <= 3 {
        return int_part.to_string();
    }
    let prefix: String = int_part
        .chars()
        .take_while(|c| !c.is_ascii_digit())
        .collect();
    let mut grouped = String::new();
    for (i, ch) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.push_str(mark);
        }
        grouped.push(ch);
    }
    let grouped: String = grouped.chars().rev().collect();
    format!("{prefix}{grouped}")
}

fn pretty_num_is_zero(s: &str) -> bool {
    let t = s.trim();
    let t = t.trim_start_matches('+').trim_start_matches('-');
    if t.is_empty() {
        return false;
    }
    let (mant, exp) = match t.find(['e', 'E']) {
        Some(at) => (&t[..at], &t[at + 1..]),
        None => (t, ""),
    };
    if !exp.is_empty() && !exp.bytes().all(|b| b == b'+' || b == b'-' || b == b'0') {
        return false;
    }
    mant.chars().all(|c| c == '0' || c == '.')
}

fn format_zero_print(original: &str, zero: &str) -> String {
    // GNU .format.zeros keeps width by overwriting the first '0'.
    let mut chars: Vec<char> = original.chars().collect();
    let Some(first0) = chars.iter().position(|c| *c == '0') else {
        return zero.to_string();
    };
    let z: Vec<char> = zero.chars().collect();
    let start = first0.saturating_sub(z.len().saturating_sub(1));
    for (i, ch) in z.iter().enumerate() {
        let at = start + i;
        if at < chars.len() {
            chars[at] = *ch;
        }
    }
    for ch in chars.iter_mut().skip(start + z.len()) {
        if *ch == '0' || *ch == '.' {
            *ch = ' ';
        }
    }
    chars.into_iter().collect()
}

unsafe fn format_character_vector(x: SEXP, n: R_xlen_t, args: SEXP) -> SEXP {
    unsafe {
        // GNU format.default: match.arg(justify) defaults to "left" (0).
        // 0=left, 1=right, 2=centre, 3=none.
        let mut justify: c_int = 0;
        let mut width: c_int = 0;
        let mut na_encode = true;
        let mut positional = 0;
        let mut cell = crate::sexp::accessors::CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                crate::sexp::accessors::PRINTNAME(tag)
            } else {
                std::ptr::null_mut()
            };
            let label: Option<std::borrow::Cow<'_, str>> = if !name.is_null()
                && name != R_NilValue()
            {
                Some(std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(name)).to_string_lossy())
            } else {
                None
            };
            let slot = label
                .as_deref()
                .and_then(|label| match label {
                    "trim" => Some(0),
                    "digits" => Some(1),
                    "nsmall" => Some(2),
                    "justify" => Some(3),
                    "width" => Some(4),
                    "na.encode" => Some(5),
                    "scientific" => Some(6),
                    "decimal.mark" => Some(7),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    if label.is_some() {
                        return 99;
                    }
                    let order = [0, 1, 2, 4, 3, 5, 6, 7];
                    let slot = order.get(positional).copied().unwrap_or(99);
                    positional += 1;
                    slot
                });
            match slot {
                3 => {
                    if TYPEOF(value) == SEXPTYPE::STRSXP {
                        // GNU format.default: justify <- match.arg(justify) against
                        // c("left", "right", "centre", "none") — prefix matching
                        // applies; unknown/ambiguous values are an error.
                        const JUSTIFY_CHOICES: [&str; 4] = ["left", "right", "centre", "none"];
                        let text = elt_to_string(value, 0);
                        let mut exact: Option<usize> = None;
                        let mut prefixes: Vec<usize> = Vec::new();
                        for (i, choice) in JUSTIFY_CHOICES.iter().enumerate() {
                            if *choice == text {
                                exact = Some(i);
                                break;
                            }
                            if choice.starts_with(text.as_str()) {
                                prefixes.push(i);
                            }
                        }
                        let idx = exact.or_else(|| {
                            if prefixes.len() == 1 {
                                prefixes.pop()
                            } else {
                                None
                            }
                        });
                        match idx {
                            Some(i) => justify = i as c_int,
                            None => {
                                let list = JUSTIFY_CHOICES
                                    .iter()
                                    .map(|c| format!("\"{c}\""))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                crate::mainutils::errors::errorcall_str(
                                    crate::mainutils::errors::R_getCurrentCall(),
                                    &format!("'arg' should be one of {list}"),
                                );
                            }
                        }
                    } else {
                        let v = crate::main::coerce::asInteger(value);
                        if v != NA_INTEGER && (0..=3).contains(&v) {
                            justify = v;
                        }
                    }
                }
                4 => {
                    let v = crate::main::coerce::asInteger(value);
                    if v != NA_INTEGER {
                        width = v;
                    }
                }
                5 => na_encode = crate::main::coerce::asLogical(value) != FALSE,
                _ => {}
            }
            cell = CDR(cell);
        }

        let mut strings = Vec::with_capacity(n as usize);
        let mut max_w = 0usize;
        for i in 0..n {
            if !na_encode && STRING_ELT(x, i) == crate::sexp::globals::R_NaString() {
                strings.push(None);
                continue;
            }
            let s = elt_to_string(x, i);
            max_w = max_w.max(s.chars().count());
            strings.push(Some(s));
        }
        let field = if width > 0 {
            (width as usize).max(max_w)
        } else {
            max_w
        };
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, s) in strings.iter().enumerate() {
            let Some(s) = s else {
                crate::sexp::accessors::SET_STRING_ELT(
                    result,
                    i as R_xlen_t,
                    crate::sexp::globals::R_NaString(),
                );
                continue;
            };
            let out = if justify == 3 {
                s.clone()
            } else {
                justify_pad(s, field, justify)
            };
            let cstr = CString::new(out).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                crate::sexp::accessors::SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
        }
        copy_format_shape(x, result);
        result
    }
}

/// GNU format retains names and array shape, excluding class and unrelated
/// attributes. Character formatting follows the same contract as numeric.
unsafe fn copy_format_shape(x: SEXP, result: SEXP) {
    unsafe {
        use crate::sexp::attrib_core::{
            R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib,
        };
        for symbol in [R_NamesSymbol(), R_DimSymbol(), R_DimNamesSymbol()] {
            let value = getAttrib(x, symbol);
            if !value.is_null() && value != R_NilValue() {
                setAttrib(result, symbol, value);
            }
        }
    }
}

fn justify_pad(s: &str, field: usize, justify: c_int) -> String {
    let len = s.chars().count();
    if len >= field {
        return s.to_string();
    }
    let pad = field - len;
    match justify {
        1 => format!("{}{s}", " ".repeat(pad)),
        2 => {
            let left = pad / 2;
            format!("{}{s}{}", " ".repeat(left), " ".repeat(pad - left))
        }
        _ => format!("{s}{}", " ".repeat(pad)),
    }
}

#[derive(Clone, Copy)]
enum CalendarLabel {
    Weekday,
    Month,
    Quarter,
}

unsafe fn calendar_days_from_element(x: SEXP, i: R_xlen_t) -> Option<f64> {
    unsafe {
        if TYPEOF(x) != SEXPTYPE::REALSXP {
            return None;
        }
        let value = *REAL(x).add(i as usize);
        if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || !value.is_finite() {
            return None;
        }
        if sexp_has_class(x, "POSIXct") {
            Some((value / 86_400.0).floor())
        } else if sexp_has_class(x, "Date") {
            Some(value.floor())
        } else {
            None
        }
    }
}

fn calendar_label(days: f64, kind: CalendarLabel) -> Option<String> {
    const WEEKDAYS: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];

    let (_, month, _) = date_days_to_civil(days)?;
    match kind {
        CalendarLabel::Weekday => {
            let day_index = ((days.floor() as i64) + 4).rem_euclid(7) as usize;
            Some(WEEKDAYS[day_index].to_string())
        }
        CalendarLabel::Month => Some(MONTHS[(month - 1) as usize].to_string()),
        CalendarLabel::Quarter => Some(format!("Q{}", (month - 1) / 3 + 1)),
    }
}

unsafe fn calendar_label_builtin(args: SEXP, kind: CalendarLabel) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        if TYPEOF(x) != SEXPTYPE::REALSXP
            || (!sexp_has_class(x, "Date") && !sexp_has_class(x, "POSIXct"))
        {
            base_error("no applicable method");
        }

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        for i in 0..n {
            let label = calendar_days_from_element(x, i)
                .and_then(|days| calendar_label(days, kind))
                .or_else(|| matches!(kind, CalendarLabel::Quarter).then(|| "QNA".to_string()));
            let charsxp = label
                .and_then(|label| CString::new(label).ok())
                .map(|label| Rf_mkChar(label.as_ptr()))
                .unwrap_or_else(|| crate::sexp::globals::R_NaString());
            SET_STRING_ELT(result, i, charsxp);
        }
        result
    }
}

/// R's `weekdays(x)` for Date/POSIXct values.
pub unsafe fn do_weekdays(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { calendar_label_builtin(args, CalendarLabel::Weekday) }
}

/// R's `months(x)` for Date/POSIXct values.
pub unsafe fn do_months(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { calendar_label_builtin(args, CalendarLabel::Month) }
}

/// R's `quarters(x)` for Date/POSIXct values.
pub unsafe fn do_quarters(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { calendar_label_builtin(args, CalendarLabel::Quarter) }
}
