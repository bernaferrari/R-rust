//! nchar/substr/substring family, case conversion, file extensions

use super::*;

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
        let mut keep_na_seen = false;
        let mut keep_na = false;
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
            let slot = named.as_deref();
            let is_type = slot == Some("type") || (slot.is_none() && positional == 0);
            let is_keep_na = slot == Some("keepNA") || (slot.is_none() && positional == 2);
            if is_type && TYPEOF(value) == SEXPTYPE::STRSXP && XLENGTH(value) >= 1 {
                let text = elt_to_string(value, 0);
                nchar_type = match text.as_str() {
                    t if t.starts_with('b') => NcharKind::Bytes,
                    t if t.starts_with('w') => NcharKind::Width,
                    _ => NcharKind::Chars,
                };
            }
            if is_keep_na {
                let l = crate::mainutils::seq::asLogical(value);
                if l == crate::sexp::ffi::NA_LOGICAL {
                    keep_na = !matches!(nchar_type, NcharKind::Width);
                } else {
                    keep_na = l != 0;
                }
                keep_na_seen = true;
            }
            if slot.is_none() {
                positional += 1;
            }
            cell = CDR(cell);
        }
        if !keep_na_seen {
            keep_na = !matches!(nchar_type, NcharKind::Width);
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
                    // GNU R_nchar: NA_STRING is NA_INTEGER when keepNA,
                    // else the two characters of "NA".
                    *dst.add(i as usize) = if keep_na { NA_INTEGER } else { 2 };
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
    crate::mainutils::unicode_width::columns(ch)
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
            let chars: Vec<char> = s.chars().collect();
            let si = if nstart == 0 { 0 } else { i % nstart };
            let ei = if nstop == 0 { 0 } else { i % nstop };
            let start = (real_elt_or_default(start_arg, si, 1.0) as usize).max(1) - 1;
            let missing = crate::sexp::globals::R_MissingArg();
            let stop_open = stop_arg.is_null() || stop_arg == R_NilValue() || stop_arg == missing;
            let stop = if stop_open {
                chars.len()
            } else {
                real_elt_or_default(stop_arg, ei, chars.len() as f64) as usize
            };
            let end = stop.min(chars.len());
            let sub: String = if start < end {
                chars[start..end].iter().collect()
            } else {
                String::new()
            };
            let cstr = CString::new(sub).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                crate::sexp::accessors::SET_STRING_ELT(result, i as R_xlen_t, charsxp);
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
        let nv = if value.is_null() || value == R_NilValue() || TYPEOF(value) != SEXPTYPE::STRSXP {
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
                SET_STRING_ELT(
                    result,
                    i,
                    crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()),
                );
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
            SET_STRING_ELT(
                result,
                i,
                crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()),
            );
        }
        let at = crate::sexp::accessors::ATTRIB(x);
        if !at.is_null() && at != R_NilValue() {
            crate::sexp::accessors::SET_ATTRIB(
                result,
                crate::mainutils::duplicate::Rf_duplicate(at),
            );
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
                crate::sexp::accessors::SET_STRING_ELT(result, i as R_xlen_t, charsxp);
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
