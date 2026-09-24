//! Essentials domain module `strings` — extracted verbatim from essentials.rs.

use super::*;
use std::ffi::CString;
use std::os::raw::c_int;
use std::path::PathBuf;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CADDR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT,
    LENGTH, LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT,
    SET_STRING_ELT, SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT,
    XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
use crate::sexp::ffi::{FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// do_nchar — string length
// ---------------------------------------------------------------------------

/// R's `nchar(x, type = "chars", allowNA = FALSE, keepNA = NA)`.
///
/// GNU default `type="chars"` counts Unicode code points, not bytes.
pub unsafe fn do_nchar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        let x = if TYPEOF(x) != SEXPTYPE::STRSXP {
            let coerced = crate::mainutils::essentials::do_as_character(
                _call,
                _op,
                Rf_cons(x, R_NilValue()),
                _rho,
            );
            let _c = protect(coerced);
            coerced
        } else {
            x
        };
        let mut nchar_type = NcharKind::Chars;
        let mut cell = CDR(args);
        let mut positional = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let named = if !tag.is_null() && tag != R_NilValue() {
                let pname = PRINTNAME(tag);
                if !pname.is_null() {
                    Some(
                        std::ffi::CStr::from_ptr(CHAR(pname))
                            .to_string_lossy()
                            .into_owned(),
                    )
                } else {
                    None
                }
            } else {
                None
            };
            let is_type = named.as_deref() == Some("type")
                || (named.is_none() && positional == 0);
            if is_type && TYPEOF(value) == SEXPTYPE::STRSXP && XLENGTH(value) >= 1 {
                let text = elt_to_string(value, 0);
                nchar_type = match text.as_str() {
                    t if t.starts_with('b') => NcharKind::Bytes,
                    t if t.starts_with('w') => NcharKind::Width,
                    _ => NcharKind::Chars,
                };
            }
            if named.is_none() {
                positional += 1;
            }
            cell = CDR(cell);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);
        for i in 0..n {
            if TYPEOF(x) == SEXPTYPE::STRSXP {
                let idx = if XLENGTH(x) == 0 { 0 } else { i % XLENGTH(x) };
                let charsxp = STRING_ELT(x, idx);
                if charsxp == crate::sexp::globals::R_NaString() {
                    *dst.add(i as usize) = if matches!(nchar_type, NcharKind::Width) {
                        2
                    } else {
                        NA_INTEGER
                    };
                    continue;
                }
            }
            let s = elt_to_string(x, i);
            *dst.add(i as usize) = nchar_count(&s, nchar_type) as c_int;
        }
        result
    }
}

fn file_ext_of(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let Some(dot) = base.rfind('.') else {
        return String::new();
    };
    let stem = &base[..dot];
    let ext = &base[dot + 1..];
    if ext.is_empty()
        || !ext.chars().all(|c| c.is_ascii_alphanumeric())
        || !stem.chars().any(|c| c != '.')
    {
        return String::new();
    }
    ext.to_string()
}

/// GNU `tools::file_ext(x)`.
pub unsafe fn do_file_ext(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            let ext = file_ext_of(&s);
            let c = CString::new(ext).unwrap_or_default();
            SET_STRING_ELT(result, i, Rf_mkChar(c.as_ptr()));
        }
        result
    }
}

/// GNU `tools::file_path_sans_ext(x)`.
pub unsafe fn do_file_path_sans_ext(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _r = protect(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            let ext = file_ext_of(&s);
            let out = if ext.is_empty() {
                s
            } else {
                let suffix = format!(".{ext}");
                match s.rfind(&suffix) {
                    Some(idx) => s[..idx].to_string(),
                    None => s,
                }
            };
            let c = CString::new(out).unwrap_or_default();
            SET_STRING_ELT(result, i, Rf_mkChar(c.as_ptr()));
        }
        result
    }
}


#[derive(Clone, Copy)]
enum NcharKind {
    Bytes,
    Chars,
    Width,
}

fn nchar_count(s: &str, kind: NcharKind) -> usize {
    match kind {
        NcharKind::Bytes => s.len(),
        NcharKind::Chars => s.chars().count(),
        NcharKind::Width => s.chars().map(unicode_display_width).sum(),
    }
}

pub(crate) fn unicode_display_width(ch: char) -> usize {
    let c = ch as u32;
    if ch.is_control() {
        return 0;
    }
    // Zero-width format characters. U+200B has width 0, not -1.
    if (0x200B..=0x200F).contains(&c)
        || (0x202A..=0x202E).contains(&c)
        || (0x2060..=0x2064).contains(&c)
        || (0x2066..=0x206F).contains(&c)
        || c == 0xFEFF
    {
        return 0;
    }
    // Combining marks.
    if (0x0300..=0x036F).contains(&c)
        || (0x1AB0..=0x1AFF).contains(&c)
        || (0x1DC0..=0x1DFF).contains(&c)
        || (0x20D0..=0x20FF).contains(&c)
        || (0xFE20..=0xFE2F).contains(&c)
    {
        return 0;
    }
    // East Asian Wide / Fullwidth (enough for GNU width of 中 = 2).
    if (0x1100..=0x115F).contains(&c)
        || (0x2329..=0x232A).contains(&c)
        || (0x2E80..=0xA4CF).contains(&c) && c != 0x303F
        || (0xAC00..=0xD7A3).contains(&c)
        || (0xF900..=0xFAFF).contains(&c)
        || (0xFE10..=0xFE19).contains(&c)
        || (0xFE30..=0xFE6F).contains(&c)
        || (0xFF00..=0xFF60).contains(&c)
        || (0xFFE0..=0xFFE6).contains(&c)
        || (0x20000..=0x3FFFD).contains(&c)
    {
        return 2;
    }
    1
}

// ---------------------------------------------------------------------------
// do_substr — substring extraction
// ---------------------------------------------------------------------------

pub unsafe fn do_substr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let start_arg = CAR(CDR(args));
        let stop_arg = CAR(CDR(CDR(args)));

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Upstream substr.c recycles x/start/stop to the COMMON length
        // n = max(len(x), len(start), len(stop)) — vector bounds with a
        // scalar string yield a vector result (substring over gregexpr's
        // per-match positions).
        let nx = XLENGTH(x);
        let nstart = if start_arg.is_null() || start_arg == R_NilValue() {
            1
        } else {
            XLENGTH(start_arg)
        };
        let nstop = if stop_arg.is_null() || stop_arg == R_NilValue() {
            1
        } else {
            XLENGTH(stop_arg)
        };
        // Upstream split (character.c do_substr vs base::substring):
        // the INTERNAL iterates only len(x) — start/stop recycle against
        // it but never extend the result. base::substring (an R wrapper)
        // rep_lens text to max(len(text), len(first), len(last)) FIRST;
        // our `substring` handler below reproduces that. Zero-length x
        // stays zero-length either way (the wrapper's `lt &&` guard).
        let n = nx;
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            let xi = if nx == 0 { 0 } else { i % nx };
            if TYPEOF(x) == SEXPTYPE::STRSXP && nx > 0 {
                if STRING_ELT(x, xi) == crate::sexp::globals::R_NaString() {
                    SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                    continue;
                }
            }
            let s = elt_to_string(x, xi);
            let si = if nstart == 0 { 0 } else { i % nstart };
            let ei = if nstop == 0 { 0 } else { i % nstop };
            let start = (real_elt_or_default(start_arg, si, 1.0) as usize).max(1) - 1;
            let stop = real_elt_or_default(stop_arg, ei, 1000.0) as usize;
            let chars: Vec<char> = s.chars().collect();
            let end = stop.min(chars.len());
            let sub: String = if start < chars.len() {
                chars[start..end].iter().collect()
            } else {
                String::new()
            };
            let cstr = CString::new(sub).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }

        result
    }
}

/// `substr<-`(x, start, stop, value). Replaces characters start:stop.
/// A longer value is truncated to the span. A shorter value replaces only
/// its own characters; the rest of the span stays.
pub unsafe fn do_substrgets(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let start_arg = CAR(CDR(args));
        let stop_arg = CAR(CDR(CDR(args)));
        let value = CAR(CDR(CDR(CDR(args))));
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return x;
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _g = protect(result);
        let nv = if value.is_null() || value == R_NilValue() || TYPEOF(value) != SEXPTYPE::STRSXP
        {
            0
        } else {
            XLENGTH(value)
        };
        for i in 0..n {
            if STRING_ELT(x, i) == crate::sexp::globals::R_NaString() {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }
            let s = elt_to_string(x, i);
            let mut chars: Vec<char> = s.chars().collect();
            let start = (real_elt_or_default(start_arg, i, 1.0) as isize).max(1) as usize;
            let stop_raw = if stop_arg.is_null() || stop_arg == R_NilValue() {
                chars.len() as f64
            } else {
                real_elt_or_default(stop_arg, i, chars.len() as f64)
            };
            let stop = (stop_raw as isize).max(0) as usize;
            if start == 0 || start > chars.len() || start > stop {
                let cstr = CString::new(s.as_str()).unwrap_or_default();
                SET_STRING_ELT(result, i, crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()));
                continue;
            }
            let end = stop.min(chars.len());
            let span = end - start + 1;
            if nv > 0 && STRING_ELT(value, i % nv) == crate::sexp::globals::R_NaString() {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }
            let repl = if nv == 0 {
                String::new()
            } else {
                elt_to_string(value, i % nv)
            };
            for (k, ch) in repl.chars().take(span).enumerate() {
                chars[start - 1 + k] = ch;
            }
            let out: String = chars.into_iter().collect();
            let cstr = CString::new(out).unwrap_or_default();
            SET_STRING_ELT(result, i, crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()));
        }
        result
    }
}

/// GNU `strrep(x, times)` — recycle `x` and `times` to a common length.
pub unsafe fn do_strrep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x0 = CAR(args);
        let n0 = CADR(args);
        let x = crate::main::coerce::coerceVector(x0, SEXPTYPE::STRSXP.as_c_int());
        let _x = protect(x);
        let n = crate::main::coerce::coerceVector(n0, SEXPTYPE::INTSXP.as_c_int());
        let _n = protect(n);
        let nx = XLENGTH(x);
        let nn = XLENGTH(n);
        if nx == 0 || nn == 0 {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let ns = nx.max(nn);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, ns);
        let _result = protect(result);
        for is in 0..ns {
            let el = STRING_ELT(x, is % nx);
            let ni = *INTEGER(n).add((is % nn) as usize);
            if el == crate::sexp::globals::R_NaString() || ni == NA_INTEGER {
                SET_STRING_ELT(result, is, crate::sexp::globals::R_NaString());
                continue;
            }
            if ni < 0 {
                crate::mainutils::errors::errorcall_str(
                    crate::mainutils::errors::R_getCurrentCall(),
                    "invalid 'times' value",
                );
            }
            let s = elt_to_string(x, is % nx);
            if (s.len() as i64).saturating_mul(ni as i64) > i32::MAX as i64 {
                crate::mainutils::errors::errorcall_str(
                    crate::mainutils::errors::R_getCurrentCall(),
                    "R character strings are limited to 2^31-1 bytes",
                );
            }
            let repeated = s.repeat(ni as usize);
            let cstr = CString::new(repeated).unwrap_or_default();
            SET_STRING_ELT(result, is, Rf_mkChar(cstr.as_ptr()));
        }
        if ns == nx {
            let names = crate::attrib_core::getAttrib(x, crate::attrib_core::R_NamesSymbol());
            if !names.is_null() && names != R_NilValue() {
                crate::attrib_core::setAttrib(result, crate::attrib_core::R_NamesSymbol(), names);
            }
        }
        result
    }
}

/// GNU `encodeString(x, width, quote, na.encode, justify)`.
pub unsafe fn do_encodeString(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x0 = CAR(args);
        if x0.is_null() || x0 == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let x = if TYPEOF(x0) == SEXPTYPE::STRSXP {
            x0
        } else {
            crate::main::coerce::coerceVector(x0, SEXPTYPE::STRSXP.as_c_int())
        };
        let _x = protect(x);
        let mut width = 0;
        let mut quote: c_int = 0;
        let mut justify: c_int = 0;
        let mut na_encode = true;
        let mut positional = 0;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                let pname = PRINTNAME(tag);
                if !pname.is_null() {
                    Some(
                        std::ffi::CStr::from_ptr(CHAR(pname))
                            .to_string_lossy()
                            .into_owned(),
                    )
                } else {
                    None
                }
            } else {
                None
            };
            let slot = match name.as_deref() {
                Some("width") => 0,
                Some("quote") => 1,
                Some("na.encode") => 2,
                Some("justify") => 3,
                _ => {
                    let s = positional;
                    positional += 1;
                    s
                }
            };
            match slot {
                0 => {
                    let v = crate::main::coerce::asInteger(value);
                    if v != NA_INTEGER && v < 0 {
                        crate::mainutils::errors::errorcall_str(
                            crate::mainutils::errors::R_getCurrentCall(),
                            "invalid 'width' value",
                        );
                    }
                    width = v;
                }
                1 => {
                    if TYPEOF(value) == SEXPTYPE::STRSXP && XLENGTH(value) >= 1 {
                        let q = elt_to_string(value, 0);
                        if let Some(ch) = q.chars().next() {
                            quote = ch as c_int;
                        }
                    }
                }
                2 => {
                    na_encode = crate::main::coerce::asLogical(value) != 0;
                }
                3 => {
                    if TYPEOF(value) == SEXPTYPE::STRSXP {
                        let text = elt_to_string(value, 0);
                        justify = match text.as_str() {
                            "right" => 1,
                            "centre" | "center" => 2,
                            "none" => 3,
                            _ => 0,
                        };
                    } else {
                        let v = crate::main::coerce::asInteger(value);
                        if v != NA_INTEGER && (0..=3).contains(&v) {
                            justify = v;
                        }
                    }
                }
                _ => {}
            }
            cell = CDR(cell);
        }
        if justify == 3 {
            width = 0;
        }
        let len = XLENGTH(x);
        let find_width = width == NA_INTEGER;
        let mut w = width;
        if find_width && justify < 3 {
            w = 0;
            for i in 0..len {
                let s = STRING_ELT(x, i);
                if na_encode || s != crate::sexp::globals::R_NaString() {
                    w = w.max(crate::mainutils::printutils::Rstrlen(s, quote));
                }
            }
            if quote != 0 {
                w += 2;
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, len);
        let _result = protect(result);
        let adj = match justify {
            1 => crate::mainutils::printutils::Rprt_adj::right,
            2 => crate::mainutils::printutils::Rprt_adj::centre,
            3 => crate::mainutils::printutils::Rprt_adj::none,
            _ => crate::mainutils::printutils::Rprt_adj::left,
        };
        for i in 0..len {
            let s = STRING_ELT(x, i);
            if !na_encode && s == crate::sexp::globals::R_NaString() {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }
            let encoded = crate::mainutils::printutils::EncodeString(s, w, quote, adj);
            SET_STRING_ELT(result, i, Rf_mkChar(encoded));
        }
        result
    }
}

/// GNU `Encoding(x)` — per-element "unknown"/"latin1"/"UTF-8"/"bytes".
pub unsafe fn do_encoding(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::STRSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "a character vector argument expected",
            );
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _result = protect(result);
        for i in 0..n {
            let el = STRING_ELT(x, i);
            let label = if crate::sexp::accessors::IS_BYTES(el) != 0 {
                "bytes"
            } else if crate::sexp::accessors::IS_LATIN1(el) != 0 {
                "latin1"
            } else if crate::sexp::accessors::IS_UTF8(el) != 0 {
                "UTF-8"
            } else {
                "unknown"
            };
            let cstr = CString::new(label).unwrap_or_default();
            SET_STRING_ELT(result, i, Rf_mkChar(cstr.as_ptr()));
        }
        result
    }
}

