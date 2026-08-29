//! Essentials domain module `strings` — extracted verbatim from essentials.rs.

use super::*;
use std::ffi::CString;
use std::os::raw::c_int;
use std::path::PathBuf;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
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

/// R's `nchar(x)` — number of characters in strings.
pub unsafe fn do_nchar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
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
                    *dst.add(i as usize) = NA_INTEGER;
                    continue;
                }
            }
            *dst.add(i as usize) = elt_to_string(x, i).len() as c_int;
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_substr — substring extraction
// ---------------------------------------------------------------------------

/// R's `substr(x, start, stop)` — extract substrings.
pub unsafe fn do_substr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let start_arg = CAR(CDR(args));
        let stop_arg = CAR(CDR(CDR(args)));

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            if TYPEOF(x) == SEXPTYPE::STRSXP {
                let idx = if XLENGTH(x) == 0 { 0 } else { i % XLENGTH(x) };
                if STRING_ELT(x, idx) == crate::sexp::globals::R_NaString() {
                    SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                    continue;
                }
            }
            let s = elt_to_string(x, i);
            let start = (real_elt_or_default(start_arg, i, 1.0) as usize).max(1) - 1;
            let stop = real_elt_or_default(stop_arg, i, 1000.0) as usize;
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

/// R's `grep(pattern, x, ..., value = FALSE)` for fixed and ERE matching.
pub unsafe fn do_grep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern_arg = arg_by_name_or_position(args, &["pattern"], 0);
        let x_arg = arg_by_name_or_position(args, &["x", "text"], 1);
        if pattern_arg.is_null() || x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
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
        for i in 0..n {
            if is_string_na(x_arg, i) {
                *dst.add(i as usize) = FALSE;
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
        let value = named_logical_arg(args, "value").unwrap_or(false);
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let max_distance = agrep_max_distance(args, pattern_arg);
        let pattern = elt_to_string(pattern_arg, 0);
        let matches = agrep_match_indices(x_arg, &pattern, max_distance, ignore_case);

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
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
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
        if value <= 1.0 {
            let pattern_len = elt_to_string(pattern_arg, 0).chars().count().max(1);
            (value * pattern_len as f64).ceil() as usize
        } else {
            value.ceil() as usize
        }
    }
}

unsafe fn agrep_match_indices(
    x: SEXP,
    pattern: &str,
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
            if approximate_contains(pattern, &elt_to_string(x, i), max_distance, ignore_case) {
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
        for i in 0..n {
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
        result
    }
}

/// R's `strsplit(x, split)` — split strings by separator, return list.
pub unsafe fn do_strsplit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let split_arg = CAR(CDR(args));
        if x_arg.is_null() || x_arg == R_NilValue() || split_arg.is_null() {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let split = elt_to_string(split_arg, 0);
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let s = elt_to_string(x_arg, i);
            let parts: Vec<&str> = if split.is_empty() {
                s.split("").filter(|p| !p.is_empty()).collect()
            } else {
                s.split(&split).collect()
            };
            let vec = Rf_allocVector3(SEXPTYPE::STRSXP, parts.len() as R_xlen_t);
            if !vec.is_null() {
                let _vec_guard = protect(vec);
                for (j, part) in parts.iter().enumerate() {
                    let cstr = CString::new(*part).unwrap_or_default();
                    let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                    if !charsxp.is_null() {
                        let data = (*vec).gengc_next_node as *mut SEXP;
                        *data.add(j) = charsxp;
                    }
                }
            }
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, vec);
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
            return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
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

/// R's `format.info(x, digits, nsmall)` width metadata.
pub unsafe fn do_format_info(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let digits = arg_by_name_or_position(args, &["digits"], 1);
        let digits = if digits.is_null() {
            R_NilValue()
        } else {
            digits
        };
        let nsmall = arg_by_name_or_position(args, &["nsmall"], 2);
        let nsmall = if nsmall.is_null() || nsmall == R_NilValue() {
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
            return Rf_ScalarLogical(FALSE);
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

/// R's `is.environment(x)` — TRUE for ENVSXP.
pub unsafe fn do_is_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::ENVSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

// ---------------------------------------------------------------------------
// String formatting
// ---------------------------------------------------------------------------

/// R's `noquote(x)` — mark object to prevent quoting in print.
pub unsafe fn do_noquote(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !class_vec.is_null() {
            let cstr = CString::new("noquote").unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*class_vec).gengc_next_node as *mut SEXP;
                *data.add(0) = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(
                x,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class_vec,
            );
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
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
        let width = if width_arg.is_null() || width_arg == R_NilValue() {
            usize::MAX
        } else {
            real_or_default(width_arg, f64::MAX) as usize
        };

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
            return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
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
            return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
        }

        let mut path = PathBuf::from(package_path);
        for part in system_file_parts(args) {
            if !part.is_empty() {
                path.push(part);
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
            Rf_mkString(CString::new("").unwrap_or_default().as_ptr())
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