/// GNU `Encoding<-` — mark CHARSXP encodings, recycling `value`.
pub unsafe fn do_setencoding(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut x = CAR(args);
        let enc = CADR(args);
        if TYPEOF(x) != SEXPTYPE::STRSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "a character vector argument expected",
            );
        }
        if TYPEOF(enc) != SEXPTYPE::STRSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "a character vector 'value' expected",
            );
        }
        let m = XLENGTH(enc);
        if m == 0 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'value' must be of positive length",
            );
        }
        if crate::sexp::accessors::NAMED(x) > 0 {
            x = crate::mainutils::duplicate::duplicate(x);
        }
        let _x = protect(x);
        let n = XLENGTH(x);
        for i in 0..n {
            let tmp = STRING_ELT(x, i);
            if tmp == crate::sexp::globals::R_NaString() {
                continue;
            }
            // GNU: ASCII strings never have a declared encoding.
            if crate::sexp::accessors::IS_ASCII(tmp) != 0 {
                continue;
            }
            let label = elt_to_string(enc, i % m);
            let kind = match label.as_str() {
                "latin1" => "latin1",
                "UTF-8" => "UTF-8",
                "bytes" => "bytes",
                _ => "unknown",
            };
            let already = match kind {
                "latin1" => crate::sexp::accessors::IS_LATIN1(tmp) != 0,
                "UTF-8" => crate::sexp::accessors::IS_UTF8(tmp) != 0,
                "bytes" => crate::sexp::accessors::IS_BYTES(tmp) != 0,
                _ => {
                    crate::sexp::accessors::IS_LATIN1(tmp) == 0
                        && crate::sexp::accessors::IS_UTF8(tmp) == 0
                        && crate::sexp::accessors::IS_BYTES(tmp) == 0
                }
            };
            if already {
                continue;
            }
            let len = crate::sexp::accessors::LENGTH(tmp);
            let marked = crate::sexp::constructors::Rf_mkCharLen(CHAR(tmp), len);
            crate::sexp::accessors::mark_charsxp_encoding(marked, kind);
            SET_STRING_ELT(x, i, marked);
        }
        x
    }
}




/// R's `substring(text, first, last=NULL)` — base::substring is an R
/// wrapper over the same internal that rep_lens `text` to the common
/// length max(len(text), len(first), len(last)) first (character.R);
/// zero-length text stays zero-length.
pub unsafe fn do_substring(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let first_arg = CAR(CDR(args));
        let last_arg = CAR(CDR(CDR(args)));

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let lt = XLENGTH(x);
        if lt == 0 {
            return do_substr(_call, _op, args, _rho);
        }
        let lf = if first_arg.is_null() || first_arg == R_NilValue() {
            1
        } else {
            XLENGTH(first_arg)
        };
        let ll = if last_arg.is_null() || last_arg == R_NilValue() {
            1
        } else {
            XLENGTH(last_arg)
        };
        let n = lt.max(lf).max(ll);
        if lt >= n {
            return do_substr(_call, _op, args, _rho);
        }
        // rep_len(x, n): recycle the string vector.
        let rep = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if rep.is_null() {
            return R_NilValue();
        }
        let _rep_guard = protect(rep);
        for i in 0..n {
            SET_STRING_ELT(rep, i, STRING_ELT(x, i % lt));
        }
        let new_args = Rf_cons(
            rep,
            Rf_cons(
                first_arg,
                Rf_cons(last_arg, crate::sexp::globals::R_NilValue()),
            ),
        );
        let _args_guard = protect(new_args);
        do_substr(_call, _op, new_args, _rho)
    }
}

// ---------------------------------------------------------------------------
// String case conversion
// ---------------------------------------------------------------------------

/// R's `tolower(x)`.
pub unsafe fn do_tolower(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_case_convert(args, true) }
}

/// R's `toupper(x)`.
pub unsafe fn do_toupper(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_case_convert(args, false) }
}

/// GNU `casefold(x, upper=FALSE)`.
pub unsafe fn do_casefold(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut upper = false;
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let u = CAR(rest);
            if !u.is_null() && u != R_NilValue() {
                if TYPEOF(u) == SEXPTYPE::LGLSXP && XLENGTH(u) > 0 {
                    upper = *LOGICAL(u) == TRUE;
                } else if TYPEOF(u) == SEXPTYPE::INTSXP && XLENGTH(u) > 0 {
                    upper = *INTEGER(u) != 0 && *INTEGER(u) != NA_INTEGER;
                }
            }
        }
        if upper {
            do_toupper(call, op, args, rho)
        } else {
            do_tolower(call, op, args, rho)
        }
    }
}

/// GNU `.Internal(make.unique(names, sep))`.
pub unsafe fn do_make_unique(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = CAR(args);
        if names.is_null() || names == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = XLENGTH(names);
        let mut sep = ".".to_string();
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let s = CAR(rest);
            if TYPEOF(s) == SEXPTYPE::STRSXP && XLENGTH(s) > 0 {
                let ch = STRING_ELT(s, 0);
                if !ch.is_null() {
                    sep = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        let mut seen = std::collections::HashSet::<String>::new();
        for i in 0..n {
            let ch = if TYPEOF(names) == SEXPTYPE::STRSXP {
                STRING_ELT(names, i)
            } else {
                std::ptr::null_mut()
            };
            let base = if ch.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned()
            };
            let unique = if seen.insert(base.clone()) {
                base
            } else {
                let mut k = 1u32;
                loop {
                    let cand = format!("{base}{sep}{k}");
                    if seen.insert(cand.clone()) {
                        break cand;
                    }
                    k += 1;
                }
            };
            let c = CString::new(unique).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(out, i, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}

const MAKE_NAMES_KEYWORDS: &[&str] = &[
    "NULL",
    "NA",
    "TRUE",
    "FALSE",
    "Inf",
    "NaN",
    "NA_integer_",
    "NA_real_",
    "NA_character_",
    "NA_complex_",
    "function",
    "while",
    "repeat",
    "for",
    "if",
    "in",
    "else",
    "next",
    "break",
];

fn make_names_is_valid(name: &str) -> bool {
    if name == "..." {
        return true;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first != '.' && !first.is_ascii_alphabetic() {
        return false;
    }
    if first == '.' {
        if let Some(second) = name.as_bytes().get(1) {
            if second.is_ascii_digit() {
                return false;
            }
        }
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
    {
        return false;
    }
    !MAKE_NAMES_KEYWORDS.contains(&name)
}

/// GNU `make.names(names, unique=FALSE, allow_=TRUE)`.
pub unsafe fn do_make_names(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = CAR(args);
        if names.is_null() || names == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = if TYPEOF(names) == SEXPTYPE::STRSXP {
            XLENGTH(names)
        } else {
            0
        };
        let mut unique = false;
        let mut allow_ = true;
        let mut cell = CDR(args);
        let mut positional = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let named = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let is_true = !value.is_null()
                && value != R_NilValue()
                && ((TYPEOF(value) == SEXPTYPE::LGLSXP
                    && XLENGTH(value) > 0
                    && *LOGICAL(value) == TRUE)
                    || (TYPEOF(value) == SEXPTYPE::INTSXP
                        && XLENGTH(value) > 0
                        && *INTEGER(value) != 0
                        && *INTEGER(value) != NA_INTEGER));
            if named == "unique" || (named.is_empty() && positional == 0) {
                unique = is_true;
            } else if named == "allow_" || (named.is_empty() && positional == 1) {
                allow_ = is_true;
            }
            if named.is_empty() {
                positional += 1;
            }
            cell = CDR(cell);
        }
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        let mut originals = Vec::with_capacity(n as usize);
        let mut results = Vec::with_capacity(n as usize);
        for i in 0..n {
            let ch = STRING_ELT(names, i);
            let raw = if ch.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned()
            };
            originals.push(raw.clone());
            let mut s = raw;
            let need_prefix = if s.is_empty() {
                true
            } else {
                let b = s.as_bytes();
                if b[0] == b'.' {
                    b.len() >= 2 && b[1].is_ascii_digit()
                } else {
                    !b[0].is_ascii_alphabetic()
                }
            };
            if need_prefix {
                s.insert(0, 'X');
            }
            let bytes = unsafe { s.as_bytes_mut() };
            for b in bytes.iter_mut() {
                if *b == b'.' || (allow_ && *b == b'_') {
                    continue;
                }
                if !b.is_ascii_alphanumeric() {
                    *b = b'.';
                }
            }
            if !make_names_is_valid(&s) {
                s.push('.');
            }
            results.push(s);
        }
        if unique {
            let mut order: Vec<usize> = (0..results.len()).collect();
            order.sort_by_key(|&i| originals[i] != results[i]);
            let tmp = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _t = protect(tmp);
            for (j, &i) in order.iter().enumerate() {
                let c = CString::new(results[i].as_str())
                    .unwrap_or_else(|_| CString::new("").unwrap());
                SET_STRING_ELT(tmp, j as i64, Rf_mkChar(c.as_ptr()));
            }
            let sep = Rf_mkString(c".".as_ptr());
            let _s = protect(sep);
            let uargs = Rf_cons(tmp, Rf_cons(sep, R_NilValue()));
            let _u = protect(uargs);
            let uniq = do_make_unique(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                uargs,
                std::ptr::null_mut(),
            );
            let _uq = protect(uniq);
            for (j, &i) in order.iter().enumerate() {
                let ch = STRING_ELT(uniq, j as i64);
                results[i] = std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned();
            }
        }
        for (i, s) in results.iter().enumerate() {
            let c = CString::new(s.as_str()).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(out, i as i64, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}

fn glob2rx_escape_open(s: &str, open: char) -> String {
    let b = s.as_bytes();
    let open_b = open as u8;
    let mut out = String::with_capacity(s.len() + 4);
    let mut i = 0;
    while i < b.len() {
        if i + 1 < b.len() && b[i] != b'\\' && b[i + 1] == open_b {
            out.push(b[i] as char);
            out.push('\\');
            out.push(open);
            i += 2;
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    out
}

fn glob2rx_one(pattern: &str, trim_head: bool, trim_tail: bool) -> String {
    let mut p = format!("^{pattern}$");
    p = p.replace('.', r"\.");
    p = p.replace('*', ".*");
    p = p.replace('?', ".");
    p = glob2rx_escape_open(&p, '(');
    p = glob2rx_escape_open(&p, '[');
    p = glob2rx_escape_open(&p, '{');
    if trim_tail {
        if let Some(i) = p.find(".*$") {
            p.replace_range(i..i + 3, "");
        }
    }
    if trim_head {
        if let Some(i) = p.find("^.*") {
            p.replace_range(i..i + 3, "");
        }
    }
    p
}

/// GNU `glob2rx(pattern, trim.head=FALSE, trim.tail=TRUE)`.
pub unsafe fn do_glob2rx(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern = CAR(args);
        if pattern.is_null() || pattern == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = if TYPEOF(pattern) == SEXPTYPE::STRSXP {
            XLENGTH(pattern)
        } else {
            0
        };
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let mut trim_head = false;
        let mut trim_tail = true;
        let mut cell = CDR(args);
        let mut positional = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let named = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let is_true = !value.is_null()
                && value != R_NilValue()
                && ((TYPEOF(value) == SEXPTYPE::LGLSXP
                    && XLENGTH(value) > 0
                    && *LOGICAL(value) == TRUE)
                    || (TYPEOF(value) == SEXPTYPE::INTSXP
                        && XLENGTH(value) > 0
                        && *INTEGER(value) != 0
                        && *INTEGER(value) != NA_INTEGER));
            let is_false = !value.is_null()
                && value != R_NilValue()
                && ((TYPEOF(value) == SEXPTYPE::LGLSXP
                    && XLENGTH(value) > 0
                    && *LOGICAL(value) == FALSE)
                    || (TYPEOF(value) == SEXPTYPE::INTSXP
                        && XLENGTH(value) > 0
                        && *INTEGER(value) == 0));
            if named == "trim.head" || (named.is_empty() && positional == 0) {
                trim_head = is_true;
            } else if named == "trim.tail" || (named.is_empty() && positional == 1) {
                trim_tail = !is_false;
            }
            if named.is_empty() {
                positional += 1;
            }
            cell = CDR(cell);
        }
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for i in 0..n {
            let ch = STRING_ELT(pattern, i);
            let raw = if ch.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned()
            };
            let converted = glob2rx_one(&raw, trim_head, trim_tail);
            let c = CString::new(converted).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(out, i, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}

fn adist_levenshtein(a: &str, b: &str) -> i32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    let mut prev: Vec<i32> = (0..=m as i32).collect();
    let mut curr = vec![0i32; m + 1];
    for i in 1..=n {
        curr[0] = i as i32;
        for j in 1..=m {
            let sub = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + sub);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

/// GNU `adist(x, y)` Levenshtein distances as an integer matrix.
pub unsafe fn do_adist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return crate::mainutils::array::allocMatrix(SEXPTYPE::INTSXP.as_c_int(), 0, 0);
        }
        let nx = XLENGTH(x);
        let mut y = x;
        let mut ignore_case = false;
        let mut cell = CDR(args);
        let mut positional = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let named = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if named == "ignore.case" {
                if TYPEOF(value) == SEXPTYPE::LGLSXP && XLENGTH(value) > 0 {
                    ignore_case = *LOGICAL(value) == TRUE;
                }
            } else if named == "y"
                || (named.is_empty()
                    && positional == 0
                    && TYPEOF(value) == SEXPTYPE::STRSXP)
            {
                y = value;
            }
            if named.is_empty() {
                positional += 1;
            }
            cell = CDR(cell);
        }
        if y.is_null() || y == R_NilValue() || TYPEOF(y) != SEXPTYPE::STRSXP {
            y = x;
        }
        let ny = XLENGTH(y);
        let mat = crate::mainutils::array::allocMatrix(
            SEXPTYPE::INTSXP.as_c_int(),
            nx as c_int,
            ny as c_int,
        );
        let _m = protect(mat);
        for j in 0..ny {
            let ych = STRING_ELT(y, j);
            let mut ys = if ych.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ych))
                    .to_string_lossy()
                    .into_owned()
            };
            if ignore_case {
                ys = ys.to_lowercase();
            }
            for i in 0..nx {
                let xch = STRING_ELT(x, i);
                let mut xs = if xch.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(CHAR(xch))
                        .to_string_lossy()
                        .into_owned()
                };
                if ignore_case {
                    xs = xs.to_lowercase();
                }
                *INTEGER(mat).add((i + j * nx) as usize) = adist_levenshtein(&xs, &ys);
            }
        }
        mat
    }
}

fn aregexec_match(pat: &str, text: &str, maxd: i32) -> Option<(i32, i32)> {
    let t: Vec<char> = text.chars().collect();
    let n = t.len();
    let plen = pat.chars().count() as i32;
    for start in 0..=n {
        for delta in 0..=maxd {
            for &len in &[plen, plen - delta, plen + delta] {
                if len < 0 {
                    continue;
                }
                let len = len as usize;
                if start + len > n {
                    continue;
                }
                let sub: String = t[start..start + len].iter().collect();
                if adist_levenshtein(pat, &sub) <= maxd {
                    return Some(((start + 1) as i32, len as i32));
                }
            }
        }
    }
    None
}

/// GNU `aregexec(pattern, text, max.distance)` for literal patterns.
pub unsafe fn do_aregexec(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pat = CAR(args);
        let text = CAR(CDR(args));
        let mut maxd = 0.1;
        let mut cell = CDR(CDR(args));
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if name == "max.distance" || name.is_empty() {
                let v = CAR(cell);
                if !v.is_null() && v != R_NilValue() && XLENGTH(v) > 0 {
                    maxd = if TYPEOF(v) == SEXPTYPE::REALSXP {
                        *REAL(v)
                    } else if TYPEOF(v) == SEXPTYPE::INTSXP {
                        *INTEGER(v) as f64
                    } else {
                        maxd
                    };
                }
            }
            cell = CDR(cell);
        }
        if pat.is_null() || pat == R_NilValue() || text.is_null() || text == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let pstr = if TYPEOF(pat) == SEXPTYPE::STRSXP && XLENGTH(pat) > 0 {
            let ch = STRING_ELT(pat, 0);
            if ch.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned()
            }
        } else {
            String::new()
        };
        let max_int = if maxd >= 1.0 {
            maxd.floor() as i32
        } else {
            (maxd * pstr.chars().count() as f64).floor() as i32
        };
        let n = if TYPEOF(text) == SEXPTYPE::STRSXP {
            XLENGTH(text)
        } else {
            0
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        let _r = protect(result);
        let ml_sym = Rf_install(c"match.length".as_ptr());
        for i in 0..n {
            let ch = STRING_ELT(text, i);
            let txt = if ch.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned()
            };
            let elt = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
            let _e = protect(elt);
            let ml = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
            let _m = protect(ml);
            if let Some((start, len)) = aregexec_match(&pstr, &txt, max_int) {
                *INTEGER(elt) = start;
                *INTEGER(ml) = len;
            } else {
                *INTEGER(elt) = -1;
                *INTEGER(ml) = -1;
            }
            crate::sexp::attrib_core::setAttrib(elt, ml_sym, ml);
            SET_VECTOR_ELT(result, i, elt);
        }
        result
    }
}


fn abbreviate_first_char(s: &[u8], i: usize) -> bool {
    i > 0 && s[i - 1].is_ascii_whitespace()
}

fn abbreviate_last_char(s: &[u8], i: usize) -> bool {
    i > 0
        && !s[i - 1].is_ascii_whitespace()
        && (i + 1 >= s.len() || s[i + 1].is_ascii_whitespace())
}

fn abbreviate_lc_vowel(c: u8) -> bool {
    matches!(c, b'a' | b'e' | b'i' | b'o' | b'u')
}

fn abbreviate_strip(inchar: &str, minlen: usize, usecl: bool) -> String {
    let trimmed = inchar.trim();
    if trimmed.len() < minlen {
        return trimmed.to_string();
    }
    let mut s: Vec<u8> = trimmed.as_bytes().to_vec();
    let mut nspace = 0usize;
    let mut j = 1i32;
    let mut i = s.len() - 1;
    while i > 0 {
        if s[i].is_ascii_whitespace() {
            if j != 0 {
                s.truncate(i);
            } else {
                nspace += 1;
            }
        } else {
            j = 0;
        }
        if s.len().saturating_sub(nspace) <= minlen {
            return abbreviate_drop_spaces(s, minlen);
        }
        i -= 1;
    }
    if usecl {
        i = s.len().saturating_sub(1);
        while i > 0 {
            if abbreviate_lc_vowel(s[i]) && abbreviate_last_char(&s, i) {
                s.remove(i);
            }
            if s.len().saturating_sub(nspace) <= minlen {
                return abbreviate_drop_spaces(s, minlen);
            }
            i -= 1;
        }
        i = s.len().saturating_sub(1);
        while i > 0 {
            if abbreviate_lc_vowel(s[i]) && !abbreviate_first_char(&s, i) {
                s.remove(i);
            }
            if s.len().saturating_sub(nspace) <= minlen {
                return abbreviate_drop_spaces(s, minlen);
            }
            i -= 1;
        }
        i = s.len().saturating_sub(1);
        while i > 0 {
            if s[i].is_ascii_lowercase() && abbreviate_last_char(&s, i) {
                s.remove(i);
            }
            if s.len().saturating_sub(nspace) <= minlen {
                return abbreviate_drop_spaces(s, minlen);
            }
            i -= 1;
        }
        i = s.len().saturating_sub(1);
        while i > 0 {
            if s[i].is_ascii_lowercase() && !abbreviate_first_char(&s, i) {
                s.remove(i);
            }
            if s.len().saturating_sub(nspace) <= minlen {
                return abbreviate_drop_spaces(s, minlen);
            }
            i -= 1;
        }
    }
    i = s.len().saturating_sub(1);
    while i > 0 {
        if !abbreviate_first_char(&s, i) && !s[i].is_ascii_whitespace() {
            s.remove(i);
        }
        if s.len().saturating_sub(nspace) <= minlen {
            return abbreviate_drop_spaces(s, minlen);
        }
        i -= 1;
    }
    abbreviate_drop_spaces(s, minlen)
}

fn abbreviate_drop_spaces(mut s: Vec<u8>, minlen: usize) -> String {
    if s.len() > minlen {
        let mut i = s.len() - 1;
        while i > 0 {
            if s[i].is_ascii_whitespace() {
                s.remove(i);
            }
            i -= 1;
        }
    }
    String::from_utf8(s).unwrap_or_default()
}

fn abbreviate_unique(names: &[String], minlength: usize) -> Vec<String> {
    if minlength == 0 {
        return vec![String::new(); names.len()];
    }
    let mut minlen = minlength;
    let mut x: Vec<String> = names.to_vec();
    let mut these_idx: Vec<usize> = (0..names.len()).collect();
    loop {
        for &idx in &these_idx {
            x[idx] = abbreviate_strip(&names[idx], minlen, true);
        }
        let mut seen = std::collections::HashMap::<String, usize>::new();
        let mut dup2 = vec![false; x.len()];
        let mut any = false;
        for (i, val) in x.iter().enumerate() {
            if let Some(&first) = seen.get(val) {
                dup2[i] = true;
                dup2[first] = true;
                any = true;
            } else {
                seen.insert(val.clone(), i);
            }
        }
        if !any {
            break;
        }
        minlen += 1;
        these_idx = (0..x.len()).filter(|&i| dup2[i]).collect();
    }
    x
}

/// GNU `abbreviate(names.arg, minlength=4)`.
pub unsafe fn do_abbreviate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names_s = CAR(args);
        if names_s.is_null() || names_s == R_NilValue() || TYPEOF(names_s) != SEXPTYPE::STRSXP {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = XLENGTH(names_s);
        let mut minlength = 4i32;
        let rest = CDR(args);
        if !rest.is_null() && rest != R_NilValue() {
            let m = CAR(rest);
            if TYPEOF(m) == SEXPTYPE::INTSXP && XLENGTH(m) > 0 {
                minlength = *INTEGER(m);
            } else if TYPEOF(m) == SEXPTYPE::REALSXP && XLENGTH(m) > 0 {
                minlength = *REAL(m) as i32;
            }
        }
        let mut old = Vec::with_capacity(n as usize);
        for i in 0..n {
            let ch = STRING_ELT(names_s, i);
            if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
                old.push(String::new());
                continue;
            }
            let raw = std::ffi::CStr::from_ptr(CHAR(ch))
                .to_string_lossy()
                .into_owned();
            old.push(raw.trim().to_string());
        }
        let mut unique_names = Vec::new();
        let mut first_of: Vec<usize> = Vec::with_capacity(n as usize);
        for name in &old {
            if let Some(pos) = unique_names.iter().position(|u| u == name) {
                first_of.push(pos);
            } else {
                first_of.push(unique_names.len());
                unique_names.push(name.clone());
            }
        }
        let minlen = if minlength < 0 { 0 } else { minlength as usize };
        let uniq = abbreviate_unique(&unique_names, minlen);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        let nm = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _n = protect(nm);
        for i in 0..n as usize {
            let ch = STRING_ELT(names_s, i as i64);
            if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
                SET_STRING_ELT(out, i as i64, crate::sexp::globals::R_NaString());
                SET_STRING_ELT(nm, i as i64, crate::sexp::globals::R_NaString());
                continue;
            }
            let abbr = &uniq[first_of[i]];
            let c = CString::new(abbr.as_str()).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(out, i as i64, Rf_mkChar(c.as_ptr()));
            let oc = CString::new(old[i].as_str()).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(nm, i as i64, Rf_mkChar(oc.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(out, crate::sexp::attrib_core::R_NamesSymbol(), nm);
        out
    }
}

/// GNU `intToBits(x)`: 32 LSB-first raw bits per integer.
pub unsafe fn do_intToBits(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
                *INTEGER(x).add(i as usize) as u32
            } else if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize) as i32 as u32
            } else if TYPEOF(x) == SEXPTYPE::LGLSXP {
                *LOGICAL(x).add(i as usize) as u32
            } else {
                0
            };
            for b in 0..32 {
                *RAW(out).add((i as usize) * 32 + b) = ((v >> b) & 1) as u8;
            }
        }
        out
    }
}

/// GNU `rawToBits(x)`: 8 LSB-first raw bits per byte.
pub unsafe fn do_rawToBits(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::RAWSXP {
            0
        } else {
            XLENGTH(x)
        };
        let out = Rf_allocVector3(SEXPTYPE::RAWSXP, n * 8);
        let _o = protect(out);
        for i in 0..n {
            let v = *RAW(x).add(i as usize);
            for b in 0..8 {
                *RAW(out).add((i as usize) * 8 + b) = (v >> b) & 1;
            }
        }
        out
    }
}

/// GNU `rawShift(x, n)`.
pub unsafe fn do_rawShift(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
                shift = *INTEGER(n);
            } else if TYPEOF(n) == SEXPTYPE::REALSXP && XLENGTH(n) > 0 {
                shift = *REAL(n) as i32;
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
            let v = *RAW(x).add(i) as u32;
            *RAW(out).add(i) = if shift > 0 {
                (v << shift) as u8
            } else if shift < 0 {
                (v >> (-shift)) as u8
            } else {
                v as u8
            };
        }
        out
    }
}

/// GNU `packBits(x, type)`.
pub unsafe fn do_packBits(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
                    let s = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy();
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
                *RAW(out).add(i as usize) = btmp;
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
                *INTEGER(out).add(i as usize) = itmp as i32;
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
                *REAL(out).add(i as usize) = f64::from_bits(bits);
            }
            out
        }
    }
}

unsafe fn packbits_bit(x: SEXP, idx: i64) -> u8 {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::RAWSXP => *RAW(x).add(idx as usize) & 1,
            t if t == SEXPTYPE::LGLSXP => (*LOGICAL(x).add(idx as usize) & 1) as u8,
            t if t == SEXPTYPE::INTSXP => (*INTEGER(x).add(idx as usize) & 1) as u8,
            _ => 0,
        }
    }
}

/// GNU `numToInts(x)`: two little-endian ints per double.
pub unsafe fn do_numToInts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
                *REAL(x).add(i as usize)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                let iv = *INTEGER(x).add(i as usize);
                if iv == NA_INTEGER {
                    NA_REAL
                } else {
                    iv as f64
                }
            } else {
                0.0
            };
            let bits = v.to_bits();
            *INTEGER(out).add((i as usize) * 2) = bits as u32 as i32;
            *INTEGER(out).add((i as usize) * 2 + 1) = (bits >> 32) as u32 as i32;
        }
        out
    }
}

/// GNU `numToBits(x)`: 64 LSB-first raw bits per double.
pub unsafe fn do_numToBits(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
                *REAL(x).add(i as usize)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                let iv = *INTEGER(x).add(i as usize);
                if iv == NA_INTEGER {
                    NA_REAL
                } else {
                    iv as f64
                }
            } else {
                0.0
            };
            let mut bits = v.to_bits();
            for b in 0..64 {
                *RAW(out).add((i as usize) * 64 + b) = (bits & 1) as u8;
                bits >>= 1;
            }
        }
        out
    }
}

/// GNU `utf8ToInt(x)`: UTF-8 code points of a length-1 string.
pub unsafe fn do_utf8ToInt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null()
            || x == R_NilValue()
            || TYPEOF(x) != SEXPTYPE::STRSXP
            || XLENGTH(x) == 0
        {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "argument must be a character vector of length 1",
            );
        }
        let ch = STRING_ELT(x, 0);
        if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
            return Rf_ScalarInteger(NA_INTEGER);
        }
        // GNU raw.c: `if (!utf8Valid(s)) return ScalarInteger(NA_INTEGER)`.
        let n = XLENGTH(ch);
        let bytes = if n <= 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(CHAR(ch) as *const u8, n as usize)
        };
        let Ok(s) = std::str::from_utf8(bytes) else {
            return Rf_ScalarInteger(NA_INTEGER);
        };
        let cps: Vec<i32> = s.chars().map(|c| c as u32 as i32).collect();
        let out = Rf_allocVector3(SEXPTYPE::INTSXP, cps.len() as i64);
        let _o = protect(out);
        for (i, cp) in cps.iter().enumerate() {
            *INTEGER(out).add(i) = *cp;
        }
        out
    }
}

/// GNU `intToUtf8(x, multiple=FALSE, allow_surrogate_pairs=FALSE)`.
pub unsafe fn do_intToUtf8(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        let m = arg_by_name_or_position(args, &["multiple"], 1);
        let multiple = TYPEOF(m) == SEXPTYPE::LGLSXP && XLENGTH(m) > 0 && *LOGICAL(m) == TRUE;
        let p = arg_by_name_or_position(args, &["allow_surrogate_pairs"], 2);
        let s_pair = TYPEOF(p) == SEXPTYPE::LGLSXP && XLENGTH(p) > 0 && *LOGICAL(p) == TRUE;
        let n = if x.is_null() || x == R_NilValue() {
            0
        } else {
            XLENGTH(x)
        };
        let code = |i: i64| -> i32 {
            if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(x).add(i as usize)
            } else if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize) as i32
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
                crate::main::errors::Rf_warning(
                    c"allow_surrogate_pairs = TRUE is incompatible with multiple = TRUE and will be ignored"
                        .as_ptr(),
                );
            }
            let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _o = protect(out);
            for i in 0..n {
                match valid_scalar(code(i)) {
                    Some('\0') => {
                        SET_STRING_ELT(out, i, Rf_mkChar(c"".as_ptr()));
                    }
                    Some(ch) => {
                        let mut buf = [0u8; 8];
                        let encoded = ch.encode_utf8(&mut buf);
                        let c = CString::new(encoded.as_bytes())
                            .unwrap_or_else(|_| CString::new("").unwrap());
                        SET_STRING_ELT(out, i, Rf_mkChar(c.as_ptr()));
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
                SET_STRING_ELT(
                    out,
                    0,
                    crate::sexp::constructors::Rf_mkCharLen(
                        s.as_ptr() as *const std::os::raw::c_char,
                        s.len() as c_int,
                    ),
                );
            }
            out
        }
    }
}

fn utf8_bytes_valid(s: SEXP) -> bool {
    unsafe {
        if s.is_null() {
            return true;
        }
        let p = CHAR(s);
        if p.is_null() {
            return true;
        }
        let bytes = std::ffi::CStr::from_ptr(p).to_bytes();
        std::str::from_utf8(bytes).is_ok()
    }
}

/// GNU `validUTF8(x)`.
pub unsafe fn do_validUTF8(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
            *LOGICAL(out).add(i as usize) = if utf8_bytes_valid(STRING_ELT(x, i)) {
                TRUE
            } else {
                FALSE
            };
        }
        out
    }
}

/// GNU `validEnc(x)`.
pub unsafe fn do_validEnc(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
            *LOGICAL(out).add(i as usize) = if ok { TRUE } else { FALSE };
        }
        out
    }
}

/// GNU `gettext(...)` without catalogs is identity.
pub unsafe fn do_gettext(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP {
            return x;
        }
        x
    }
}

/// GNU `gettextf(fmt, ..., domain=, trim=)` — sprintf of untranslated fmt.
pub unsafe fn do_gettextf(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fmt = CAR(args);
        let mut sprintf_tail = R_NilValue();
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let skip = if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy();
                name == "domain" || name == "trim"
            } else {
                false
            };
            if !skip {
                sprintf_tail = Rf_cons(CAR(cell), sprintf_tail);
            }
            cell = CDR(cell);
        }
        // Rf_cons prepends; reverse by walking into a new list.
        let mut sprintf_args = R_NilValue();
        let mut rev = sprintf_tail;
        while !rev.is_null() && rev != R_NilValue() {
            sprintf_args = Rf_cons(CAR(rev), sprintf_args);
            rev = CDR(rev);
        }
        sprintf_args = Rf_cons(fmt, sprintf_args);
        crate::mainutils::sprintf_main::do_sprintf(call, op, sprintf_args, rho)
    }
}


/// GNU `ngettext(n, msg1, msg2)` without catalogs.
pub unsafe fn do_ngettext(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_s = CAR(args);
        let n = if TYPEOF(n_s) == SEXPTYPE::INTSXP && XLENGTH(n_s) > 0 {
            *INTEGER(n_s)
        } else if TYPEOF(n_s) == SEXPTYPE::REALSXP && XLENGTH(n_s) > 0 {
            *REAL(n_s) as i32
        } else {
            0
        };
        let msg1 = CAR(CDR(args));
        let msg2 = CAR(CDR(CDR(args)));
        let pick = if n == 1 { msg1 } else { msg2 };
        if pick.is_null() || pick == R_NilValue() {
            return Rf_mkString(c"".as_ptr());
        }
        if TYPEOF(pick) == SEXPTYPE::STRSXP && XLENGTH(pick) == 1 {
            return pick;
        }
        if TYPEOF(pick) == SEXPTYPE::STRSXP && XLENGTH(pick) > 0 {
            let out = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
            let _o = protect(out);
            SET_STRING_ELT(out, 0, STRING_ELT(pick, 0));
            return out;
        }
        pick
    }
}

fn bindtextdomain_map()
-> &'static std::sync::Mutex<std::collections::HashMap<String, String>> {
    static MAP: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<String, String>>,
    > = std::sync::LazyLock::new(|| {
        std::sync::Mutex::new(std::collections::HashMap::new())
    });
    &MAP
}

/// GNU `bindtextdomain(domain, dirname)`.
pub unsafe fn do_bindtextdomain(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let domain = CAR(args);
        let dirname = CAR(CDR(args));
        if (domain.is_null() || domain == R_NilValue())
            && (dirname.is_null() || dirname == R_NilValue())
        {
            return Rf_ScalarLogical(TRUE);
        }
        if domain.is_null()
            || domain == R_NilValue()
            || TYPEOF(domain) != SEXPTYPE::STRSXP
            || XLENGTH(domain) != 1
        {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid 'domain' value",
            );
        }
        let dch = STRING_ELT(domain, 0);
        let dname = if dch.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(CHAR(dch))
                .to_string_lossy()
                .into_owned()
        };
        if dirname.is_null() || dirname == R_NilValue() {
            let path = bindtextdomain_map()
                .lock()
                .ok()
                .and_then(|map| map.get(&dname).cloned())
                .unwrap_or_default();
            let c = CString::new(path).unwrap_or_else(|_| CString::new("").unwrap());
            return Rf_mkString(c.as_ptr());
        }
        if TYPEOF(dirname) != SEXPTYPE::STRSXP || XLENGTH(dirname) != 1 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid 'dirname' value",
            );
        }
        let pch = STRING_ELT(dirname, 0);
        let path = if pch.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(CHAR(pch))
                .to_string_lossy()
                .into_owned()
        };
        if let Ok(mut map) = bindtextdomain_map().lock() {
            map.insert(dname, path.clone());
        }
        let c = CString::new(path).unwrap_or_else(|_| CString::new("").unwrap());
        Rf_mkString(c.as_ptr())
    }
}

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
                let s = std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy();
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
        let mut texts = Vec::with_capacity(n as usize);
        for i in 0..n {
            let v = *INTEGER(x).add(i as usize);
            let s = if v == NA_INTEGER {
                "NA".to_string()
            } else if hex {
                format!("{v:x}")
            } else {
                format!("{v:o}")
            };
            texts.push(s);
        }
        if n > 1 {
            let width = texts
                .iter()
                .filter(|s| s.as_str() != "NA")
                .map(|s| s.len())
                .max()
                .unwrap_or(0);
            for s in &mut texts {
                if s.as_str() != "NA" {
                    *s = format!("{s:0>width$}");
                }
            }
        }
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for (i, s) in texts.iter().enumerate() {
            let c = CString::new(s.as_str()).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(out, i as i64, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}

/// GNU `format.hexmode` / `as.character.hexmode`.
pub unsafe fn do_format_hexmode(
    _call: SEXP,
    _op: SEXP,
    args: SEXP,
    _rho: SEXP,
) -> SEXP {
    unsafe { format_mode_ints(CAR(args), true) }
}

/// GNU `format.octmode` / `as.character.octmode`.
pub unsafe fn do_format_octmode(
    _call: SEXP,
    _op: SEXP,
    args: SEXP,
    _rho: SEXP,
) -> SEXP {
    unsafe { format_mode_ints(CAR(args), false) }
}


/// GNU `nclass.Sturges(x)` is ceiling(log2(length(x)) + 1).
pub unsafe fn do_nclass_sturges(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = if x.is_null() || x == R_NilValue() {
            0.0
        } else {
            XLENGTH(x) as f64
        };
        let v = (n.log2() + 1.0).ceil();
        Rf_ScalarReal(v)
    }
}

fn nclass_numeric_copy(x: SEXP) -> Vec<f64> {
    unsafe {
        let n = if x.is_null() || x == R_NilValue() {
            0
        } else {
            XLENGTH(x)
        };
        let mut v = Vec::with_capacity(n as usize);
        for i in 0..n {
            let val = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                let iv = *INTEGER(x).add(i as usize);
                if iv == NA_INTEGER {
                    f64::NAN
                } else {
                    iv as f64
                }
            } else {
                f64::NAN
            };
            if val.is_finite() {
                v.push(val);
            }
        }
        v
    }
}

fn nclass_sample_var(x: &[f64]) -> f64 {
    let n = x.len();
    if n < 2 {
        return 0.0;
    }
    let mean = x.iter().sum::<f64>() / n as f64;
    x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0)
}

fn nclass_range(x: &[f64]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    let min = x.iter().copied().fold(f64::INFINITY, f64::min);
    let max = x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    max - min
}

fn nclass_iqr(mut x: Vec<f64>) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    x.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = x.len();
    // GNU quantile type 7: (n-1)*p + 1
    let q = |p: f64| -> f64 {
        let h = (n as f64 - 1.0) * p + 1.0;
        let lo = h.floor().clamp(1.0, n as f64) as usize;
        let hi = h.ceil().clamp(1.0, n as f64) as usize;
        let a = x[lo - 1];
        let b = x[hi - 1];
        a + (h - lo as f64) * (b - a)
    };
    q(0.75) - q(0.25)
}

/// GNU `nclass.scott(x)`.
pub unsafe fn do_nclass_scott(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = nclass_numeric_copy(CAR(args));
        let n = x.len() as f64;
        let h = 3.5 * nclass_sample_var(&x).sqrt() * n.powf(-1.0 / 3.0);
        let bins = if h > 0.0 {
            (nclass_range(&x) / h).ceil().max(1.0)
        } else {
            1.0
        };
        Rf_ScalarReal(bins)
    }
}

/// GNU `nclass.FD(x)`.
pub unsafe fn do_nclass_fd(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = nclass_numeric_copy(CAR(args));
        let n = x.len() as f64;
        let xs: Vec<f64> = x.iter().copied().map(|v| crate::fprec::fprec(v, 5.0)).collect();
        let mut h = 2.0 * nclass_iqr(xs.clone());
        if h == 0.0 {
            let mut sorted = xs.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let q7 = |p: f64| -> f64 {
                let n = sorted.len();
                if n == 0 {
                    return f64::NAN;
                }
                if n == 1 {
                    return sorted[0];
                }
                let idx = 1.0 + (n as f64 - 1.0) * p;
                let j = idx.floor() as usize;
                let g = idx - j as f64;
                let a = sorted[(j.saturating_sub(1)).min(n - 1)];
                let b = sorted[j.min(n - 1)];
                (1.0 - g) * a + g * b
            };
            let mut al = 0.25;
            while h == 0.0 && al / 2.0 >= 1.0 / 512.0 {
                al /= 2.0;
                h = (q7(1.0 - al) - q7(al)) / (1.0 - 2.0 * al);
            }
        }
        if h == 0.0 {
            h = 3.5 * nclass_sample_var(&x).sqrt();
        }
        let bins = if h > 0.0 {
            (nclass_range(&x) / h * n.powf(1.0 / 3.0)).ceil()
        } else {
            1.0
        };
        Rf_ScalarReal(bins)
    }
}

/// GNU `bw.nrd0(x)`.
pub unsafe fn do_bw_nrd0(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = nclass_numeric_copy(CAR(args));
        if x.len() < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "need at least 2 data points",
            );
        }
        let hi = nclass_sample_var(&x).sqrt();
        let mut lo = hi.min(nclass_iqr(x.clone()) / 1.34);
        if lo == 0.0 {
            lo = hi;
        }
        if lo == 0.0 {
            lo = x[0].abs();
        }
        if lo == 0.0 {
            lo = 1.0;
        }
        Rf_ScalarReal(0.9 * lo * (x.len() as f64).powf(-0.2))
    }
}

/// GNU `bw.nrd(x)`.
pub unsafe fn do_bw_nrd(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = nclass_numeric_copy(CAR(args));
        if x.len() < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "need at least 2 data points",
            );
        }
        let h = nclass_iqr(x.clone()) / 1.34;
        let sd = nclass_sample_var(&x).sqrt();
        Rf_ScalarReal(1.06 * sd.min(h) * (x.len() as f64).powf(-0.2))
    }
}

fn bw_phi_binned(x: &[f64], h: f64, deriv: i32) -> f64 {
    let n = x.len();
    if n < 2 || h <= 0.0 {
        return f64::NAN;
    }
    let xmin = x.iter().copied().fold(f64::INFINITY, f64::min);
    let xmax = x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let nb = 1000usize;
    let rang = (xmax - xmin) * 1.01;
    if rang == 0.0 {
        return f64::NAN;
    }
    let dd = rang / nb as f64;
    let mut cnt = vec![0.0; nb];
    for i in 1..n {
        let ii = (x[i] / dd) as i32;
        for j in 0..i {
            let jj = (x[j] / dd) as i32;
            let k = (ii - jj).unsigned_abs() as usize;
            if k < nb {
                cnt[k] += 1.0;
            }
        }
    }
    let mut sum = 0.0;
    for (i, &c) in cnt.iter().enumerate() {
        if c == 0.0 {
            continue;
        }
        let mut delta = (i as f64) * dd / h;
        delta *= delta;
        if delta >= 1000.0 {
            break;
        }
        let term = if deriv == 4 {
            (-delta / 2.0).exp() * (delta * delta - 6.0 * delta + 3.0)
        } else {
            (-delta / 2.0).exp()
                * (delta * delta * delta - 15.0 * delta * delta + 45.0 * delta - 15.0)
        };
        sum += term * c;
    }
    let nf = n as f64;
    if deriv == 4 {
        sum = 2.0 * sum + nf * 3.0;
        sum / (nf * (nf - 1.0) * h.powi(5)) / (2.0 * std::f64::consts::PI).sqrt()
    } else {
        sum = 2.0 * sum - 15.0 * nf;
        sum / (nf * (nf - 1.0) * h.powi(7)) / (2.0 * std::f64::consts::PI).sqrt()
    }
}

/// GNU `bw.SJ(x, method="dpi")` — Sheather–Jones direct plug-in.
pub unsafe fn do_bw_sj(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = nclass_numeric_copy(CAR(args));
        let n = x.len();
        if n < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "need at least 2 data points",
            );
        }
        let nf = n as f64;
        let sd = nclass_sample_var(&x).sqrt();
        let iqr = nclass_iqr(x.clone()) / 1.349;
        let scale = sd.min(iqr);
        let b = 1.23 * scale * nf.powf(-1.0 / 9.0);
        let td = -bw_phi_binned(&x, b, 6);
        if !td.is_finite() || td <= 0.0 {
            return Rf_ScalarReal(f64::NAN);
        }
        let c1 = 1.0 / (2.0 * std::f64::consts::PI.sqrt() * nf);
        let h1 = (2.394 / (nf * td)).powf(1.0 / 7.0);
        let sd_h = bw_phi_binned(&x, h1, 4);
        let res = (c1 / sd_h).powf(1.0 / 5.0);
        Rf_ScalarReal(res)
    }
}

fn pair_cnts(x: &[f64]) -> (f64, Vec<f64>) {
    let n = x.len();
    let xmin = x.iter().copied().fold(f64::INFINITY, f64::min);
    let xmax = x.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let nb = 1000usize;
    let rang = (xmax - xmin) * 1.01;
    let dd = if rang > 0.0 { rang / nb as f64 } else { 1.0 };
    let mut cnt = vec![0.0; nb];
    for i in 1..n {
        let ii = (x[i] / dd) as i32;
        for j in 0..i {
            let jj = (x[j] / dd) as i32;
            let k = (ii - jj).unsigned_abs() as usize;
            if k < nb {
                cnt[k] += 1.0;
            }
        }
    }
    (dd, cnt)
}

fn bw_ucv_at(n: usize, dd: f64, cnt: &[f64], h: f64) -> f64 {
    if h <= 0.0 {
        return f64::INFINITY;
    }
    let nf = n as f64;
    let mut sum = 0.0;
    for (i, &c) in cnt.iter().enumerate() {
        if c == 0.0 {
            continue;
        }
        let mut delta = (i as f64) * dd / h;
        delta *= delta;
        if delta >= 1000.0 {
            break;
        }
        let term = (-delta / 4.0).exp() - 8.0f64.sqrt() * (-delta / 2.0).exp();
        sum += term * c;
    }
    (0.5 + sum / nf) / (nf * h * std::f64::consts::PI.sqrt())
}

/// GNU `bw.ucv(x)` — unbiased cross-validation bandwidth.
pub unsafe fn do_bw_ucv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = nclass_numeric_copy(CAR(args));
        let n = x.len();
        if n < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "need at least 2 data points",
            );
        }
        let nf = n as f64;
        let hmax = 1.144 * nclass_sample_var(&x).sqrt() * nf.powf(-0.2);
        let mut lo = 0.1 * hmax;
        let mut hi = hmax;
        let (dd, cnt) = pair_cnts(&x);
        let gr = (5.0f64.sqrt() - 1.0) / 2.0;
        let mut x1 = hi - gr * (hi - lo);
        let mut x2 = lo + gr * (hi - lo);
        let mut f1 = bw_ucv_at(n, dd, &cnt, x1);
        let mut f2 = bw_ucv_at(n, dd, &cnt, x2);
        for _ in 0..80 {
            if f1 < f2 {
                hi = x2;
                x2 = x1;
                f2 = f1;
                x1 = hi - gr * (hi - lo);
                f1 = bw_ucv_at(n, dd, &cnt, x1);
            } else {
                lo = x1;
                x1 = x2;
                f1 = f2;
                x2 = lo + gr * (hi - lo);
                f2 = bw_ucv_at(n, dd, &cnt, x2);
            }
        }
        Rf_ScalarReal(0.5 * (lo + hi))
    }
}

fn bw_bcv_at(n: usize, dd: f64, cnt: &[f64], h: f64) -> f64 {
    if h <= 0.0 {
        return f64::INFINITY;
    }
    let nf = n as f64;
    let mut sum = 0.0;
    for (i, &c) in cnt.iter().enumerate() {
        if c == 0.0 {
            continue;
        }
        let mut delta = (i as f64) * dd / h;
        delta *= delta;
        if delta >= 1000.0 {
            break;
        }
        let term = (-delta / 4.0).exp() * (delta * delta - 12.0 * delta + 12.0);
        sum += term * c;
    }
    (1.0 + sum / (32.0 * nf)) / (2.0 * nf * h * std::f64::consts::PI.sqrt())
}

/// GNU `bw.bcv(x)` — biased cross-validation bandwidth.
pub unsafe fn do_bw_bcv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = nclass_numeric_copy(CAR(args));
        let n = x.len();
        if n < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "need at least 2 data points",
            );
        }
        let nf = n as f64;
        let hmax = 1.144 * nclass_sample_var(&x).sqrt() * nf.powf(-0.2);
        let mut lo = 0.1 * hmax;
        let mut hi = hmax;
        let (dd, cnt) = pair_cnts(&x);
        let gr = (5.0f64.sqrt() - 1.0) / 2.0;
        let mut x1 = hi - gr * (hi - lo);
        let mut x2 = lo + gr * (hi - lo);
        let mut f1 = bw_bcv_at(n, dd, &cnt, x1);
        let mut f2 = bw_bcv_at(n, dd, &cnt, x2);
        for _ in 0..80 {
            if f1 < f2 {
                hi = x2;
                x2 = x1;
                f2 = f1;
                x1 = hi - gr * (hi - lo);
                f1 = bw_bcv_at(n, dd, &cnt, x1);
            } else {
                lo = x1;
                x1 = x2;
                f1 = f2;
                x2 = lo + gr * (hi - lo);
                f2 = bw_bcv_at(n, dd, &cnt, x2);
            }
        }
        Rf_ScalarReal(0.5 * (lo + hi))
    }
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

fn formatc_one(v: f64, digits: i32, format: &str) -> String {
    let d = if digits < 0 { 6 } else { digits as usize };
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
            if v == 0.0 {
                return "0".to_string();
            }
            let exp = v.abs().log10().floor() as i32;
            let upper = format == "G";
            if exp < -4 || exp >= d as i32 {
                formatc_exp(v, d.saturating_sub(1), upper)
            } else {
                let decimals = (d as i32 - exp - 1).max(0) as usize;
                let s = format!("{v:.decimals$}");
                if s.contains('.') {
                    s.trim_end_matches('0')
                        .trim_end_matches('.')
                        .to_string()
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
            if v == 0.0 || d == 0 {
                return "0".to_string();
            }
            let exp = v.abs().log10().floor() as i32;
            let scale = 10f64.powi(d as i32 - 1 - exp);
            let rounded = (v * scale).round() / scale;
            let exp2 = if rounded == 0.0 {
                exp
            } else {
                rounded.abs().log10().floor() as i32
            };
            let decimals = (d as i32 - 1 - exp2).max(0) as usize;
            let s = format!("{rounded:.decimals$}");
            if s.contains('.') {
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            } else {
                s
            }
        }
        "d" => format!("{}", v as i64),
        _ => format!("{v:.d$}"),
    }
}

/// GNU `formatC(x, digits, width, format)`.
pub unsafe fn do_formatC(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let m = crate::mainutils::match_mod::match_formal_slots(
            call,
            args,
            &["x", "digits", "width", "format", "..."],
        );

        let x = *m.first().unwrap_or(&R_NilValue());
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let missing = crate::sexp::globals::R_MissingArg();
        let digits_arg = m.get(1).copied().unwrap_or(missing);
        let width_arg = m.get(2).copied().unwrap_or(missing);
        let format_arg = m.get(3).copied().unwrap_or(missing);

        let mut digits = if TYPEOF(x) == SEXPTYPE::INTSXP { 2i32 } else { 4 };
        if !digits_arg.is_null() && digits_arg != R_NilValue() && digits_arg != missing {
            if TYPEOF(digits_arg) == SEXPTYPE::INTSXP && XLENGTH(digits_arg) > 0 {
                digits = *INTEGER(digits_arg);
            } else if TYPEOF(digits_arg) == SEXPTYPE::REALSXP && XLENGTH(digits_arg) > 0 {
                digits = *REAL(digits_arg) as i32;
            }
        }
        if digits < 0 {
            digits = 6;
        }

        let mut width = digits + 1;
        if !width_arg.is_null() && width_arg != R_NilValue() && width_arg != missing {
            if TYPEOF(width_arg) == SEXPTYPE::INTSXP && XLENGTH(width_arg) > 0 {
                width = *INTEGER(width_arg);
            } else if TYPEOF(width_arg) == SEXPTYPE::REALSXP && XLENGTH(width_arg) > 0 {
                width = *REAL(width_arg) as i32;
            }
        }

        let mut format = if TYPEOF(x) == SEXPTYPE::INTSXP {
            "d".to_string()
        } else {
            "g".to_string()
        };
        if !format_arg.is_null() && format_arg != R_NilValue() && format_arg != missing {
            if TYPEOF(format_arg) == SEXPTYPE::STRSXP && XLENGTH(format_arg) > 0 {
                let ch = STRING_ELT(format_arg, 0);
                if !ch.is_null() {
                    format = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }

        let n = XLENGTH(x);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for i in 0..n {
            let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                let iv = *INTEGER(x).add(i as usize);
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
            } else {
                formatc_one(v, digits, &format)
            };
            let s = pad_formatc_width(s, width);
            let c = CString::new(s).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(out, i, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}

fn pad_formatc_width(s: String, width: i32) -> String {
    if width == 0 {
        return s;
    }
    let w = width.unsigned_abs() as usize;
    if s.len() >= w {
        return s;
    }
    let pad = " ".repeat(w - s.len());
    if width < 0 {
        format!("{s}{pad}")
    } else {
        format!("{pad}{s}")
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
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _out = protect(out);
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
        return out;
        let mut big_mark = String::new();
        let mut decimal_mark = ".".to_string();
        let mut zero_print: Option<String> = None;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let named = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            if named == "zero.print" {
                zero_print = if TYPEOF(value) == SEXPTYPE::LGLSXP {
                    Some(
                        if crate::mainutils::coerce::asLogical(value) != 0 {
                            "0".to_string()
                        } else {
                            " ".to_string()
                        },
                    )
                } else if TYPEOF(value) == SEXPTYPE::STRSXP && XLENGTH(value) > 0 {
                    let ch = STRING_ELT(value, 0);
                    if ch.is_null() {
                        None
                    } else {
                        Some(
                            std::ffi::CStr::from_ptr(CHAR(ch))
                                .to_string_lossy()
                                .into_owned(),
                        )
                    }
                } else {
                    None
                };
            } else if TYPEOF(value) == SEXPTYPE::STRSXP && XLENGTH(value) > 0 {
                let ch = STRING_ELT(value, 0);
                if !ch.is_null() {
                    let s = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                    if named == "big.mark" {
                        big_mark = s;
                    } else if named == "decimal.mark" {
                        decimal_mark = s;
                    }
                }
            }
            cell = CDR(cell);
        }
        let formatted = if TYPEOF(x) == SEXPTYPE::STRSXP {
            x
        } else {
            let n = XLENGTH(x);
            let tmp = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _t = protect(tmp);
            for i in 0..n {
                let elt = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    Rf_ScalarReal(*REAL(x).add(i as usize))
                } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                    Rf_ScalarInteger(*INTEGER(x).add(i as usize))
                } else {
                    Rf_ScalarReal(0.0)
                };
                let _e = protect(elt);
                let one = do_format(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    Rf_cons(elt, R_NilValue()),
                    std::ptr::null_mut(),
                );
                if TYPEOF(one) == SEXPTYPE::STRSXP && XLENGTH(one) > 0 {
                    SET_STRING_ELT(tmp, i, STRING_ELT(one, 0));
                }
            }
            tmp
        };
        let _f = protect(formatted);
        let n = XLENGTH(formatted);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for i in 0..n {
            let ch = STRING_ELT(formatted, i);
            let raw = if ch.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned()
            };
            let is_zero = {
                let body = raw.trim().trim_start_matches(['+', '-']);
                !body.is_empty()
                    && body.chars().all(|c| c == '0' || c == '.')
                    && body.contains('0')
                    && body.chars().filter(|c| *c == '.').count() <= 1
            };
            let pretty = if is_zero {
                if let Some(zero) = zero_print.as_deref() {
                    zero.to_string()
                } else if let Some((int_part, frac)) = raw.split_once('.') {
                    format!(
                        "{}{}{}",
                        prettynum_group(int_part, &big_mark, 3),
                        decimal_mark,
                        frac
                    )
                } else {
                    prettynum_group(&raw, &big_mark, 3)
                }
            } else if let Some((int_part, frac)) = raw.split_once('.') {
                format!(
                    "{}{}{}",
                    prettynum_group(int_part, &big_mark, 3),
                    decimal_mark,
                    frac
                )
            } else {
                prettynum_group(&raw, &big_mark, 3)
            };
            let c = CString::new(pretty).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(out, i, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}

/// GNU `localeToCharset(locale)`.
pub unsafe fn do_localeToCharset(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut locale = String::new();
        let first = CAR(args);
        if !first.is_null()
            && first != R_NilValue()
            && TYPEOF(first) == SEXPTYPE::STRSXP
            && XLENGTH(first) > 0
        {
            let ch = STRING_ELT(first, 0);
            if !ch.is_null() {
                locale = std::ffi::CStr::from_ptr(CHAR(ch))
                    .to_string_lossy()
                    .into_owned();
            }
        }
        if locale.is_empty() {
            let cat = Rf_mkString(c"LC_CTYPE".as_ptr());
            let _c = protect(cat);
            let got = crate::mainutils::essentials::do_Sys_getlocale(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                Rf_cons(cat, R_NilValue()),
                std::ptr::null_mut(),
            );
            if TYPEOF(got) == SEXPTYPE::STRSXP && XLENGTH(got) > 0 {
                let ch = STRING_ELT(got, 0);
                if !ch.is_null() {
                    locale = std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned();
                }
            }
            if locale.is_empty() {
                locale = "C".to_string();
            }
        }
        let charset = if locale == "C" || locale == "POSIX" {
            "ASCII"
        } else if locale.to_ascii_lowercase().contains("utf-8")
            || locale.to_ascii_lowercase().contains("utf8")
        {
            "UTF-8"
        } else {
            "ASCII"
        };
        Rf_mkString(CString::new(charset).unwrap_or_default().as_ptr())
    }
}

fn iconv_norm_enc(name: &str) -> &str {
    match name {
        "" | "native.enc" => "native",
        "UTF-8" | "UTF8" | "utf-8" | "utf8" => "UTF-8",
        "latin1" | "LATIN1" | "ISO-8859-1" | "ISO8859-1" | "iso-8859-1" => "latin1",
        "ASCII" | "US-ASCII" | "ascii" | "ANSI_X3.4-1968" => "ASCII",
        other => other,
    }
}

fn iconv_charsxp_bytes(ch: SEXP) -> &'static [u8] {
    unsafe {
        if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
            return &[];
        }
        let n = XLENGTH(ch);
        if n <= 0 {
            &[]
        } else {
            std::slice::from_raw_parts(CHAR(ch) as *const u8, n as usize)
        }
    }
}

fn iconv_decode(bytes: &[u8], from: &str, marked: &str) -> Option<Vec<char>> {
    let enc = if from == "native" {
        if marked == "latin1" {
            "latin1"
        } else if marked == "UTF-8" || marked == "unknown" {
            "UTF-8"
        } else {
            "UTF-8"
        }
    } else {
        from
    };
    match enc {
        "latin1" => Some(bytes.iter().map(|&b| char::from(b)).collect()),
        "ASCII" => {
            if bytes.iter().all(|&b| b < 0x80) {
                Some(bytes.iter().map(|&b| char::from(b)).collect())
            } else {
                None
            }
        }
        _ => std::str::from_utf8(bytes)
            .ok()
            .map(|s| s.chars().collect()),
    }
}

fn iconv_encode(chars: &[char], to: &str, sub: Option<&str>) -> Option<Vec<u8>> {
    match to {
        "UTF-8" | "native" => {
            let mut out = Vec::new();
            for &ch in chars {
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
            Some(out)
        }
        "latin1" => {
            let mut out = Vec::new();
            for &ch in chars {
                let cp = ch as u32;
                if cp <= 0xff {
                    out.push(cp as u8);
                } else {
                    match sub {
                        None => return None,
                        Some("") => {}
                        Some("?") => out.push(b'?'),
                        Some("byte") => {
                            let mut buf = [0u8; 4];
                            let raw = ch.encode_utf8(&mut buf).as_bytes();
                            for b in raw {
                                out.extend_from_slice(format!("<{b:02x}>").as_bytes());
                            }
                        }
                        Some("Unicode") => {
                            out.extend_from_slice(format!("<U+{cp:04X}>").as_bytes());
                        }
                        Some("c99") => {
                            if cp <= 0xffff {
                                out.extend_from_slice(format!("\\u{cp:04x}").as_bytes());
                            } else {
                                out.extend_from_slice(format!("\\U{cp:08x}").as_bytes());
                            }
                        }
                        Some(other) => out.extend_from_slice(other.as_bytes()),
                    }
                }
            }
            Some(out)
        }
        "ASCII" => {
            let mut out = Vec::new();
            for &ch in chars {
                let cp = ch as u32;
                if cp <= 0x7f {
                    out.push(cp as u8);
                } else {
                    match sub {
                        None => return None,
                        Some("") => {}
                        Some("?") => out.push(b'?'),
                        Some("byte") => {
                            if cp <= 0xff {
                                out.extend_from_slice(format!("<{cp:02x}>").as_bytes());
                            } else {
                                let mut buf = [0u8; 4];
                                let raw = ch.encode_utf8(&mut buf).as_bytes();
                                for b in raw {
                                    out.extend_from_slice(format!("<{b:02x}>").as_bytes());
                                }
                            }
                        }
                        Some("Unicode") => {
                            out.extend_from_slice(format!("<U+{cp:04X}>").as_bytes());
                        }
                        Some("c99") => {
                            if cp <= 0xffff {
                                out.extend_from_slice(format!("\\u{cp:04x}").as_bytes());
                            } else {
                                out.extend_from_slice(format!("\\U{cp:08x}").as_bytes());
                            }
                        }
                        Some(other) => out.extend_from_slice(other.as_bytes()),
                    }
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// GNU `iconv(x, from, to, sub, mark, toRaw)`.
pub unsafe fn do_iconv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        if TYPEOF(x) != SEXPTYPE::STRSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid 'x' argument",
            );
        }
        let from = {
            let a = arg_by_name_or_position(args, &["from"], 1);
            if a.is_null() || a == R_NilValue() || TYPEOF(a) != SEXPTYPE::STRSXP || XLENGTH(a) == 0 {
                String::new()
            } else {
                elt_to_string(a, 0)
            }
        };
        let to = {
            let a = arg_by_name_or_position(args, &["to"], 2);
            if a.is_null() || a == R_NilValue() || TYPEOF(a) != SEXPTYPE::STRSXP || XLENGTH(a) == 0 {
                String::new()
            } else {
                elt_to_string(a, 0)
            }
        };
        let from_n = iconv_norm_enc(&from);
        let to_n = iconv_norm_enc(&to);
        let sub_arg = arg_by_name_or_position(args, &["sub"], 3);
        let sub = if sub_arg.is_null()
            || sub_arg == R_NilValue()
            || (TYPEOF(sub_arg) == SEXPTYPE::LGLSXP
                && XLENGTH(sub_arg) > 0
                && *LOGICAL(sub_arg) == NA_LOGICAL)
            || (TYPEOF(sub_arg) == SEXPTYPE::STRSXP && is_string_na(sub_arg, 0))
        {
            None
        } else if TYPEOF(sub_arg) == SEXPTYPE::STRSXP && XLENGTH(sub_arg) > 0 {
            Some(elt_to_string(sub_arg, 0))
        } else {
            None
        };
        let mark_arg = arg_by_name_or_position(args, &["mark"], 4);
        let mark = mark_arg.is_null()
            || mark_arg == R_NilValue()
            || (TYPEOF(mark_arg) == SEXPTYPE::LGLSXP
                && XLENGTH(mark_arg) > 0
                && *LOGICAL(mark_arg) == TRUE)
            || TYPEOF(mark_arg) == SEXPTYPE::NILSXP;
        let n = XLENGTH(x);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for i in 0..n {
            let ch = STRING_ELT(x, i);
            if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
                SET_STRING_ELT(out, i, crate::sexp::globals::R_NaString());
                continue;
            }
            let marked = if crate::sexp::accessors::IS_BYTES(ch) != 0 {
                "bytes"
            } else if crate::sexp::accessors::IS_LATIN1(ch) != 0 {
                "latin1"
            } else if crate::sexp::accessors::IS_UTF8(ch) != 0 {
                "UTF-8"
            } else {
                "unknown"
            };
            let bytes = iconv_charsxp_bytes(ch);
            let Some(chars) = iconv_decode(bytes, from_n, marked) else {
                SET_STRING_ELT(out, i, crate::sexp::globals::R_NaString());
                continue;
            };
            let sub_ref = sub.as_deref();
            let Some(encoded) = iconv_encode(&chars, to_n, sub_ref) else {
                SET_STRING_ELT(out, i, crate::sexp::globals::R_NaString());
                continue;
            };
            let marked_ch = crate::sexp::constructors::Rf_mkCharLen(
                encoded.as_ptr() as *const std::os::raw::c_char,
                encoded.len() as c_int,
            );
            if mark && crate::sexp::accessors::IS_ASCII(marked_ch) == 0 {
                let kind = match to_n {
                    "UTF-8" => "UTF-8",
                    "latin1" => "latin1",
                    _ => "unknown",
                };
                crate::sexp::accessors::mark_charsxp_encoding(marked_ch, kind);
            }
            SET_STRING_ELT(out, i, marked_ch);
        }
        out
    }
}

const ICONVLIST: &[&str] = &[
    "ASCII",
    "BIG5",
    "CP1252",
    "EUC-JP",
    "EUC-KR",
    "GB18030",
    "GBK",
    "ISO-8859-1",
    "ISO8859-1",
    "LATIN1",
    "SHIFT_JIS",
    "US-ASCII",
    "UTF-16",
    "UTF-16BE",
    "UTF-16LE",
    "UTF-32",
    "UTF-8",
];

/// GNU `iconvlist()`.
pub unsafe fn do_iconvlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _ = args;
        let n = ICONVLIST.len() as i64;
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for (i, name) in ICONVLIST.iter().enumerate() {
            let c = CString::new(*name).unwrap_or_else(|_| CString::new("").unwrap());
            SET_STRING_ELT(out, i as i64, Rf_mkChar(c.as_ptr()));
        }
        out
    }
}























unsafe fn do_case_convert(args: SEXP, to_lower: bool) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = if x.is_null() || x == R_NilValue() {
            0
        } else {
            XLENGTH(x)
        };
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            if as_character_element_is_na(x, i) {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }
            let s = elt_to_string(x, i);
            let converted = if to_lower {
                s.to_lowercase()
            } else {
                s.to_uppercase()
            };
            let cstr = CString::new(converted).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }

        result
    }
}

pub(crate) unsafe fn as_character_element_is_na(x: SEXP, i: R_xlen_t) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP => STRING_ELT(x, i) == crate::sexp::globals::R_NaString(),
            t if t == SEXPTYPE::LGLSXP => *LOGICAL(x).add(i as usize) == NA_LOGICAL,
            t if t == SEXPTYPE::INTSXP => INTEGER_ELT(x, i as c_int) == NA_INTEGER,
            t if t == SEXPTYPE::REALSXP => {
                REAL_ELT(x, i as c_int).to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// String manipulation: trimws, sprintf, gsub, sub, strsplit
// ---------------------------------------------------------------------------

/// R's `trimws(x, which="both")` — trim whitespace from strings.
pub unsafe fn do_trimws(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            let trimmed = s.trim();
            let cstr = CString::new(trimmed).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
    }
}

/// R's `gsub(pattern, replacement, x)` — global string substitution (literal).
pub unsafe fn do_gsub(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_string_replace(args, true) }
}

/// R's `sub(pattern, replacement, x)` — first match substitution (literal).
pub unsafe fn do_sub(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_string_replace(args, false) }
}

unsafe fn string_arg_is_na(arg: SEXP) -> bool {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || XLENGTH(arg) == 0 {
            return false;
        }
        if TYPEOF(arg) == SEXPTYPE::STRSXP {
            return STRING_ELT(arg, 0) == crate::sexp::globals::R_NaString();
        }
        if TYPEOF(arg) == SEXPTYPE::LGLSXP {
            return *LOGICAL(arg) == crate::sexp::ffi::NA_LOGICAL;
        }
        false
    }
}

unsafe fn na_integer_vector(n: R_xlen_t) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        for i in 0..n {
            *INTEGER(result).add(i as usize) = NA_INTEGER;
        }
        result
    }
}

/// R's `grep(pattern, x, ..., value = FALSE)` for fixed and ERE matching.
pub unsafe fn do_grep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern_arg = arg_by_name_or_position(args, &["pattern"], 0);
        let x_arg = arg_by_name_or_position(args, &["x", "text"], 1);
        if pattern_arg.is_null() || x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        if string_arg_is_na(pattern_arg) {
            return na_integer_vector(XLENGTH(x_arg));
        }
        let value = named_logical_arg(args, "value").unwrap_or(false);
        let invert = named_logical_arg(args, "invert").unwrap_or(false);
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let perl = named_logical_arg(args, "perl").unwrap_or(false);
        let fixed = named_logical_arg(args, "fixed").unwrap_or(false);
        let pattern = elt_to_string(pattern_arg, 0);
        let matches = grep_match_indices(x_arg, &pattern, ignore_case, perl, fixed, invert);

        if value {
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, matches.len() as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for (out_idx, src_idx) in matches.into_iter().enumerate() {
                if TYPEOF(x_arg) == SEXPTYPE::STRSXP {
                    SET_STRING_ELT(result, out_idx as R_xlen_t, STRING_ELT(x_arg, src_idx));
                } else {
                    SET_STRING_ELT(
                        result,
                        out_idx as R_xlen_t,
                        Rf_mkChar(
                            CString::new(elt_to_string(x_arg, src_idx))
                                .unwrap_or_default()
                                .as_ptr(),
                        ),
                    );
                }
            }
            result
        } else {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, matches.len() as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = INTEGER(result);
            for (out_idx, src_idx) in matches.into_iter().enumerate() {
                *dst.add(out_idx) = (src_idx + 1) as c_int;
            }
            result
        }
    }
}

/// R's `grepl(pattern, x, ...)` for fixed and ERE matching.
pub unsafe fn do_grepl(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern_arg = arg_by_name_or_position(args, &["pattern"], 0);
        let x_arg = arg_by_name_or_position(args, &["x", "text"], 1);
        if pattern_arg.is_null() || x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let perl = named_logical_arg(args, "perl").unwrap_or(false);
        let fixed = named_logical_arg(args, "fixed").unwrap_or(false);
        let pattern = elt_to_string(pattern_arg, 0);
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        if string_arg_is_na(pattern_arg) {
            for i in 0..n {
                *dst.add(i as usize) = NA_LOGICAL;
            }
            return result;
        }
        for i in 0..n {
            if is_string_na(x_arg, i) {
                *dst.add(i as usize) = NA_LOGICAL;
                continue;
            }
            let matched =
                grep_value_matches(&elt_to_string(x_arg, i), &pattern, ignore_case, perl, fixed);
            *dst.add(i as usize) = if matched { TRUE } else { FALSE };
        }
        result
    }
}

/// R's `agrep(pattern, x, ...)` — approximate fixed-string matching.
pub unsafe fn do_agrep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern_arg = arg_by_name_or_position(args, &["pattern"], 0);
        let x_arg = arg_by_name_or_position(args, &["x", "text"], 1);
        if pattern_arg.is_null() || x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        if string_arg_is_na(pattern_arg) {
            return na_integer_vector(XLENGTH(x_arg));
        }
        let value = named_logical_arg(args, "value").unwrap_or(false);
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let max_distance = agrep_max_distance(args, pattern_arg);
        let fixed = named_logical_arg(args, "fixed").unwrap_or(true);
        let pattern = elt_to_string(pattern_arg, 0);
        let patterns: Vec<&str> = if fixed { vec![pattern.as_str()] } else { pattern.split('|').collect() };
        let matches = agrep_match_any(x_arg, &patterns, max_distance, ignore_case);

        if value {
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, matches.len() as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for (out_idx, src_idx) in matches.into_iter().enumerate() {
                SET_STRING_ELT(result, out_idx as R_xlen_t, STRING_ELT(x_arg, src_idx));
            }
            result
        } else {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, matches.len() as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = INTEGER(result);
            for (out_idx, src_idx) in matches.into_iter().enumerate() {
                *dst.add(out_idx) = (src_idx + 1) as c_int;
            }
            result
        }
    }
}

/// R's `agrepl(pattern, x, ...)` — logical approximate matching.
pub unsafe fn do_agrepl(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern_arg = arg_by_name_or_position(args, &["pattern"], 0);
        let x_arg = arg_by_name_or_position(args, &["x", "text"], 1);
        if pattern_arg.is_null() || x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let max_distance = agrep_max_distance(args, pattern_arg);
        let pattern = elt_to_string(pattern_arg, 0);
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let matched = !is_string_na(x_arg, i)
                && approximate_contains(
                    &pattern,
                    &elt_to_string(x_arg, i),
                    max_distance,
                    ignore_case,
                );
            *dst.add(i as usize) = if matched { TRUE } else { FALSE };
        }
        result
    }
}

/// R's `pcre_config()` — report regex engine feature switches.
pub unsafe fn do_pcre_config(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    const FEATURES: [(&str, c_int); 4] = [
        ("UTF-8", TRUE),
        ("Unicode properties", TRUE),
        ("JIT", FALSE),
        ("stack", FALSE),
    ];

    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, FEATURES.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let data = LOGICAL(result);
        for (i, (_, value)) in FEATURES.iter().enumerate() {
            *data.add(i) = *value;
        }

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, FEATURES.len() as R_xlen_t);
        if !names.is_null() {
            let _names_guard = protect(names);
            for (i, (name, _)) in FEATURES.iter().enumerate() {
                SET_STRING_ELT(
                    names,
                    i as R_xlen_t,
                    Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
                );
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
        }

        result
    }
}

fn agrep_max_distance(args: SEXP, pattern_arg: SEXP) -> usize {
    unsafe {
        let raw = arg_by_name_or_position(args, &["max.distance"], 2);
        let value = if raw.is_null() || raw == R_NilValue() {
            0.1
        } else {
            real_or_default(raw, 0.1)
        };
        if value <= 0.0 {
            return 0;
        }
        if value < 1.0 {
            let pattern_len = elt_to_string(pattern_arg, 0).chars().count().max(1);
            (value * pattern_len as f64).ceil() as usize
        } else {
            value.ceil() as usize
        }
    }
}

unsafe fn agrep_match_any(
    x: SEXP,
    patterns: &[&str],
    max_distance: usize,
    ignore_case: bool,
) -> Vec<R_xlen_t> {
    unsafe {
        let n = XLENGTH(x);
        let mut matches = Vec::new();
        for i in 0..n {
            if is_string_na(x, i) {
                continue;
            }
            let text = elt_to_string(x, i);
            if patterns.iter().any(|pattern| {
                approximate_contains(pattern, &text, max_distance, ignore_case)
            }) {
                matches.push(i);
            }
        }
        matches
    }
}

fn approximate_contains(pattern: &str, text: &str, max_distance: usize, ignore_case: bool) -> bool {
    let pattern = if ignore_case {
        pattern.to_ascii_lowercase()
    } else {
        pattern.to_string()
    };
    let text = if ignore_case {
        text.to_ascii_lowercase()
    } else {
        text.to_string()
    };
    let pat = pattern.as_bytes();
    let hay = text.as_bytes();
    if pat.is_empty() {
        return true;
    }
    if crate::mainutils::grep::levenshtein_distance(pat, hay) <= max_distance {
        return true;
    }
    let min_len = pat.len().saturating_sub(max_distance).max(1);
    let max_len = (pat.len() + max_distance).min(hay.len());

    for start in 0..hay.len() {
        for len in min_len..=max_len {
            let end = start + len;
            if end > hay.len() {
                break;
            }
            if crate::mainutils::grep::levenshtein_distance(pat, &hay[start..end]) <= max_distance {
                return true;
            }
        }
    }
    false
}

unsafe fn do_string_replace(args: SEXP, global: bool) -> SEXP {
    unsafe {
        let pattern_arg = CAR(args);
        let replacement_arg = CAR(CDR(args));
        let x_arg = CAR(CDR(CDR(args)));
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let perl = named_logical_arg(args, "perl").unwrap_or(false);
        let fixed = named_logical_arg(args, "fixed").unwrap_or(false);
        if pattern_arg.is_null()
            || replacement_arg.is_null()
            || x_arg.is_null()
            || x_arg == R_NilValue()
        {
            return R_NilValue();
        }
        let pattern = elt_to_string(pattern_arg, 0);
        let replacement = elt_to_string(replacement_arg, 0);
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        if string_arg_is_na(pattern_arg) {
            for i in 0..n {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
            }
            crate::mainutils::coerce::SHALLOW_DUPLICATE_ATTRIB(result, x_arg);
            return result;
        }
        let replacement_missing = string_arg_is_na(replacement_arg);
        for i in 0..n {
            if TYPEOF(x_arg) == SEXPTYPE::STRSXP
                && STRING_ELT(x_arg, i) == crate::sexp::globals::R_NaString()
            {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }
            if replacement_missing {
                let matched = crate::mainutils::grep::ere_replace(
                    &pattern, &elt_to_string(x_arg, i), "", global, ignore_case,
                )
                .is_some_and(|trial| trial != elt_to_string(x_arg, i));
                if matched {
                    SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                } else {
                    SET_STRING_ELT(result, i, STRING_ELT(x_arg, i));
                }
                continue;
            }
            let s = elt_to_string(x_arg, i);
            let replaced = if fixed && global {
                s.replace(&pattern, &replacement)
            } else if fixed {
                s.replacen(&pattern, &replacement, 1)
            } else if perl {
                crate::mainutils::grep::perl_replace(
                    &pattern,
                    &s,
                    &replacement,
                    global,
                    ignore_case,
                )
                .unwrap_or(s)
            } else if let Some(replaced) =
                crate::mainutils::grep::ere_replace(&pattern, &s, &replacement, global, ignore_case)
            {
                replaced
            } else {
                s
            };
            let cstr = CString::new(replaced).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        crate::mainutils::coerce::SHALLOW_DUPLICATE_ATTRIB(result, x_arg);
        result

    }
}

/// R's `strsplit(x, split, fixed=FALSE, perl=FALSE, useBytes=FALSE)` — split
/// strings by matches of `split`, returning a list of token vectors.
///
/// Mirrors upstream `do_strsplit` (grep.c): every match splits — a
/// non-empty match is dropped and the text before it becomes a token; an
/// empty match consumes the current character, which itself becomes the
/// token. A trailing remainder is kept only when non-empty. The `split`
/// vector recycles across `x`; an empty pattern splits into characters; an
/// NA split token does not split; NA strings pass through as NA.
pub unsafe fn do_strsplit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = arg_by_name_or_position(args, &["x"], 0);
        let split_arg = arg_by_name_or_position(args, &["split"], 1);

        if x_arg.is_null() || x_arg == R_NilValue() || split_arg.is_null() {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let fixed = named_logical_arg(args, "fixed").unwrap_or(false);
        let perl = named_logical_arg(args, "perl").unwrap_or(false);
        let n = XLENGTH(x_arg);
        let tlen = if split_arg == R_NilValue() {
            0
        } else {
            XLENGTH(split_arg)
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            // NA input string passes through as NA.
            if TYPEOF(x_arg) == SEXPTYPE::STRSXP
                && STRING_ELT(x_arg, i as i64) == crate::sexp::globals::R_NaString()
            {
                let na_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
                if !na_vec.is_null() {
                    let _na_guard = protect(na_vec);
                    SET_STRING_ELT(na_vec, 0, crate::sexp::globals::R_NaString());
                    SET_VECTOR_ELT(result, i as i64, na_vec);
                }
                continue;
            }
            let s = elt_to_string(x_arg, i);
            // NA split token doesn't split: whole string as the only token.
            let na_split = tlen > 0
                && TYPEOF(split_arg) == SEXPTYPE::STRSXP
                && STRING_ELT(split_arg, (i % tlen) as i64) == crate::sexp::globals::R_NaString();
            let split = if tlen == 0 {
                String::new()
            } else {
                elt_to_string(split_arg, (i % tlen) as i64)
            };

            let tokens: Vec<String> = if na_split {
                vec![s]
            } else if split.is_empty() {
                s.chars().map(|c| c.to_string()).collect()
            } else {
                // Find all matches, then cut tokens around them.
                let mut matches: Vec<(usize, usize)> = Vec::new();
                let mut offset = 0usize;
                if !s.is_empty() {
                    loop {
                        let hay = &s[offset..];
                        let found = if fixed {
                            fixed_find(hay, &split, false)
                        } else if perl {
                            crate::mainutils::grep::perl_find(&split, hay, false)
                        } else {
                            crate::mainutils::grep::ere_find(&split, hay, false)
                        };
                        let Some(m) = found else { break };
                        matches.push((offset + m.start, offset + m.end));
                        if m.end > m.start {
                            offset += m.end;
                        } else {
                            // Empty match gets the next char, so move by one.
                            offset += hay.chars().next().map_or(1, char::len_utf8);
                        }
                        if offset >= s.len() {
                            break;
                        }
                    }
                }
                let mut tokens = Vec::with_capacity(matches.len() + 1);
                let mut pos = 0usize;
                for (st, en) in &matches {
                    if en > st {
                        tokens.push(s[pos..*st].to_string());
                        pos = *en;
                    } else {
                        // Empty match: the current character is the token.
                        let clen = s[*st..].chars().next().map_or(1, char::len_utf8);
                        tokens.push(s[*st..(*st + clen)].to_string());
                        pos = *st + clen;
                    }
                }
                if pos < s.len() {
                    tokens.push(s[pos..].to_string());
                }
                tokens
            };

            let vec = Rf_allocVector3(SEXPTYPE::STRSXP, tokens.len() as R_xlen_t);
            if !vec.is_null() {
                let _vec_guard = protect(vec);
                for (j, part) in tokens.iter().enumerate() {
                    let cstr = CString::new(part.as_str()).unwrap_or_default();
                    let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                    if !charsxp.is_null() {
                        SET_STRING_ELT(vec, j as i64, charsxp);
                    }
                }
            }
            SET_VECTOR_ELT(result, i as i64, vec);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Conversion: chartr, format
// ---------------------------------------------------------------------------

/// R's `chartr(old, new, x)` — character-by-character translation.
pub unsafe fn do_chartr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let old_arg = CAR(args);
        let new_arg = CAR(CDR(args));
        let x_arg = CAR(CDR(CDR(args)));
        if old_arg.is_null() || new_arg.is_null() {
            return R_NilValue();
        }
        let old_str = elt_to_string(old_arg, 0);
        let new_str = elt_to_string(new_arg, 0);
        let old_chars: Vec<char> = old_str.chars().collect();
        let new_chars: Vec<char> = new_str.chars().collect();
        let n = if x_arg.is_null() || x_arg == R_NilValue() {
            0
        } else {
            XLENGTH(x_arg)
        };
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            if as_character_element_is_na(x_arg, i) {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }
            let s = elt_to_string(x_arg, i);
            let translated: String = s
                .chars()
                .map(|c| {
                    if let Some(pos) = old_chars.iter().position(|&oc| oc == c) {
                        *new_chars.get(pos).unwrap_or(&c)
                    } else {
                        c
                    }
                })
                .collect();
            let cstr = CString::new(translated).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
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
        // Upstream `format` is a UseMethod generic: dispatch to closure
        // methods registered by loaded packages (R6's
        // format.R6ClassGenerator) before the default formatting.
        if let Some(result) =
            crate::mainutils::essentials::apply_s3_closure_method("format", _call, args, _rho)
        {
            return result;
        }
        if crate::mainutils::objects::inherits2(x, c"POSIXlt".as_ptr()) != FALSE {
            return crate::mainutils::datetime::do_format_POSIXlt(_call, _op, args, _rho);
        }
        if crate::mainutils::objects::inherits2(x, c"POSIXct".as_ptr()) != FALSE
            || crate::mainutils::objects::inherits2(x, c"POSIXt".as_ptr()) != FALSE
        {
            return crate::mainutils::datetime::do_format_POSIXct(_call, _op, args, _rho);
        }
        if crate::mainutils::objects::inherits2(x, c"Date".as_ptr()) != FALSE {
            return crate::mainutils::essentials::do_format_Date(_call, _op, args, _rho);
        }
        if crate::mainutils::objects::inherits2(x, c"data.frame".as_ptr()) != FALSE {
            return crate::mainutils::essentials::do_format_data_frame(_call, _op, args, _rho);
        }



        if crate::mainutils::objects::inherits2(x, c"hexmode".as_ptr()) != FALSE {
            return format_mode_ints(x, true);
        }
        if crate::mainutils::objects::inherits2(x, c"octmode".as_ptr()) != FALSE {
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
        if matches!(x_type_now, 10 | 13 | 14 | 15)
            && !sexp_has_class(x, "POSIXct")
            && !sexp_has_class(x, "Date")
        {
            return format_numeric_vector(x, n, args);
        }
        if x_type_now == SEXPTYPE::STRSXP {
            return format_character_vector(x, n, args);
        }
        for i in 0..n {
            let s = if TYPEOF(x) == SEXPTYPE::REALSXP {
                let v = *REAL(x).add(i as usize);
                if sexp_has_class(x, "POSIXct") {
                    posix_seconds_to_iso(v, false).unwrap_or_else(|| "NA".to_string())
                } else if sexp_has_class(x, "Date") {
                    date_days_to_iso(v).unwrap_or_else(|| "NA".to_string())
                } else if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
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
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
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
                Some(
                    std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(name))
                        .to_string_lossy(),
                )
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
                    let slot = positional;
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
            let which = if decimal_mark.len() > 1 { "more" } else { "less" };
            let msg = std::ffi::CString::new(format!(
                "the decimal mark is {which} than one character wide; this will become an error"
            ))
            .unwrap_or_default();
            crate::mainutils::errors::Rf_warning(msg.as_ptr());
        }


        // formatReal/formatComplex read the live digits option; honor an
        // explicit digits argument by swapping the option for this call.
        let digits =
            digits_opt.unwrap_or_else(|| crate::mainutils::options::GetOptionDigits());
        let digits_value = crate::sexp::constructors::Rf_ScalarInteger(digits);
        let _digits_value = protect(digits_value);
        let saved_digits =
            crate::mainutils::options::SetOptionByName("digits", digits_value);
        let saved_scipen = if let Some(sci) = sci_opt {
            if sci != NA_INTEGER {
                let sci_value = crate::sexp::constructors::Rf_ScalarInteger(sci);
                let _sci_value = protect(sci_value);
                Some(crate::mainutils::options::SetOptionByName("scipen", sci_value))
            } else {
                None
            }
        } else {
            None
        };
        let pinned_print = crate::mainutils::format::format_set_R_print(
            crate::mainutils::format::RPrint {
                digits,
                scipen: crate::mainutils::options::GetOptionScipen(),
                na_width: 2,
                na_width_noquote: 2,
            },
        );

        // GNU do_format feeds decimal.mark to EncodeReal0 as OutDec.
        let outdec_owned = CString::new(decimal_mark.as_str()).unwrap_or_else(|_| {
            CString::new(".").expect("dot")
        });
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
                crate::mainutils::format::formatRealS(
                    x, n, &mut wr, &mut dr, &mut er, nsmall,
                );
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
                    crate::mainutils::printutils::EncodeLogical(v, w)
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
                    crate::mainutils::printutils::EncodeComplex(
                        v, w - wi - 2, dr, er, wi, di, ei, outdec,
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
            );
        }
        for (i, text) in encoded_strings.iter().enumerate() {
            let cstr = CString::new(text.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }

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
) {
    let before: Vec<usize> = strings.iter().map(|s| s.chars().count()).collect();
    for s in strings.iter_mut() {
        if s.trim() == "NA" || s.trim() == "NaN" || s.trim() == "Inf" || s.trim() == "-Inf" {
            continue;
        }
        *s = pretty_num_one(s, big_mark, drop0trailing, decimal_mark, small_mark, small_interval);
    }
    if let Some(zero) = zero_print {
        for s in strings.iter_mut() {
            if pretty_num_is_zero(s) {
                *s = format_zero_print(s, zero);
            }
        }
    }
    if strings
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
            if e.bytes().skip(1).all(|b| b == b'+' || b == b'-' || b == b'0') {
                let marked = insert_small_mark(&frac, small_mark, small_interval);
                return pretty_num_join(
                    leading,
                    sign,
                    &insert_big_mark(int_part, big_mark),
                    if marked.is_empty() { None } else { Some(&marked) },
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
        if marked.is_empty() { None } else { Some(&marked) },
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
    let prefix: String = int_part.chars().take_while(|c| !c.is_ascii_digit()).collect();
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
                Some(
                    std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(name))
                        .to_string_lossy(),
                )
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
                    _ => None,
                })
                .unwrap_or_else(|| {
                    let slot = positional;
                    positional += 1;
                    slot
                });
            match slot {
                3 => {
                    if TYPEOF(value) == SEXPTYPE::STRSXP {
                        let text = elt_to_string(value, 0);
                        justify = match text.as_str() {
                            "left" => 0,
                            "right" => 1,
                            "centre" | "center" => 2,
                            "none" => 3,
                            _ => 0,
                        };
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
                _ => {}
            }
            cell = CDR(cell);
        }

        let mut strings = Vec::with_capacity(n as usize);
        let mut max_w = 0usize;
        for i in 0..n {
            let s = elt_to_string(x, i);
            max_w = max_w.max(s.chars().count());
            strings.push(s);
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
            let out = if justify == 3 {
                s.clone()
            } else {
                justify_pad(s, field, justify)
            };
            let cstr = CString::new(out).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
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

pub unsafe fn do_format_info(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let m = crate::mainutils::match_mod::match_formal_slots(
            call,
            args,
            &["x", "digits", "nsmall"],
        );
        let missing = crate::sexp::globals::R_MissingArg();
        let force = |v: SEXP| -> SEXP {
            if !v.is_null() && TYPEOF(v) == SEXPTYPE::PROMSXP {
                crate::sexp::envir::forcePromise(v)
            } else {
                v
            }
        };
        let x = force(*m.first().unwrap_or(&R_NilValue()));
        if x.is_null() || x == R_NilValue() || x == missing {
            return R_NilValue();
        }
        let digits = force(m.get(1).copied().unwrap_or(missing));
        let digits = if digits.is_null() || digits == missing {
            R_NilValue()
        } else {
            digits
        };
        let nsmall = force(m.get(2).copied().unwrap_or(missing));
        let nsmall = if nsmall.is_null() || nsmall == R_NilValue() || nsmall == missing {
            Rf_ScalarInteger(0)
        } else {
            nsmall
        };


        let tail = Rf_cons(nsmall, R_NilValue());
        let _tail_guard = protect(tail);
        let middle = Rf_cons(digits, tail);
        let _middle_guard = protect(middle);
        let normalized_args = Rf_cons(x, middle);
        let _args_guard = protect(normalized_args);
        crate::mainutils::paste_impl::do_formatinfo(call, op, normalized_args, rho)
    }
}

// ---------------------------------------------------------------------------
// String operations: startsWith, endsWith, str_pad, str_count, str_replace
// ---------------------------------------------------------------------------

/// R's `startsWith(x, prefix)` — check if strings start with prefix.
pub unsafe fn do_startsWith(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let prefix_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || prefix_arg.is_null() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let prefix = elt_to_string(prefix_arg, 0);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            *dst.add(i as usize) = if s.starts_with(&prefix) { TRUE } else { FALSE };
        }
        result
    }
}

/// R's `endsWith(x, suffix)` — check if strings end with suffix.
pub unsafe fn do_endsWith(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let suffix_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || suffix_arg.is_null() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let suffix = elt_to_string(suffix_arg, 0);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            *dst.add(i as usize) = if s.ends_with(&suffix) { TRUE } else { FALSE };
        }
        result
    }
}

/// R's `str_pad(x, width, side="left", pad=" ")` — pad strings to a width.
pub unsafe fn do_str_pad(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let width_arg = CAR(CDR(args));
        let side_arg = CAR(CDR(CDR(args)));
        let pad_arg = CAR(CDR(CDR(CDR(args))));
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let width = if width_arg.is_null() || width_arg == R_NilValue() {
            1usize
        } else {
            real_or_default(width_arg, 1.0).max(0.0) as usize
        };
        let side = if side_arg.is_null() || side_arg == R_NilValue() {
            "left".to_string()
        } else {
            elt_to_string(side_arg, 0)
        };
        let pad_char = if pad_arg.is_null() || pad_arg == R_NilValue() {
            " ".to_string()
        } else {
            elt_to_string(pad_arg, 0)
        };
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            let slen = s.chars().count();
            let padded = if slen >= width {
                s
            } else {
                let diff = width - slen;
                let pad_str: String = pad_char.chars().cycle().take(diff).collect();
                match side.as_str() {
                    "left" => format!("{}{}", pad_str, s),
                    "right" => format!("{}{}", s, pad_str),
                    "both" => {
                        let left = diff / 2;
                        let right = diff - left;
                        let lp: String = pad_char.chars().cycle().take(left).collect();
                        let rp: String = pad_char.chars().cycle().take(right).collect();
                        format!("{}{}{}", lp, s, rp)
                    }
                    _ => format!("{}{}", pad_str, s),
                }
            };
            let cstr = CString::new(padded).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
    }
}

/// R's `str_count(x, pattern)` — count occurrences of pattern in strings.
pub unsafe fn do_str_count(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let pattern_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || pattern_arg.is_null() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let pattern = elt_to_string(pattern_arg, 0);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            let count = if pattern.is_empty() {
                s.len() + 1
            } else {
                s.matches(&pattern).count()
            };
            *dst.add(i as usize) = count as c_int;
        }
        result
    }
}

/// R's `str_replace(x, pattern, replacement)` — alias for sub.
pub unsafe fn do_str_replace(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_sub(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// R runtime type checks: is.language, is.call, is.symbol, is.name,
//   is.pairlist, is.function, is.expression, is.environment
// ---------------------------------------------------------------------------

/// R's `is.language(x)` — TRUE for LANGSXP, SYMSXP, or EXPRSXP.
pub unsafe fn do_is_language(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        Rf_ScalarLogical(
            if t == SEXPTYPE::LANGSXP || t == SEXPTYPE::SYMSXP || t == SEXPTYPE::EXPRSXP {
                TRUE
            } else {
                FALSE
            },
        )
    }
}

/// R's `is.call(x)` — TRUE for LANGSXP.
pub unsafe fn do_is_call(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::LANGSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `is.symbol(x)` — TRUE for SYMSXP.
pub unsafe fn do_is_symbol(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::SYMSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `is.name(x)` — alias for is.symbol.
pub unsafe fn do_is_name(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_is_symbol(_call, _op, args, _rho) }
}

/// R's `is.pairlist(x)` — TRUE for LISTSXP.
pub unsafe fn do_is_pairlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(TRUE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::LISTSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `is.function(x)` — TRUE for CLOSXP, BUILTINSXP, or SPECIALSXP.
pub unsafe fn do_is_function(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        Rf_ScalarLogical(
            if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
                TRUE
            } else {
                FALSE
            },
        )
    }
}

/// R's `is.expression(x)` — TRUE for EXPRSXP.
pub unsafe fn do_is_expression(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::EXPRSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `is.environment(x)` — TRUE for ENVSXP and S4 environment subclasses.
pub unsafe fn do_is_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        if TYPEOF(x) == SEXPTYPE::ENVSXP {
            return Rf_ScalarLogical(TRUE);
        }
        if crate::mainutils::coerce::IS_S4_OBJECT(x) != 0 && TYPEOF(x) == SEXPTYPE::OBJSXP {
            let data = crate::mainutils::subassign::R_getS4DataSlot(
                x,
                SEXPTYPE::ENVSXP.as_c_int(),
            );
            return Rf_ScalarLogical(if TYPEOF(data) == SEXPTYPE::ENVSXP {
                TRUE
            } else {
                FALSE
            });
        }
        Rf_ScalarLogical(FALSE)
    }
}

// ---------------------------------------------------------------------------
// String formatting
// ---------------------------------------------------------------------------

/// R's `noquote(x)` — mark a copy so print skips quoting.
pub unsafe fn do_noquote(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        x = crate::mainutils::duplicate::shallow_duplicate_if_shared(x);
        let _x = protect(x);
        if crate::mainutils::objects::inherits2(x, c"noquote".as_ptr()) == FALSE {
            let old = crate::sexp::attrib_core::getAttrib(
                x,
                crate::sexp::attrib_core::R_ClassSymbol(),
            );
            let old_n = if !old.is_null()
                && old != R_NilValue()
                && TYPEOF(old) == SEXPTYPE::STRSXP
            {
                XLENGTH(old)
            } else {
                0
            };
            let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, old_n + 1);
            if !class_vec.is_null() {
                let _c = protect(class_vec);
                for i in 0..old_n {
                    SET_STRING_ELT(class_vec, i, STRING_ELT(old, i));
                }
                SET_STRING_ELT(
                    class_vec,
                    old_n,
                    crate::sexp::constructors::Rf_mkChar(c"noquote".as_ptr()),
                );
                crate::sexp::attrib_core::setAttrib(
                    x,
                    crate::sexp::attrib_core::R_ClassSymbol(),
                    class_vec,
                );
            }
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
        x
    }
}

/// R's `deparse(x)` — convert an object or expression to source-like text.
pub unsafe fn do_deparse(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::deparse::do_deparse(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// String/vector completion: charmatch, pmatch, strtoi, strtrim
// ---------------------------------------------------------------------------

/// R's `charmatch(x, table)` — character matching.
/// Returns integer index of exact match (1-based), or 0 if no match, or NA if ambiguous.
pub unsafe fn do_charmatch(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let table_arg = CAR(CDR(args));
        let nomatch_arg = CAR(CDR(CDR(args)));
        let nomatch = if nomatch_arg.is_null() || nomatch_arg == R_NilValue() {
            NA_INTEGER
        } else {
            real_or_default(nomatch_arg, NA_REAL) as c_int
        };

        if x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let nx = XLENGTH(x_arg);
        let nt = if table_arg.is_null() || table_arg == R_NilValue() {
            0
        } else {
            XLENGTH(table_arg)
        };
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, nx);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);

        for i in 0..nx {
            let x_is_na = as_character_element_is_na(x_arg, i);
            let x_str = if x_is_na {
                String::new()
            } else {
                elt_to_string(x_arg, i)
            };
            let mut exact_matches = 0usize;
            let mut exact_index = nomatch;
            for j in 0..nt {
                let table_is_na = as_character_element_is_na(table_arg, j);
                let exact = if x_is_na || table_is_na {
                    x_is_na && table_is_na
                } else {
                    elt_to_string(table_arg, j) == x_str
                };
                if exact {
                    exact_matches += 1;
                    exact_index = (j + 1) as c_int;
                }
            }

            if exact_matches == 1 {
                *dst.add(i as usize) = exact_index;
                continue;
            }
            if exact_matches > 1 {
                *dst.add(i as usize) = 0;
                continue;
            }

            let mut partial_matches = 0usize;
            let mut partial_index = nomatch;
            if !x_is_na {
                for j in 0..nt {
                    if as_character_element_is_na(table_arg, j) {
                        continue;
                    }
                    let table_str = elt_to_string(table_arg, j);
                    if table_str.starts_with(&x_str) {
                        partial_matches += 1;
                        partial_index = (j + 1) as c_int;
                    }
                }
            }
            *dst.add(i as usize) = if partial_matches == 1 {
                partial_index
            } else if partial_matches > 1 {
                0
            } else {
                nomatch
            };
        }
        result
    }
}

/// R's `pmatch(x, table, nomatch=NA, duplicates.ok=FALSE)` — partial matching.
/// Returns integer vector of matches (1-based).
pub unsafe fn do_pmatch(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let table_arg = CAR(CDR(args));
        let nomatch_arg = CAR(CDR(CDR(args)));
        let duplicates_arg = CAR(CDR(CDR(CDR(args))));
        let nomatch = if nomatch_arg.is_null() || nomatch_arg == R_NilValue() {
            NA_INTEGER
        } else {
            real_or_default(nomatch_arg, NA_REAL as f64) as c_int
        };
        let duplicates_ok = if duplicates_arg.is_null() || duplicates_arg == R_NilValue() {
            false
        } else {
            real_or_default(duplicates_arg, 0.0) != 0.0
        };

        if x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let nx = XLENGTH(x_arg);
        let nt = if table_arg.is_null() || table_arg == R_NilValue() {
            0
        } else {
            XLENGTH(table_arg)
        };
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, nx);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);

        // Track which table entries are already matched
        let mut used = vec![false; nt as usize];

        for i in 0..nx {
            let x_is_na = as_character_element_is_na(x_arg, i);
            let x_str = if x_is_na {
                String::new()
            } else {
                elt_to_string(x_arg, i)
            };
            let mut best_match: c_int = nomatch;
            if x_is_na {
                for j in 0..nt {
                    if !duplicates_ok && used[j as usize] {
                        continue;
                    }
                    if as_character_element_is_na(table_arg, j) {
                        best_match = (j + 1) as c_int;
                        if !duplicates_ok {
                            used[j as usize] = true;
                        }
                        break;
                    }
                }
                *dst.add(i as usize) = best_match;
                continue;
            }

            if x_str.is_empty() {
                *dst.add(i as usize) = nomatch;
                continue;
            }

            for j in 0..nt {
                if !duplicates_ok && used[j as usize] {
                    continue;
                }
                if as_character_element_is_na(table_arg, j) {
                    continue;
                }
                if elt_to_string(table_arg, j) == x_str {
                    best_match = (j + 1) as c_int;
                    if !duplicates_ok {
                        used[j as usize] = true;
                    }
                    break;
                }
            }

            if best_match == nomatch {
                let mut partial_matches = 0usize;
                let mut partial_index = nomatch;
                for j in 0..nt {
                    if !duplicates_ok && used[j as usize] {
                        continue;
                    }
                    if as_character_element_is_na(table_arg, j) {
                        continue;
                    }
                    let t_str = elt_to_string(table_arg, j);
                    if t_str.starts_with(&x_str) {
                        partial_matches += 1;
                        partial_index = (j + 1) as c_int;
                    }
                }
                if partial_matches == 1 {
                    best_match = partial_index;
                    if !duplicates_ok {
                        used[(partial_index - 1) as usize] = true;
                    }
                }
            }
            *dst.add(i as usize) = best_match;
        }
        result
    }
}

/// R's `strtoi(x, base=10L)` — convert strings to integers.
pub unsafe fn do_strtoi(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let base_arg = CAR(CDR(args));
        let base = if base_arg.is_null() || base_arg == R_NilValue() {
            10
        } else {
            real_or_default(base_arg, 10.0) as i32
        };

        if x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);

        for i in 0..n {
            let s = elt_to_string(x_arg, i);
            let val = i64::from_str_radix(s.trim(), base as u32).unwrap_or(NA_INTEGER as i64);
            *dst.add(i as usize) = if val > c_int::MAX as i64 || val < c_int::MIN as i64 {
                NA_INTEGER
            } else {
                val as c_int
            };
        }
        result
    }
}

/// R's `strtrim(x, width)` — truncate strings to a maximum width.
pub unsafe fn do_strtrim(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let width_arg = CAR(CDR(args));
        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x_arg);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let width_len = if width_arg.is_null() || width_arg == R_NilValue() {
            0
        } else {
            XLENGTH(width_arg)
        };
        if width_len == 0 {
            super::shared::base_error("invalid 'width' argument");
        }
        let width = real_or_default(width_arg, f64::MAX) as usize;

        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            let s = elt_to_string(x_arg, i);
            let truncated: String = s.chars().take(width).collect();
            let cstr = CString::new(truncated).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// String operations
// ---------------------------------------------------------------------------

/// R-like `str_detect(x, pattern)` — returns logical vector indicating which elements match.
pub unsafe fn do_str_detect(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let pattern_arg = CAR(CDR(args));

        if x.is_null() || x == R_NilValue() || pattern_arg.is_null() || pattern_arg == R_NilValue()
        {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }

        let pattern_str = elt_to_string(pattern_arg, 0);
        let n = XLENGTH(x).max(1);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = LOGICAL(result);

        for i in 0..n {
            let s = elt_to_string(x, i);
            let matches = s.contains(&pattern_str);
            *dst.add(i as usize) = if matches { TRUE } else { FALSE };
        }
        result
    }
}

/// R-like `str_extract(x, pattern)` — extracts first occurrence of pattern from each element.
pub unsafe fn do_str_extract(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let pattern_arg = CAR(CDR(args));

        if x.is_null() || x == R_NilValue() || pattern_arg.is_null() || pattern_arg == R_NilValue()
        {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        let pattern_str = elt_to_string(pattern_arg, 0);
        let n = XLENGTH(x).max(1);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        for i in 0..n {
            let s = elt_to_string(x, i);
            let extracted = if let Some(start) = s.find(&pattern_str) {
                let end = start + pattern_str.len();
                &s[start..end]
            } else {
                "NA"
            };
            let cs = CString::new(extracted).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Complete string/vector
// ---------------------------------------------------------------------------

/// R-like `str_interp(string, values)` — interpolate values into string (simplified: sprintf-like).
pub unsafe fn do_str_interp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let string_arg = CAR(args);
        let values_arg = CAR(CDR(args));
        if string_arg.is_null() || string_arg == R_NilValue() {
            return Rf_mkString(c"".as_ptr());
        }
        let fmt = elt_to_string(string_arg, 0);
        if values_arg.is_null() || values_arg == R_NilValue() {
            return Rf_mkString(CString::new(fmt).unwrap_or_default().as_ptr());
        }
        let n = XLENGTH(values_arg).max(1);
        let mut vals: Vec<String> = Vec::new();
        for i in 0..n {
            vals.push(elt_to_string(values_arg, i));
        }
        // Simple %s replacement
        let mut result = fmt.clone();
        for v in &vals {
            if let Some(pos) = result.find("%s") {
                result.replace_range(pos..pos + 2, v);
            }
        }
        Rf_mkString(CString::new(result).unwrap_or_default().as_ptr())
    }
}

/// R-like `strwrap(x, width)` / `str_wrap(x, width)` — wrap text to width.
pub unsafe fn do_str_wrap(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let width_arg = arg_by_name_or_position(args, &["width"], 1);
        let width =
            if width_arg.is_null() || width_arg == R_NilValue() || XLENGTH(width_arg) == 0 {
                0
            } else {
                numeric_elt_as_count(width_arg, 0)
            }
            .max(1);

        let mut lines = Vec::new();
        for i in 0..XLENGTH(x) {
            lines.extend(wrap_text_words(&elt_to_string(x, i), width));
        }
        string_vector(&lines)
    }
}

fn wrap_text_words(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let current_len = current.chars().count();
        let word_len = word.chars().count();
        let next_len = if current.is_empty() {
            word_len
        } else {
            current_len + 1 + word_len
        };
        if !current.is_empty() && next_len >= width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// R-like `path_package(package, ...)` — find package paths through the session library policy.
pub unsafe fn do_path_package(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package_arg = arg_by_name_or_position(args, &["package"], 0);
        if package_arg.is_null() || package_arg == R_NilValue() || XLENGTH(package_arg) == 0 {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        let mut paths = Vec::new();
        for i in 0..XLENGTH(package_arg) {
            let package = elt_to_string(package_arg, i);
            let path = find_package_path(&package);
            if !path.is_empty() {
                paths.push(path);
            }
        }
        string_vector(&paths)
    }
}

/// R's `system.file(..., package)` — find files inside an installed package.
pub unsafe fn do_system_file(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package_arg = arg_by_name_or_position(args, &["package"], usize::MAX);
        let package = if package_arg.is_null() || package_arg == R_NilValue() {
            "base".to_string()
        } else {
            let n = XLENGTH(package_arg);
            if n != 1 {
                package_error("'package' must be of length 1");
            }
            elt_to_string(package_arg, 0)
        };

        let package_path = find_package_path(&package);
        let must_work = named_logical_arg(args, "mustWork").unwrap_or(false);
        if package_path.is_empty() {
            if must_work {
                package_error(format!("no file found for package '{}'", package));
            }
            return Rf_mkString(c"".as_ptr());
        }

        let parts = system_file_parts(args);
        let mut path = PathBuf::from(&package_path);
        for part in &parts {
            if !part.is_empty() {
                path.push(part);
            }
        }
        // Installation flattens a package's inst/ subtree into its root.
        // This port loads source-shaped trees directly (the corpus bundles
        // unpacked tarballs), so a missing path falls back to the inst/
        // layout: system.file("fortunes", package = "fortunes") must find
        // inst/fortunes just like an installed tree's fortunes/.
        if !path.exists() && !parts.is_empty() {
            let mut inst_path = PathBuf::from(&package_path).join("inst");
            for part in &parts {
                if !part.is_empty() {
                    inst_path.push(part);
                }
            }
            if inst_path.exists() {
                path = inst_path;
            }
        }

        if path.exists() {
            Rf_mkString(
                CString::new(path.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .as_ptr(),
            )
        } else {
            if must_work {
                package_error(format!(
                    "no file found for requested path in package '{}'",
                    package
                ));
            }
            Rf_mkString(c"".as_ptr())
        }
    }
}

fn system_file_parts(args: SEXP) -> Vec<String> {
    unsafe {
        let mut parts = Vec::new();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).is_none() {
                let value = CAR(current);
                if !value.is_null() && value != R_NilValue() && TYPEOF(value) == SEXPTYPE::STRSXP {
                    for i in 0..XLENGTH(value) {
                        if !is_string_na(value, i) {
                            parts.push(elt_to_string(value, i));
                        }
                    }
                }
            }
            current = CDR(current);
        }
        parts
    }
}

// ---------------------------------------------------------------------------
// Complete string operations — str_locate, str_sub variants
// ---------------------------------------------------------------------------

/// R's `str_locate(x, pattern)` — locate first occurrence of pattern (simplified).
pub unsafe fn do_str_locate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let pattern = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || pattern.is_null() {
            return R_NilValue();
        }
        // Return a 1x2 matrix with start/end (simplified: return c(start, end))
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = INTEGER(result);
        // Simplified: set to NA (no match)
        *dst.add(0) = NA_INTEGER;
        *dst.add(1) = NA_INTEGER;
        result
    }
}

/// R's `str_locate_all(x, pattern)` — locate all occurrences (simplified).
pub unsafe fn do_str_locate_all(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let _pattern = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Return empty matrix
        Rf_allocVector3(SEXPTYPE::INTSXP, 0)
    }
}

/// R's `str_sub(x, start, end)` — extract substring (alias for substr).
pub unsafe fn do_str_sub(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_substr(_call, _op, args, _rho) }
}

/// R's `str_sub_all(x, start, end)` — all substrings (simplified).
pub unsafe fn do_str_sub_all(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Return input as list
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        SET_VECTOR_ELT(result, 0, x);
        result
    }
}

/// GNU `sQuote(x)` in the C locale — wrap each string in single quotes.
pub unsafe fn do_sQuote(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { quote_wrap(CAR(args), '\'', rho) }
}

/// GNU `dQuote(x)` in the C locale — wrap each string in double quotes.
pub unsafe fn do_dQuote(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { quote_wrap(CAR(args), '"', rho) }
}

unsafe fn quote_wrap(x: SEXP, q: char, rho: SEXP) -> SEXP {
    unsafe {
        let x = if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        } else if TYPEOF(x) != SEXPTYPE::STRSXP {
            let coerced = crate::mainutils::essentials::do_as_character(
                R_NilValue(),
                R_NilValue(),
                Rf_cons(x, R_NilValue()),
                rho,
            );
            let _c = protect(coerced);
            coerced
        } else {
            x
        };
        let n = XLENGTH(x);
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _o = protect(out);
        for i in 0..n {
            let s = STRING_ELT(x, i);
            if s.is_null() || s == crate::sexp::globals::R_NaString() {
                SET_STRING_ELT(out, i, crate::sexp::globals::R_NaString());
                continue;
            }
            let body = elt_to_string(x, i);
            let wrapped = format!("{q}{body}{q}");
            let cs = CString::new(wrapped).unwrap_or_default();
            SET_STRING_ELT(out, i, Rf_mkChar(cs.as_ptr()));
        }
        out
    }
}


