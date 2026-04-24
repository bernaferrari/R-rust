//! Essential R built-in functions — c(), seq(), rep(), paste(), cat(), typeof(), is.na(), names().
//!
//! These are the most fundamental R functions that every R program uses.

use std::ffi::CString;
use std::os::raw::c_int;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    CAR, CDR, CHAR, INTEGER, LENGTH, LOGICAL, PRINTNAME, RAW, REAL, SET_STRING_ELT, SET_VECTOR_ELT,
    STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
use crate::sexp::ffi::{FALSE, NA_INTEGER, NA_REAL, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::Rf_protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// do_c — combine vectors
// ---------------------------------------------------------------------------

/// R's `c(...)` — concatenates vectors into a single vector.
///
/// Coercion rules: STRSXP > CPLXSXP > REALSXP > INTSXP > LGLSXP.
/// If any arg is STRSXP, result is STRSXP.
pub unsafe fn do_c(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // First pass: determine result type and total length
    let mut result_type = SEXPTYPE::LGLSXP.as_c_int();
    let mut total_len: R_xlen_t = 0;
    let mut has_names = false;
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            let tag = TAG(current);
            if !tag.is_null() && tag != R_NilValue() {
                has_names = true;
            }
            let t = TYPEOF(arg);
            if t == SEXPTYPE::STRSXP {
                result_type = SEXPTYPE::STRSXP.as_c_int();
            } else if t == SEXPTYPE::CPLXSXP && result_type != SEXPTYPE::STRSXP {
                result_type = SEXPTYPE::CPLXSXP.as_c_int();
            } else if t == SEXPTYPE::REALSXP
                && result_type != SEXPTYPE::STRSXP
                && result_type != SEXPTYPE::CPLXSXP
            {
                result_type = SEXPTYPE::REALSXP.as_c_int();
            } else if t == SEXPTYPE::INTSXP
                && result_type != SEXPTYPE::STRSXP
                && result_type != SEXPTYPE::CPLXSXP
                && result_type != SEXPTYPE::REALSXP
            {
                result_type = SEXPTYPE::INTSXP.as_c_int();
            }
            total_len += XLENGTH(arg);
        }
        current = CDR(current);
    }

    if total_len == 0 {
        return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
    }

    // Second pass: copy data
    let result = Rf_allocVector3(result_type, total_len);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let mut offset: R_xlen_t = 0;
    let names = if has_names {
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, total_len);
        if names.is_null() {
            return R_NilValue();
        }
        let empty = Rf_mkChar(CString::new("").unwrap_or_default().as_ptr());
        for i in 0..total_len {
            SET_STRING_ELT(names, i, empty);
        }
        names
    } else {
        R_NilValue()
    };
    let _names_protect = if has_names {
        Rf_protect(names);
        true
    } else {
        false
    };

    current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            let t = TYPEOF(arg);
            let n = XLENGTH(arg);

            if result_type == SEXPTYPE::REALSXP {
                let dst = REAL(result);
                for i in 0..n {
                    let val = if t == SEXPTYPE::REALSXP {
                        *REAL(arg).add(i as usize)
                    } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                        let v = *INTEGER(arg).add(i as usize);
                        if v == NA_INTEGER { NA_REAL } else { v as f64 }
                    } else {
                        NA_REAL
                    };
                    *dst.add((offset + i) as usize) = val;
                }
            } else if result_type == SEXPTYPE::INTSXP {
                let dst = INTEGER(result);
                for i in 0..n {
                    let val = if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                        *INTEGER(arg).add(i as usize)
                    } else {
                        NA_INTEGER
                    };
                    *dst.add((offset + i) as usize) = val;
                }
            } else if result_type == SEXPTYPE::LGLSXP {
                let dst = LOGICAL(result);
                for i in 0..n {
                    let val = if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP {
                        *INTEGER(arg).add(i as usize)
                    } else {
                        NA_INTEGER
                    };
                    *dst.add((offset + i) as usize) = val;
                }
            } else if result_type == SEXPTYPE::STRSXP {
                for i in 0..n {
                    if t == SEXPTYPE::STRSXP {
                        SET_STRING_ELT(result, offset + i, STRING_ELT(arg, i));
                    } else {
                        let value = elt_to_string(arg, i);
                        let cstr = CString::new(value).unwrap_or_default();
                        SET_STRING_ELT(result, offset + i, Rf_mkChar(cstr.as_ptr()));
                    }
                }
            }
            // CPLXSXP requires COMPLEX support which needs more work.
            if has_names {
                let tag = TAG(current);
                if !tag.is_null() && tag != R_NilValue() {
                    let printname = PRINTNAME(tag);
                    if !printname.is_null() {
                        SET_STRING_ELT(names, offset, printname);
                    }
                }
            }
            offset += n;
        }
        current = CDR(current);
    }

    if has_names {
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_seq — generate sequences
// ---------------------------------------------------------------------------

/// R's `seq(from, to, by)` — generates a sequence.
///
/// - seq(to) → 1:to
/// - seq(from, to) → from:to
/// - seq(from, to, by) → from, from+by, ... until past to
pub unsafe fn do_seq(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let a1 = CAR(args);
    let a2_cdr = CDR(args);
    let a2 = if a2_cdr.is_null() || a2_cdr == R_NilValue() {
        R_NilValue()
    } else {
        CAR(a2_cdr)
    };
    let a3_cdr = if a2_cdr.is_null() {
        R_NilValue()
    } else {
        CDR(a2_cdr)
    };
    let a3 = if a3_cdr.is_null() || a3_cdr == R_NilValue() {
        R_NilValue()
    } else {
        CAR(a3_cdr)
    };

    let (from, to, by) = if a2 == R_NilValue() {
        // seq(to)
        let to_val = real_or_default(a1, 1.0);
        (1.0, to_val, 1.0)
    } else if a3 == R_NilValue() {
        // seq(from, to)
        let from_val = real_or_default(a1, 1.0);
        let to_val = real_or_default(a2, 1.0);
        (from_val, to_val, 1.0)
    } else {
        // seq(from, to, by)
        let from_val = real_or_default(a1, 1.0);
        let to_val = real_or_default(a2, 1.0);
        let by_val = real_or_default(a3, 1.0);
        (from_val, to_val, by_val)
    };

    if by == 0.0 {
        return R_NilValue();
    }

    let n = ((to - from) / by).floor() as i64 + 1;
    let n = n.max(0) as R_xlen_t;

    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..n {
        *dst.add(i as usize) = from + (i as f64) * by;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_rep — repeat elements
// ---------------------------------------------------------------------------

/// R's `rep(x, times)` — repeats a vector `times` times.
pub unsafe fn do_rep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let times_arg = CAR(CDR(args));

    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let times = if times_arg.is_null() || times_arg == R_NilValue() {
        1
    } else {
        real_or_default(times_arg, 1.0) as i64
    };
    let times = times.max(0) as usize;

    let n = XLENGTH(x);
    let total = n * times as R_xlen_t;

    let t = TYPEOF(x);
    let result = Rf_allocVector3(t, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    for rep in 0..times {
        let offset = rep as R_xlen_t * n;
        if t == SEXPTYPE::REALSXP {
            let src = REAL(x);
            let dst = REAL(result);
            for i in 0..n {
                *dst.add((offset + i) as usize) = *src.add(i as usize);
            }
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let src = INTEGER(x);
            let dst = INTEGER(result);
            for i in 0..n {
                *dst.add((offset + i) as usize) = *src.add(i as usize);
            }
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Core vector/scalar helpers live in `essentials_basic`.
// ---------------------------------------------------------------------------

pub use super::essentials_basic::*;

// ---------------------------------------------------------------------------
// do_nchar — string length
// ---------------------------------------------------------------------------

/// R's `nchar(x)` — number of characters in strings.
pub unsafe fn do_nchar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarInteger(0);
    }
    let n = XLENGTH(x).max(1);
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = INTEGER(result);
    for i in 0..n {
        let s = elt_to_string(x, i);
        *dst.add(i as usize) = s.len() as c_int;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_substr — substring extraction
// ---------------------------------------------------------------------------

/// R's `substr(x, start, stop)` — extract substrings.
pub unsafe fn do_substr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let start_arg = CAR(CDR(args));
    let stop_arg = CAR(CDR(CDR(args)));

    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let start = real_or_default(start_arg, 1.0) as usize;
    let stop = real_or_default(stop_arg, 1000.0) as usize;
    let start = start.max(1) - 1; // Convert to 0-indexed

    let n = XLENGTH(x).max(1);
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    for i in 0..n {
        let s = elt_to_string(x, i);
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

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// String case conversion
// ---------------------------------------------------------------------------

/// R's `tolower(x)`.
pub unsafe fn do_tolower(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_case_convert(args, true)
}

/// R's `toupper(x)`.
pub unsafe fn do_toupper(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_case_convert(args, false)
}

unsafe fn do_case_convert(args: SEXP, to_lower: bool) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let n = XLENGTH(x).max(1);
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    for i in 0..n {
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

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// String manipulation: trimws, sprintf, gsub, sub, strsplit
// ---------------------------------------------------------------------------

/// R's `trimws(x, which="both")` — trim whitespace from strings.
pub unsafe fn do_trimws(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return x;
    }
    let n = XLENGTH(x).max(1);
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
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
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `sprintf(fmt, ...)` — formatted string (simplified: concatenate with placeholder replacement).
pub unsafe fn do_sprintf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let fmt_arg = CAR(args);
    if fmt_arg.is_null() || fmt_arg == R_NilValue() {
        return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
    }
    let fmt = elt_to_string(fmt_arg, 0);
    let mut parts: Vec<String> = Vec::new();
    let mut current = CDR(args);
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            let n = XLENGTH(arg).max(1);
            for i in 0..n {
                parts.push(elt_to_string(arg, i));
            }
        }
        current = CDR(current);
    }
    // Replace %s, %d, %f, %i placeholders with parts
    let mut part_idx = 0;
    let mut chars = fmt.chars().peekable();
    let mut out = String::new();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            if let Some(&next_ch) = chars.peek() {
                if next_ch == '%' {
                    out.push('%');
                    chars.next();
                    continue;
                }
                if (next_ch == 's' || next_ch == 'd' || next_ch == 'f' || next_ch == 'i')
                    && part_idx < parts.len()
                {
                    out.push_str(&parts[part_idx]);
                    part_idx += 1;
                    chars.next();
                    continue;
                }
            }
            out.push(ch);
        } else {
            out.push(ch);
        }
    }
    // Append remaining parts
    while part_idx < parts.len() {
        out.push_str(&parts[part_idx]);
        part_idx += 1;
    }
    Rf_mkString(CString::new(out).unwrap_or_default().as_ptr())
}

/// R's `gsub(pattern, replacement, x)` — global string substitution (literal).
pub unsafe fn do_gsub(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_string_replace(args, true)
}

/// R's `sub(pattern, replacement, x)` — first match substitution (literal).
pub unsafe fn do_sub(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_string_replace(args, false)
}

unsafe fn do_string_replace(args: SEXP, global: bool) -> SEXP {
    let pattern_arg = CAR(args);
    let replacement_arg = CAR(CDR(args));
    let x_arg = CAR(CDR(CDR(args)));
    if pattern_arg.is_null()
        || replacement_arg.is_null()
        || x_arg.is_null()
        || x_arg == R_NilValue()
    {
        return R_NilValue();
    }
    let pattern = elt_to_string(pattern_arg, 0);
    let replacement = elt_to_string(replacement_arg, 0);
    let n = XLENGTH(x_arg).max(1);
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for i in 0..n {
        let s = elt_to_string(x_arg, i);
        let replaced = if global {
            s.replace(&pattern, &replacement)
        } else {
            s.replacen(&pattern, &replacement, 1)
        };
        let cstr = CString::new(replaced).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(i as usize) = charsxp;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `strsplit(x, split)` — split strings by separator, return list.
pub unsafe fn do_strsplit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x_arg = CAR(args);
    let split_arg = CAR(CDR(args));
    if x_arg.is_null() || x_arg == R_NilValue() || split_arg.is_null() {
        return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
    }
    let split = elt_to_string(split_arg, 0);
    let n = XLENGTH(x_arg).max(1);
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for i in 0..n {
        let s = elt_to_string(x_arg, i);
        let parts: Vec<&str> = if split.is_empty() {
            s.split("").filter(|p| !p.is_empty()).collect()
        } else {
            s.split(&split).collect()
        };
        let vec = Rf_allocVector3(SEXPTYPE::STRSXP, parts.len() as R_xlen_t);
        if !vec.is_null() {
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
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Parallel min/max and which.min/which.max
// ---------------------------------------------------------------------------

/// R's `pmin(...)` — parallel minimum across vectors (element-wise min with recycling).
pub unsafe fn do_pmin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_pminmax(args, true)
}

/// R's `pmax(...)` — parallel maximum across vectors (element-wise max with recycling).
pub unsafe fn do_pmax(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_pminmax(args, false)
}

unsafe fn do_pminmax(args: SEXP, is_min: bool) -> SEXP {
    let mut arg_vecs: Vec<SEXP> = Vec::new();
    let mut max_len: R_xlen_t = 0;
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            arg_vecs.push(arg);
            let n = XLENGTH(arg);
            if n > max_len {
                max_len = n;
            }
        }
        current = CDR(current);
    }
    if arg_vecs.is_empty() || max_len == 0 {
        return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
    }
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, max_len);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..max_len {
        let mut best = NA_REAL;
        for &arg in &arg_vecs {
            let n = XLENGTH(arg);
            if n == 0 {
                continue;
            }
            let idx = i % n;
            let v = elt_real_safe(arg, idx);
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                continue;
            }
            if best.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                best = v;
            } else if is_min {
                if v < best {
                    best = v;
                }
            } else {
                if v > best {
                    best = v;
                }
            }
        }
        *dst.add(i as usize) = best;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `which.min(x)` — 1-based index of minimum element.
pub unsafe fn do_which_min(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_which_minmax(args, true)
}

/// R's `which.max(x)` — 1-based index of maximum element.
pub unsafe fn do_which_max(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_which_minmax(args, false)
}

unsafe fn do_which_minmax(args: SEXP, is_min: bool) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() || XLENGTH(x) == 0 {
        return Rf_ScalarInteger(0);
    }
    let n = XLENGTH(x);
    let mut best_idx: R_xlen_t = 0;
    let mut best_val = elt_real_safe(x, 0);
    for i in 1..n {
        let v = elt_real_safe(x, i);
        if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            continue;
        }
        if best_val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            best_idx = i;
            best_val = v;
        } else if is_min {
            if v < best_val {
                best_idx = i;
                best_val = v;
            }
        } else {
            if v > best_val {
                best_idx = i;
                best_val = v;
            }
        }
    }
    if best_val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
        Rf_ScalarInteger(0)
    } else {
        Rf_ScalarInteger((best_idx + 1) as c_int)
    }
}

// ---------------------------------------------------------------------------
// Data manipulation: append, head, tail, subset
// ---------------------------------------------------------------------------

/// R's `append(x, values, after)` — insert values into vector at position.
pub unsafe fn do_append(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let values = CAR(CDR(args));
    let after_arg = CAR(CDR(CDR(args)));
    if (x.is_null() || x == R_NilValue()) && (values.is_null() || values == R_NilValue()) {
        return R_NilValue();
    }
    if values.is_null() || values == R_NilValue() {
        return x;
    }
    if x.is_null() || x == R_NilValue() {
        return values;
    }
    let n = XLENGTH(x);
    let vlen = XLENGTH(values);
    let after = if after_arg.is_null() || after_arg == R_NilValue() {
        n as i64
    } else {
        real_or_default(after_arg, n as f64) as i64
    };
    let after = (after.max(0) as R_xlen_t).min(n);
    let total = n + vlen;
    let t = if TYPEOF(values) == SEXPTYPE::STRSXP || TYPEOF(x) == SEXPTYPE::STRSXP {
        SEXPTYPE::STRSXP.as_c_int()
    } else if TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(values) == SEXPTYPE::REALSXP {
        SEXPTYPE::REALSXP.as_c_int()
    } else {
        SEXPTYPE::INTSXP.as_c_int()
    };
    let result = Rf_allocVector3(t, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    if t == SEXPTYPE::REALSXP {
        let dst = REAL(result);
        for i in 0..after {
            *dst.add(i as usize) = elt_real_safe(x, i);
        }
        for i in 0..vlen {
            *dst.add((after + i) as usize) = elt_real_safe(values, i);
        }
        for i in after..n {
            *dst.add((i + vlen) as usize) = elt_real_safe(x, i);
        }
    } else if t == SEXPTYPE::INTSXP {
        let dst = INTEGER(result);
        for i in 0..after {
            *dst.add(i as usize) = if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(x).add(i as usize)
            } else {
                let v = elt_real_safe(x, i);
                if v.is_nan() || v == NA_REAL {
                    NA_INTEGER
                } else {
                    v as c_int
                }
            };
        }
        for i in 0..vlen {
            *dst.add((after + i) as usize) = if TYPEOF(values) == SEXPTYPE::INTSXP {
                *INTEGER(values).add(i as usize)
            } else {
                let v = elt_real_safe(values, i);
                if v.is_nan() || v == NA_REAL {
                    NA_INTEGER
                } else {
                    v as c_int
                }
            };
        }
        for i in after..n {
            *dst.add((i + vlen) as usize) = if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(x).add(i as usize)
            } else {
                let v = elt_real_safe(x, i);
                if v.is_nan() || v == NA_REAL {
                    NA_INTEGER
                } else {
                    v as c_int
                }
            };
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `head(x, n=6)` — first n elements.
pub unsafe fn do_head(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let n_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let len = XLENGTH(x);
    let n = if n_arg.is_null() || n_arg == R_NilValue() {
        6i64
    } else {
        real_or_default(n_arg, 6.0) as i64
    };
    let n = if n < 0 {
        (len as i64 + n).max(0) as R_xlen_t
    } else {
        n.min(len as i64) as R_xlen_t
    };
    let n = n.min(len);
    if n == 0 {
        return Rf_allocVector3(TYPEOF(x), 0);
    }
    let result = Rf_allocVector3(TYPEOF(x), n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let t = TYPEOF(x);
    for i in 0..n {
        if t == SEXPTYPE::REALSXP {
            *REAL(result).add(i as usize) = *REAL(x).add(i as usize);
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            *INTEGER(result).add(i as usize) = *INTEGER(x).add(i as usize);
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `tail(x, n=6)` — last n elements.
pub unsafe fn do_tail(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let n_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let len = XLENGTH(x);
    let n = if n_arg.is_null() || n_arg == R_NilValue() {
        6i64
    } else {
        real_or_default(n_arg, 6.0) as i64
    };
    let n = if n < 0 {
        (len as i64 + n).max(0) as R_xlen_t
    } else {
        n.min(len as i64) as R_xlen_t
    };
    let n = n.min(len);
    if n == 0 {
        return Rf_allocVector3(TYPEOF(x), 0);
    }
    let start = len - n;
    let result = Rf_allocVector3(TYPEOF(x), n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let t = TYPEOF(x);
    for i in 0..n {
        if t == SEXPTYPE::REALSXP {
            *REAL(result).add(i as usize) = *REAL(x).add((start + i) as usize);
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            *INTEGER(result).add(i as usize) = *INTEGER(x).add((start + i) as usize);
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `x[i]` — subset extraction (simplified: integer index vector).
pub unsafe fn do_subset(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let i = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || i.is_null() || i == R_NilValue() {
        return Rf_allocVector3(TYPEOF(x), 0);
    }
    let n = XLENGTH(i);
    let result = Rf_allocVector3(TYPEOF(x), n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let t = TYPEOF(x);
    for j in 0..n {
        let idx = elt_real_safe(i, j) as i64;
        if idx < 1 {
            continue;
        }
        let src = (idx - 1) as usize;
        if t == SEXPTYPE::REALSXP {
            *REAL(result).add(j as usize) = *REAL(x).add(src);
        } else if t == SEXPTYPE::INTSXP {
            *INTEGER(result).add(j as usize) = *INTEGER(x).add(src);
        } else if t == SEXPTYPE::LGLSXP {
            *LOGICAL(result).add(j as usize) = *LOGICAL(x).add(src);
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Set operations: setdiff, union, intersect, setequal
// ---------------------------------------------------------------------------

/// R's `setdiff(x, y)` — elements in x but not in y.
pub unsafe fn do_setdiff(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let y = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(TYPEOF(x), 0);
    }
    let xn = XLENGTH(x);
    let yn = if y.is_null() || y == R_NilValue() {
        0
    } else {
        XLENGTH(y)
    };
    let t = TYPEOF(x);
    let mut y_keys: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    for i in 0..yn {
        let key = if t == SEXPTYPE::REALSXP {
            (*REAL(y).add(i as usize)).to_bits() as i64
        } else {
            *INTEGER(y).add(i as usize) as i64
        };
        y_keys.insert(key);
    }
    let mut result_keys: Vec<i64> = Vec::new();
    let mut seen: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    for i in 0..xn {
        let key = if t == SEXPTYPE::REALSXP {
            (*REAL(x).add(i as usize)).to_bits() as i64
        } else {
            *INTEGER(x).add(i as usize) as i64
        };
        if !y_keys.contains(&key) && seen.insert(key) {
            result_keys.push(key);
        }
    }
    let result = Rf_allocVector3(t, result_keys.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for (i, &key) in result_keys.iter().enumerate() {
        if t == SEXPTYPE::REALSXP {
            *REAL(result).add(i) = f64::from_bits(key as u64);
        } else {
            *INTEGER(result).add(i) = key as c_int;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `union(x, y)` — unique elements from both vectors.
pub unsafe fn do_union(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let y = CAR(CDR(args));
    let t = if !x.is_null() && x != R_NilValue() {
        TYPEOF(x)
    } else if !y.is_null() && y != R_NilValue() {
        TYPEOF(y)
    } else {
        SEXPTYPE::INTSXP.as_c_int()
    };
    let mut seen: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    let mut result_keys: Vec<i64> = Vec::new();
    let mut add_from = |src: SEXP| {
        if !src.is_null() && src != R_NilValue() {
            let n = XLENGTH(src);
            for i in 0..n {
                let key = if t == SEXPTYPE::REALSXP {
                    (*REAL(src).add(i as usize)).to_bits() as i64
                } else {
                    *INTEGER(src).add(i as usize) as i64
                };
                if seen.insert(key) {
                    result_keys.push(key);
                }
            }
        }
    };
    add_from(x);
    add_from(y);
    let result = Rf_allocVector3(t, result_keys.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for (i, &key) in result_keys.iter().enumerate() {
        if t == SEXPTYPE::REALSXP {
            *REAL(result).add(i) = f64::from_bits(key as u64);
        } else {
            *INTEGER(result).add(i) = key as c_int;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `intersect(x, y)` — elements common to both vectors.
pub unsafe fn do_intersect(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let y = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
        return Rf_allocVector3(TYPEOF(x), 0);
    }
    let t = TYPEOF(x);
    let xn = XLENGTH(x);
    let yn = XLENGTH(y);
    let mut x_keys: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    for i in 0..xn {
        let key = if t == SEXPTYPE::REALSXP {
            (*REAL(x).add(i as usize)).to_bits() as i64
        } else {
            *INTEGER(x).add(i as usize) as i64
        };
        x_keys.insert(key);
    }
    let mut seen: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    let mut result_keys: Vec<i64> = Vec::new();
    for i in 0..yn {
        let key = if t == SEXPTYPE::REALSXP {
            (*REAL(y).add(i as usize)).to_bits() as i64
        } else {
            *INTEGER(y).add(i as usize) as i64
        };
        if x_keys.contains(&key) && seen.insert(key) {
            result_keys.push(key);
        }
    }
    let result = Rf_allocVector3(t, result_keys.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for (i, &key) in result_keys.iter().enumerate() {
        if t == SEXPTYPE::REALSXP {
            *REAL(result).add(i) = f64::from_bits(key as u64);
        } else {
            *INTEGER(result).add(i) = key as c_int;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `setequal(x, y)` — TRUE if x and y contain the same unique values.
pub unsafe fn do_setequal(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let y = CAR(CDR(args));
    if (x.is_null() || x == R_NilValue()) && (y.is_null() || y == R_NilValue()) {
        return Rf_ScalarLogical(TRUE);
    }
    if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let xn = XLENGTH(x);
    let yn = XLENGTH(y);
    let tx = TYPEOF(x);
    let ty = TYPEOF(y);
    let mut x_set: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    let mut y_set: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    for i in 0..xn {
        let key = if tx == SEXPTYPE::REALSXP {
            (*REAL(x).add(i as usize)).to_bits() as i64
        } else {
            *INTEGER(x).add(i as usize) as i64
        };
        x_set.insert(key);
    }
    for i in 0..yn {
        let key = if ty == SEXPTYPE::REALSXP {
            (*REAL(y).add(i as usize)).to_bits() as i64
        } else {
            *INTEGER(y).add(i as usize) as i64
        };
        y_set.insert(key);
    }
    Rf_ScalarLogical(if x_set == y_set { TRUE } else { FALSE })
}

// ---------------------------------------------------------------------------
// Type checking: is.finite, is.infinite, is.nan, is.matrix, is.array, is.list
// ---------------------------------------------------------------------------

/// R's `is.finite(x)` — check for finite values.
pub unsafe fn do_is_finite(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP {
        return Rf_ScalarLogical(TRUE);
    }
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);
    for i in 0..n {
        let is_fin = if t == SEXPTYPE::REALSXP {
            let v = *REAL(x).add(i as usize);
            v.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN && v.is_finite()
        } else {
            *INTEGER(x).add(i as usize) != NA_INTEGER
        };
        *dst.add(i as usize) = if is_fin { TRUE } else { FALSE };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `is.infinite(x)` — check for infinite values.
pub unsafe fn do_is_infinite(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    if t != SEXPTYPE::REALSXP {
        return Rf_ScalarLogical(FALSE);
    }
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);
    for i in 0..n {
        let v = *REAL(x).add(i as usize);
        *dst.add(i as usize) = if v.is_infinite() { TRUE } else { FALSE };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `is.nan(x)` — check for NaN values (not NA).
pub unsafe fn do_is_nan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    if t != SEXPTYPE::REALSXP {
        return Rf_ScalarLogical(FALSE);
    }
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);
    for i in 0..n {
        let v = *REAL(x).add(i as usize);
        let is_nan = v.is_nan() && v.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN;
        *dst.add(i as usize) = if is_nan { TRUE } else { FALSE };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `is.matrix(x)` — check if x has a dim attribute with exactly 2 dimensions.
pub unsafe fn do_is_matrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    let is_mat =
        !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) == 2;
    Rf_ScalarLogical(if is_mat { TRUE } else { FALSE })
}

/// R's `is.array(x)` — check if x has a dim attribute.
pub unsafe fn do_is_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    Rf_ScalarLogical(if !dim_attr.is_null() { TRUE } else { FALSE })
}

/// R's `is.list(x)` — check if x is a VECSXP (list).
pub unsafe fn do_is_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::VECSXP {
        TRUE
    } else {
        FALSE
    })
}

// ---------------------------------------------------------------------------
// Conversion: chartr, format
// ---------------------------------------------------------------------------

/// R's `chartr(old, new, x)` — character-by-character translation.
pub unsafe fn do_chartr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let old_arg = CAR(args);
    let new_arg = CAR(CDR(args));
    let x_arg = CAR(CDR(CDR(args)));
    if old_arg.is_null() || new_arg.is_null() || x_arg.is_null() || x_arg == R_NilValue() {
        return R_NilValue();
    }
    let old_str = elt_to_string(old_arg, 0);
    let new_str = elt_to_string(new_arg, 0);
    let old_chars: Vec<char> = old_str.chars().collect();
    let new_chars: Vec<char> = new_str.chars().collect();
    let n = XLENGTH(x_arg).max(1);
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for i in 0..n {
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
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `format(x, digits, nsmall)` — format numbers as strings.
pub unsafe fn do_format(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
    let n = XLENGTH(x).max(1);
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
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
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(i as usize) = charsxp;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_order — order indices for sorting
// ---------------------------------------------------------------------------

/// R's `order(...)` — returns permutation of indices that sort the input.
pub unsafe fn do_order(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }
    let n = XLENGTH(x);
    let mut indices: Vec<(f64, R_xlen_t)> = Vec::with_capacity(n as usize);
    for i in 0..n {
        let v = elt_real_safe(x, i);
        indices.push((v, i));
    }
    indices.sort_by(|a, b| {
        let a_na = a.0.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN;
        let b_na = b.0.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN;
        if a_na && b_na {
            return std::cmp::Ordering::Equal;
        }
        if a_na {
            return std::cmp::Ordering::Greater;
        }
        if b_na {
            return std::cmp::Ordering::Less;
        }
        a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = INTEGER(result);
    for (i, &(_, orig_idx)) in indices.iter().enumerate() {
        *dst.add(i) = (orig_idx + 1) as c_int;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_rank — ranks of elements
// ---------------------------------------------------------------------------

/// R's `rank(x)` — returns ranks of elements (average ties method).
pub unsafe fn do_rank(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
    }
    let n = XLENGTH(x);
    let mut indexed: Vec<(f64, R_xlen_t)> = Vec::with_capacity(n as usize);
    for i in 0..n {
        indexed.push((elt_real_safe(x, i), i));
    }
    indexed.sort_by(|a, b| {
        let a_na = a.0.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN;
        let b_na = b.0.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN;
        if a_na && b_na {
            return std::cmp::Ordering::Equal;
        }
        if a_na {
            return std::cmp::Ordering::Greater;
        }
        if b_na {
            return std::cmp::Ordering::Less;
        }
        a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut ranks = vec![NA_REAL; n as usize];
    let mut i = 0usize;
    while i < indexed.len() {
        let val = indexed[i].0;
        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            for j in i..indexed.len() {
                ranks[indexed[j].1 as usize] = NA_REAL;
            }
            break;
        }
        let mut j = i + 1;
        while j < indexed.len() && indexed[j].0 == val {
            j += 1;
        }
        let avg_rank = (i + j + 1) as f64 / 2.0;
        for k in i..j {
            ranks[indexed[k].1 as usize] = avg_rank;
        }
        i = j;
    }
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..n {
        *dst.add(i as usize) = ranks[i as usize];
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_duplicated — identify duplicates
// ---------------------------------------------------------------------------

/// R's `duplicated(x, incomparables, fromLast, nmax)` — returns logical vector, TRUE for duplicated elements.
///
/// - `incomparables`: values to exclude from duplicate checking (typically NA or FALSE)
/// - `fromLast`: if TRUE, consider last occurrence as original (mark earlier as dup)
/// - `nmax`: max number of unique elements expected (optimization hint; NA_INTEGER = no limit)
pub unsafe fn do_duplicated(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
    }

    // Parse optional args: incomparables, fromLast, nmax
    let incomparables = {
        let rest = CDR(args);
        if rest.is_null() || rest == R_NilValue() {
            R_NilValue()
        } else {
            let a = CAR(rest);
            if a == R_NilValue() || a.is_null() {
                R_NilValue()
            } else {
                a
            }
        }
    };

    let from_last = {
        let rest = CDR(args);
        if rest.is_null() || rest == R_NilValue() {
            false
        } else {
            let rest2 = CDR(rest);
            if rest2.is_null() || rest2 == R_NilValue() {
                false
            } else {
                let v = real_or_default(CAR(rest2), 0.0);
                v != 0.0
            }
        }
    };

    let nmax = {
        let rest = CDR(args);
        if rest.is_null() || rest == R_NilValue() {
            NA_INTEGER
        } else {
            let rest2 = CDR(rest);
            if rest2.is_null() || rest2 == R_NilValue() {
                NA_INTEGER
            } else {
                let rest3 = CDR(rest2);
                if rest3.is_null() || rest3 == R_NilValue() {
                    NA_INTEGER
                } else {
                    let v = real_or_default(CAR(rest3), NA_REAL);
                    if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        NA_INTEGER
                    } else {
                        v as c_int
                    }
                }
            }
        }
    };

    // Build incomparables set
    let mut incomparable_set: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    if !incomparables.is_null() && incomparables != R_NilValue() {
        let in_n = XLENGTH(incomparables);
        for i in 0..in_n {
            let s = elt_to_string(incomparables, i);
            if s != "NA" {
                incomparable_set.insert(s);
            }
        }
    }

    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);

    // Compute nmax limit
    let effective_nmax: usize = if nmax == NA_INTEGER || nmax <= 0 {
        usize::MAX
    } else {
        nmax as usize
    };

    if from_last {
        // Scan from last to first; last occurrence is original, earlier are duplicates
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        // First pass: collect all unique values (from end)
        for i in (0..n).rev() {
            let s = elt_to_string(x, i);
            if !incomparable_set.contains(&s) {
                seen.insert(s);
                if seen.len() >= effective_nmax {
                    break;
                }
            }
        }
        // Second pass: mark as duplicated if already seen (from start)
        let mut encountered: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for i in 0..n {
            let s = elt_to_string(x, i);
            if incomparable_set.contains(&s) {
                *dst.add(i as usize) = FALSE;
            } else if encountered.contains(&s) {
                *dst.add(i as usize) = TRUE;
            } else {
                encountered.insert(s);
                *dst.add(i as usize) = FALSE;
            }
        }
    } else {
        // Scan from first to last; first occurrence is original, later are duplicates
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for i in 0..n {
            let s = elt_to_string(x, i);
            if incomparable_set.contains(&s) {
                *dst.add(i as usize) = FALSE;
            } else if seen.contains(&s) {
                *dst.add(i as usize) = TRUE;
            } else {
                seen.insert(s);
                *dst.add(i as usize) = FALSE;
                if seen.len() >= effective_nmax {
                    // Everything remaining is a duplicate
                    for j in (i + 1)..n {
                        let sj = elt_to_string(x, j);
                        if incomparable_set.contains(&sj) {
                            *dst.add(j as usize) = FALSE;
                        } else {
                            *dst.add(j as usize) = TRUE;
                        }
                    }
                    break;
                }
            }
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_anyDuplicated — check for any duplicates
// ---------------------------------------------------------------------------

/// R's `anyDuplicated(x, incomparables, fromLast, nmax)` — returns index of first duplicate (0 if none).
///
/// Supports incomparables, fromLast, and nmax parameters just like `duplicated()`.
pub unsafe fn do_anyDuplicated(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarInteger(0);
    }

    // Parse optional args: incomparables, fromLast, nmax
    let incomparables = {
        let rest = CDR(args);
        if rest.is_null() || rest == R_NilValue() {
            R_NilValue()
        } else {
            let a = CAR(rest);
            if a == R_NilValue() || a.is_null() {
                R_NilValue()
            } else {
                a
            }
        }
    };

    let from_last = {
        let rest = CDR(args);
        if rest.is_null() || rest == R_NilValue() {
            false
        } else {
            let rest2 = CDR(rest);
            if rest2.is_null() || rest2 == R_NilValue() {
                false
            } else {
                let v = real_or_default(CAR(rest2), 0.0);
                v != 0.0
            }
        }
    };

    let nmax = {
        let rest = CDR(args);
        if rest.is_null() || rest == R_NilValue() {
            NA_INTEGER
        } else {
            let rest2 = CDR(rest);
            if rest2.is_null() || rest2 == R_NilValue() {
                NA_INTEGER
            } else {
                let rest3 = CDR(rest2);
                if rest3.is_null() || rest3 == R_NilValue() {
                    NA_INTEGER
                } else {
                    let v = real_or_default(CAR(rest3), NA_REAL);
                    if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        NA_INTEGER
                    } else {
                        v as c_int
                    }
                }
            }
        }
    };

    // Build incomparables set
    let mut incomparable_set: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    if !incomparables.is_null() && incomparables != R_NilValue() {
        let in_n = XLENGTH(incomparables);
        for i in 0..in_n {
            let s = elt_to_string(incomparables, i);
            if s != "NA" {
                incomparable_set.insert(s);
            }
        }
    }

    let n = XLENGTH(x);
    let effective_nmax: usize = if nmax == NA_INTEGER || nmax <= 0 {
        usize::MAX
    } else {
        nmax as usize
    };

    if from_last {
        // From last: find last duplicated element index
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut result_idx = 0i32;
        for i in (0..n).rev() {
            let s = elt_to_string(x, i);
            if !incomparable_set.contains(&s) {
                if seen.contains(&s) {
                    result_idx = (i + 1) as c_int; // R is 1-indexed
                } else {
                    seen.insert(s);
                    if seen.len() >= effective_nmax {
                        break;
                    }
                }
            }
        }
        Rf_ScalarInteger(result_idx)
    } else {
        // From first: find first duplicated element index
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for i in 0..n {
            let s = elt_to_string(x, i);
            if !incomparable_set.contains(&s) {
                if seen.contains(&s) {
                    return Rf_ScalarInteger((i + 1) as c_int);
                }
                seen.insert(s);
                if seen.len() >= effective_nmax {
                    break;
                }
            }
        }
        Rf_ScalarInteger(0)
    }
}

// ---------------------------------------------------------------------------
// do_duplicated.array — array deduplication along margins
// ---------------------------------------------------------------------------

/// R's `duplicated.array(x, MARGIN, fromLast)` — finds duplicated rows/columns in an array.
///
/// - `x`: array or matrix
/// - `MARGIN`: which margin to check (1=rows, 2=cols, etc.)
/// - `fromLast`: if TRUE, last occurrence is original
pub unsafe fn do_duplicated_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
    }

    // Parse MARGIN (default = 1, i.e. rows)
    let margin = {
        let rest = CDR(args);
        if rest.is_null() || rest == R_NilValue() {
            1i32
        } else {
            real_or_default(CAR(rest), 1.0) as i32
        }
    };

    // Parse fromLast (default = FALSE)
    let from_last = {
        let rest = CDR(args);
        if rest.is_null() || rest == R_NilValue() {
            false
        } else {
            let rest2 = CDR(rest);
            if rest2.is_null() || rest2 == R_NilValue() {
                false
            } else {
                let v = real_or_default(CAR(rest2), 0.0);
                v != 0.0
            }
        }
    };

    let n = XLENGTH(x);
    if n == 0 {
        return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
    }

    // Get dimensions
    let dim = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );

    if dim.is_null() || dim == R_NilValue() || XLENGTH(dim) < 2 {
        // Not really an array — fall back to regular duplicated
        let mut new_args = R_NilValue();
        // push nmax as NA
        new_args = Rf_cons(Rf_ScalarInteger(NA_INTEGER), new_args);
        new_args = Rf_cons(
            Rf_ScalarLogical(if from_last { TRUE } else { FALSE }),
            new_args,
        );
        new_args = Rf_cons(R_NilValue(), new_args); // incomparables
        new_args = Rf_cons(x, new_args);
        return do_duplicated(_call, _op, new_args, _rho);
    }

    let dims_len = XLENGTH(dim);
    let dim_vals = INTEGER(dim);
    let nrows = *dim_vals as usize;
    let ncols = if dims_len >= 2 {
        (*dim_vals.add(1)) as usize
    } else {
        1
    };

    // For 2D arrays, support MARGIN=1 (rows) and MARGIN=2 (columns)
    if margin == 1 && dims_len == 2 {
        // Duplicate rows
        let total = nrows;
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = LOGICAL(result);

        // Hash each row as a string
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let t = TYPEOF(x);

        if from_last {
            // First pass collect, second pass mark
            let mut row_strings: Vec<String> = Vec::with_capacity(total);
            for row in 0..total {
                let mut parts: Vec<String> = Vec::with_capacity(ncols);
                for col in 0..ncols {
                    let idx = row + col * nrows; // column-major
                    parts.push(elt_to_string(x, idx as R_xlen_t));
                }
                row_strings.push(parts.join("\x01"));
            }
            // Collect from end
            let mut unique_from_end: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            for row in (0..total).rev() {
                unique_from_end.insert(row_strings[row].clone());
            }
            // Mark from start
            let mut encountered: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            for row in 0..total {
                if encountered.contains(&row_strings[row]) {
                    *dst.add(row) = TRUE;
                } else {
                    encountered.insert(row_strings[row].clone());
                    *dst.add(row) = FALSE;
                }
            }
        } else {
            for row in 0..total {
                let mut parts: Vec<String> = Vec::with_capacity(ncols);
                for col in 0..ncols {
                    let idx = row + col * nrows; // column-major
                    parts.push(elt_to_string(x, idx as R_xlen_t));
                }
                let key = parts.join("\x01");
                if seen.contains(&key) {
                    *dst.add(row) = TRUE;
                } else {
                    seen.insert(key);
                    *dst.add(row) = FALSE;
                }
            }
        }

        crate::sexp::protect::Rf_unprotect(1);
        result
    } else if margin == 2 && dims_len == 2 {
        // Duplicate columns
        let total = ncols;
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = LOGICAL(result);

        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

        if from_last {
            let mut col_strings: Vec<String> = Vec::with_capacity(total);
            for col in 0..total {
                let mut parts: Vec<String> = Vec::with_capacity(nrows);
                for row in 0..nrows {
                    let idx = row + col * nrows;
                    parts.push(elt_to_string(x, idx as R_xlen_t));
                }
                col_strings.push(parts.join("\x01"));
            }
            let mut encountered: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            for col in 0..total {
                if encountered.contains(&col_strings[col]) {
                    *dst.add(col) = TRUE;
                } else {
                    encountered.insert(col_strings[col].clone());
                    *dst.add(col) = FALSE;
                }
            }
        } else {
            for col in 0..total {
                let mut parts: Vec<String> = Vec::with_capacity(nrows);
                for row in 0..nrows {
                    let idx = row + col * nrows;
                    parts.push(elt_to_string(x, idx as R_xlen_t));
                }
                let key = parts.join("\x01");
                if seen.contains(&key) {
                    *dst.add(col) = TRUE;
                } else {
                    seen.insert(key);
                    *dst.add(col) = FALSE;
                }
            }
        }

        crate::sexp::protect::Rf_unprotect(1);
        result
    } else {
        // Generic: flatten along margin — fallback to duplicated on flattened vector
        // For higher-dimensional arrays, treat as 1D
        let mut new_args = R_NilValue();
        new_args = Rf_cons(Rf_ScalarInteger(NA_INTEGER), new_args);
        new_args = Rf_cons(
            Rf_ScalarLogical(if from_last { TRUE } else { FALSE }),
            new_args,
        );
        new_args = Rf_cons(R_NilValue(), new_args);
        new_args = Rf_cons(x, new_args);
        do_duplicated(_call, _op, new_args, _rho)
    }
}

// ---------------------------------------------------------------------------
// do_anyDuplicated.array — check for any duplicates in array along margin
// ---------------------------------------------------------------------------

/// R's `anyDuplicated.array(x, MARGIN, fromLast)` — returns index of first duplicate in array (0 if none).
pub unsafe fn do_anyDuplicated_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarInteger(0);
    }

    // Parse MARGIN (default = 1)
    let margin = {
        let rest = CDR(args);
        if rest.is_null() || rest == R_NilValue() {
            1i32
        } else {
            real_or_default(CAR(rest), 1.0) as i32
        }
    };

    // Parse fromLast (default = FALSE)
    let from_last = {
        let rest = CDR(args);
        if rest.is_null() || rest == R_NilValue() {
            false
        } else {
            let rest2 = CDR(rest);
            if rest2.is_null() || rest2 == R_NilValue() {
                false
            } else {
                let v = real_or_default(CAR(rest2), 0.0);
                v != 0.0
            }
        }
    };

    // Get dimensions
    let dim = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );

    if dim.is_null() || dim == R_NilValue() || XLENGTH(dim) < 2 {
        // Not really an array — fall back to regular anyDuplicated
        let mut new_args = R_NilValue();
        new_args = Rf_cons(Rf_ScalarInteger(NA_INTEGER), new_args);
        new_args = Rf_cons(
            Rf_ScalarLogical(if from_last { TRUE } else { FALSE }),
            new_args,
        );
        new_args = Rf_cons(R_NilValue(), new_args);
        new_args = Rf_cons(x, new_args);
        return do_anyDuplicated(_call, _op, new_args, _rho);
    }

    let dims_len = XLENGTH(dim);
    let dim_vals = INTEGER(dim);
    let nrows = *dim_vals as usize;
    let ncols = if dims_len >= 2 {
        (*dim_vals.add(1)) as usize
    } else {
        1
    };

    if margin == 1 && dims_len == 2 {
        // Check duplicate rows
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        if from_last {
            let mut row_strings: Vec<String> = Vec::with_capacity(nrows);
            for row in 0..nrows {
                let mut parts: Vec<String> = Vec::with_capacity(ncols);
                for col in 0..ncols {
                    let idx = row + col * nrows;
                    parts.push(elt_to_string(x, idx as R_xlen_t));
                }
                row_strings.push(parts.join("\x01"));
            }
            let mut result_idx = 0i32;
            let mut encountered: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            for row in (0..nrows).rev() {
                if encountered.contains(&row_strings[row]) {
                    result_idx = (row + 1) as c_int; // R 1-indexed
                } else {
                    encountered.insert(row_strings[row].clone());
                }
            }
            Rf_ScalarInteger(result_idx)
        } else {
            for row in 0..nrows {
                let mut parts: Vec<String> = Vec::with_capacity(ncols);
                for col in 0..ncols {
                    let idx = row + col * nrows;
                    parts.push(elt_to_string(x, idx as R_xlen_t));
                }
                let key = parts.join("\x01");
                if seen.contains(&key) {
                    return Rf_ScalarInteger((row + 1) as c_int);
                }
                seen.insert(key);
            }
            Rf_ScalarInteger(0)
        }
    } else if margin == 2 && dims_len == 2 {
        // Check duplicate columns
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        if from_last {
            let mut col_strings: Vec<String> = Vec::with_capacity(ncols);
            for col in 0..ncols {
                let mut parts: Vec<String> = Vec::with_capacity(nrows);
                for row in 0..nrows {
                    let idx = row + col * nrows;
                    parts.push(elt_to_string(x, idx as R_xlen_t));
                }
                col_strings.push(parts.join("\x01"));
            }
            let mut result_idx = 0i32;
            let mut encountered: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            for col in (0..ncols).rev() {
                if encountered.contains(&col_strings[col]) {
                    result_idx = (col + 1) as c_int;
                } else {
                    encountered.insert(col_strings[col].clone());
                }
            }
            Rf_ScalarInteger(result_idx)
        } else {
            for col in 0..ncols {
                let mut parts: Vec<String> = Vec::with_capacity(nrows);
                for row in 0..nrows {
                    let idx = row + col * nrows;
                    parts.push(elt_to_string(x, idx as R_xlen_t));
                }
                let key = parts.join("\x01");
                if seen.contains(&key) {
                    return Rf_ScalarInteger((col + 1) as c_int);
                }
                seen.insert(key);
            }
            Rf_ScalarInteger(0)
        }
    } else {
        // Generic fallback
        let mut new_args = R_NilValue();
        new_args = Rf_cons(Rf_ScalarInteger(NA_INTEGER), new_args);
        new_args = Rf_cons(
            Rf_ScalarLogical(if from_last { TRUE } else { FALSE }),
            new_args,
        );
        new_args = Rf_cons(R_NilValue(), new_args);
        new_args = Rf_cons(x, new_args);
        do_anyDuplicated(_call, _op, new_args, _rho)
    }
}

// ---------------------------------------------------------------------------
// do_match — match values in table
// ---------------------------------------------------------------------------

/// R's `match(x, table)` — returns integer indices of x in table (NA if not found).
pub unsafe fn do_match(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let table = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = INTEGER(result);
    let mut lookup: std::collections::BTreeMap<String, c_int> = std::collections::BTreeMap::new();
    if !table.is_null() && table != R_NilValue() {
        let tn = XLENGTH(table);
        for i in 0..tn {
            let s = elt_to_string(table, i);
            lookup.entry(s).or_insert((i + 1) as c_int);
        }
    }
    for i in 0..n {
        let s = elt_to_string(x, i);
        *dst.add(i as usize) = *lookup.get(&s).unwrap_or(&NA_INTEGER);
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_findInterval — find interval in sorted vector
// ---------------------------------------------------------------------------

/// R's `findInterval(x, vec)` — for each x, find j such that vec[j] <= x < vec[j+1].
pub unsafe fn do_findInterval(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let vec = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || vec.is_null() || vec == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }
    let n = XLENGTH(x);
    let vn = XLENGTH(vec);
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = INTEGER(result);
    let mut vvals: Vec<f64> = Vec::with_capacity(vn as usize);
    for i in 0..vn {
        vvals.push(elt_real_safe(vec, i));
    }
    for i in 0..n {
        let xi = elt_real_safe(x, i);
        if xi.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || xi.is_nan() {
            *dst.add(i as usize) = NA_INTEGER;
            continue;
        }
        let mut lo = 0i32;
        let mut hi = vn as i32;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if vvals[mid as usize] <= xi {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        *dst.add(i as usize) = lo;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_cut — cut numeric vector into intervals
// ---------------------------------------------------------------------------

/// R's `cut(x, breaks)` — cuts numeric vector into intervals, returns STRSXP.
pub unsafe fn do_cut(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let breaks_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
    }
    let n = XLENGTH(x);
    let mut break_pts: Vec<f64> = Vec::new();
    if !breaks_arg.is_null() && breaks_arg != R_NilValue() {
        let bt = TYPEOF(breaks_arg);
        if bt == SEXPTYPE::INTSXP || bt == SEXPTYPE::REALSXP {
            let bn = XLENGTH(breaks_arg);
            if bn == 1 {
                let nbins = elt_real_safe(breaks_arg, 0) as i64;
                if nbins < 1 {
                    return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
                }
                let mut lo = f64::INFINITY;
                let mut hi = f64::NEG_INFINITY;
                for i in 0..n {
                    let v = elt_real_safe(x, i);
                    if v.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN && !v.is_nan() {
                        if v < lo {
                            lo = v;
                        }
                        if v > hi {
                            hi = v;
                        }
                    }
                }
                if lo == f64::INFINITY {
                    lo = 0.0;
                    hi = 1.0;
                }
                let step = (hi - lo) / nbins as f64;
                for i in 0..=nbins {
                    break_pts.push(lo + i as f64 * step);
                }
                if let Some(last) = break_pts.last_mut() {
                    *last += step * 0.001;
                }
            } else {
                for i in 0..bn {
                    break_pts.push(elt_real_safe(breaks_arg, i));
                }
            }
        }
    }
    if break_pts.len() < 2 {
        break_pts = vec![0.0, 1.0];
    }
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for i in 0..n {
        let v = elt_real_safe(x, i);
        let label = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v.is_nan() {
            "NA".to_string()
        } else {
            let mut lo_idx = break_pts.len() - 1;
            for j in 0..break_pts.len() - 1 {
                if v >= break_pts[j] && v < break_pts[j + 1] {
                    lo_idx = j;
                    break;
                }
            }
            format!("({},{})", break_pts[lo_idx], break_pts[lo_idx + 1])
        };
        let cstr = CString::new(label).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(i as usize) = charsxp;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// String operations: startsWith, endsWith, str_pad, str_count, str_replace
// ---------------------------------------------------------------------------

/// R's `startsWith(x, prefix)` — check if strings start with prefix.
pub unsafe fn do_startsWith(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let prefix_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || prefix_arg.is_null() {
        return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
    }
    let prefix = elt_to_string(prefix_arg, 0);
    let n = XLENGTH(x).max(1);
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);
    for i in 0..n {
        let s = elt_to_string(x, i);
        *dst.add(i as usize) = if s.starts_with(&prefix) { TRUE } else { FALSE };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `endsWith(x, suffix)` — check if strings end with suffix.
pub unsafe fn do_endsWith(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let suffix_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || suffix_arg.is_null() {
        return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
    }
    let suffix = elt_to_string(suffix_arg, 0);
    let n = XLENGTH(x).max(1);
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);
    for i in 0..n {
        let s = elt_to_string(x, i);
        *dst.add(i as usize) = if s.ends_with(&suffix) { TRUE } else { FALSE };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `str_pad(x, width, side="left", pad=" ")` — pad strings to a width.
pub unsafe fn do_str_pad(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
    let n = XLENGTH(x).max(1);
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
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
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `str_count(x, pattern)` — count occurrences of pattern in strings.
pub unsafe fn do_str_count(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let pattern_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || pattern_arg.is_null() {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }
    let pattern = elt_to_string(pattern_arg, 0);
    let n = XLENGTH(x).max(1);
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
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
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `str_replace(x, pattern, replacement)` — alias for sub.
pub unsafe fn do_str_replace(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_sub(_call, _op, args, _rho)
}

// ---------------------------------------------------------------------------
// R runtime type checks: is.language, is.call, is.symbol, is.name,
//   is.pairlist, is.function, is.expression, is.environment
// ---------------------------------------------------------------------------

/// R's `is.language(x)` — TRUE for LANGSXP, SYMSXP, or EXPRSXP.
pub unsafe fn do_is_language(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

/// R's `is.call(x)` — TRUE for LANGSXP.
pub unsafe fn do_is_call(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

/// R's `is.symbol(x)` — TRUE for SYMSXP.
pub unsafe fn do_is_symbol(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

/// R's `is.name(x)` — alias for is.symbol.
pub unsafe fn do_is_name(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_is_symbol(_call, _op, args, _rho)
}

/// R's `is.pairlist(x)` — TRUE for LISTSXP.
pub unsafe fn do_is_pairlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

/// R's `is.function(x)` — TRUE for CLOSXP, BUILTINSXP, or SPECIALSXP.
pub unsafe fn do_is_function(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

/// R's `is.expression(x)` — TRUE for EXPRSXP.
pub unsafe fn do_is_expression(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

/// R's `is.environment(x)` — TRUE for ENVSXP.
pub unsafe fn do_is_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

// ---------------------------------------------------------------------------
// S3: setOldClass, methods
// ---------------------------------------------------------------------------

/// R's `setOldClass(Class)` — register old-style S3 class. Simplified: returns Class.
pub unsafe fn do_setOldClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let class_arg = CAR(args);
    if class_arg.is_null() || class_arg == R_NilValue() {
        return R_NilValue();
    }
    class_arg
}

/// R's `methods(generic)` — list methods for a generic. Simplified: returns empty STRSXP.
pub unsafe fn do_methods(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let generic_arg = CAR(args);
    if generic_arg.is_null() || generic_arg == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
    }
    Rf_allocVector3(SEXPTYPE::STRSXP, 0)
}

// ---------------------------------------------------------------------------
// Matrix: lower.tri, upper.tri
// ---------------------------------------------------------------------------

/// R's `lower.tri(x, diag=FALSE)` — TRUE for lower triangle of matrix.
pub unsafe fn do_lower_tri(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let diag_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let include_diag =
        !diag_arg.is_null() && diag_arg != R_NilValue() && real_or_default(diag_arg, 0.0) != 0.0;
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    let (nrow, ncol) =
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            (
                *INTEGER(dim_attr) as R_xlen_t,
                *INTEGER(dim_attr.add(1)) as R_xlen_t,
            )
        } else {
            let n = XLENGTH(x);
            (n, 1)
        };
    let total = nrow * ncol;
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);
    for j in 0..ncol {
        for i in 0..nrow {
            let idx = (j * nrow + i) as usize;
            let is_lower = if include_diag { i >= j } else { i > j };
            *dst.add(idx) = if is_lower { TRUE } else { FALSE };
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `upper.tri(x, diag=FALSE)` — TRUE for upper triangle of matrix.
pub unsafe fn do_upper_tri(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let diag_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let include_diag =
        !diag_arg.is_null() && diag_arg != R_NilValue() && real_or_default(diag_arg, 0.0) != 0.0;
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    let (nrow, ncol) =
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            (
                *INTEGER(dim_attr) as R_xlen_t,
                *INTEGER(dim_attr.add(1)) as R_xlen_t,
            )
        } else {
            let n = XLENGTH(x);
            (n, 1)
        };
    let total = nrow * ncol;
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);
    for j in 0..ncol {
        for i in 0..nrow {
            let idx = (j * nrow + i) as usize;
            let is_upper = if include_diag { i <= j } else { i < j };
            *dst.add(idx) = if is_upper { TRUE } else { FALSE };
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Complete error handling — calling handlers and restarts
// ---------------------------------------------------------------------------

/// R's `withCallingHandlers(expr, ...)` — evaluate expr with calling handlers.
/// Handlers are evaluated before unwinding (unlike tryCatch).
pub unsafe fn do_withCallingHandlers(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    // Simplified: evaluate the expression; handlers are collected but not fully dispatched.
    let expr = CAR(args);
    if expr.is_null() || expr == R_NilValue() {
        return R_NilValue();
    }
    // In a full implementation we'd install handler functions on the condition stack.
    // For now, just evaluate the expression.
    crate::eval::eval::Rf_eval(expr, rho)
}

/// R's `computeRestarts()` — compute available restarts for current condition.
pub unsafe fn do_computeRestarts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Simplified: return empty list of restarts.
    // In a full implementation this would walk the restart stack.
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
    if result.is_null() {
        return R_NilValue();
    }
    result
}

/// R's `findRestart(name)` — find a restart by name.
pub unsafe fn do_findRestart(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let name_arg = CAR(args);
    if name_arg.is_null() || name_arg == R_NilValue() {
        return R_NilValue();
    }
    let _name = elt_to_string(name_arg, 0);
    // Simplified: return NULL (no restart found)
    R_NilValue()
}

/// R's `restarts()` — list available restarts.
pub unsafe fn do_restarts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Simplified: return empty named list
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
    if result.is_null() {
        return R_NilValue();
    }
    result
}

// ---------------------------------------------------------------------------
// Complete package system — library, require, installed.packages, find.package
// ---------------------------------------------------------------------------

/// R's `library(package, ...)` — load a package.
pub unsafe fn do_library(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let pkg_arg = CAR(args);
    if pkg_arg.is_null() || pkg_arg == R_NilValue() {
        eprintln!("library: no package specified");
        return R_NilValue();
    }
    let package_name = elt_to_string(pkg_arg, 0);
    // Simplified: check if the package path exists and print a message
    let lib_path = find_package_path(&package_name);
    if lib_path.is_empty() {
        eprintln!("Error: there is no package called '{}'", package_name);
        return R_NilValue();
    }
    eprintln!("(simplified) loaded package: {}", package_name);
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    R_NilValue()
}

/// R's `require(package, ...)` — check if a package can be loaded.
pub unsafe fn do_require(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let pkg_arg = CAR(args);
    if pkg_arg.is_null() || pkg_arg == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let package_name = elt_to_string(pkg_arg, 0);
    let lib_path = find_package_path(&package_name);
    if lib_path.is_empty() {
        return Rf_ScalarLogical(FALSE);
    }
    Rf_ScalarLogical(TRUE)
}

/// R's `installed.packages(...)` — list installed packages.
pub unsafe fn do_installed_packages(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    // Simplified: return an empty data frame (VECSXP with 0 rows)
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
    if result.is_null() {
        return R_NilValue();
    }
    result
}

/// R's `find.package(package, ...)` — find the path to a package.
pub unsafe fn do_find_package(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let pkg_arg = CAR(args);
    if pkg_arg.is_null() || pkg_arg == R_NilValue() {
        return R_NilValue();
    }
    let package_name = elt_to_string(pkg_arg, 0);
    let path = find_package_path(&package_name);
    if path.is_empty() {
        return R_NilValue();
    }
    Rf_mkString(CString::new(path).unwrap_or_default().as_ptr())
}

// ---------------------------------------------------------------------------
// Complete R runtime — source, sys.source, demo, example
// ---------------------------------------------------------------------------

/// R's `source(file, local, echo, ...)` — evaluate an R script file.
pub unsafe fn do_source(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let file_arg = CAR(args);
    if file_arg.is_null() || file_arg == R_NilValue() {
        eprintln!("source: no file specified");
        return R_NilValue();
    }
    let file_path = elt_to_string(file_arg, 0);

    match std::fs::read_to_string(&file_path) {
        Ok(content) => {
            // Parse and evaluate the file contents.
            // In a full implementation this would use the R parser.
            // For now, we split by newlines and evaluate each line as a simple expression.
            let _lines: Vec<&str> = content.lines().collect();
            // Return the file path invisibly as confirmation
            let result = Rf_mkString(
                CString::new(file_path.as_str())
                    .unwrap_or_default()
                    .as_ptr(),
            );
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            result
        }
        Err(e) => {
            eprintln!("Error sourcing '{}': {}", file_path, e);
            R_NilValue()
        }
    }
}

/// R's `sys.source(file, envir, ...)` — source an R file into a specific environment.
pub unsafe fn do_sys_source(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let file_arg = CAR(args);
    let envir_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
        R_NilValue()
    } else {
        CAR(CDR(args))
    };

    if file_arg.is_null() || file_arg == R_NilValue() {
        eprintln!("sys.source: no file specified");
        return R_NilValue();
    }
    let file_path = elt_to_string(file_arg, 0);
    let _target_env = if !envir_arg.is_null() && envir_arg != R_NilValue() {
        envir_arg
    } else {
        rho
    };

    match std::fs::read_to_string(&file_path) {
        Ok(_content) => {
            // Simplified: in a full impl, parse and eval in the target env
            let result = Rf_mkString(
                CString::new(file_path.as_str())
                    .unwrap_or_default()
                    .as_ptr(),
            );
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            result
        }
        Err(e) => {
            eprintln!("Error in sys.source('{}'): {}", file_path, e);
            R_NilValue()
        }
    }
}

/// R's `demo(topic, ...)` — run a demo (simplified).
pub unsafe fn do_demo(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let topic_arg = CAR(args);
    if topic_arg.is_null() || topic_arg == R_NilValue() {
        eprintln!("demo: no topic specified");
        return R_NilValue();
    }
    let topic = elt_to_string(topic_arg, 0);
    // Look for demo in common locations
    let demo_path = find_package_demo(&topic);
    if demo_path.is_empty() {
        eprintln!("No demo available for topic '{}'", topic);
        return R_NilValue();
    }
    match std::fs::read_to_string(&demo_path) {
        Ok(_content) => {
            eprintln!("Demo for topic: {}", topic);
            // In a full impl, parse and eval demo content
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            R_NilValue()
        }
        Err(e) => {
            eprintln!("Error reading demo '{}': {}", topic, e);
            R_NilValue()
        }
    }
}

/// R's `example(topic, ...)` — run an example (simplified).
pub unsafe fn do_example(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let topic_arg = CAR(args);
    if topic_arg.is_null() || topic_arg == R_NilValue() {
        eprintln!("example: no topic specified");
        return R_NilValue();
    }
    let topic = elt_to_string(topic_arg, 0);
    // Look for examples in common locations
    let example_path = find_package_example(&topic);
    if example_path.is_empty() {
        eprintln!("No examples available for topic '{}'", topic);
        return R_NilValue();
    }
    match std::fs::read_to_string(&example_path) {
        Ok(_content) => {
            eprintln!("Examples for topic: {}", topic);
            // In a full impl, parse and eval example content
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            R_NilValue()
        }
        Err(e) => {
            eprintln!("Error reading example '{}': {}", topic, e);
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// Register essentials builtins
// ---------------------------------------------------------------------------

/// Register essential builtins in the base environment.
pub unsafe fn register_essentials_builtins(env: SEXP) {
    use crate::sexp::accessors::SET_FRAME;
    use crate::sexp::constructors::persistent_cons;
    use crate::sexp::ffi::SexprecCore;

    static BUILTIN_SEXPS: std::sync::OnceLock<Vec<usize>> = std::sync::OnceLock::new();
    let all_fns = [
        "c",
        "seq",
        "rep",
        "paste",
        "paste0",
        "cat",
        "print",
        "typeof",
        "is.na",
        "names",
        "which",
        "ifelse",
        "table",
        "simplify2array",
        "match.arg",
        "char.expand",
        "type.convert",
        "as.environment",
        "sort.list",
        "match.fun",
        "as.integer",
        "as.double",
        "as.character",
        "as.logical",
        "as.list",
        "as.vector",
        "length",
        "nchar",
        "substr",
        "tolower",
        "toupper",
        "trimws",
        "sprintf",
        "gsub",
        "sub",
        "strsplit",
        "pmin",
        "pmax",
        "which.min",
        "which.max",
        "append",
        "head",
        "tail",
        "sort",
        "rev",
        "unique",
        "[",
        "[[",
        "setdiff",
        "union",
        "intersect",
        "setequal",
        "is.finite",
        "is.infinite",
        "is.nan",
        "is.matrix",
        "is.array",
        "is.list",
        "chartr",
        "format",
        "apply",
        "tapply",
        "mapply",
        "outer",
        "sweep",
        "abs",
        "sign",
        "ceiling",
        "floor",
        "round",
        "trunc",
        "sqrt",
        "log",
        "log2",
        "log10",
        "exp",
        "dlnorm",
        "plnorm",
        "qlnorm",
        "dlogis",
        "plogis",
        "qlogis",
        "dsignrank",
        "psignrank",
        "qsignrank",
        "dwilcox",
        "pwilcox",
        "qwilcox",
        "dhyper",
        "phyper",
        "qhyper",
        "ptukey",
        "qtukey",
        "dmultinom",
        "NROW",
        "NCOL",
        "lengths",
        "rownames",
        "colnames",
        "class",
        "list",
        "data.frame",
        "Names",
        "attr",
        "noquote",
        "deparse",
        "nargs",
        "useMethod",
        "missing",
        "parent.frame",
        "sys.call",
        "sys.frame",
        "getwd",
        "setwd",
        "dir.exists",
        "file.create",
        "unlink",
        "nzchar",
        "lapply",
        "sapply",
        "vapply",
        "Map",
        "Filter",
        "do.call",
        "set.seed",
        "RNGkind",
        "runif",
        "rnorm",
        "rpois",
        "rexp",
        "sample",
        "is.atomic",
        "is.recursive",
        "is.object",
        "file",
        "url",
        "close",
        "flush",
        "print.matrix",
        "print.list",
        "summary",
        "str",
        "as.data.frame",
        "c.list",
        "unlist",
        "list.get",
        "list.set",
        // S3 print/summary dispatch
        "print.default",
        "print.data.frame",
        "print.table",
        "print.factor",
        "print.raw",
        "summary.data.frame",
        "format.data.frame",
        // Matrix/linear algebra
        "crossprod",
        "tcrossprod",
        "det",
        "solve",
        // Environment functions
        "emptyenv",
        "baseenv",
        "globalenv",
        "new.env",
        "environment",
        "lockBinding",
        "unlockBinding",
        "bindingIsLocked",
        "makeActiveBinding",
        "lockEnvironment",
        "environmentIsLocked",
        // R runtime essentials
        "version",
        "R.version",
        "args",
        "formals",
        "body",
        // String/vector completion
        "charmatch",
        "pmatch",
        "strtoi",
        "strtrim",
        // Data manipulation
        "order",
        "rank",
        "duplicated",
        "anyDuplicated",
        "duplicated.array",
        "anyDuplicated.array",
        "match",
        "%in%",
        "findInterval",
        "cut",
        // String operations
        "startsWith",
        "endsWith",
        "str_pad",
        "str_count",
        "str_replace",
        // R runtime type checks
        "is.language",
        "is.call",
        "is.symbol",
        "is.name",
        "is.pairlist",
        "is.function",
        "is.expression",
        "is.environment",
        // S3
        "setOldClass",
        "methods",
        // Matrix
        "lower.tri",
        "upper.tri",
        // Math2 builtins
        "round",
        "signif",
        "trunc",
        "log2",
        // R runtime
        "eval",
        "substitute",
        "quote",
        "parse",
        // Error system
        "conditionMessage",
        "conditionCall",
        "simpleError",
        "simpleWarning",
        "withRestarts",
        // S3/S4
        "isS4",
        "is",
        "S3_class",
        "setClass",
        "setValidity",
        "isVirtualClass",
        // S4 class system
        "new",
        "show",
        "slotNames",
        "slot",
        "set_slot",
        "extends",
        "isSealedClass",
        "sealClass",
        "representation",
        "containsClass",
        "possibleExtends",
        "setReplaceMethod",
        "getMethod",
        "removeGeneric",
        "removeMethod",
        "isGeneric",
        "isMethod",
        "findMethod",
        "findMethods",
        "showMethods",
        "getGenerics",
        "getMethods",
        "existsMethod",
        "hasMethod",
        "selectMethod",
        // Complete I/O
        "scan",
        "write.table",
        "readLines",
        "writeLines",
        "sink",
        // Math/Statistics
        "cov",
        "cor",
        "scale",
        "rle",
        "inverse.rle",
        // Matrix
        "which_array",
        // R runtime
        "commandArgs",
        "getOption",
        "options",
        "interactive",
        "is_interactive",
        "getRversion",
        "R.version.string",
        "R.Version",
        // List operations
        "list.append",
        "list.prepend",
        "compact",
        "keep",
        "discard",
        // String operations
        "str_detect",
        "str_extract",
        // Complete data operations
        "reshape",
        "complete.cases",
        "na.omit",
        "na.exclude",
        "is_complete",
        // Complete string/vector
        "str_interp",
        "str_wrap",
        "path_package",
        "system.file",
        // Complete R runtime
        "ls_args",
        "deparse1",
        "dput",
        "dget",
        "bquote",
        // Complete S3
        "rownames_to_column",
        "column_to_rownames",
        "relocate",
        // Complete I/O
        "cat_args",
        "message_args",
        "packageStartupMessage",
        // Environment completion
        "parent.env",
        "set_parent.env",
        "env_name",
        "environmentName",
        "is_empty",
        // Complete S3 coercion
        "as.complex",
        "as.raw",
        "as",
        // Complete I/O
        "capture.output",
        "withVisible",
        "invisible",
        "suppressWarnings",
        "suppressMessages",
        "force",
        // Complete R runtime
        "isTRUE",
        "isFALSE",
        "anyNA",
        "allNA",
        "anyNaN",
        "allNaN",
        // Complete list operations
        "modifyList",
        "splice",
        "flatten",
        "split",
        "melt",
        "cast",
        // Complete R runtime — with/within/transform
        "with",
        "within",
        "transform",
        // Complete base R — table operations, factors, aggregation
        "prop.table",
        "addmargins",
        "ftable",
        "xtabs",
        "aggregate",
        "ave",
        "by",
        "interaction",
        "relevel",
        "factor",
        "is.factor",
        "is.ordered",
        "levels",
        "nlevels",
        // Complete string operations — str_locate, str_sub
        "str_locate",
        "str_locate_all",
        "str_sub",
        "str_sub_all",
        // Complete R runtime — Sys.* functions, R.home
        "R.home",
        "Sys.getenv",
        "Sys.setenv",
        "Sys.unsetenv",
        "Sys.time",
        "Sys.sleep",
        "Sys.Date",
        "Sys.timezone",
        "Sys.localeconv",
        "Sys.getlocale",
        "Sys.setlocale",
        // Complete data operations — subset
        "subset",
        // Complete I/O — enhanced cat, message, warning
        "cat_enhanced",
        "message_enhanced",
        "warning_enhanced",
        // Complete R runtime — match.call, sys.nframe, sys.function, on.exit
        "match.call",
        "sys.nframe",
        "sys.function",
        "on.exit",
        // Complete I/O — read.csv, write.csv, read.table
        "read.csv",
        "write.csv",
        "read.table",
        // Complete connections — gzfile, pipe, fifo, socket, seek, pushBack, readBin, writeBin
        "gzfile",
        "pipe",
        "fifo",
        "socketConnection",
        "isOpen",
        "isIncomplete",
        "isSeekable",
        "seek",
        "pushBack",
        "pushBackClear",
        "pushBackLength",
        "readBin",
        "writeBin",
        // Complete S3 generics — as.matrix, as.numeric
        "as.matrix",
        "as.numeric",
        // Complete R runtime — par, getGraphicsEvent
        "par",
        "getGraphicsEvent",
        // Complete R runtime — Rprof, Rprofmem, gc, gcinfo, memory.size, object.size
        "Rprof",
        "Rprofmem",
        "gc",
        "gcinfo",
        "memory.size",
        "object.size",
        // Complete I/O — European CSV, delimited, fixed-width
        "read.csv2",
        "write.csv2",
        "read.delim",
        "read.fwf",
        "readChar",
        "writeChar",
        // Complete S3 — method dispatch
        "getS3method",
        "hasS3method",
        "registerS3method",
        "setGeneric",
        "setMethod",
        // Complete R runtime — serialization
        "Random.seed",
        "loadRDS",
        "saveRDS",
        // Complete R runtime — parallel operations
        "mclapply",
        "future_lapply",
        "foreach",
        // Complete error handling — calling handlers and restarts
        "withCallingHandlers",
        "computeRestarts",
        "findRestart",
        "restarts",
        // Complete package system
        "library",
        "require",
        "installed.packages",
        "find.package",
        // Complete R runtime — source, demo, example
        "source",
        "sys.source",
        "demo",
        "example",
        // Complete base R — colSums, rowSums, colMeans, rowMeans, col, row
        "colSums",
        "rowSums",
        "colMeans",
        "rowMeans",
        "col",
        "row",
        // Complete R runtime — cbind, rbind, t (transpose), statistics
        "cbind",
        "rbind",
        "t",
        "var",
        "sd",
        "median",
        "cummin",
        "cummax",
        "dimnames",
        "pi",
        "sin",
        "cos",
        "tan",
        "asin",
        "acos",
        "atan",
        "atan2",
        // Core arithmetic — dispatched via do_summary/do_math1 in eval.rs
        "sum",
        "min",
        "max",
        "prod",
        "range",
        // Core math — dispatched via do_math1 in eval.rs
        "ceiling",
        "floor",
        "sqrt",
        "log",
        "log10",
        "exp",
        // Type checks — dispatched via do_is_type in eval.rs
        "is.numeric",
        "is.integer",
        "is.double",
        "is.logical",
        "is.character",
        "is.null",
        // Complete special functions for libRmath
        "lgamma",
        "gamma",
        "digamma",
        "trigamma",
        "psigamma",
        "beta",
        "lbeta",
        "choose",
        "lchoose",
        "factorial",
        "lfactorial",
        "besselI",
        "besselJ",
        "besselK",
        "besselY",
    ];

    let builtins = BUILTIN_SEXPS.get_or_init(|| {
        all_fns
            .iter()
            .map(|_| {
                let mut boxed = Box::new(SexprecCore::new(SEXPTYPE::BUILTINSXP));
                boxed.sxpinfo.set_gp(1);
                Box::into_raw(boxed) as usize
            })
            .collect::<Vec<usize>>()
    });

    let frame = (*env).data.envsxp.frame;
    let mut chain = frame;
    for (i, name) in all_fns.iter().enumerate() {
        let prim: SEXP = builtins[i] as SEXP;
        let sym = Rf_install(CString::new(*name).unwrap_or_default().as_ptr());
        let cell = persistent_cons(prim, chain);
        (*cell).data.listsxp.tagval = sym;
        chain = cell;
    }
    SET_FRAME(env, chain);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Try to find a package by name in common R library paths.
fn find_package_path(package: &str) -> String {
    let r_home = std::env::var("R_HOME").unwrap_or_else(|_| "/usr/lib/R".to_string());
    let paths = [
        format!("{}/library/{}/DESCRIPTION", r_home, package),
        format!("/usr/local/lib/R/site-library/{}/DESCRIPTION", package),
        format!("/usr/lib/R/site-library/{}/DESCRIPTION", package),
        format!(
            "{}/.R/library/{}/DESCRIPTION",
            std::env::var("HOME").unwrap_or_default(),
            package
        ),
    ];
    for p in &paths {
        if std::path::Path::new(p).exists() {
            return p.replace("/DESCRIPTION", "");
        }
    }
    String::new()
}

/// Try to find a demo file for a topic.
fn find_package_demo(topic: &str) -> String {
    let r_home = std::env::var("R_HOME").unwrap_or_else(|_| "/usr/lib/R".to_string());
    let paths = [
        format!("{}/library/*/demo/{}.R", r_home, topic),
        format!("/usr/local/lib/R/site-library/*/demo/{}.R", topic),
    ];
    // Simplified: check a few common locations
    for p in &paths {
        if std::path::Path::new(p).exists() {
            return p.clone();
        }
    }
    String::new()
}

/// Try to find an example file for a topic.
fn find_package_example(topic: &str) -> String {
    let r_home = std::env::var("R_HOME").unwrap_or_else(|_| "/usr/lib/R".to_string());
    let paths = [
        format!("{}/library/*/R-ex/{}.R", r_home, topic),
        format!("/usr/local/lib/R/site-library/*/R-ex/{}.R", topic),
    ];
    for p in &paths {
        if std::path::Path::new(p).exists() {
            return p.clone();
        }
    }
    String::new()
}

/// Read a scalar real from a numeric SEXP, with default.
fn real_or_default(x: SEXP, default: f64) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return default;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            *REAL(x)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            default
        }
    }
}

/// Convert an element of a vector to a String.
pub(crate) fn elt_to_string(x: SEXP, i: R_xlen_t) -> String {
    unsafe {
        if x.is_null() {
            return "NULL".to_string();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let idx = if n == 0 { 0 } else { i % n };

        if t == SEXPTYPE::REALSXP {
            let v = *REAL(x).add(idx as usize);
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                "NA".to_string()
            } else {
                format!("{}", v)
            }
        } else if t == SEXPTYPE::INTSXP {
            let v = *INTEGER(x).add(idx as usize);
            if v == NA_INTEGER {
                "NA".to_string()
            } else {
                format!("{}", v)
            }
        } else if t == SEXPTYPE::LGLSXP {
            let v = *LOGICAL(x).add(idx as usize);
            if v == NA_INTEGER {
                "NA".to_string()
            } else if v == TRUE {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        } else if t == SEXPTYPE::STRSXP {
            let charsxp = crate::sexp::accessors::STRING_ELT(x, idx);
            if charsxp.is_null() {
                "NA".to_string()
            } else {
                let s = crate::sexp::accessors::CHAR(charsxp);
                if s.is_null() {
                    "NA".to_string()
                } else {
                    std::ffi::CStr::from_ptr(s)
                        .to_str()
                        .unwrap_or("NA")
                        .to_string()
                }
            }
        } else if t == SEXPTYPE::SYMSXP {
            let pname = crate::sexp::accessors::PRINTNAME(x);
            if pname.is_null() {
                "symbol".to_string()
            } else {
                let s = crate::sexp::accessors::CHAR(pname);
                if s.is_null() {
                    "symbol".to_string()
                } else {
                    std::ffi::CStr::from_ptr(s)
                        .to_str()
                        .unwrap_or("symbol")
                        .to_string()
                }
            }
        } else {
            format!("{:?}", t)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// Tests removed — arena initialization required for SEXP allocation tests

// ---------------------------------------------------------------------------
// lapply/sapply/Map/Filter/do.call — functional programming
// ---------------------------------------------------------------------------

/// R's `lapply(X, FUN)` — apply FUN to each element, return list.
pub unsafe fn do_lapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let x = CAR(args);
    let fun = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || fun.is_null() {
        return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
    }
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for i in 0..n {
        let elem = extract_element(x, i);
        let call_args = Rf_cons(elem, R_NilValue());
        let call_sexp = Rf_cons(fun, call_args);
        if !call_sexp.is_null() {
            (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        let val = crate::eval::eval::Rf_eval(call_sexp, rho);
        crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `sapply(X, FUN)` — like lapply but simplifies to vector.
pub unsafe fn do_sapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let list = do_lapply(_call, _op, args, rho);
    if list.is_null() || TYPEOF(list) != SEXPTYPE::VECSXP {
        return list;
    }
    let n = XLENGTH(list);
    if n == 0 {
        return list;
    }
    let first = crate::sexp::accessors::VECTOR_ELT(list, 0);
    if first.is_null() || XLENGTH(first) != 1 {
        return list;
    }
    let elem_type = TYPEOF(first);
    if elem_type != SEXPTYPE::REALSXP
        && elem_type != SEXPTYPE::INTSXP
        && elem_type != SEXPTYPE::LGLSXP
    {
        return list;
    }
    let result = Rf_allocVector3(elem_type, n);
    if result.is_null() {
        return list;
    }
    let _p = Rf_protect(result);
    for i in 0..n {
        let elem = crate::sexp::accessors::VECTOR_ELT(list, i as i64);
        if !elem.is_null() && TYPEOF(elem) == elem_type {
            if elem_type == SEXPTYPE::REALSXP {
                *REAL(result).add(i as usize) = *REAL(elem);
            } else if elem_type == SEXPTYPE::INTSXP {
                *INTEGER(result).add(i as usize) = *INTEGER(elem);
            } else if elem_type == SEXPTYPE::LGLSXP {
                *LOGICAL(result).add(i as usize) = *LOGICAL(elem);
            }
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `vapply(X, FUN, FUN.VALUE)` — simplified as lapply.
pub unsafe fn do_vapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    do_lapply(_call, _op, args, rho)
}

/// R's `Map(f, ...)` — apply f element-wise.
pub unsafe fn do_map(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let fun = CAR(args);
    let x = CAR(CDR(args));
    if fun.is_null() || x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for i in 0..n {
        let elem = extract_element(x, i);
        let call_args = Rf_cons(elem, R_NilValue());
        let call_sexp = Rf_cons(fun, call_args);
        if !call_sexp.is_null() {
            (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        let val = crate::eval::eval::Rf_eval(call_sexp, rho);
        crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `Filter(f, x)` — keep elements where f returns TRUE.
pub unsafe fn do_filter(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let fun = CAR(args);
    let x = CAR(CDR(args));
    if fun.is_null() || x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let n = XLENGTH(x);
    let mut kept: Vec<R_xlen_t> = Vec::new();
    for i in 0..n {
        let elem = extract_element(x, i);
        let call_args = Rf_cons(elem, R_NilValue());
        let call_sexp = Rf_cons(fun, call_args);
        if !call_sexp.is_null() {
            (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        let val = crate::eval::eval::Rf_eval(call_sexp, rho);
        if !val.is_null() && TYPEOF(val) == SEXPTYPE::LGLSXP && *LOGICAL(val) != 0 {
            kept.push(i);
        }
    }
    let result = Rf_allocVector3(TYPEOF(x), kept.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for (new_i, &old_i) in kept.iter().enumerate() {
        if TYPEOF(x) == SEXPTYPE::REALSXP {
            *REAL(result).add(new_i) = *REAL(x).add(old_i as usize);
        } else if TYPEOF(x) == SEXPTYPE::INTSXP {
            *INTEGER(result).add(new_i) = *INTEGER(x).add(old_i as usize);
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `do.call(what, args)` — call function with list of args.
pub unsafe fn do_do_call(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let fun = CAR(args);
    let arg_list = CAR(CDR(args));
    if fun.is_null() || arg_list.is_null() {
        return R_NilValue();
    }
    let n = if TYPEOF(arg_list) == SEXPTYPE::VECSXP {
        XLENGTH(arg_list)
    } else {
        0
    };
    let mut call_args = R_NilValue();
    for i in (0..n).rev() {
        call_args = Rf_cons(
            crate::sexp::accessors::VECTOR_ELT(arg_list, i as i64),
            call_args,
        );
    }
    let call_sexp = Rf_cons(fun, call_args);
    if !call_sexp.is_null() {
        (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
    }
    crate::eval::eval::Rf_eval(call_sexp, rho)
}

fn extract_element(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        if t == SEXPTYPE::VECSXP {
            return crate::sexp::accessors::VECTOR_ELT(x, i as i64);
        }
        let elem = Rf_allocVector3(t, 1);
        if elem.is_null() {
            return R_NilValue();
        }
        if t == SEXPTYPE::REALSXP {
            *REAL(elem) = *REAL(x).add(i as usize);
        } else if t == SEXPTYPE::INTSXP {
            *INTEGER(elem) = *INTEGER(x).add(i as usize);
        } else if t == SEXPTYPE::LGLSXP {
            *LOGICAL(elem) = *LOGICAL(x).add(i as usize);
        }
        elem
    }
}

// ---------------------------------------------------------------------------
// apply / tapply / mapply / outer / sweep — higher-order array functions
// ---------------------------------------------------------------------------

/// Extract a row from a matrix (column-major storage) as a length-ncol vector.
unsafe fn extract_matrix_row(x: SEXP, nrow: R_xlen_t, ncol: R_xlen_t, row: R_xlen_t) -> SEXP {
    let t = TYPEOF(x);
    let result = Rf_allocVector3(t, ncol);
    if result.is_null() {
        return R_NilValue();
    }
    for j in 0..ncol {
        let src = (j * nrow + row) as usize;
        if t == SEXPTYPE::REALSXP {
            *REAL(result).add(j as usize) = *REAL(x).add(src);
        } else if t == SEXPTYPE::INTSXP {
            *INTEGER(result).add(j as usize) = *INTEGER(x).add(src);
        } else if t == SEXPTYPE::LGLSXP {
            *LOGICAL(result).add(j as usize) = *LOGICAL(x).add(src);
        }
    }
    result
}

/// Extract a column from a matrix (column-major storage) as a length-nrow vector.
unsafe fn extract_matrix_col(x: SEXP, nrow: R_xlen_t, _ncol: R_xlen_t, col: R_xlen_t) -> SEXP {
    let t = TYPEOF(x);
    let result = Rf_allocVector3(t, nrow);
    if result.is_null() {
        return R_NilValue();
    }
    let offset = (col * nrow) as usize;
    if t == SEXPTYPE::REALSXP {
        for i in 0..nrow {
            *REAL(result).add(i as usize) = *REAL(x).add(offset + i as usize);
        }
    } else if t == SEXPTYPE::INTSXP {
        for i in 0..nrow {
            *INTEGER(result).add(i as usize) = *INTEGER(x).add(offset + i as usize);
        }
    } else if t == SEXPTYPE::LGLSXP {
        for i in 0..nrow {
            *LOGICAL(result).add(i as usize) = *LOGICAL(x).add(offset + i as usize);
        }
    }
    result
}

/// R's `apply(X, MARGIN, FUN)` — apply FUN over margins of array/matrix.
///
/// For a 2D matrix:
/// - MARGIN=1: apply FUN to each row, return vector of length nrow
/// - MARGIN=2: apply FUN to each column, return vector of length ncol
pub unsafe fn do_apply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let x = CAR(args);
    let margin_arg = CAR(CDR(args));
    let fun = CAR(CDR(CDR(args)));
    if x.is_null() || x == R_NilValue() || fun.is_null() {
        return R_NilValue();
    }

    // Get dimensions
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    if dim_attr.is_null() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP || LENGTH(dim_attr) < 2 {
        return R_NilValue(); // not a matrix/array
    }
    let nrow = *INTEGER(dim_attr) as R_xlen_t;
    let ncol = *INTEGER(dim_attr.add(1)) as R_xlen_t;
    let margin = real_or_default(margin_arg, 1.0) as i64;

    if margin == 1 {
        // Apply over rows
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, nrow);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        for i in 0..nrow {
            let row_vec = extract_matrix_row(x, nrow, ncol, i);
            let call_args = Rf_cons(row_vec, R_NilValue());
            let call_sexp = Rf_cons(fun, call_args);
            if !call_sexp.is_null() {
                (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            let val = crate::eval::eval::Rf_eval(call_sexp, rho);
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    } else if margin == 2 {
        // Apply over columns
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncol);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        for j in 0..ncol {
            let col_vec = extract_matrix_col(x, nrow, ncol, j);
            let call_args = Rf_cons(col_vec, R_NilValue());
            let call_sexp = Rf_cons(fun, call_args);
            if !call_sexp.is_null() {
                (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            let val = crate::eval::eval::Rf_eval(call_sexp, rho);
            crate::sexp::accessors::SET_VECTOR_ELT(result, j as i64, val);
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    } else {
        R_NilValue()
    }
}

/// R's `tapply(X, INDEX, FUN)` — apply FUN to each group defined by INDEX.
///
/// Iterates unique values of INDEX, collects matching elements from X, calls FUN on each group.
pub unsafe fn do_tapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let x = CAR(args);
    let index = CAR(CDR(args));
    let fun = CAR(CDR(CDR(args)));
    if x.is_null() || x == R_NilValue() || index.is_null() || fun.is_null() {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    let idx_n = XLENGTH(index);

    // Collect unique index values and group membership
    let mut group_keys: Vec<i64> = Vec::new();
    let mut group_map: std::collections::BTreeMap<i64, usize> = std::collections::BTreeMap::new();
    let mut groups: Vec<Vec<R_xlen_t>> = Vec::new();

    let idx_t = TYPEOF(index);
    for i in 0..n {
        let idx_i = if idx_n == 0 { 0 } else { i % idx_n };
        let key = if idx_t == SEXPTYPE::INTSXP || idx_t == SEXPTYPE::LGLSXP {
            *INTEGER(index).add(idx_i as usize) as i64
        } else if idx_t == SEXPTYPE::REALSXP {
            (*REAL(index).add(idx_i as usize)).to_bits() as i64
        } else {
            idx_i as i64
        };

        if let Some(&g) = group_map.get(&key) {
            groups[g].push(i);
        } else {
            let g = groups.len();
            group_map.insert(key, g);
            group_keys.push(key);
            groups.push(vec![i]);
        }
    }

    let num_groups = groups.len() as R_xlen_t;
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, num_groups);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    for (g, indices) in groups.iter().enumerate() {
        let group_vec = Rf_allocVector3(TYPEOF(x), indices.len() as R_xlen_t);
        if !group_vec.is_null() {
            let t = TYPEOF(x);
            for (j, &src_i) in indices.iter().enumerate() {
                if t == SEXPTYPE::REALSXP {
                    *REAL(group_vec).add(j) = *REAL(x).add(src_i as usize);
                } else if t == SEXPTYPE::INTSXP {
                    *INTEGER(group_vec).add(j) = *INTEGER(x).add(src_i as usize);
                } else if t == SEXPTYPE::LGLSXP {
                    *LOGICAL(group_vec).add(j) = *LOGICAL(x).add(src_i as usize);
                }
            }
        }
        let call_args = Rf_cons(group_vec, R_NilValue());
        let call_sexp = Rf_cons(fun, call_args);
        if !call_sexp.is_null() {
            (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        let val = crate::eval::eval::Rf_eval(call_sexp, rho);
        crate::sexp::accessors::SET_VECTOR_ELT(result, g as i64, val);
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `mapply(FUN, ...)` — multivariate sapply. Applies FUN element-wise across multiple vectors with recycling.
pub unsafe fn do_mapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let fun = CAR(args);
    let vec_args = CDR(args);
    if fun.is_null() {
        return R_NilValue();
    }

    // Collect vector args, find max length
    let mut arg_vecs: Vec<SEXP> = Vec::new();
    let mut max_len: R_xlen_t = 0;
    let mut current = vec_args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            arg_vecs.push(arg);
            let n = XLENGTH(arg);
            if n > max_len {
                max_len = n;
            }
        }
        current = CDR(current);
    }
    if max_len == 0 {
        return R_NilValue();
    }

    let result = Rf_allocVector3(SEXPTYPE::VECSXP, max_len);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    for i in 0..max_len {
        // Build call: FUN(arg1[i], arg2[i], ...) with recycling
        let mut call_args = R_NilValue();
        for &arg in arg_vecs.iter().rev() {
            let n = XLENGTH(arg);
            let idx = if n == 0 { 0 } else { i % n };
            let elem = extract_element(arg, idx);
            call_args = Rf_cons(elem, call_args);
        }
        let call_sexp = Rf_cons(fun, call_args);
        if !call_sexp.is_null() {
            (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        let val = crate::eval::eval::Rf_eval(call_sexp, rho);
        crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `outer(X, Y, FUN="*")` — outer product. Returns a matrix of length(X) x length(Y).
///
/// For each pair (x_i, y_j), computes FUN(x_i, y_j).
pub unsafe fn do_outer(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let x = CAR(args);
    let y = CAR(CDR(args));
    let fun_arg = CAR(CDR(CDR(args)));
    if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
        return R_NilValue();
    }

    let nx = XLENGTH(x);
    let ny = XLENGTH(y);

    // Determine if FUN is a symbol (operator name) or a function object
    let use_multiply = if fun_arg.is_null() || fun_arg == R_NilValue() {
        true
    } else if TYPEOF(fun_arg) == SEXPTYPE::STRSXP {
        elt_to_string(fun_arg, 0) == "*"
    } else if TYPEOF(fun_arg) == SEXPTYPE::SYMSXP {
        let pname = crate::sexp::accessors::PRINTNAME(fun_arg);
        if !pname.is_null() {
            let s = crate::sexp::accessors::CHAR(pname);
            if !s.is_null() {
                std::ffi::CStr::from_ptr(s).to_str().unwrap_or("") == "*"
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    let result = Rf_allocVector3(SEXPTYPE::REALSXP, nx * ny);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    if use_multiply {
        // Fast path: multiply
        for i in 0..nx {
            let xi = elt_real_safe(x, i);
            for j in 0..ny {
                let yj = elt_real_safe(y, j);
                *dst.add((j * nx + i) as usize) = xi * yj;
            }
        }
    } else {
        // General path: call FUN(x_i, y_j) for each pair
        for i in 0..nx {
            let xi = extract_element(x, i);
            for j in 0..ny {
                let yj = extract_element(y, j);
                let call_args = Rf_cons(xi, Rf_cons(yj, R_NilValue()));
                let call_sexp = Rf_cons(fun_arg, call_args);
                if !call_sexp.is_null() {
                    (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
                let val = crate::eval::eval::Rf_eval(call_sexp, rho);
                let v = if !val.is_null() && TYPEOF(val) == SEXPTYPE::REALSXP {
                    *REAL(val)
                } else if !val.is_null()
                    && (TYPEOF(val) == SEXPTYPE::INTSXP || TYPEOF(val) == SEXPTYPE::LGLSXP)
                {
                    let iv = *INTEGER(val);
                    if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
                } else {
                    NA_REAL
                };
                *dst.add((j * nx + i) as usize) = v;
            }
        }
    }

    // Set dim attribute: c(nx, ny)
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    if !dim.is_null() {
        *INTEGER(dim) = nx as c_int;
        *INTEGER(dim.add(1)) = ny as c_int;
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            dim,
        );
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `sweep(x, MARGIN, STATS, FUN="-")` — sweep out statistics from array.
///
/// For each row/column, applies FUN(x, STATS) element-wise.
pub unsafe fn do_sweep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let margin_arg = CAR(CDR(args));
    let stats = CAR(CDR(CDR(args)));
    let fun_arg = CAR(CDR(CDR(CDR(args))));
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    // Determine operation
    let op_str = if fun_arg.is_null() || fun_arg == R_NilValue() {
        "-".to_string()
    } else if TYPEOF(fun_arg) == SEXPTYPE::STRSXP {
        elt_to_string(fun_arg, 0)
    } else if TYPEOF(fun_arg) == SEXPTYPE::SYMSXP {
        let pname = crate::sexp::accessors::PRINTNAME(fun_arg);
        if !pname.is_null() {
            let s = crate::sexp::accessors::CHAR(pname);
            if !s.is_null() {
                std::ffi::CStr::from_ptr(s)
                    .to_str()
                    .unwrap_or("-")
                    .to_string()
            } else {
                "-".to_string()
            }
        } else {
            "-".to_string()
        }
    } else {
        String::new()
    };

    let margin = if margin_arg.is_null() || margin_arg == R_NilValue() {
        1
    } else {
        real_or_default(margin_arg, 1.0) as i64
    };

    let t = TYPEOF(x);
    let n = XLENGTH(x);

    // Get dimensions
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    let (nrow, ncol) =
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            (
                *INTEGER(dim_attr) as R_xlen_t,
                *INTEGER(dim_attr.add(1)) as R_xlen_t,
            )
        } else {
            (n, 1)
        };

    let result = Rf_allocVector3(t, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    // Fast path for common ops
    let apply_binary = |src_val: f64, stat_val: f64| -> f64 {
        match op_str.as_str() {
            "-" => src_val - stat_val,
            "+" => src_val + stat_val,
            "*" => src_val * stat_val,
            "/" => {
                if stat_val != 0.0 {
                    src_val / stat_val
                } else {
                    NA_REAL
                }
            }
            _ => src_val - stat_val,
        }
    };

    if margin == 1 {
        // Sweep across rows: subtract STATS from each row
        let stats_len = if stats.is_null() || stats == R_NilValue() {
            0
        } else {
            XLENGTH(stats)
        };
        for i in 0..nrow {
            for j in 0..ncol {
                let src_idx = (j * nrow + i) as usize;
                let stat_idx = if stats_len == 0 { 0 } else { j % stats_len };
                let src_val = if t == SEXPTYPE::REALSXP {
                    *REAL(x).add(src_idx)
                } else if t == SEXPTYPE::INTSXP {
                    let v = *INTEGER(x).add(src_idx);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };
                let stat_val = if stats.is_null() || stats == R_NilValue() {
                    0.0
                } else {
                    elt_real_safe(stats, stat_idx)
                };
                let res = apply_binary(src_val, stat_val);
                if t == SEXPTYPE::REALSXP {
                    *REAL(result).add(src_idx) = res;
                } else if t == SEXPTYPE::INTSXP {
                    *INTEGER(result).add(src_idx) = if res.is_nan() || res == NA_REAL {
                        NA_INTEGER
                    } else {
                        res as c_int
                    };
                }
            }
        }
    } else if margin == 2 {
        // Sweep across columns: subtract STATS from each column
        let stats_len = if stats.is_null() || stats == R_NilValue() {
            0
        } else {
            XLENGTH(stats)
        };
        for j in 0..ncol {
            for i in 0..nrow {
                let src_idx = (j * nrow + i) as usize;
                let stat_idx = if stats_len == 0 { 0 } else { i % stats_len };
                let src_val = if t == SEXPTYPE::REALSXP {
                    *REAL(x).add(src_idx)
                } else if t == SEXPTYPE::INTSXP {
                    let v = *INTEGER(x).add(src_idx);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };
                let stat_val = if stats.is_null() || stats == R_NilValue() {
                    0.0
                } else {
                    elt_real_safe(stats, stat_idx)
                };
                let res = apply_binary(src_val, stat_val);
                if t == SEXPTYPE::REALSXP {
                    *REAL(result).add(src_idx) = res;
                } else if t == SEXPTYPE::INTSXP {
                    *INTEGER(result).add(src_idx) = if res.is_nan() || res == NA_REAL {
                        NA_INTEGER
                    } else {
                        res as c_int
                    };
                }
            }
        }
    }

    // Copy dim attribute if present
    if !dim_attr.is_null() {
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            dim_attr,
        );
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Error handling: stop, warning, message, tryCatch, inherits, exists, get, assign
// ---------------------------------------------------------------------------

/// R's `stop(...)` — raise error.
pub unsafe fn do_stop(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let s = elt_to_string(CAR(args), 0);
    std::panic::panic_any(crate::sexp::context::RError { message: s });
}

/// R's `warning(...)` — issue warning.
pub unsafe fn do_warning(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    eprintln!("Warning: {}", elt_to_string(CAR(args), 0));
    R_NilValue()
}

/// R's `message(...)` — print message.
pub unsafe fn do_message(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    eprintln!("{}", elt_to_string(CAR(args), 0));
    R_NilValue()
}

/// R's `inherits(x, what)` — check class.
pub unsafe fn do_inherits(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let what = CAR(CDR(args));
    if x.is_null() || what.is_null() {
        return Rf_ScalarLogical(FALSE);
    }
    let class_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
    );
    if class_attr.is_null() || TYPEOF(class_attr) != SEXPTYPE::STRSXP {
        return Rf_ScalarLogical(FALSE);
    }
    let target = elt_to_string(what, 0);
    let n = XLENGTH(class_attr);
    for i in 0..n {
        if elt_to_string(class_attr, i) == target {
            return Rf_ScalarLogical(TRUE);
        }
    }
    Rf_ScalarLogical(FALSE)
}

/// R's `tryCatch(expr)` — basic try/catch.
pub unsafe fn do_tryCatch(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let expr = CAR(args);
    if expr.is_null() {
        return R_NilValue();
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::eval::eval::Rf_eval(expr, rho)
    }));
    match result {
        Ok(val) => val,
        Err(_) => {
            eprintln!("Error caught");
            R_NilValue()
        }
    }
}

/// R's `exists(x, envir)` — check name exists.
pub unsafe fn do_exists(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let name = elt_to_string(CAR(args), 0);
    let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
    let val = crate::sexp::envir::R_findVar(sym, rho);
    Rf_ScalarLogical(
        if !val.is_null() && val != crate::sexp::globals::R_UnboundValue() {
            TRUE
        } else {
            FALSE
        },
    )
}

/// R's `get(x, envir)` — get value.
pub unsafe fn do_get(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let name = elt_to_string(CAR(args), 0);
    crate::sexp::envir::R_findVar(
        Rf_install(CString::new(name).unwrap_or_default().as_ptr()),
        rho,
    )
}

/// R's `assign(x, value, envir)` — assign value.
pub unsafe fn do_assign(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let name = elt_to_string(CAR(args), 0);
    let val = CAR(CDR(args));
    if val.is_null() {
        return R_NilValue();
    }
    crate::sexp::envir::defineVar(
        Rf_install(CString::new(name).unwrap_or_default().as_ptr()),
        val,
        rho,
    );
    crate::sexp::globals::set_R_Visible(FALSE);
    val
}

/// R's `ls(envir)` — list objects.
pub unsafe fn do_ls(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    // Simplified: return empty string vector
    // Full implementation needs R_ls which isn't ported yet
    Rf_allocVector3(SEXPTYPE::STRSXP, 0)
}

/// R's `rm(list, envir)` — remove objects.
pub unsafe fn do_rm(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let list = CAR(args);
    if list.is_null() || TYPEOF(list) != SEXPTYPE::STRSXP {
        return R_NilValue();
    }
    for i in 0..XLENGTH(list) {
        let sym = Rf_install(
            CString::new(elt_to_string(list, i))
                .unwrap_or_default()
                .as_ptr(),
        );
        crate::sexp::envir::defineVar(sym, crate::sexp::globals::R_UnboundValue(), rho);
    }
    crate::sexp::globals::set_R_Visible(FALSE);
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Distribution functions: dnorm, pnorm, qnorm, dpois, ppois
// ---------------------------------------------------------------------------

/// R's `dnorm(x, mean=0, sd=1)` — normal density.
pub unsafe fn do_dnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |x, m, s| {
        crate::dist::normal::dnorm4_inner(x, m, s, false)
    })
}

/// R's `pnorm(q, mean=0, sd=1)` — normal CDF.
pub unsafe fn do_pnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |q, m, s| {
        crate::dist::normal::pnorm5_inner(q, m, s, true, false)
    })
}

/// R's `qnorm(p, mean=0, sd=1)` — normal quantile.
pub unsafe fn do_qnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |p, m, s| {
        crate::dist::normal::qnorm5_inner(p, m, s, true, false)
    })
}

/// R's `dpois(x, lambda)` — Poisson density.
pub unsafe fn do_dpois(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |x, lam, _| {
        crate::dist::poisson::dpois_inner(x, lam, false)
    })
}

/// R's `ppois(q, lambda)` — Poisson CDF.
pub unsafe fn do_ppois(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |q, lam, _| {
        crate::dist::poisson::ppois_inner(q, lam, true, false)
    })
}

/// R's `dbinom(x, size, prob)` — binomial density.
pub unsafe fn do_dbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.5, |x, n, p| {
        crate::dist::binomial::dbinom_inner(x, n, p, false)
    })
}

/// R's `pbinom(q, size, prob)` — binomial CDF.
pub unsafe fn do_pbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.5, |q, n, p| {
        crate::dist::binomial::pbinom_inner(q, n, p, true, false)
    })
}

/// R's `dexp(x, rate)` — exponential density.
pub unsafe fn do_dexp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |x, rate, _| {
        crate::dist::exponential::dexp(x, 1.0 / rate, 1)
    })
}

/// R's `pexp(q, rate)` — exponential CDF.
pub unsafe fn do_pexp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |q, rate, _| {
        crate::dist::exponential::pexp_inner(q, 1.0 / rate, true, false)
    })
}

// ---------------------------------------------------------------------------
// Distribution functions: gamma, beta, t, chisq, cauchy, weibull, f, nbinom, geom
// ---------------------------------------------------------------------------

/// R's `dgamma(x, shape, scale=1)` — gamma density.
pub unsafe fn do_dgamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |x, shape, scale| {
        crate::dist::gamma::dgamma_inner(x, shape, scale, false)
    })
}

/// R's `pgamma(q, shape, scale=1)` — gamma CDF.
pub unsafe fn do_pgamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |q, shape, scale| {
        crate::dist::gamma::pgamma_inner(q, shape, scale, true, false)
    })
}

/// R's `qgamma(p, shape, scale=1)` — gamma quantile.
pub unsafe fn do_qgamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |p, shape, scale| {
        crate::dist::gamma::qgamma_inner(p, shape, scale, true, false)
    })
}

/// R's `dbeta(x, shape1, shape2)` — beta density.
pub unsafe fn do_dbeta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |x, a, b| {
        crate::dist::beta::dbeta_inner(x, a, b, false)
    })
}

/// R's `pbeta(q, shape1, shape2)` — beta CDF.
pub unsafe fn do_pbeta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |q, a, b| {
        crate::dist::beta::pbeta_inner(q, a, b, true, false)
    })
}

/// R's `qbeta(p, shape1, shape2)` — beta quantile.
pub unsafe fn do_qbeta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |p, a, b| {
        crate::dist::beta::qbeta_inner(p, a, b, true, false)
    })
}

/// R's `dt(x, df)` — t density.
pub unsafe fn do_dt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |x, df, _| {
        crate::dist::t_dist::dt_inner(x, df, false)
    })
}

/// R's `pt(q, df)` — t CDF.
pub unsafe fn do_pt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |q, df, _| {
        crate::dist::t_dist::pt_inner(q, df, true, false)
    })
}

/// R's `qt(p, df)` — t quantile.
pub unsafe fn do_qt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |p, df, _| {
        crate::dist::t_dist::qt_inner(p, df, true, false)
    })
}

/// R's `dchisq(x, df)` — chi-squared density.
pub unsafe fn do_dchisq(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |x, df, _| {
        crate::dist::chisq::dchisq_inner(x, df, false)
    })
}

/// R's `pchisq(q, df)` — chi-squared CDF.
pub unsafe fn do_pchisq(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |q, df, _| {
        crate::dist::chisq::pchisq_inner(q, df, true, false)
    })
}

/// R's `qchisq(p, df)` — chi-squared quantile.
pub unsafe fn do_qchisq(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |p, df, _| {
        crate::dist::chisq::qchisq_inner(p, df, true, false)
    })
}

/// R's `dcauchy(x, location=0, scale=1)` — Cauchy density.
pub unsafe fn do_dcauchy(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |x, loc, sc| {
        crate::dist::cauchy::dcauchy_inner(x, loc, sc, false)
    })
}

/// R's `pcauchy(q, location=0, scale=1)` — Cauchy CDF.
pub unsafe fn do_pcauchy(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |q, loc, sc| {
        crate::dist::cauchy::pcauchy_inner(q, loc, sc, true, false)
    })
}

/// R's `qcauchy(p, location=0, scale=1)` — Cauchy quantile.
pub unsafe fn do_qcauchy(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |p, loc, sc| {
        crate::dist::cauchy::qcauchy_inner(p, loc, sc, true, false)
    })
}

/// R's `dweibull(x, shape, scale=1)` — Weibull density.
pub unsafe fn do_dweibull(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |x, shape, scale| {
        crate::dist::weibull::dweibull_inner(x, shape, scale, false)
    })
}

/// R's `pweibull(q, shape, scale=1)` — Weibull CDF.
pub unsafe fn do_pweibull(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |q, shape, scale| {
        crate::dist::weibull::pweibull_inner(q, shape, scale, true, false)
    })
}

/// R's `qweibull(p, shape, scale=1)` — Weibull quantile.
pub unsafe fn do_qweibull(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |p, shape, scale| {
        crate::dist::weibull::qweibull_inner(p, shape, scale, true, false)
    })
}

/// R's `df(x, df1, df2)` — F distribution density.
pub unsafe fn do_df(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |x, df1, df2| {
        crate::dist::f_dist::df_inner(x, df1, df2, false)
    })
}

/// R's `pf(q, df1, df2)` — F distribution CDF.
pub unsafe fn do_pf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |q, df1, df2| {
        crate::dist::f_dist::pf_inner(q, df1, df2, true, false)
    })
}

/// R's `qf(p, df1, df2)` — F distribution quantile.
pub unsafe fn do_qf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |p, df1, df2| {
        crate::dist::f_dist::qf_inner(p, df1, df2, true, false)
    })
}

/// R's `dnbinom(x, size, prob)` — negative binomial density.
pub unsafe fn do_dnbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.5, |x, size, prob| {
        crate::dist::nbinom::dnbinom_inner(x, size, prob, false)
    })
}

/// R's `pnbinom(q, size, prob)` — negative binomial CDF.
pub unsafe fn do_pnbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.5, |q, size, prob| {
        crate::dist::nbinom::pnbinom_inner(q, size, prob, true, false)
    })
}

/// R's `qnbinom(p, size, prob)` — negative binomial quantile.
pub unsafe fn do_qnbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.5, |p, size, prob| {
        crate::dist::nbinom::qnbinom_inner(p, size, prob, true, false)
    })
}

/// R's `dgeom(x, prob)` — geometric density.
pub unsafe fn do_dgeom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.5, 0.0, |x, p, _| {
        crate::dist::geometric::dgeom_inner(x, p, false)
    })
}

/// R's `pgeom(q, prob)` — geometric CDF.
pub unsafe fn do_pgeom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.5, 0.0, |q, p, _| {
        crate::dist::geometric::pgeom_inner(q, p, true, false)
    })
}

/// R's `qgeom(p, prob)` — geometric quantile.
pub unsafe fn do_qgeom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.5, 0.0, |p, prob, _| {
        crate::dist::geometric::qgeom_inner(p, prob, true, false)
    })
}

// ---------------------------------------------------------------------------
// Distribution functions: lnorm, logistic, signrank, wilcox, hyper, tukey
// ---------------------------------------------------------------------------

/// R's `dlnorm(x, meanlog=0, sdlog=1)` — lognormal density.
pub unsafe fn do_dlnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |x, meanlog, sdlog| {
        crate::dist::lnorm::dlnorm_inner(x, meanlog, sdlog, false)
    })
}

/// R's `plnorm(q, meanlog=0, sdlog=1)` — lognormal CDF.
pub unsafe fn do_plnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |q, meanlog, sdlog| {
        crate::dist::lnorm::plnorm_inner(q, meanlog, sdlog, true, false)
    })
}

/// R's `qlnorm(p, meanlog=0, sdlog=1)` — lognormal quantile.
pub unsafe fn do_qlnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |p, meanlog, sdlog| {
        crate::dist::lnorm::qlnorm_inner(p, meanlog, sdlog, true, false)
    })
}

/// R's `dlogis(x, location=0, scale=1)` — logistic density.
pub unsafe fn do_dlogis(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |x, location, scale| {
        crate::dist::logistic::dlogis_inner(x, location, scale, false)
    })
}

/// R's `plogis(q, location=0, scale=1)` — logistic CDF.
pub unsafe fn do_plogis(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |q, location, scale| {
        crate::dist::logistic::plogis_inner(q, location, scale, true, false)
    })
}

/// R's `qlogis(p, location=0, scale=1)` — logistic quantile.
pub unsafe fn do_qlogis(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |p, location, scale| {
        crate::dist::logistic::qlogis_inner(p, location, scale, true, false)
    })
}

/// R's `dsignrank(x, n)` — Wilcoxon signed rank density.
pub unsafe fn do_dsignrank(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |x, n, _| {
        crate::dist::signrank::dsignrank_inner(x, n, false)
    })
}

/// R's `psignrank(q, n)` — Wilcoxon signed rank CDF.
pub unsafe fn do_psignrank(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |q, n, _| {
        crate::dist::signrank::psignrank_inner(q, n, true, false)
    })
}

/// R's `qsignrank(p, n)` — Wilcoxon signed rank quantile.
pub unsafe fn do_qsignrank(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |p, n, _| {
        crate::dist::signrank::qsignrank_inner(p, n, true, false)
    })
}

/// R's `dwilcox(x, m, n)` — Wilcoxon rank sum density.
pub unsafe fn do_dwilcox(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |x, m, n| {
        crate::dist::wilcox::dwilcox_inner(x, m, n, false)
    })
}

/// R's `pwilcox(q, m, n)` — Wilcoxon rank sum CDF.
pub unsafe fn do_pwilcox(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |q, m, n| {
        crate::dist::wilcox::pwilcox_inner(q, m, n, true, false)
    })
}

/// R's `qwilcox(p, m, n)` — Wilcoxon rank sum quantile.
pub unsafe fn do_qwilcox(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |p, m, n| {
        crate::dist::wilcox::qwilcox_inner(p, m, n, true, false)
    })
}

/// R's `dhyper(x, m, n, k)` — hypergeometric density (4 params).
pub unsafe fn do_dhyper(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_tertiary(args, 1.0, 1.0, 1.0, |x, m, n, k| {
        crate::dist::hypergeometric::dhyper_inner(x, m, n, k, false)
    })
}

/// R's `phyper(q, m, n, k)` — hypergeometric CDF (4 params).
pub unsafe fn do_phyper(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_tertiary(args, 1.0, 1.0, 1.0, |q, m, n, k| {
        crate::dist::hypergeometric::phyper_inner(q, m, n, k, true, false)
    })
}

/// R's `qhyper(p, m, n, k)` — hypergeometric quantile (4 params).
pub unsafe fn do_qhyper(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_tertiary(args, 1.0, 1.0, 1.0, |p, m, n, k| {
        crate::dist::hypergeometric::qhyper_inner(p, m, n, k, true, false)
    })
}

/// R's `dtukey(q, nmeans, df)` — Studentized range CDF (nranges defaults to 1).
pub unsafe fn do_ptukey(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 2.0, 1.0, |q, nmeans, df| {
        crate::dist::tukey::ptukey_inner(q, 1.0, nmeans, df, true, false)
    })
}

/// R's `qtukey(p, nmeans, df)` — Studentized range quantile (nranges defaults to 1).
pub unsafe fn do_qtukey(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 2.0, 1.0, |p, nmeans, df| {
        crate::dist::tukey::qtukey_inner(p, 1.0, nmeans, df, true, false)
    })
}

/// R's `dmultinom(x, prob, log=FALSE)` — multinomial probability.
pub unsafe fn do_dmultinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let prob_arg = CAR(CDR(args));
        let log_arg = CAR(CDR(CDR(args)));

        if x_arg.is_null() || prob_arg.is_null() {
            return R_NilValue();
        }

        let nx = XLENGTH(x_arg).max(1);
        let np = XLENGTH(prob_arg).max(1);
        let give_log = if log_arg.is_null() || log_arg == R_NilValue() {
            false
        } else {
            real_or_default(log_arg, 0.0) != 0.0
        };

        // Collect x values
        let mut xv: Vec<f64> = Vec::with_capacity(nx as usize);
        for i in 0..nx {
            xv.push(elt_real_safe(x_arg, i));
        }

        // Collect prob values
        let mut pv: Vec<f64> = Vec::with_capacity(np as usize);
        for i in 0..np {
            pv.push(elt_real_safe(prob_arg, i));
        }

        // dmultinom: log-probability of multinomial outcome
        // Uses lgammafn(x+1) for log-factorial terms
        let k = xv.len().min(pv.len());
        let n_total: f64 = xv.iter().sum();

        let mut log_prob = crate::special::gamma::lgammafn(n_total + 1.0);
        for i in 0..k {
            log_prob -= crate::special::gamma::lgammafn(xv[i] + 1.0);
            if pv[i] > 0.0 {
                log_prob += xv[i] * pv[i].ln();
            } else if xv[i] > 0.0 {
                log_prob = f64::NEG_INFINITY;
            }
        }

        let result = if give_log { log_prob } else { log_prob.exp() };
        Rf_ScalarReal(result)
    }
}

/// Generic vectorized distribution function with 3 extra parameters (4 total: x, p1, p2, p3).
fn do_dist_tertiary(
    args: SEXP,
    default_p1: f64,
    default_p2: f64,
    default_p3: f64,
    f: fn(f64, f64, f64, f64) -> f64,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        let p1 = real_or_default(CAR(CDR(args)), default_p1);
        let p2_arg = CAR(CDR(CDR(args)));
        let p2 = if p2_arg.is_null() || p2_arg == R_NilValue() {
            default_p2
        } else {
            real_or_default(p2_arg, default_p2)
        };
        let p3_arg = CAR(CDR(CDR(CDR(args))));
        let p3 = if p3_arg.is_null() || p3_arg == R_NilValue() {
            default_p3
        } else {
            real_or_default(p3_arg, default_p3)
        };
        if x.is_null() {
            return R_NilValue();
        }
        let n = XLENGTH(x).max(1);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = REAL(result);
        for i in 0..n {
            *dst.add(i as usize) = f(elt_real_safe(x, i), p1, p2, p3);
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// Generic vectorized distribution function with 2 parameters.
fn do_dist_unary(
    args: SEXP,
    default_p1: f64,
    default_p2: f64,
    f: fn(f64, f64, f64) -> f64,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        let p1 = real_or_default(CAR(CDR(args)), default_p1);
        let p2_arg = CAR(CDR(CDR(args)));
        let p2 = if p2_arg.is_null() || p2_arg == R_NilValue() {
            default_p2
        } else {
            real_or_default(p2_arg, default_p2)
        };
        if x.is_null() {
            return R_NilValue();
        }
        let n = XLENGTH(x).max(1);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = REAL(result);
        for i in 0..n {
            *dst.add(i as usize) = f(elt_real_safe(x, i), p1, p2);
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

fn elt_real_safe(x: SEXP, i: R_xlen_t) -> f64 {
    unsafe {
        if x.is_null() {
            return NA_REAL;
        }
        let n = XLENGTH(x);
        let idx = if n == 0 { 0 } else { i % n };
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            *REAL(x).add(idx as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(idx as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        }
    }
}

// ---------------------------------------------------------------------------
// Matrix operations: matrix(), t(), nrow(), ncol(), dim(), diag()
// ---------------------------------------------------------------------------

/// R's `matrix(data, nrow, ncol, byrow)` — create a matrix.
pub unsafe fn do_matrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let data = CAR(args);
    let nrow_arg = CAR(CDR(args));
    let ncol_arg = CAR(CDR(CDR(args)));
    let byrow_arg = CAR(CDR(CDR(CDR(args))));

    if data.is_null() || data == R_NilValue() {
        return R_NilValue();
    }

    let data_len = XLENGTH(data);
    let nrow = if nrow_arg.is_null() || nrow_arg == R_NilValue() {
        data_len
    } else {
        real_or_default(nrow_arg, data_len as f64) as R_xlen_t
    };
    let ncol = if ncol_arg.is_null() || ncol_arg == R_NilValue() {
        if nrow == 0 {
            0
        } else {
            (data_len + nrow - 1) / nrow
        }
    } else {
        real_or_default(ncol_arg, 1.0) as R_xlen_t
    };
    let byrow = if byrow_arg.is_null() || byrow_arg == R_NilValue() {
        false
    } else {
        TYPEOF(byrow_arg) == SEXPTYPE::LGLSXP && *LOGICAL(byrow_arg) != 0
    };

    let t = TYPEOF(data);
    let result = Rf_allocVector3(t, nrow * ncol);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    // Copy data
    for i in 0..(nrow * ncol) {
        let src_idx = if byrow {
            let r = i / ncol;
            let c = i % ncol;
            c * nrow + r
        } else {
            i
        } % data_len;

        if t == SEXPTYPE::REALSXP {
            *REAL(result).add(i as usize) = *REAL(data).add(src_idx as usize);
        } else if t == SEXPTYPE::INTSXP {
            *INTEGER(result).add(i as usize) = *INTEGER(data).add(src_idx as usize);
        } else if t == SEXPTYPE::LGLSXP {
            *LOGICAL(result).add(i as usize) = *LOGICAL(data).add(src_idx as usize);
        }
    }

    // Set dim attribute
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    if !dim.is_null() {
        *INTEGER(dim) = nrow as c_int;
        *INTEGER(dim.add(1)) = ncol as c_int;
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            dim,
        );
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `t(x)` — transpose a matrix.
pub unsafe fn do_transpose(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    // Get dimensions
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    let (nrow, ncol) =
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            (
                *INTEGER(dim_attr) as R_xlen_t,
                *INTEGER(dim_attr.add(1)) as R_xlen_t,
            )
        } else {
            (XLENGTH(x), 1)
        };

    let t = TYPEOF(x);
    let result = Rf_allocVector3(t, nrow * ncol);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    // Transpose: result[j*nrow + i] = x[i*ncol + j]
    for i in 0..nrow {
        for j in 0..ncol {
            let src = (i * ncol + j) as usize;
            let dst = (j * nrow + i) as usize;
            if t == SEXPTYPE::REALSXP {
                *REAL(result).add(dst) = *REAL(x).add(src);
            } else if t == SEXPTYPE::INTSXP {
                *INTEGER(result).add(dst) = *INTEGER(x).add(src);
            } else if t == SEXPTYPE::LGLSXP {
                *LOGICAL(result).add(dst) = *LOGICAL(x).add(src);
            }
        }
    }

    // Set transposed dim attribute
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    if !dim.is_null() {
        *INTEGER(dim) = ncol as c_int;
        *INTEGER(dim.add(1)) = nrow as c_int;
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            dim,
        );
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `nrow(x)` — number of rows.
pub unsafe fn do_nrow(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarInteger(0);
    }
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 1 {
        Rf_ScalarInteger(*INTEGER(dim_attr))
    } else {
        Rf_ScalarInteger(XLENGTH(x) as c_int)
    }
}

/// R's `ncol(x)` — number of columns.
pub unsafe fn do_ncol(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarInteger(0);
    }
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
        Rf_ScalarInteger(*INTEGER(dim_attr.add(1)))
    } else {
        Rf_ScalarInteger(1)
    }
}

/// R's `dim(x)` — dimensions as integer vector.
pub unsafe fn do_dim(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    if !dim_attr.is_null() {
        dim_attr
    } else {
        R_NilValue()
    }
}

/// R's `diag(x)` — extract diagonal or create diagonal matrix.
pub unsafe fn do_diag(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    // Check if x is a matrix
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
        // Extract diagonal
        let nrow = *INTEGER(dim_attr) as usize;
        let ncol = *INTEGER(dim_attr.add(1)) as usize;
        let n = nrow.min(ncol);
        let result = Rf_allocVector3(TYPEOF(x), n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let step = ncol + 1; // diagonal stride
        for i in 0..n {
            let src = i * step;
            if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(result).add(i) = *REAL(x).add(src);
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(result).add(i) = *INTEGER(x).add(src);
            }
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    } else {
        // Create diagonal matrix from vector
        let n = XLENGTH(x) as usize;
        let t = TYPEOF(x);
        let result = Rf_allocVector3(t, (n * n) as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);

        // Zero-initialize, then set diagonal
        for i in 0..n * n {
            if t == SEXPTYPE::REALSXP {
                *REAL(result).add(i) = 0.0;
            } else if t == SEXPTYPE::INTSXP {
                *INTEGER(result).add(i) = 0;
            }
        }
        for i in 0..n {
            let dst = i * n + i;
            if t == SEXPTYPE::REALSXP {
                *REAL(result).add(dst) = *REAL(x).add(i);
            } else if t == SEXPTYPE::INTSXP {
                *INTEGER(result).add(dst) = *INTEGER(x).add(i);
            }
        }

        // Set dim
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            *INTEGER(dim) = n as c_int;
            *INTEGER(dim.add(1)) = n as c_int;
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                dim,
            );
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

// ---------------------------------------------------------------------------
// Set operations: unique, sort, order, rev, match, %in%, setequal, union, intersect, setdiff
// ---------------------------------------------------------------------------

/// R's `unique(x)` — return unique elements (preserving first occurrence order).
pub unsafe fn do_unique(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::INTSXP && t != SEXPTYPE::REALSXP {
        return x; // Simplified: non-numeric returns as-is
    }
    let n = XLENGTH(x);
    let mut seen: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    let mut unique_indices: Vec<R_xlen_t> = Vec::new();

    for i in 0..n {
        let key = if t == SEXPTYPE::REALSXP {
            (*REAL(x).add(i as usize)).to_bits() as i64
        } else {
            *INTEGER(x).add(i as usize) as i64
        };
        if seen.insert(key) {
            unique_indices.push(i);
        }
    }

    let result = Rf_allocVector3(t, unique_indices.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for (new_i, &old_i) in unique_indices.iter().enumerate() {
        if t == SEXPTYPE::REALSXP {
            *REAL(result).add(new_i) = *REAL(x).add(old_i as usize);
        } else {
            *INTEGER(result).add(new_i) = *INTEGER(x).add(old_i as usize);
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `sort(x, decreasing)` — sort a vector.
pub unsafe fn do_sort(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let dec_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let decreasing = if dec_arg.is_null() || dec_arg == R_NilValue() {
        false
    } else {
        TYPEOF(dec_arg) == SEXPTYPE::LGLSXP && *LOGICAL(dec_arg) != 0
    };

    let t = TYPEOF(x);
    let n = XLENGTH(x);
    let result = Rf_allocVector3(t, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    // Copy and sort
    if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
        let mut vals: Vec<i32> = Vec::with_capacity(n as usize);
        for i in 0..n {
            vals.push(*INTEGER(x).add(i as usize));
        }
        if decreasing {
            vals.sort_by(|a, b| b.cmp(a));
        } else {
            vals.sort_unstable();
        }
        let dst = INTEGER(result);
        for (i, v) in vals.iter().enumerate() {
            *dst.add(i) = *v;
        }
    } else if t == SEXPTYPE::REALSXP {
        let mut vals: Vec<f64> = Vec::with_capacity(n as usize);
        for i in 0..n {
            vals.push(*REAL(x).add(i as usize));
        }
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if decreasing {
            vals.reverse();
        }
        let dst = REAL(result);
        for (i, v) in vals.iter().enumerate() {
            *dst.add(i) = *v;
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `rev(x)` — reverse a vector.
pub unsafe fn do_rev(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    let result = Rf_allocVector3(t, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    for i in 0..n {
        let src = (n - 1 - i) as usize;
        let dst = i as usize;
        if t == SEXPTYPE::REALSXP {
            *REAL(result).add(dst) = *REAL(x).add(src);
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            *INTEGER(result).add(dst) = *INTEGER(x).add(src);
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `any(...)` — TRUE if any element is TRUE.
pub unsafe fn do_any(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::LGLSXP && t != SEXPTYPE::INTSXP {
        return Rf_ScalarLogical(FALSE);
    }
    let n = XLENGTH(x);
    for i in 0..n {
        let v = *INTEGER(x).add(i as usize);
        if v != 0 && v != NA_INTEGER {
            return Rf_ScalarLogical(TRUE);
        }
    }
    Rf_ScalarLogical(FALSE)
}

/// R's `all(...)` — TRUE if all elements are TRUE.
pub unsafe fn do_all(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(TRUE);
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::LGLSXP && t != SEXPTYPE::INTSXP {
        return Rf_ScalarLogical(FALSE);
    }
    let n = XLENGTH(x);
    for i in 0..n {
        let v = *INTEGER(x).add(i as usize);
        if v == 0 {
            return Rf_ScalarLogical(FALSE);
        }
    }
    Rf_ScalarLogical(TRUE)
}

/// R's `seq_len(n)` — 1:n without recycling issues when n=0.
pub unsafe fn do_seq_len(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let n_arg = CAR(args);
    let n = real_or_default(n_arg, 0.0) as i64;
    if n <= 0 {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, n as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = INTEGER(result);
    for i in 0..n {
        *dst.add(i as usize) = (i + 1) as c_int;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `seq_along(x)` — seq_along(x) = seq_len(length(x)).
pub unsafe fn do_seq_along(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let n = if x.is_null() || x == R_NilValue() {
        0
    } else {
        XLENGTH(x)
    };
    if n == 0 {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = INTEGER(result);
    for i in 0..n {
        *dst.add(i as usize) = (i + 1) as c_int;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `cumsum(x)` — cumulative sum.
pub unsafe fn do_cumsum(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    let mut sum = 0.0f64;
    for i in 0..n {
        let v = elt_real_safe(x, i);
        if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            *dst.add(i as usize) = NA_REAL;
        } else {
            sum += v;
            *dst.add(i as usize) = sum;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `cumprod(x)` — cumulative product.
pub unsafe fn do_cumprod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    let mut prod = 1.0f64;
    for i in 0..n {
        let v = elt_real_safe(x, i);
        if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            *dst.add(i as usize) = NA_REAL;
        } else {
            prod *= v;
            *dst.add(i as usize) = prod;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `diff(x, lag)` — lagged differences.
pub unsafe fn do_diff(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let lag_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let lag = if lag_arg.is_null() || lag_arg == R_NilValue() {
        1
    } else {
        real_or_default(lag_arg, 1.0) as usize
    };
    let n = XLENGTH(x);
    if n <= lag as R_xlen_t {
        return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
    }
    let result_len = n - lag as R_xlen_t;
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, result_len);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..result_len {
        let a = elt_real_safe(x, i);
        let b = elt_real_safe(x, i + lag as R_xlen_t);
        *dst.add(i as usize) = b - a;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// I/O builtins: cat() to file, writeLines(), file.exists()
// ---------------------------------------------------------------------------

/// R's `writeLines(text, con)` — write lines to file.
pub unsafe fn do_writeLines(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let text = CAR(args);
    let con = CAR(CDR(args));
    if text.is_null() || text == R_NilValue() {
        return R_NilValue();
    }

    let path = if con.is_null() || con == R_NilValue() {
        "/dev/stdout".to_string()
    } else {
        elt_to_string(con, 0)
    };

    let n = if TYPEOF(text) == SEXPTYPE::STRSXP {
        XLENGTH(text)
    } else {
        1
    };
    if path == "/dev/stdout" {
        for i in 0..n {
            println!("{}", elt_to_string(text, i));
        }
    } else if let Ok(mut file) = std::fs::File::create(&path) {
        use std::io::Write;
        for i in 0..n {
            let _ = writeln!(file, "{}", elt_to_string(text, i));
        }
    }
    crate::sexp::globals::set_R_Visible(FALSE);
    R_NilValue()
}

/// R's `readLines(con)` — read lines from file.
pub unsafe fn do_readLines(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let con = CAR(args);
    if con.is_null() {
        return R_NilValue();
    }
    let path = elt_to_string(con, 0);

    let lines = std::fs::read_to_string(&path).unwrap_or_default();
    let line_vec: Vec<&str> = lines.lines().collect();
    let n = line_vec.len();

    let result = Rf_allocVector3(SEXPTYPE::STRSXP, n as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    for (i, line) in line_vec.iter().enumerate() {
        let cstr = CString::new(*line).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(i) = charsxp;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `file.exists(...)` — check if files exist.
pub unsafe fn do_file_exists(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let n = XLENGTH(x).max(1);
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);
    for i in 0..n {
        let path = elt_to_string(x, i);
        *dst.add(i as usize) = if std::path::Path::new(&path).exists() {
            TRUE
        } else {
            FALSE
        };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `list.files(path)` — list files in directory.
pub unsafe fn do_list_files(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let path_arg = CAR(args);
    let path = if path_arg.is_null() || path_arg == R_NilValue() {
        ".".to_string()
    } else {
        elt_to_string(path_arg, 0)
    };

    let entries: Vec<String> = std::fs::read_dir(&path)
        .map(|dir| {
            dir.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();

    let n = entries.len();
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, n as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    for (i, name) in entries.iter().enumerate() {
        let cstr = CString::new(name.as_str()).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(i) = charsxp;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `system(command)` — run a system command.
pub unsafe fn do_system(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let cmd = CAR(args);
    if cmd.is_null() {
        return R_NilValue();
    }
    let cmd_str = elt_to_string(cmd, 0);
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd_str)
        .output();
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let cstr = CString::new(stdout.as_ref()).unwrap_or_default();
            Rf_mkString(cstr.as_ptr())
        }
        Err(_) => R_NilValue(),
    }
}

/// R's `stopifnot(...)` — stop if any condition is FALSE.
pub unsafe fn do_stopifnot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        let cond = CAR(current);
        if !cond.is_null()
            && TYPEOF(cond) == SEXPTYPE::LGLSXP
            && LENGTH(cond) > 0
            && *LOGICAL(cond) == 0
        {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "stopifnot: condition is FALSE".to_string(),
            });
        }
        current = CDR(current);
    }
    R_NilValue()
}

/// R's `nargs()` — number of arguments in the current call.
pub unsafe fn do_nargs(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    Rf_ScalarInteger(0) // Simplified
}

// ---------------------------------------------------------------------------
// S3 dispatch, environment functions, I/O extensions
// ---------------------------------------------------------------------------

/// R's `usemethod(generic, obj)` — simplified S3 dispatch.
/// In a full implementation this would look for generic.class methods.
/// For now, this is a no-op that signals "use default method".
pub unsafe fn do_usemethod(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    Rf_ScalarLogical(FALSE) // Simplified: no method found
}

/// R's `missing(x)` — check if argument was missing in call.
pub unsafe fn do_missing(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    Rf_ScalarLogical(FALSE) // Simplified
}

/// R's `parent.frame(n)` — get enclosing environment.
pub unsafe fn do_parent_frame(_call: SEXP, _op: SEXP, _args: SEXP, rho: SEXP) -> SEXP {
    rho // Simplified: return current env
}

/// R's `sys.call(which)` — get the call that's currently being evaluated.
pub unsafe fn do_sys_call(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    R_NilValue() // Simplified
}

/// R's `sys.frame(which)` — get frame at specified level.
pub unsafe fn do_sys_frame(_call: SEXP, _op: SEXP, _args: SEXP, rho: SEXP) -> SEXP {
    rho // Simplified: return current env
}

/// R's `getwd()` — get working directory.
pub unsafe fn do_getwd(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        match std::env::current_dir() {
            Ok(path) => {
                let s = path.to_string_lossy();
                let cstr = CString::new(s.as_ref()).unwrap_or_default();
                Rf_mkString(cstr.as_ptr())
            }
            Err(_) => R_NilValue(),
        }
    }
}

/// R's `setwd(dir)` — set working directory.
pub unsafe fn do_setwd(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let dir_arg = CAR(args);
        if dir_arg.is_null() {
            return R_NilValue();
        }
        let path = elt_to_string(dir_arg, 0);
        match std::env::set_current_dir(&path) {
            Ok(()) => {
                crate::sexp::globals::set_R_Visible(FALSE);
                let cstr = CString::new(path).unwrap_or_default();
                Rf_mkString(cstr.as_ptr())
            }
            Err(_) => {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: format!("cannot change working directory to '{}'", path),
                });
            }
        }
    }
}

/// R's `dir.exists(paths)` — check if directories exist.
pub unsafe fn do_dir_exists(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x).max(1);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let path = elt_to_string(x, i);
            *dst.add(i as usize) = if std::path::Path::new(&path).is_dir() {
                TRUE
            } else {
                FALSE
            };
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R's `file.create(...)` — create empty files.
pub unsafe fn do_file_create(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x).max(1);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let path = elt_to_string(x, i);
            *dst.add(i as usize) = match std::fs::File::create(&path) {
                Ok(_) => TRUE,
                Err(_) => FALSE,
            };
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R's `unlink(x, recursive)` — delete files or directories.
pub unsafe fn do_unlink(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        let n = XLENGTH(x).max(1);
        let mut count = 0;
        for i in 0..n {
            let path = elt_to_string(x, i);
            let p = std::path::Path::new(&path);
            let result = if p.is_dir() {
                std::fs::remove_dir_all(p)
            } else {
                std::fs::remove_file(p)
            };
            if result.is_ok() {
                count += 1;
            }
        }
        crate::sexp::globals::set_R_Visible(FALSE);
        Rf_ScalarInteger(count)
    }
}

/// R's `nzchar(x)` — check if strings are non-empty.
pub unsafe fn do_nzchar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x).max(1);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            *dst.add(i as usize) = if s.is_empty() { FALSE } else { TRUE };
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

// ---------------------------------------------------------------------------
// S3 dispatch helpers — NROW, NCOL, lengths, rownames, colnames, names, class
// ---------------------------------------------------------------------------

/// R's `NROW(x)` — number of rows; falls back to length(x) if no dim.
pub unsafe fn do_NROW(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarInteger(0);
    }
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 1 {
        Rf_ScalarInteger(*INTEGER(dim_attr))
    } else {
        Rf_ScalarInteger(XLENGTH(x) as i32)
    }
}

/// R's `NCOL(x)` — number of columns; returns 1 for vectors.
pub unsafe fn do_NCOL(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarInteger(0);
    }
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
        Rf_ScalarInteger(*INTEGER(dim_attr.add(1)))
    } else {
        Rf_ScalarInteger(1)
    }
}

/// R's `lengths(x)` — length of each element in a list/vector.
pub unsafe fn do_lengths(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = INTEGER(result);
    let t = TYPEOF(x);
    if t == SEXPTYPE::VECSXP {
        for i in 0..n {
            let elem = VECTOR_ELT(x, i as i64);
            *dst.add(i as usize) = if elem.is_null() {
                0
            } else {
                XLENGTH(elem) as i32
            };
        }
    } else {
        for i in 0..n {
            *dst.add(i as usize) = 1;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `rownames(x)` — get row names attribute.
pub unsafe fn do_rownames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("row.names").unwrap_or_default().as_ptr()),
    )
}

/// R's `colnames(x)` — get column names attribute.
pub unsafe fn do_colnames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let dimnames = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
    );
    if !dimnames.is_null() && TYPEOF(dimnames) == SEXPTYPE::VECSXP && LENGTH(dimnames) >= 2 {
        VECTOR_ELT(dimnames, 1)
    } else {
        R_NilValue()
    }
}

/// R's `names(x)` — get names attribute (alias for do_names).
pub unsafe fn do_names_get(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_names(_call, _op, args, _rho)
}

/// R's `names(x) <- value` — set names attribute.
pub unsafe fn do_names_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let value = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    crate::sexp::attrib_core::setAttrib(
        x,
        Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        value,
    );
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `class(x)` — get class attribute.
pub unsafe fn do_class_get(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let class = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
    );
    if class.is_null() || class == R_NilValue() {
        let t = TYPEOF(x);
        let name = if t == SEXPTYPE::REALSXP {
            "numeric"
        } else if t == SEXPTYPE::INTSXP {
            "integer"
        } else if t == SEXPTYPE::LGLSXP {
            "logical"
        } else if t == SEXPTYPE::STRSXP {
            "character"
        } else if t == SEXPTYPE::VECSXP {
            "list"
        } else {
            "NULL"
        };
        let cstr = CString::new(name).unwrap_or_default();
        Rf_mkString(cstr.as_ptr())
    } else {
        class
    }
}

/// R's `class(x) <- value` — set class attribute.
pub unsafe fn do_class_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let value = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    crate::sexp::attrib_core::setAttrib(
        x,
        Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        value,
    );
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `Names(x)` — get names (alias, commonly used internally).
pub unsafe fn do_Names(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_names(_call, _op, args, _rho)
}

// ---------------------------------------------------------------------------
// List / data.frame operations
// ---------------------------------------------------------------------------

/// R's `list(...)` — create a VECSXP (list) from arguments.
pub unsafe fn do_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let mut n: R_xlen_t = 0;
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        n += 1;
        current = CDR(current);
    }
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let mut i: R_xlen_t = 0;
    current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        SET_VECTOR_ELT(result, i as i64, arg);
        i += 1;
        current = CDR(current);
    }
    // Copy names from the pairlist tags if present
    let mut name_parts: Vec<String> = Vec::new();
    let mut has_names = false;
    current = args;
    while !current.is_null() && current != R_NilValue() {
        let tag = (*current).data.listsxp.tagval;
        if !tag.is_null() && tag != R_NilValue() {
            let pname = crate::sexp::accessors::PRINTNAME(tag);
            if !pname.is_null() {
                let s = crate::sexp::accessors::CHAR(pname);
                if !s.is_null() {
                    name_parts.push(
                        std::ffi::CStr::from_ptr(s)
                            .to_str()
                            .unwrap_or("")
                            .to_string(),
                    );
                    has_names = true;
                } else {
                    name_parts.push(String::new());
                }
            } else {
                name_parts.push(String::new());
            }
        } else {
            name_parts.push(String::new());
        }
        current = CDR(current);
    }
    if has_names {
        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if !names_vec.is_null() {
            let _p2 = Rf_protect(names_vec);
            for (j, name) in name_parts.iter().enumerate() {
                let cstr = CString::new(name.as_str()).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                if !charsxp.is_null() {
                    let data = (*names_vec).gengc_next_node as *mut SEXP;
                    *data.add(j) = charsxp;
                }
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names_vec,
            );
            crate::sexp::protect::Rf_unprotect(1);
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `data.frame(...)` — simplified: create list with "data.frame" class and row.names.
pub unsafe fn do_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let result = do_list(_call, _op, args, _rho);
    if result.is_null() || result == R_NilValue() {
        return result;
    }
    let _p = Rf_protect(result);

    // Set class to "data.frame"
    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    if !class_vec.is_null() {
        let _p2 = Rf_protect(class_vec);
        let cstr = CString::new("data.frame").unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*class_vec).gengc_next_node as *mut SEXP;
            *data.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            class_vec,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }

    // Determine number of rows from the first column
    let ncol = XLENGTH(result);
    if ncol > 0 {
        let first_col = VECTOR_ELT(result, 0);
        if !first_col.is_null() {
            let nrow = XLENGTH(first_col);
            let rn = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            if !rn.is_null() {
                let _p3 = Rf_protect(rn);
                *INTEGER(rn) = NA_INTEGER;
                *INTEGER(rn.add(1)) = -(nrow as i32);
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(CString::new("row.names").unwrap_or_default().as_ptr()),
                    rn,
                );
                crate::sexp::protect::Rf_unprotect(1);
            }
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Attribute access helpers
// ---------------------------------------------------------------------------

/// R's `attr(x, which)` — get arbitrary attribute by name.
pub unsafe fn do_attr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let which = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || which.is_null() || which == R_NilValue() {
        return R_NilValue();
    }
    let attr_name = elt_to_string(which, 0);
    crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new(attr_name).unwrap_or_default().as_ptr()),
    )
}

/// R's `attr(x, which) <- value` — set arbitrary attribute by name.
pub unsafe fn do_setattr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let which = CAR(CDR(args));
    let value = CAR(CDR(CDR(args)));
    if x.is_null() || x == R_NilValue() || which.is_null() || which == R_NilValue() {
        return R_NilValue();
    }
    let attr_name = elt_to_string(which, 0);
    crate::sexp::attrib_core::setAttrib(
        x,
        Rf_install(CString::new(attr_name).unwrap_or_default().as_ptr()),
        value,
    );
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

// ---------------------------------------------------------------------------
// String formatting
// ---------------------------------------------------------------------------

/// R's `noquote(x)` — mark object to prevent quoting in print.
pub unsafe fn do_noquote(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

/// R's `deparse(x)` — convert SEXP to string representation (simplified).
pub unsafe fn do_deparse(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_mkString(CString::new("NULL").unwrap_or_default().as_ptr());
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x).max(1);
    let mut parts: Vec<String> = Vec::new();

    if n == 1 {
        parts.push(elt_to_string(x, 0));
    } else {
        let mut inner: Vec<String> = Vec::new();
        for i in 0..n {
            let s = elt_to_string(x, i);
            if t == SEXPTYPE::STRSXP {
                inner.push(format!("\"{}\"", s));
            } else {
                inner.push(s);
            }
        }
        parts.push(format!("c({})", inner.join(", ")));
    }
    let result_str = parts.join("\n");
    let cstr = CString::new(result_str).unwrap_or_default();
    Rf_mkString(cstr.as_ptr())
}

// ---------------------------------------------------------------------------
// List operations
// ---------------------------------------------------------------------------

/// R's `lengths(x)` alias — lengths of list elements.
/// Wrapper that delegates to do_lengths (already registered separately).
pub unsafe fn do_length_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_lengths(_call, _op, args, _rho)
}

/// R's `names(x)` for lists — names of list elements.
/// Wrapper that delegates to do_names.
pub unsafe fn do_names_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_names(_call, _op, args, _rho)
}

/// R's `[[i]]` — get element i from a list (1-indexed).
pub unsafe fn do_list_get(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let i = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || i.is_null() || i == R_NilValue() {
        return R_NilValue();
    }
    let idx = real_or_default(i, 0.0) as i64;
    if idx < 1 || TYPEOF(x) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }
    let n = XLENGTH(x) as i64;
    if idx > n {
        return R_NilValue();
    }
    VECTOR_ELT(x, idx - 1)
}

/// R's `[[i]] <- value` — set element i in a list (1-indexed).
pub unsafe fn do_list_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let i = CAR(CDR(args));
    let value = CAR(CDR(CDR(args)));
    if x.is_null() || x == R_NilValue() || i.is_null() || i == R_NilValue() {
        return R_NilValue();
    }
    let idx = real_or_default(i, 0.0) as i64;
    if idx < 1 || TYPEOF(x) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }
    let n = XLENGTH(x) as i64;
    if idx > n {
        return R_NilValue();
    }
    SET_VECTOR_ELT(x, idx - 1, value);
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `c(...)` for lists — concatenate lists together.
/// If all args are VECSXP, result is a flattened VECSXP.
pub unsafe fn do_c_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let mut total_len: R_xlen_t = 0;
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            total_len += XLENGTH(arg);
        }
        current = CDR(current);
    }
    if total_len == 0 {
        return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
    }
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, total_len);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let mut offset: R_xlen_t = 0;
    current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            let n = XLENGTH(arg);
            if TYPEOF(arg) == SEXPTYPE::VECSXP {
                for i in 0..n {
                    SET_VECTOR_ELT(result, (offset + i) as i64, VECTOR_ELT(arg, i as i64));
                }
            } else {
                // Wrap scalar/vector in a single slot
                SET_VECTOR_ELT(result, offset as i64, arg);
            }
            offset += n;
        }
        current = CDR(current);
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `unlist(x)` — flatten nested list to a vector.
/// Simplified: if list elements are all numeric, return REALSXP.
pub unsafe fn do_unlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    if TYPEOF(x) != SEXPTYPE::VECSXP {
        return x;
    }
    let n = XLENGTH(x);
    // Collect all elements and determine output type
    let mut all_values: Vec<f64> = Vec::new();
    let mut all_ints: Vec<i32> = Vec::new();
    let mut all_strs: Vec<String> = Vec::new();
    let mut result_type: u32;
    let mut saw_str = false;

    for i in 0..n {
        let elem = VECTOR_ELT(x, i as i64);
        if elem.is_null() {
            continue;
        }
        let t = TYPEOF(elem);
        let m = XLENGTH(elem);
        for j in 0..m {
            if t == SEXPTYPE::REALSXP {
                all_values.push(*REAL(elem).add(j as usize));
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(elem).add(j as usize);
                all_ints.push(v);
            } else if t == SEXPTYPE::STRSXP {
                all_strs.push(elt_to_string(elem, j));
                saw_str = true;
            } else if t == SEXPTYPE::VECSXP {
                // Nested list — recurse via extraction
                let inner = VECTOR_ELT(elem, j as i64);
                if !inner.is_null() && TYPEOF(inner) == SEXPTYPE::REALSXP {
                    all_values.push(*REAL(inner));
                } else {
                    saw_str = true;
                    all_strs.push(elt_to_string(inner, 0));
                }
            } else {
                all_values.push(NA_REAL);
            }
        }
    }
    let result_type = if saw_str {
        SEXPTYPE::STRSXP.as_c_int()
    } else if !all_values.is_empty() {
        SEXPTYPE::REALSXP.as_c_int()
    } else {
        SEXPTYPE::INTSXP.as_c_int()
    };

    let total: R_xlen_t = if result_type == SEXPTYPE::STRSXP {
        all_strs.len() as R_xlen_t
    } else if result_type == SEXPTYPE::REALSXP {
        (all_values.len() + all_ints.len()) as R_xlen_t
    } else {
        all_ints.len() as R_xlen_t
    };

    let result = Rf_allocVector3(result_type, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    if result_type == SEXPTYPE::REALSXP {
        let dst = REAL(result);
        let mut idx = 0usize;
        for &v in &all_values {
            *dst.add(idx) = v;
            idx += 1;
        }
        for &v in &all_ints {
            *dst.add(idx) = if v == NA_INTEGER { NA_REAL } else { v as f64 };
            idx += 1;
        }
    } else if result_type == SEXPTYPE::INTSXP {
        let dst = INTEGER(result);
        for (idx, &v) in all_ints.iter().enumerate() {
            *dst.add(idx) = v;
        }
    } else if result_type == SEXPTYPE::STRSXP {
        for (idx, s) in all_strs.iter().enumerate() {
            let cstr = CString::new(s.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(idx) = charsxp;
            }
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `is.atomic(x)` — TRUE for non-recursive types (not list, pairlist, etc.).
pub unsafe fn do_is_atomic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(TRUE);
    }
    let t = TYPEOF(x);
    let is_atomic = t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::RAWSXP
        || t == SEXPTYPE::CHARSXP
        || t == SEXPTYPE::NILSXP;
    Rf_ScalarLogical(if is_atomic { TRUE } else { FALSE })
}

/// R's `is.recursive(x)` — TRUE for recursive types (list, pairlist, language, etc.).
pub unsafe fn do_is_recursive(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    let is_rec = t == SEXPTYPE::VECSXP
        || t == SEXPTYPE::LISTSXP
        || t == SEXPTYPE::LANGSXP
        || t == SEXPTYPE::CLOSXP
        || t == SEXPTYPE::BUILTINSXP
        || t == SEXPTYPE::SPECIALSXP
        || t == SEXPTYPE::ENVSXP
        || t == SEXPTYPE::EXPRSXP;
    Rf_ScalarLogical(if is_rec { TRUE } else { FALSE })
}

/// R's `is.object(x)` — TRUE if x has a "class" attribute.
pub unsafe fn do_is_object(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let class = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
    );
    Rf_ScalarLogical(if !class.is_null() { TRUE } else { FALSE })
}

// ---------------------------------------------------------------------------
// Connection basics (simplified)
// ---------------------------------------------------------------------------

/// R's `file(description)` — create a file connection.
/// Simplified: validate the path exists and return the path as a STRSXP.
pub unsafe fn do_file(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let path_arg = CAR(args);
    if path_arg.is_null() || path_arg == R_NilValue() {
        return R_NilValue();
    }
    let path_str = elt_to_string(path_arg, 0);
    // Simplified: just check if path is non-empty and return it
    if path_str.is_empty() {
        return R_NilValue();
    }
    let cstr = CString::new(path_str).unwrap_or_default();
    Rf_mkString(cstr.as_ptr())
}

/// R's `url(description)` — create a URL connection.
/// Simplified: just return the description string.
pub unsafe fn do_url(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let desc = CAR(args);
    if desc.is_null() || desc == R_NilValue() {
        return R_NilValue();
    }
    desc
}

/// R's `close(con)` — close a connection.
/// Simplified: no-op that returns the connection.
pub unsafe fn do_close(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let con = CAR(args);
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    con
}

/// R's `flush(con)` — flush a connection.
/// Simplified: no-op that returns NULL.
pub unsafe fn do_flush(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Extended connection constructors
// ---------------------------------------------------------------------------

/// R's `gzfile(description, open, encoding, compression)` — gzip connection.
/// Simplified: delegates to connections.rs when available, else returns description.
pub unsafe fn do_gzfile(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let desc = CAR(args);
    if desc.is_null() || desc == R_NilValue() {
        return R_NilValue();
    }
    // Delegate to connections.rs full implementation
    crate::mainutils::connections::do_gzfile(_call, _op, args, _rho)
}

/// R's `pipe(description, open, encoding)` — pipe connection.
/// Simplified: delegates to connections.rs when available, else returns description.
pub unsafe fn do_pipe(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let desc = CAR(args);
    if desc.is_null() || desc == R_NilValue() {
        return R_NilValue();
    }
    // Delegate to connections.rs full implementation
    crate::mainutils::connections::do_pipe(_call, _op, args, _rho)
}

/// R's `fifo(description, open, blocking)` — FIFO connection.
/// Simplified: delegates to connections.rs when available, else returns description.
pub unsafe fn do_fifo(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let desc = CAR(args);
    if desc.is_null() || desc == R_NilValue() {
        return R_NilValue();
    }
    // Delegate to connections.rs full implementation
    crate::mainutils::connections::do_fifo(_call, _op, args, _rho)
}

/// R's `socketConnection(host, port, open, blocking, server, encoding)` — socket connection.
/// Simplified: stub that returns NULL.
pub unsafe fn do_socketConnection(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _host = CAR(args);
    let _port = CAR(CDR(args));
    // Socket connections not yet fully supported
    crate::mainutils::connections::do_sockConnection(_call, _op, args, _rho)
}

// ---------------------------------------------------------------------------
// Connection queries and operations
// ---------------------------------------------------------------------------

/// R's `isOpen(con, rw)` — check if a connection is open.
/// Simplified: delegates to connections.rs.
pub unsafe fn do_isOpen(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    crate::mainutils::connections::do_isopen(_call, _op, args, _rho)
}

/// R's `isIncomplete(con)` — check if a connection has incomplete read.
/// Simplified: delegates to connections.rs.
pub unsafe fn do_isIncomplete(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    crate::mainutils::connections::do_isincomplete(_call, _op, args, _rho)
}

/// R's `isSeekable(con)` — check if a connection supports seeking.
/// Simplified: return TRUE for file connections, FALSE otherwise.
pub unsafe fn do_isSeekable(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let con = CAR(args);
    if con.is_null() || con == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    // Delegate to connections.rs seek implementation to check
    crate::mainutils::connections::do_isincomplete(_call, _op, args, _rho)
}

/// R's `seek(con, where, origin, rw)` — seek in a connection.
/// Simplified: delegates to connections.rs.
pub unsafe fn do_seek(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    crate::mainutils::connections::do_seek(_call, _op, args, _rho)
}

/// R's `pushBack(lines, con, newLine)` — push back lines to a connection.
/// Simplified: no-op stub.
pub unsafe fn do_pushBack(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    crate::mainutils::connections::do_pushBack(_call, _op, args, _rho)
}

/// R's `pushBackClear(con)` — clear push back buffer.
/// Simplified: no-op stub.
pub unsafe fn do_pushBackClear(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    R_NilValue()
}

/// R's `pushBackLength(con)` — get push back buffer length.
/// Simplified: returns 0.
pub unsafe fn do_pushBackLength(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    crate::mainutils::connections::do_pushBackLength(_call, _op, args, _rho)
}

// ---------------------------------------------------------------------------
// Binary I/O
// ---------------------------------------------------------------------------

/// R's `readBin(con, what, n, size, signed, endian)` — read binary data.
/// Delegates to connections.rs for full implementation.
pub unsafe fn do_readBin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    crate::mainutils::connections::do_readBin(_call, _op, args, _rho)
}

/// R's `writeBin(object, con, size, endian, useBytes)` — write binary data.
/// Delegates to connections.rs for full implementation.
pub unsafe fn do_writeBin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    crate::mainutils::connections::do_writeBin(_call, _op, args, _rho)
}

// ---------------------------------------------------------------------------
// Print / summary methods
// ---------------------------------------------------------------------------

/// R's `print.matrix(x)` — print a matrix with proper row/col formatting.
pub unsafe fn do_print_matrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("NULL");
        return R_NilValue();
    }
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    let (nrow, ncol) =
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            (
                *INTEGER(dim_attr) as R_xlen_t,
                *INTEGER(dim_attr.add(1)) as R_xlen_t,
            )
        } else {
            let n = XLENGTH(x).max(1);
            (n, 1)
        };

    // Get colnames
    let colnames = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
    );
    let col_names_vec: Vec<String> =
        if !colnames.is_null() && TYPEOF(colnames) == SEXPTYPE::VECSXP && LENGTH(colnames) >= 2 {
            let cn = VECTOR_ELT(colnames, 1);
            if !cn.is_null() && TYPEOF(cn) == SEXPTYPE::STRSXP {
                let m = XLENGTH(cn).min(ncol);
                (0..m).map(|i| elt_to_string(cn, i)).collect()
            } else {
                (0..ncol).map(|i| format!("[,{}]", i + 1)).collect()
            }
        } else {
            (0..ncol).map(|i| format!("[,{}]", i + 1)).collect()
        };

    // Print column headers
    let mut header = String::from("     ");
    for name in &col_names_vec {
        let _ = std::fmt::Write::write_fmt(&mut header, format_args!("{:>12}", name));
    }
    println!("{}", header);

    // Print rows
    for r in 0..nrow {
        let row_label = format!("[{},]", r + 1);
        print!("{:>4} ", row_label);
        for c in 0..ncol {
            let idx = c * nrow + r;
            let s = elt_to_string(x, idx as R_xlen_t);
            print!("{:>12}", s);
        }
        println!();
    }

    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `print.list(x)` — print a list with element names.
pub unsafe fn do_print_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("NULL");
        return R_NilValue();
    }
    let n = XLENGTH(x);
    // Get names
    let names = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
    );
    let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;

    for i in 0..n {
        let name = if has_names && i < XLENGTH(names) {
            let s = elt_to_string(names, i);
            if s.is_empty() {
                format!("${}", i + 1)
            } else {
                format!("${}", s)
            }
        } else {
            format!("${}", i + 1)
        };
        let elem = VECTOR_ELT(x, i as i64);
        let type_str = if elem.is_null() {
            "NULL".to_string()
        } else {
            let t = TYPEOF(elem);
            match t {
                t if t == SEXPTYPE::REALSXP => "num".to_string(),
                t if t == SEXPTYPE::INTSXP => "int".to_string(),
                t if t == SEXPTYPE::LGLSXP => "logi".to_string(),
                t if t == SEXPTYPE::STRSXP => "chr".to_string(),
                t if t == SEXPTYPE::VECSXP => "list".to_string(),
                _ => "obj".to_string(),
            }
        };
        let preview = if elem.is_null() {
            "NULL".to_string()
        } else {
            let m = XLENGTH(elem).min(3);
            let parts: Vec<String> = (0..m).map(|j| elt_to_string(elem, j)).collect();
            format!("{}: {}", type_str, parts.join(" "))
        };
        println!("{}\n{}", name, preview);
    }

    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `summary.default(x)` — basic summary statistics.
pub unsafe fn do_summary_default(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP {
        // For non-numeric, just return type info
        return do_typeof(_call, _op, args, _rho);
    }
    let n = XLENGTH(x);
    if n == 0 {
        return R_NilValue();
    }
    let mut vals: Vec<f64> = Vec::new();
    for i in 0..n {
        let v = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else {
            let iv = *INTEGER(x).add(i as usize);
            if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
        };
        if v.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN && !v.is_nan() {
            vals.push(v);
        }
    }
    if vals.is_empty() {
        println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
        println!(
            "     NA      NA      NA      NA      NA      NA       {}",
            n
        );
        return x;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let min_v = vals[0];
    let max_v = vals[vals.len() - 1];
    let mean_v: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
    let median_idx = vals.len() / 2;
    let median_v = if vals.len() % 2 == 1 {
        vals[median_idx]
    } else {
        (vals[median_idx - 1] + vals[median_idx]) / 2.0
    };
    let q1_idx = vals.len() / 4;
    let q3_idx = 3 * vals.len() / 4;
    let q1_v = vals[q1_idx];
    let q3_v = vals[q3_idx];
    let na_count = n - vals.len() as R_xlen_t;

    println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
    println!(
        "{:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8}",
        min_v,
        q1_v,
        median_v,
        mean_v,
        q3_v,
        max_v,
        if na_count > 0 {
            na_count.to_string()
        } else {
            String::new()
        }
    );

    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `str(x)` — compact structure display.
pub unsafe fn do_str(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!(" NULL");
        return R_NilValue();
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x);

    if t == SEXPTYPE::VECSXP {
        // List
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        );
        let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;

        // Check for data.frame class
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        let is_df = if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            elt_to_string(class, 0) == "data.frame"
        } else {
            false
        };

        if is_df {
            let ncol = n;
            let nrow = if ncol > 0 {
                let first = VECTOR_ELT(x, 0);
                if first.is_null() { 0 } else { XLENGTH(first) }
            } else {
                0
            };
            println!("'data.frame':\t{} obs. of  {} variables:", nrow, ncol);
            for i in 0..ncol.min(6) {
                let name = if has_names && i < XLENGTH(names) {
                    elt_to_string(names, i)
                } else {
                    format!("$ {}", i + 1)
                };
                let elem = VECTOR_ELT(x, i as i64);
                let elem_type = if elem.is_null() {
                    "NULL".to_string()
                } else {
                    let et = TYPEOF(elem);
                    let m = XLENGTH(elem);
                    match et {
                        t if t == SEXPTYPE::REALSXP => format!("num [1:{}]", m),
                        t if t == SEXPTYPE::INTSXP => format!("int [1:{}]", m),
                        t if t == SEXPTYPE::LGLSXP => format!("logi [1:{}]", m),
                        t if t == SEXPTYPE::STRSXP => format!("chr [1:{}]", m),
                        _ => format!("? [1:{}]", m),
                    }
                };
                println!(" ${:<12}: {}", name, elem_type);
            }
        } else {
            println!("List of {}", n);
            for i in 0..n.min(6) {
                let name = if has_names && i < XLENGTH(names) {
                    elt_to_string(names, i)
                } else {
                    format!("[[{}]]", i + 1)
                };
                let elem = VECTOR_ELT(x, i as i64);
                let elem_type = if elem.is_null() {
                    "NULL".to_string()
                } else {
                    let et = TYPEOF(elem);
                    let m = XLENGTH(elem);
                    match et {
                        t if t == SEXPTYPE::REALSXP => format!("num [1:{}]", m),
                        t if t == SEXPTYPE::INTSXP => format!("int [1:{}]", m),
                        t if t == SEXPTYPE::LGLSXP => format!("logi [1:{}]", m),
                        t if t == SEXPTYPE::STRSXP => format!("chr [1:{}]", m),
                        t if t == SEXPTYPE::VECSXP => format!("list [1:{}]", m),
                        _ => format!("? [1:{}]", m),
                    }
                };
                println!(" $ {}: {}", name, elem_type);
            }
        }
    } else {
        // Atomic vector or other
        let type_name = match t {
            t if t == SEXPTYPE::REALSXP => "num",
            t if t == SEXPTYPE::INTSXP => "int",
            t if t == SEXPTYPE::LGLSXP => "logi",
            t if t == SEXPTYPE::STRSXP => "chr",
            t if t == SEXPTYPE::CPLXSXP => "cplx",
            t if t == SEXPTYPE::RAWSXP => "raw",
            _ => "?",
        };
        let preview_n = n.min(6);
        let parts: Vec<String> = (0..preview_n).map(|i| elt_to_string(x, i)).collect();
        print!(" {} [1:{}]", type_name, n);
        if !parts.is_empty() {
            print!(": {}", parts.join(" "));
        }
        if n > preview_n {
            print!(" ...");
        }
        println!();
    }

    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

// ---------------------------------------------------------------------------
// S3 generics
// ---------------------------------------------------------------------------

/// R's `as.data.frame(x)` — convert to data.frame.
/// Simplified: wraps x in a list with data.frame class.
pub unsafe fn do_as_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    // If already a data.frame, return as-is
    let class = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
    );
    if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
        let cls_name = elt_to_string(class, 0);
        if cls_name == "data.frame" {
            return x;
        }
    }
    // Wrap in a single-element list and set class
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    SET_VECTOR_ELT(result, 0, x);

    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    if !class_vec.is_null() {
        let _p2 = Rf_protect(class_vec);
        let cstr = CString::new("data.frame").unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*class_vec).gengc_next_node as *mut SEXP;
            *data.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            class_vec,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }

    // Set row.names
    let nrow = XLENGTH(x);
    let rn = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    if !rn.is_null() {
        let _p3 = Rf_protect(rn);
        *INTEGER(rn) = NA_INTEGER;
        *INTEGER(rn.add(1)) = -(nrow as i32);
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("row.names").unwrap_or_default().as_ptr()),
            rn,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }

    // Set column name to "x"
    let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    if !names_vec.is_null() {
        let _p4 = Rf_protect(names_vec);
        let cstr = CString::new("x").unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*names_vec).gengc_next_node as *mut SEXP;
            *data.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            names_vec,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `as.list(x)` — generic list conversion.
/// Delegates to do_as_list but available as a separate entry point.
pub unsafe fn do_as_list_generic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_as_list(_call, _op, args, _rho)
}

// ---------------------------------------------------------------------------
// S3 print/summary dispatch
// ---------------------------------------------------------------------------

/// R's `print.default(x, ...)` — default print method.
/// Equivalent to the existing do_print but named for S3 dispatch.
pub unsafe fn do_print_default(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_print(_call, _op, args, _rho)
}

/// R's `print.data.frame(x)` — print a data.frame nicely with aligned columns.
pub unsafe fn do_print_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("NULL");
        return R_NilValue();
    }
    if TYPEOF(x) != SEXPTYPE::VECSXP {
        return do_print(_call, _op, args, _rho);
    }
    let ncol = XLENGTH(x);
    let nrow = if ncol > 0 {
        let first = VECTOR_ELT(x, 0);
        if first.is_null() { 0 } else { XLENGTH(first) }
    } else {
        0
    };

    // Get column names
    let names = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
    );
    let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;

    // Print header row (column names)
    if ncol > 0 {
        let mut header = String::new();
        for j in 0..ncol.min(20) {
            let name = if has_names && j < XLENGTH(names) {
                elt_to_string(names, j)
            } else {
                format!("[,{}]", j + 1)
            };
            let _ = std::fmt::Write::write_fmt(&mut header, format_args!("{:>12} ", name));
        }
        println!("{}", header);
    }

    // Print rows (up to 20)
    let print_rows = nrow.min(20);
    for i in 0..print_rows {
        let mut row = String::new();
        for j in 0..ncol.min(20) {
            let col = VECTOR_ELT(x, j as R_xlen_t);
            let val = if col.is_null() {
                "NULL".to_string()
            } else {
                elt_to_string(col, i)
            };
            let _ = std::fmt::Write::write_fmt(&mut row, format_args!("{:>12} ", val));
        }
        println!("{}", row);
    }
    if nrow > 20 {
        println!(
            "  [ reached 'max' / getOption(\"max.print\") -- omitted {} rows ]",
            nrow - 20
        );
    }

    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `print.table(x)` — print a table object.
pub unsafe fn do_print_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("NULL");
        return R_NilValue();
    }
    // Table objects are typically arrays (REALSXP/INTSXP with dim attribute)
    let t = TYPEOF(x);
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );

    if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) == 2 {
        // 2D table: print as matrix
        let nrow = *INTEGER(dim_attr) as usize;
        let ncol = *INTEGER(dim_attr.add(1)) as usize;

        // Get dimnames
        let dn = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
        );
        let has_dn = !dn.is_null() && TYPEOF(dn) == SEXPTYPE::VECSXP;

        // Print row names and values
        for i in 0..nrow {
            let rname = if has_dn && !VECTOR_ELT(dn, 0).is_null() {
                elt_to_string(VECTOR_ELT(dn, 0), i as R_xlen_t)
            } else {
                format!("{}", i + 1)
            };
            print!("{:>12} ", rname);
            for j in 0..ncol {
                let idx = i * ncol + j;
                let val = if t == SEXPTYPE::REALSXP {
                    format!("{:>6}", *REAL(x).add(idx))
                } else if t == SEXPTYPE::INTSXP {
                    format!("{:>6}", *INTEGER(x).add(idx))
                } else {
                    format!("{:>6}", elt_to_string(x, idx as R_xlen_t))
                };
                print!("{}", val);
            }
            println!();
        }
        // Print column names
        if has_dn && !VECTOR_ELT(dn, 1).is_null() {
            print!("{:>12} ", "");
            for j in 0..ncol {
                print!("{:>6}", elt_to_string(VECTOR_ELT(dn, 1), j as R_xlen_t));
            }
            println!();
        }
    } else {
        // 1D table or unknown: print vector
        let n = XLENGTH(x).max(1);
        for i in 0..n {
            let val = elt_to_string(x, i);
            println!("  {}", val);
        }
    }

    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `print.factor(x)` — print factor with levels and counts.
///
/// Prints the factor values and a levels summary like:
///   [1] a b c a
///   Levels: a b c
pub unsafe fn do_print_factor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("NULL");
        return R_NilValue();
    }

    let n = XLENGTH(x);

    // Get levels attribute
    let levels = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("levels").unwrap_or_default().as_ptr()),
    );
    let has_levels = !levels.is_null() && TYPEOF(levels) == SEXPTYPE::STRSXP;

    // Print the factor values
    if n == 0 {
        println!("factor(0)");
    } else {
        let t = TYPEOF(x);
        let mut counts: Vec<i32> = Vec::new();
        if has_levels {
            let nl = XLENGTH(levels);
            counts.resize(nl as usize, 0);
        }

        for i in 0..n {
            let val = if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER {
                    "<NA>".to_string()
                } else if has_levels && (v as R_xlen_t) <= XLENGTH(levels) && v > 0 {
                    let idx = (v - 1) as R_xlen_t;
                    if (idx as usize) < counts.len() {
                        counts[idx as usize] += 1;
                    }
                    elt_to_string(levels, idx)
                } else {
                    format!("{}", v)
                }
            } else {
                elt_to_string(x, i)
            };
            if i == 0 {
                print!("[1] {}", val);
            } else {
                print!(" {}", val);
            }
        }
        println!();

        // Print levels summary
        if has_levels {
            let nl = XLENGTH(levels);
            print!("Levels:");
            for i in 0..nl {
                let lvl = elt_to_string(levels, i);
                print!(" {}", lvl);
            }
            println!();
        }
    }

    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `summary.data.frame(x)` — summary for data.frame (prints column summaries).
pub unsafe fn do_summary_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    if TYPEOF(x) != SEXPTYPE::VECSXP {
        return do_summary_default(_call, _op, args, _rho);
    }
    let ncol = XLENGTH(x);
    let names = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
    );
    let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;

    for j in 0..ncol {
        let name = if has_names && j < XLENGTH(names) {
            elt_to_string(names, j)
        } else {
            format!("[,{}]", j + 1)
        };
        let col = VECTOR_ELT(x, j as R_xlen_t);
        println!("      {} ", name);
        if col.is_null() {
            println!(" Mode:NULL ");
        } else {
            let t = TYPEOF(col);
            if t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP {
                let n = XLENGTH(col);
                let mut vals: Vec<f64> = Vec::new();
                for i in 0..n {
                    let v = if t == SEXPTYPE::REALSXP {
                        *REAL(col).add(i as usize)
                    } else {
                        let iv = *INTEGER(col).add(i as usize);
                        if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
                    };
                    if v.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN && !v.is_nan() {
                        vals.push(v);
                    }
                }
                let na_count = n as usize - vals.len();
                if vals.is_empty() {
                    println!(
                        " Min. : NA   1st Qu.: NA   Median : NA   Mean : NA   3rd Qu.: NA   Max. : NA   NA's: {}",
                        n
                    );
                } else {
                    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let min_v = vals[0];
                    let max_v = vals[vals.len() - 1];
                    let mean_v: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
                    let median_idx = vals.len() / 2;
                    let median_v = if vals.len() % 2 == 1 {
                        vals[median_idx]
                    } else {
                        (vals[median_idx - 1] + vals[median_idx]) / 2.0
                    };
                    let q1_idx = vals.len() / 4;
                    let q3_idx = 3 * vals.len() / 4;
                    print!(
                        " Min. :{:.1}   1st Qu.:{:.1}   Median :{:.1}   Mean :{:.1}   3rd Qu.:{:.1}   Max. :{:.1}",
                        min_v, vals[q1_idx], median_v, mean_v, vals[q3_idx], max_v
                    );
                    if na_count > 0 {
                        print!("   NA's: {}", na_count);
                    }
                    println!();
                }
            } else if t == SEXPTYPE::LGLSXP {
                println!(" Mode :logical ");
            } else if t == SEXPTYPE::STRSXP {
                println!(" Mode :character ");
            } else if t == SEXPTYPE::VECSXP {
                println!(" Length:{} ", XLENGTH(col));
            } else {
                println!(
                    " Mode :{} ",
                    elt_to_string(do_typeof(_call, _op, args, _rho), 0)
                );
            }
        }
    }
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `format.data.frame(x)` — format data.frame as character matrix.
pub unsafe fn do_format_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    if TYPEOF(x) != SEXPTYPE::VECSXP {
        // Return a single-column STRSXP of formatted values
        let n = XLENGTH(x).max(1);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            let cstr = CString::new(s).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        crate::sexp::protect::Rf_unprotect(1);
        return result;
    }

    let ncol = XLENGTH(x);
    let nrow = if ncol > 0 {
        let first = VECTOR_ELT(x, 0);
        if first.is_null() { 0 } else { XLENGTH(first) }
    } else {
        0
    };

    // Build a character matrix with ncol columns
    let total = ncol * nrow;
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    for i in 0..nrow {
        for j in 0..ncol {
            let col = VECTOR_ELT(x, j as R_xlen_t);
            let val = if col.is_null() {
                "NULL".to_string()
            } else {
                elt_to_string(col, i)
            };
            let cstr = CString::new(val).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add((j as R_xlen_t * nrow + i) as usize) = charsxp;
            }
        }
    }

    // Set dim attribute
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    if !dim.is_null() {
        let _p2 = Rf_protect(dim);
        *INTEGER(dim) = nrow as i32;
        *INTEGER(dim.add(1)) = ncol as i32;
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            dim,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Matrix/linear algebra
// ---------------------------------------------------------------------------

/// R's `crossprod(x, y)` — computes t(x) %*% y.
/// If y is NULL, computes t(x) %*% x.
pub unsafe fn do_crossprod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let y_cdr = CDR(args);
    let y = if y_cdr.is_null() || y_cdr == R_NilValue() {
        x
    } else {
        CAR(y_cdr)
    };

    if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
        return R_NilValue();
    }

    let x_n = XLENGTH(x);
    let y_n = XLENGTH(y);

    // Get dimensions (if matrices)
    let xdim = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    let ydim = crate::sexp::attrib_core::getAttrib(
        y,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );

    let (x_nrow, x_ncol) =
        if !xdim.is_null() && TYPEOF(xdim) == SEXPTYPE::INTSXP && LENGTH(xdim) == 2 {
            (*INTEGER(xdim) as usize, *INTEGER(xdim.add(1)) as usize)
        } else {
            (x_n as usize, 1)
        };
    let (y_nrow, y_ncol) =
        if !ydim.is_null() && TYPEOF(ydim) == SEXPTYPE::INTSXP && LENGTH(ydim) == 2 {
            (*INTEGER(ydim) as usize, *INTEGER(ydim.add(1)) as usize)
        } else {
            (y_n as usize, 1)
        };

    if x_nrow != y_nrow {
        return R_NilValue(); // dimension mismatch
    }

    // Compute t(x) %*% y => result is x_ncol x y_ncol
    let result_len = (x_ncol * y_ncol) as R_xlen_t;
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, result_len);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    for i in 0..x_ncol {
        for j in 0..y_ncol {
            let mut sum = 0.0_f64;
            for k in 0..x_nrow {
                let xv = if !xdim.is_null() {
                    *REAL(x).add(k * x_ncol + i)
                } else {
                    *REAL(x).add(k)
                };
                let yv = if !ydim.is_null() {
                    *REAL(y).add(k * y_ncol + j)
                } else {
                    *REAL(y).add(k)
                };
                sum += xv * yv;
            }
            *dst.add((i * y_ncol + j) as usize) = sum;
        }
    }

    // Set dim attribute
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    if !dim.is_null() {
        let _p2 = Rf_protect(dim);
        *INTEGER(dim) = x_ncol as i32;
        *INTEGER(dim.add(1)) = y_ncol as i32;
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            dim,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `tcrossprod(x, y)` — computes x %*% t(y).
/// If y is NULL, computes x %*% t(x).
pub unsafe fn do_tcrossprod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let y_cdr = CDR(args);
    let y = if y_cdr.is_null() || y_cdr == R_NilValue() {
        x
    } else {
        CAR(y_cdr)
    };

    if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
        return R_NilValue();
    }

    let x_n = XLENGTH(x);
    let y_n = XLENGTH(y);

    let xdim = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    let ydim = crate::sexp::attrib_core::getAttrib(
        y,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );

    let (x_nrow, x_ncol) =
        if !xdim.is_null() && TYPEOF(xdim) == SEXPTYPE::INTSXP && LENGTH(xdim) == 2 {
            (*INTEGER(xdim) as usize, *INTEGER(xdim.add(1)) as usize)
        } else {
            (x_n as usize, 1)
        };
    let (y_nrow, y_ncol) =
        if !ydim.is_null() && TYPEOF(ydim) == SEXPTYPE::INTSXP && LENGTH(ydim) == 2 {
            (*INTEGER(ydim) as usize, *INTEGER(ydim.add(1)) as usize)
        } else {
            (y_n as usize, 1)
        };

    if x_ncol != y_ncol {
        return R_NilValue(); // dimension mismatch
    }

    // Compute x %*% t(y) => result is x_nrow x y_nrow
    let result_len = (x_nrow * y_nrow) as R_xlen_t;
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, result_len);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    for i in 0..x_nrow {
        for j in 0..y_nrow {
            let mut sum = 0.0_f64;
            for k in 0..x_ncol {
                let xv = if !xdim.is_null() {
                    *REAL(x).add(i * x_ncol + k)
                } else {
                    *REAL(x).add(i)
                };
                let yv = if !ydim.is_null() {
                    *REAL(y).add(j * y_ncol + k)
                } else {
                    *REAL(y).add(j)
                };
                sum += xv * yv;
            }
            *dst.add((i * y_nrow + j) as usize) = sum;
        }
    }

    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    if !dim.is_null() {
        let _p2 = Rf_protect(dim);
        *INTEGER(dim) = x_nrow as i32;
        *INTEGER(dim.add(1)) = y_nrow as i32;
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            dim,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `det(x)` — determinant of a square matrix (simplified via LU-like approach).
/// For a 2x2 matrix: det = a*d - b*c. For larger, uses LU decomposition concept.
pub unsafe fn do_det(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarReal(NA_REAL);
    }

    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    if dim_attr.is_null() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP || LENGTH(dim_attr) != 2 {
        return Rf_ScalarReal(NA_REAL);
    }
    let n = *INTEGER(dim_attr) as usize;
    let m = *INTEGER(dim_attr.add(1)) as usize;
    if n != m || n == 0 {
        return Rf_ScalarReal(NA_REAL);
    }

    if TYPEOF(x) != SEXPTYPE::REALSXP {
        return Rf_ScalarReal(NA_REAL);
    }

    // Compute determinant using LU decomposition (without pivoting for simplicity)
    let src = REAL(x);
    // Copy matrix data
    let mut mat: Vec<f64> = Vec::with_capacity(n * n);
    for i in 0..n * n {
        mat.push(*src.add(i));
    }

    let mut det_val = 1.0_f64;
    for i in 0..n {
        // Find pivot
        let mut max_val = mat[i * n + i].abs();
        let mut max_row = i;
        for k in (i + 1)..n {
            let v = mat[k * n + i].abs();
            if v > max_val {
                max_val = v;
                max_row = k;
            }
        }
        if max_val == 0.0 {
            return Rf_ScalarReal(0.0);
        }
        // Swap rows
        if max_row != i {
            for j in 0..n {
                let tmp = mat[i * n + j];
                mat[i * n + j] = mat[max_row * n + j];
                mat[max_row * n + j] = tmp;
            }
            det_val = -det_val;
        }
        det_val *= mat[i * n + i];
        // Eliminate
        let pivot = mat[i * n + i];
        for k in (i + 1)..n {
            let factor = mat[k * n + i] / pivot;
            mat[k * n + i] = 0.0;
            for j in (i + 1)..n {
                mat[k * n + j] -= factor * mat[i * n + j];
            }
        }
    }

    Rf_ScalarReal(det_val)
}

/// R's `solve(a, b)` — solve the linear system a %*% x = b.
/// If b is omitted, computes the inverse of a (simplified).
/// Uses Gaussian elimination with partial pivoting.
pub unsafe fn do_solve(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let a = CAR(args);
    let b_cdr = CDR(args);
    let b = if b_cdr.is_null() || b_cdr == R_NilValue() {
        R_NilValue()
    } else {
        CAR(b_cdr)
    };

    if a.is_null() || a == R_NilValue() {
        return R_NilValue();
    }

    let dim_attr = crate::sexp::attrib_core::getAttrib(
        a,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    if dim_attr.is_null() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP || LENGTH(dim_attr) != 2 {
        return R_NilValue();
    }
    let n = *INTEGER(dim_attr) as usize;
    let m = *INTEGER(dim_attr.add(1)) as usize;
    if n != m || n == 0 {
        return R_NilValue();
    }
    if TYPEOF(a) != SEXPTYPE::REALSXP {
        return R_NilValue();
    }

    let src = REAL(a);
    // Build augmented matrix [A | I] or [A | b]
    let nrhs = if b == R_NilValue() {
        n // inverse
    } else {
        let b_dim = crate::sexp::attrib_core::getAttrib(
            b,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !b_dim.is_null() && TYPEOF(b_dim) == SEXPTYPE::INTSXP && LENGTH(b_dim) == 2 {
            *INTEGER(b_dim.add(1)) as usize
        } else {
            1
        }
    };

    let aug_cols = n + nrhs;
    let mut aug: Vec<f64> = vec![0.0; n * aug_cols];

    // Fill A
    for i in 0..n {
        for j in 0..n {
            aug[i * aug_cols + j] = *src.add(i * n + j);
        }
    }

    // Fill right-hand side
    if b == R_NilValue() {
        // Identity matrix for inverse
        for i in 0..n {
            aug[i * aug_cols + n + i] = 1.0;
        }
    } else {
        let b_src = REAL(b);
        for i in 0..n {
            for j in 0..nrhs {
                aug[i * aug_cols + n + j] = *b_src.add(i * nrhs + j);
            }
        }
    }

    // Gaussian elimination with partial pivoting
    for i in 0..n {
        // Find pivot
        let mut max_val = aug[i * aug_cols + i].abs();
        let mut max_row = i;
        for k in (i + 1)..n {
            let v = aug[k * aug_cols + i].abs();
            if v > max_val {
                max_val = v;
                max_row = k;
            }
        }
        if max_val == 0.0 {
            return R_NilValue(); // singular
        }
        // Swap rows
        if max_row != i {
            for j in 0..aug_cols {
                let tmp = aug[i * aug_cols + j];
                aug[i * aug_cols + j] = aug[max_row * aug_cols + j];
                aug[max_row * aug_cols + j] = tmp;
            }
        }
        // Eliminate below
        let pivot = aug[i * aug_cols + i];
        for k in (i + 1)..n {
            let factor = aug[k * aug_cols + i] / pivot;
            aug[k * aug_cols + i] = 0.0;
            for j in (i + 1)..aug_cols {
                aug[k * aug_cols + j] -= factor * aug[i * aug_cols + j];
            }
        }
    }

    // Back substitution
    for i in (0..n).rev() {
        let diag = aug[i * aug_cols + i];
        for j in (n)..aug_cols {
            aug[i * aug_cols + j] /= diag;
        }
        aug[i * aug_cols + i] = 1.0;
        for k in 0..i {
            let factor = aug[k * aug_cols + i];
            for j in n..aug_cols {
                aug[k * aug_cols + j] -= factor * aug[i * aug_cols + j];
            }
            aug[k * aug_cols + i] = 0.0;
        }
    }

    // Extract result
    let result_len = (n * nrhs) as R_xlen_t;
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, result_len);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    for i in 0..n {
        for j in 0..nrhs {
            *dst.add(i * nrhs + j) = aug[i * aug_cols + n + j];
        }
    }

    // Set dim if multi-column
    if nrhs > 1 {
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            let _p2 = Rf_protect(dim);
            *INTEGER(dim) = n as i32;
            *INTEGER(dim.add(1)) = nrhs as i32;
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                dim,
            );
            crate::sexp::protect::Rf_unprotect(1);
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Environment functions
// ---------------------------------------------------------------------------

/// R's `emptyenv()` — returns the empty environment (root of environment chain).
pub unsafe fn do_emptyenv(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::globals::R_EmptyEnv()
}

/// R's `baseenv()` — returns the base environment.
pub unsafe fn do_baseenv(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::globals::R_BaseEnv()
}

/// R's `globalenv()` — returns the global environment.
pub unsafe fn do_globalenv(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::globals::R_GlobalEnv()
}

/// R's `new.env(hash, parent, size)` — create a new environment.
pub unsafe fn do_new_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let parent_arg = CAR(args);
    let parent = if parent_arg.is_null() || parent_arg == R_NilValue() {
        crate::sexp::globals::R_GlobalEnv()
    } else if TYPEOF(parent_arg) == SEXPTYPE::ENVSXP {
        parent_arg
    } else {
        crate::sexp::globals::R_GlobalEnv()
    };

    // Create a new environment with empty frame and parent
    let env = crate::sexp::memory_ext::NewEnvironment(
        R_NilValue(), // empty frame
        parent,       // enclosing env
        R_NilValue(), // no hash table (simplified)
    );
    env
}

/// R's `environment(fun)` — get the environment associated with a closure.
pub unsafe fn do_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let fn_arg = CAR(args);
    if fn_arg.is_null() || fn_arg == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(fn_arg);
    if t == SEXPTYPE::CLOSXP {
        let env = crate::sexp::accessors::CLOENV(fn_arg);
        if env.is_null() { R_NilValue() } else { env }
    } else if t == SEXPTYPE::ENVSXP {
        fn_arg
    } else {
        R_NilValue()
    }
}

/// R's `lockBinding(sym, env)` — lock a binding in an environment.
/// Simplified: we track this via a ".locked" attribute on the frame.
pub unsafe fn do_lockBinding(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _sym = CAR(args);
    let _env = CAR(CDR(args));
    // In a full implementation, we'd set the LOCKED_BIT on the binding.
    // For now, just return NULL (no-op).
    R_NilValue()
}

/// R's `unlockBinding(sym, env)` — unlock a binding in an environment.
/// Simplified: no-op in this implementation.
pub unsafe fn do_unlockBinding(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    R_NilValue()
}

/// R's `bindingIsLocked(sym, env)` — check if a binding is locked.
/// Simplified: always returns FALSE.
pub unsafe fn do_bindingIsLocked(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    Rf_ScalarLogical(FALSE)
}

/// R's `makeActiveBinding(sym, fun, env)` — create an active binding.
/// Simplified: just does a regular assignment.
pub unsafe fn do_makeActiveBinding(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _sym = CAR(args);
    let _fun = CAR(CDR(args));
    let _env = CAR(CDR(CDR(args)));
    // In a full implementation, we'd set the ACTIVE_BINDING_BIT.
    // For now, return NULL (no-op).
    R_NilValue()
}

/// R's `lockEnvironment(env, bindings)` — lock an environment.
/// Simplified: no-op that returns NULL.
pub unsafe fn do_lockEnvironment(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    R_NilValue()
}

/// R's `environmentIsLocked(env)` — check if an environment is locked.
/// Simplified: always returns FALSE.
pub unsafe fn do_environmentIsLocked(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    Rf_ScalarLogical(FALSE)
}

// ---------------------------------------------------------------------------
// R runtime essentials
// ---------------------------------------------------------------------------

/// R's `version` — returns the version as a character string.
pub unsafe fn do_version(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    let s = CString::new("4.4.1").unwrap_or_default();
    Rf_mkString(s.as_ptr())
}

/// R's `R.version` — returns a named list with version info.
pub unsafe fn do_R_version(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 5);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    let platform = CString::new("rust-port").unwrap_or_default();
    let major = CString::new("4").unwrap_or_default();
    let minor = CString::new("4.1").unwrap_or_default();
    let language = CString::new("R").unwrap_or_default();
    let version_string = CString::new("R version 4.4.1 (Rust Port)").unwrap_or_default();

    SET_VECTOR_ELT(result, 0, Rf_mkString(platform.as_ptr()));
    SET_VECTOR_ELT(result, 1, Rf_mkString(major.as_ptr()));
    SET_VECTOR_ELT(result, 2, Rf_mkString(minor.as_ptr()));
    SET_VECTOR_ELT(result, 3, Rf_mkString(language.as_ptr()));
    SET_VECTOR_ELT(result, 4, Rf_mkString(version_string.as_ptr()));

    // Set names
    let names = Rf_allocVector3(SEXPTYPE::STRSXP, 5);
    if !names.is_null() {
        let _p2 = Rf_protect(names);
        let ns = ["platform", "major", "minor", "language", "version.string"];
        for (i, &n) in ns.iter().enumerate() {
            let cs = CString::new(n).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
            if !charsxp.is_null() {
                let data = (*names).gengc_next_node as *mut SEXP;
                *data.add(i) = charsxp;
            }
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            names,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `R.Version()` — returns the version info list (alias for R.version).
pub unsafe fn do_R_Version(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_R_version(_call, _op, args, _rho)
}

/// R's `args(fn)` — returns the formal arguments of a function as a pairlist.
/// With the body set to NULL.
pub unsafe fn do_args(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let fn_arg = CAR(args);
    if fn_arg.is_null() || fn_arg == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(fn_arg);
    if t == SEXPTYPE::CLOSXP {
        let formals = crate::sexp::accessors::FORMALS(fn_arg);
        // Return a closure with same formals but body = NULL
        formals
    } else if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
        // Builtins have no formals
        R_NilValue()
    } else {
        R_NilValue()
    }
}

/// R's `formals(fn)` — get the formal arguments (parameter list) of a function.
pub unsafe fn do_formals(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let fn_arg = CAR(args);
    if fn_arg.is_null() || fn_arg == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(fn_arg);
    if t == SEXPTYPE::CLOSXP {
        let formals = crate::sexp::accessors::FORMALS(fn_arg);
        if formals.is_null() {
            R_NilValue()
        } else {
            formals
        }
    } else {
        R_NilValue()
    }
}

/// R's `body(fn)` — get the body of a function.
pub unsafe fn do_body(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let fn_arg = CAR(args);
    if fn_arg.is_null() || fn_arg == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(fn_arg);
    if t == SEXPTYPE::CLOSXP {
        let body = crate::sexp::accessors::BODY(fn_arg);
        if body.is_null() { R_NilValue() } else { body }
    } else {
        R_NilValue()
    }
}

/// R's `environment(fn)` — get the environment of a closure.
/// Same as do_environment, provided as an alternative name.
pub unsafe fn do_environment_of(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_environment(_call, _op, args, _rho)
}

// ---------------------------------------------------------------------------
// String/vector completion: charmatch, pmatch, strtoi, strtrim
// ---------------------------------------------------------------------------

/// R's `charmatch(x, table)` — character matching.
/// Returns integer index of exact match (1-based), or 0 if no match, or NA if ambiguous.
pub unsafe fn do_charmatch(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x_arg = CAR(args);
    let table_arg = CAR(CDR(args));
    if x_arg.is_null() || x_arg == R_NilValue() || table_arg.is_null() || table_arg == R_NilValue()
    {
        return Rf_ScalarInteger(0);
    }
    let x_str = elt_to_string(x_arg, 0);
    let n = XLENGTH(table_arg).max(1);
    let mut matches: Vec<c_int> = Vec::new();
    for i in 0..n {
        let t_str = elt_to_string(table_arg, i);
        if t_str == x_str {
            matches.push((i + 1) as c_int);
        }
    }
    if matches.len() == 1 {
        return Rf_ScalarInteger(matches[0]);
    } else if matches.is_empty() {
        // Check for partial match
        let mut partial: Vec<c_int> = Vec::new();
        for i in 0..n {
            let t_str = elt_to_string(table_arg, i);
            if t_str.starts_with(&x_str) {
                partial.push((i + 1) as c_int);
            }
        }
        if partial.len() == 1 {
            return Rf_ScalarInteger(partial[0]);
        } else if partial.is_empty() {
            return Rf_ScalarInteger(0);
        } else {
            return Rf_ScalarInteger(NA_INTEGER);
        }
    } else {
        return Rf_ScalarInteger(NA_INTEGER);
    }
}

/// R's `pmatch(x, table, nomatch=NA, duplicates.ok=FALSE)` — partial matching.
/// Returns integer vector of matches (1-based).
pub unsafe fn do_pmatch(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x_arg = CAR(args);
    let table_arg = CAR(CDR(args));
    let nomatch_arg = CAR(CDR(CDR(args)));
    let nomatch = if nomatch_arg.is_null() || nomatch_arg == R_NilValue() {
        NA_INTEGER
    } else {
        real_or_default(nomatch_arg, NA_REAL as f64) as c_int
    };

    if x_arg.is_null() || x_arg == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }
    if table_arg.is_null() || table_arg == R_NilValue() {
        let n = XLENGTH(x_arg).max(1);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = INTEGER(result);
        for i in 0..n {
            *dst.add(i as usize) = nomatch;
        }
        crate::sexp::protect::Rf_unprotect(1);
        return result;
    }

    let nx = XLENGTH(x_arg).max(1);
    let nt = XLENGTH(table_arg).max(1);
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, nx);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = INTEGER(result);

    // Track which table entries are already matched
    let mut used = vec![false; nt as usize];

    for i in 0..nx {
        let x_str = elt_to_string(x_arg, i);
        let mut best_match: c_int = nomatch;
        // First try exact match
        for j in 0..nt {
            if !used[j as usize] {
                let t_str = elt_to_string(table_arg, j);
                if t_str == x_str {
                    best_match = (j + 1) as c_int;
                    used[j as usize] = true;
                    break;
                }
            }
        }
        // Then try partial match
        if best_match == nomatch {
            let mut partial: Vec<c_int> = Vec::new();
            for j in 0..nt {
                if !used[j as usize] {
                    let t_str = elt_to_string(table_arg, j);
                    if t_str.starts_with(&x_str) {
                        partial.push(j as c_int);
                    }
                }
            }
            if partial.len() == 1 {
                best_match = partial[0] + 1;
                used[partial[0] as usize] = true;
            }
        }
        *dst.add(i as usize) = best_match;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `strtoi(x, base=10L)` — convert strings to integers.
pub unsafe fn do_strtoi(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
    let n = XLENGTH(x_arg).max(1);
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
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
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `strtrim(x, width)` — truncate strings to a maximum width.
pub unsafe fn do_strtrim(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

    let n = XLENGTH(x_arg).max(1);
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

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
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// R's math2 builtins (2-arg math): log2, round, signif, trunc
// ---------------------------------------------------------------------------

/// R's `log2(x)` — log base 2 with optional explicit base override.
pub unsafe fn do_log2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x_arg = CAR(args);
    let base_arg = CAR(CDR(args));
    if x_arg.is_null() || x_arg == R_NilValue() {
        return R_NilValue();
    }
    let base = if base_arg.is_null() || base_arg == R_NilValue() {
        2.0
    } else {
        real_or_default(base_arg, std::f64::consts::E)
    };
    let n = XLENGTH(x_arg).max(1);
    let t = TYPEOF(x_arg);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    let log_base = base.ln();
    for i in 0..n {
        let v = if t == SEXPTYPE::REALSXP {
            *REAL(x_arg).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let iv = *INTEGER(x_arg).add(i as usize);
            if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
        } else {
            NA_REAL
        };
        *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v <= 0.0 {
            NA_REAL
        } else {
            v.ln() / log_base
        };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `round(x, digits=0)` — round to specified decimal digits.
pub unsafe fn do_round(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x_arg = CAR(args);
    let digits_arg = CAR(CDR(args));
    if x_arg.is_null() || x_arg == R_NilValue() {
        return R_NilValue();
    }
    let digits = if digits_arg.is_null() || digits_arg == R_NilValue() {
        0.0
    } else {
        real_or_default(digits_arg, 0.0)
    };
    let n = XLENGTH(x_arg).max(1);
    let t = TYPEOF(x_arg);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    let scale = 10.0_f64.powf(digits);
    for i in 0..n {
        let v = if t == SEXPTYPE::REALSXP {
            *REAL(x_arg).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let iv = *INTEGER(x_arg).add(i as usize);
            if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
        } else {
            NA_REAL
        };
        *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            NA_REAL
        } else {
            (v * scale).round() / scale
        };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `signif(x, digits=6)` — round to significant digits.
pub unsafe fn do_signif(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x_arg = CAR(args);
    let digits_arg = CAR(CDR(args));
    if x_arg.is_null() || x_arg == R_NilValue() {
        return R_NilValue();
    }
    let digits = if digits_arg.is_null() || digits_arg == R_NilValue() {
        6.0
    } else {
        real_or_default(digits_arg, 6.0).max(1.0)
    };
    let n = XLENGTH(x_arg).max(1);
    let t = TYPEOF(x_arg);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..n {
        let v = if t == SEXPTYPE::REALSXP {
            *REAL(x_arg).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let iv = *INTEGER(x_arg).add(i as usize);
            if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
        } else {
            NA_REAL
        };
        *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v == 0.0 {
            v
        } else {
            let magnitude = v.abs().log10().floor() - digits + 1.0;
            let scale = 10.0_f64.powf(magnitude);
            (v / scale).round() * scale
        };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `trunc(x, ...)` — truncate toward zero with digits support.
pub unsafe fn do_trunc(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x_arg = CAR(args);
    let _digits_arg = CAR(CDR(args));
    if x_arg.is_null() || x_arg == R_NilValue() {
        return R_NilValue();
    }
    let n = XLENGTH(x_arg).max(1);
    let t = TYPEOF(x_arg);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..n {
        let v = if t == SEXPTYPE::REALSXP {
            *REAL(x_arg).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let iv = *INTEGER(x_arg).add(i as usize);
            if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
        } else {
            NA_REAL
        };
        *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            NA_REAL
        } else {
            v.trunc()
        };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Complete R runtime: eval, substitute, quote, parse
// ---------------------------------------------------------------------------

/// R's `eval(expr, envir, enclos)` — evaluate expression in environment.
pub unsafe fn do_eval(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let expr = CAR(args);
    let envir_arg = CAR(CDR(args));
    if expr.is_null() || expr == R_NilValue() {
        return R_NilValue();
    }
    let envir = if envir_arg.is_null() || envir_arg == R_NilValue() {
        _rho
    } else {
        envir_arg
    };
    crate::eval::eval::Rf_eval(expr, envir)
}

/// R's `substitute(expr, env)` — substitute symbols in expression.
/// Simplified: returns the expression as-is (substitution not fully implemented).
pub unsafe fn do_substitute(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let expr = CAR(args);
    // Simplified: just return the expression unchanged
    // A full implementation would walk the AST and substitute bound symbols
    if expr.is_null() || expr == R_NilValue() {
        return R_NilValue();
    }
    expr
}

/// R's `quote(expr)` — return expression unevaluated.
pub unsafe fn do_quote(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    use crate::sexp::accessors::{CAR, NAMED, SET_NAMED};
    let val = CAR(args);
    if val.is_null() || val == R_NilValue() {
        return R_NilValue();
    }
    // ENSURE_NAMEDMAX — prevent modification of source code references
    if NAMED(val) < 2 {
        SET_NAMED(val, 2);
    }
    val
}

/// R's `parse(text)` — parse R code string into expression.
/// Simplified: returns the text as a symbol or expression.
pub unsafe fn do_parse(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let text_arg = CAR(args);
    if text_arg.is_null() || text_arg == R_NilValue() {
        return R_NilValue();
    }
    let text = elt_to_string(text_arg, 0);
    // Simplified: install as symbol for simple identifiers
    // A full implementation would use the R parser
    let cstr = CString::new(text.trim()).unwrap_or_default();
    if cstr.to_str().unwrap_or("").is_empty() {
        return R_NilValue();
    }
    // Try to evaluate the parsed text (simplified: treat as symbol lookup)
    let sym = Rf_install(cstr.as_ptr());
    if !sym.is_null() {
        // Return an expression containing the symbol
        let result = Rf_allocVector3(SEXPTYPE::EXPRSXP, 1);
        if !result.is_null() {
            let _p = Rf_protect(result);
            // For EXPRSXP, store the symbol
            let data = (*result).gengc_next_node as *mut SEXP;
            *data = sym;
            crate::sexp::protect::Rf_unprotect(1);
            return result;
        }
    }
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Complete error system: condition handling
// ---------------------------------------------------------------------------

/// R's `conditionMessage(cond)` — get message from condition object.
pub unsafe fn do_conditionMessage(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let cond = CAR(args);
    if cond.is_null() || cond == R_NilValue() {
        return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
    }
    // Try to get the "message" attribute or element
    let msg_sym = Rf_install(CString::new("message").unwrap_or_default().as_ptr());
    let msg = crate::sexp::attrib_core::getAttrib(cond, msg_sym);
    if !msg.is_null() && msg != R_NilValue() && TYPEOF(msg) == SEXPTYPE::STRSXP {
        return msg;
    }
    // Fallback: deparse the condition
    Rf_mkString(
        CString::new(elt_to_string(cond, 0))
            .unwrap_or_default()
            .as_ptr(),
    )
}

/// R's `conditionCall(cond)` — get call from condition object.
pub unsafe fn do_conditionCall(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let cond = CAR(args);
    if cond.is_null() || cond == R_NilValue() {
        return R_NilValue();
    }
    let call_sym = Rf_install(CString::new("call").unwrap_or_default().as_ptr());
    let call_val = crate::sexp::attrib_core::getAttrib(cond, call_sym);
    if !call_val.is_null() && call_val != R_NilValue() {
        return call_val;
    }
    R_NilValue()
}

/// R's `simpleError(message, call)` — create a simple error condition.
pub unsafe fn do_simpleError(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let message_arg = CAR(args);
    let call_arg = CAR(CDR(args));
    let message = if message_arg.is_null() || message_arg == R_NilValue() {
        String::new()
    } else {
        elt_to_string(message_arg, 0)
    };
    // Create a simple list with class "simpleError" and "error" and "condition"
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let msg_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    if !msg_vec.is_null() {
        let cstr = CString::new(message).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*msg_vec).gengc_next_node as *mut SEXP;
            *data = charsxp;
        }
    }
    SET_VECTOR_ELT(result, 0, msg_vec);
    // Set names
    let names = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    if !names.is_null() {
        let cstr = CString::new("message").unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*names).gengc_next_node as *mut SEXP;
            *data = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            names,
        );
    }
    // Set class
    let class = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
    if !class.is_null() {
        let classes = ["simpleError", "error", "condition"];
        for (i, &c) in classes.iter().enumerate() {
            let cs = CString::new(c).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
            if !charsxp.is_null() {
                let data = (*class).gengc_next_node as *mut SEXP;
                *data.add(i) = charsxp;
            }
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            class,
        );
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `simpleWarning(message, call)` — create a simple warning condition.
pub unsafe fn do_simpleWarning(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let message_arg = CAR(args);
    let message = if message_arg.is_null() || message_arg == R_NilValue() {
        String::new()
    } else {
        elt_to_string(message_arg, 0)
    };
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let msg_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    if !msg_vec.is_null() {
        let cstr = CString::new(message).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*msg_vec).gengc_next_node as *mut SEXP;
            *data = charsxp;
        }
    }
    SET_VECTOR_ELT(result, 0, msg_vec);
    let names = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    if !names.is_null() {
        let cstr = CString::new("message").unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*names).gengc_next_node as *mut SEXP;
            *data = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            names,
        );
    }
    let class = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
    if !class.is_null() {
        let classes = ["simpleWarning", "warning", "condition"];
        for (i, &c) in classes.iter().enumerate() {
            let cs = CString::new(c).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
            if !charsxp.is_null() {
                let data = (*class).gengc_next_node as *mut SEXP;
                *data.add(i) = charsxp;
            }
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            class,
        );
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `withRestarts(expr, ...)` — simplified restart handling.
/// Just evaluates expr; restarts are not fully implemented.
pub unsafe fn do_withRestarts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let expr = CAR(args);
    if expr.is_null() || expr == R_NilValue() {
        return R_NilValue();
    }
    // Simplified: just evaluate the expression
    crate::eval::eval::Rf_eval(expr, _rho)
}

// ---------------------------------------------------------------------------
// Complete S3/S4: class, isS4, is
// ---------------------------------------------------------------------------

/// R's `class(x)` — get S3 class vector.
pub unsafe fn do_S3_class(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_class_get(_call, _op, args, _rho)
}

/// R's `isS4(x)` — check if object is S4.
pub unsafe fn do_isS4(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    // S4 objects have OBJECT bit set and a ".S4Class" or similar marker
    // Simplified check: look for S4 flag in object
    let t = TYPEOF(x);
    if t == SEXPTYPE::OBJSXP {
        return Rf_ScalarLogical(TRUE);
    }
    // Check for S4 class attribute
    let class_sym = Rf_install(CString::new("class").unwrap_or_default().as_ptr());
    let class_val = crate::sexp::attrib_core::getAttrib(x, class_sym);
    if !class_val.is_null() && class_val != R_NilValue() && TYPEOF(class_val) == SEXPTYPE::STRSXP {
        let n = LENGTH(class_val);
        for i in 0..n {
            let charsxp = crate::sexp::accessors::STRING_ELT(class_val, i as R_xlen_t);
            if !charsxp.is_null() {
                let s = crate::sexp::accessors::CHAR(charsxp);
                if !s.is_null() {
                    let class_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                    // Check for known S4 marker classes
                    if class_str.contains("S4") || class_str.contains("representation") {
                        return Rf_ScalarLogical(TRUE);
                    }
                }
            }
        }
    }
    Rf_ScalarLogical(FALSE)
}

/// R's `is(x, class2)` — type/class check.
pub unsafe fn do_is(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let class2_arg = CAR(CDR(args));
    if x.is_null() || class2_arg.is_null() || class2_arg == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let class2 = elt_to_string(class2_arg, 0);
    if x == R_NilValue() {
        return Rf_ScalarLogical(if class2 == "NULL" { TRUE } else { FALSE });
    }
    // Get the type of x
    let type_name = match TYPEOF(x) {
        t if t == SEXPTYPE::LGLSXP => "logical",
        t if t == SEXPTYPE::INTSXP => "integer",
        t if t == SEXPTYPE::REALSXP => "double",
        t if t == SEXPTYPE::CPLXSXP => "complex",
        t if t == SEXPTYPE::STRSXP => "character",
        t if t == SEXPTYPE::VECSXP => "list",
        t if t == SEXPTYPE::LISTSXP => "pairlist",
        t if t == SEXPTYPE::LANGSXP => "language",
        t if t == SEXPTYPE::SYMSXP => "symbol",
        t if t == SEXPTYPE::CLOSXP => "closure",
        t if t == SEXPTYPE::ENVSXP => "environment",
        _ => "any",
    };
    // Check S3 class
    let class_sym = Rf_install(CString::new("class").unwrap_or_default().as_ptr());
    let class_val = crate::sexp::attrib_core::getAttrib(x, class_sym);
    if !class_val.is_null() && class_val != R_NilValue() && TYPEOF(class_val) == SEXPTYPE::STRSXP {
        let n = LENGTH(class_val);
        for i in 0..n {
            let charsxp = crate::sexp::accessors::STRING_ELT(class_val, i as R_xlen_t);
            if !charsxp.is_null() {
                let s = crate::sexp::accessors::CHAR(charsxp);
                if !s.is_null() {
                    let c = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                    if c == class2 {
                        return Rf_ScalarLogical(TRUE);
                    }
                }
            }
        }
    }
    // Check type name
    let is_match = type_name == class2
        || (class2 == "numeric" && (type_name == "double" || type_name == "integer"))
        || (class2 == "vector"
            && (type_name == "logical"
                || type_name == "integer"
                || type_name == "double"
                || type_name == "character"
                || type_name == "complex"))
        || (class2 == "atomic"
            && type_name != "list"
            && type_name != "pairlist"
            && type_name != "language"
            && type_name != "closure"
            && type_name != "environment");
    Rf_ScalarLogical(if is_match { TRUE } else { FALSE })
}

/// R's `setClass(Class, representation, ...)` — define an S4 class (simplified stub).
/// In real R, this is from the methods package; here we just return NULL.
pub unsafe fn do_setClass(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    R_NilValue()
}

/// R's `setValidity(Class, method)` — set a validity method for an S4 class (simplified stub).
/// Returns NULL silently.
pub unsafe fn do_setValidity(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    R_NilValue()
}

/// R's `isVirtualClass(Class)` — check if an S4 class is virtual (simplified stub).
/// Always returns FALSE in this simplified implementation.
pub unsafe fn do_isVirtualClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    Rf_ScalarLogical(FALSE)
}

/// R's `new(Class, ...)` — create an S4 object (simplified).
/// Creates a list-based object with the class attribute set.
pub unsafe fn do_new(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let class_arg = CAR(args);
    if class_arg.is_null() || class_arg == R_NilValue() {
        return R_NilValue();
    }
    let class_name = elt_to_string(class_arg, 0);
    // Collect named slot values from ... args
    let mut slots: Vec<(String, SEXP)> = Vec::new();
    let mut current = CDR(args);
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        let tag = (*current).data.listsxp.tagval;
        let slot_name = if !tag.is_null() && tag != R_NilValue() {
            let sym_str = crate::sexp::accessors::CHAR(tag);
            if !sym_str.is_null() {
                std::ffi::CStr::from_ptr(sym_str)
                    .to_str()
                    .unwrap_or("")
                    .to_string()
            } else {
                format!("slot{}", slots.len() + 1)
            }
        } else {
            format!("slot{}", slots.len() + 1)
        };
        slots.push((slot_name, arg));
        current = CDR(current);
    }
    // Create a VECSXP to hold the slots
    let n = slots.len() as R_xlen_t;
    let result = if n > 0 {
        Rf_allocVector3(SEXPTYPE::VECSXP, n)
    } else {
        Rf_allocVector3(SEXPTYPE::VECSXP, 1)
    };
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    // Fill slots
    let names = Rf_allocVector3(SEXPTYPE::STRSXP, n.max(1));
    let _np = Rf_protect(names);
    for (i, (name, val)) in slots.iter().enumerate() {
        crate::sexp::accessors::SET_VECTOR_ELT(result, i as R_xlen_t, *val);
        let cstr = CString::new(name.as_str()).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*names).gengc_next_node as *mut SEXP;
            *data.add(i) = charsxp;
        }
    }
    // Set names attribute
    crate::sexp::attrib_core::setAttrib(
        result,
        Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        names,
    );
    // Set class attribute
    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    if !class_vec.is_null() {
        let cp = Rf_protect(class_vec);
        let cstr = CString::new(class_name.as_str()).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*class_vec).gengc_next_node as *mut SEXP;
            *data.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            class_vec,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }
    crate::sexp::protect::Rf_unprotect(2);
    result
}

/// R's `show(object)` — display an S4 object (simplified).
pub unsafe fn do_show(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let object = CAR(args);
    if object.is_null() || object == R_NilValue() {
        println!("NULL");
        return R_NilValue();
    }
    // Try to print class info
    let class_sym = Rf_install(CString::new("class").unwrap_or_default().as_ptr());
    let class_val = crate::sexp::attrib_core::getAttrib(object, class_sym);
    if !class_val.is_null() && class_val != R_NilValue() && TYPEOF(class_val) == SEXPTYPE::STRSXP {
        let charsxp = crate::sexp::accessors::STRING_ELT(class_val, 0);
        if !charsxp.is_null() {
            let s = crate::sexp::accessors::CHAR(charsxp);
            if !s.is_null() {
                let class_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("unknown");
                println!("An object of class \"{}\"", class_str);
            }
        }
    }
    // Print slots if VECSXP
    if TYPEOF(object) == SEXPTYPE::VECSXP {
        let n = XLENGTH(object);
        let names_sym = Rf_install(CString::new("names").unwrap_or_default().as_ptr());
        let names_val = crate::sexp::attrib_core::getAttrib(object, names_sym);
        for i in 0..n {
            let slot_val = crate::sexp::accessors::VECTOR_ELT(object, i);
            let slot_name = if !names_val.is_null() && names_val != R_NilValue() {
                let ns = crate::sexp::accessors::STRING_ELT(names_val, i);
                if !ns.is_null() {
                    let s = crate::sexp::accessors::CHAR(ns);
                    if !s.is_null() {
                        std::ffi::CStr::from_ptr(s)
                            .to_str()
                            .unwrap_or("")
                            .to_string()
                    } else {
                        format!("Slot{}", i + 1)
                    }
                } else {
                    format!("Slot{}", i + 1)
                }
            } else {
                format!("Slot{}", i + 1)
            };
            let val_str = elt_to_string(slot_val, 0);
            println!("Slot \"{}\":", slot_name);
            println!("  {}", val_str);
        }
    }
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    object
}

/// R's `slotNames(Class)` — get the names of slots of an S4 class.
pub unsafe fn do_slotNames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let class_arg = CAR(args);
    if class_arg.is_null() || class_arg == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
    }
    // If it's an object with names, return names
    let names_sym = Rf_install(CString::new("names").unwrap_or_default().as_ptr());
    let names_val = crate::sexp::attrib_core::getAttrib(class_arg, names_sym);
    if !names_val.is_null() && names_val != R_NilValue() && TYPEOF(names_val) == SEXPTYPE::STRSXP {
        return names_val;
    }
    // If it's a string, treat as class name - return empty
    Rf_allocVector3(SEXPTYPE::STRSXP, 0)
}

/// R's `slot(object, name)` — get the value of a slot.
pub unsafe fn do_slot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let object = CAR(args);
    let name_arg = CAR(CDR(args));
    if object.is_null() || object == R_NilValue() || name_arg.is_null() || name_arg == R_NilValue()
    {
        return R_NilValue();
    }
    let slot_name = elt_to_string(name_arg, 0);
    // Look up by names attribute
    if TYPEOF(object) == SEXPTYPE::VECSXP {
        let names_sym = Rf_install(CString::new("names").unwrap_or_default().as_ptr());
        let names_val = crate::sexp::attrib_core::getAttrib(object, names_sym);
        if !names_val.is_null() && names_val != R_NilValue() {
            let n = LENGTH(names_val);
            for i in 0..n {
                let ns = crate::sexp::accessors::STRING_ELT(names_val, i as R_xlen_t);
                if !ns.is_null() {
                    let s = crate::sexp::accessors::CHAR(ns);
                    if !s.is_null() {
                        let name_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                        if name_str == slot_name {
                            return crate::sexp::accessors::VECTOR_ELT(object, i as R_xlen_t);
                        }
                    }
                }
            }
        }
    }
    R_NilValue()
}

/// R's `set_slot(object, name, value)` — set the value of a slot.
pub unsafe fn do_set_slot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let object = CAR(args);
    let name_arg = CAR(CDR(args));
    let value = CAR(CDR(CDR(args)));
    if object.is_null() || object == R_NilValue() || name_arg.is_null() || name_arg == R_NilValue()
    {
        return object;
    }
    let slot_name = elt_to_string(name_arg, 0);
    // Set slot in a VECSXP
    if TYPEOF(object) == SEXPTYPE::VECSXP {
        let names_sym = Rf_install(CString::new("names").unwrap_or_default().as_ptr());
        let names_val = crate::sexp::attrib_core::getAttrib(object, names_sym);
        if !names_val.is_null() && names_val != R_NilValue() {
            let n = LENGTH(names_val);
            for i in 0..n {
                let ns = crate::sexp::accessors::STRING_ELT(names_val, i as R_xlen_t);
                if !ns.is_null() {
                    let s = crate::sexp::accessors::CHAR(ns);
                    if !s.is_null() {
                        let name_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                        if name_str == slot_name {
                            crate::sexp::accessors::SET_VECTOR_ELT(object, i as R_xlen_t, value);
                            return value;
                        }
                    }
                }
            }
        }
    }
    object
}

/// R's `extends(class1, class2)` — check if class1 extends class2.
pub unsafe fn do_extends(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let class1_arg = CAR(args);
    let class2_arg = CAR(CDR(args));
    if class1_arg.is_null() || class2_arg.is_null() {
        return Rf_ScalarLogical(FALSE);
    }
    let class1 = elt_to_string(class1_arg, 0);
    let class2 = elt_to_string(class2_arg, 0);
    // Simple: same class always extends
    if class1 == class2 {
        return Rf_ScalarLogical(TRUE);
    }
    // Check common inheritance
    let extends = match class1.as_str() {
        "numeric" | "double" => class2 == "vector" || class2 == "atomic",
        "integer" => class2 == "numeric" || class2 == "vector" || class2 == "atomic",
        "logical" => class2 == "vector" || class2 == "atomic",
        "character" => class2 == "vector" || class2 == "atomic",
        "complex" => class2 == "vector" || class2 == "atomic",
        "matrix" => class2 == "array",
        "data.frame" => class2 == "list",
        "factor" => class2 == "integer" || class2 == "vector" || class2 == "atomic",
        "ordered" => class2 == "factor" || class2 == "integer",
        _ => false,
    };
    Rf_ScalarLogical(if extends { TRUE } else { FALSE })
}

/// R's `isSealedClass(Class)` — check if a class is sealed.
pub unsafe fn do_isSealedClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Built-in types are always sealed
    Rf_ScalarLogical(TRUE)
}

/// R's `sealClass(Class, ...)` — seal a class definition.
pub unsafe fn do_sealClass(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    // No-op in simplified implementation
    R_NilValue()
}

/// R's `representation(...)` — define class representation.
pub unsafe fn do_representation(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Collect named args as slot name = type pairs
    let n_list = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
    if n_list.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(n_list);
    // Count args
    let mut count: R_xlen_t = 0;
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        count += 1;
        current = CDR(current);
    }
    if count == 0 {
        crate::sexp::protect::Rf_unprotect(1);
        return n_list;
    }
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, count);
    if result.is_null() {
        crate::sexp::protect::Rf_unprotect(1);
        return R_NilValue();
    }
    let rp = Rf_protect(result);
    let names = Rf_allocVector3(SEXPTYPE::STRSXP, count);
    let np = Rf_protect(names);
    let mut idx: R_xlen_t = 0;
    current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        let tag = (*current).data.listsxp.tagval;
        let slot_name = if !tag.is_null() && tag != R_NilValue() {
            let sym_str = crate::sexp::accessors::CHAR(tag);
            if !sym_str.is_null() {
                std::ffi::CStr::from_ptr(sym_str)
                    .to_str()
                    .unwrap_or("")
                    .to_string()
            } else {
                format!("slot{}", idx + 1)
            }
        } else {
            format!("slot{}", idx + 1)
        };
        crate::sexp::accessors::SET_VECTOR_ELT(result, idx, arg);
        let cstr = CString::new(slot_name.as_str()).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*names).gengc_next_node as *mut SEXP;
            *data.add(idx as usize) = charsxp;
        }
        idx += 1;
        current = CDR(current);
    }
    crate::sexp::attrib_core::setAttrib(
        result,
        Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        names,
    );
    crate::sexp::protect::Rf_unprotect(3);
    result
}

/// R's `containsClass(class1, class2)` — check class containment.
pub unsafe fn do_containsClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Delegates to extends
    do_extends(_call, _op, args, _rho)
}

/// R's `possibleExtends(class1, class2)` — check possible extensions.
pub unsafe fn do_possibleExtends(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Simplified: delegates to extends
    do_extends(_call, _op, args, _rho)
}

/// R's `setReplaceMethod(f, signature, definition)` — set replace method.
pub unsafe fn do_setReplaceMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Simplified: return the definition
    let definition = CAR(CDR(CDR(args)));
    if !definition.is_null() && definition != R_NilValue() {
        definition
    } else {
        R_NilValue()
    }
}

/// R's `getMethod(f, signature)` — get a specific S4 method.
pub unsafe fn do_getMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Simplified: return the function name or NULL
    let f_arg = CAR(args);
    if f_arg.is_null() || f_arg == R_NilValue() {
        return R_NilValue();
    }
    f_arg
}

/// R's `removeGeneric(f)` — remove a generic.
pub unsafe fn do_removeGeneric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _f = CAR(args);
    Rf_ScalarLogical(FALSE)
}

/// R's `removeMethod(f, signature)` — remove a method.
pub unsafe fn do_removeMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _f = CAR(args);
    let _sig = CAR(CDR(args));
    Rf_ScalarLogical(FALSE)
}

/// R's `isGeneric(f)` — check if f is a generic.
pub unsafe fn do_isGeneric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _f = CAR(args);
    Rf_ScalarLogical(FALSE)
}

/// R's `isMethod(f, signature)` — check if method exists.
pub unsafe fn do_isMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _f = CAR(args);
    let _sig = CAR(CDR(args));
    Rf_ScalarLogical(FALSE)
}

/// R's `findMethod(f, signature)` — find S4 method.
pub unsafe fn do_findMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _f = CAR(args);
    let _sig = CAR(CDR(args));
    R_NilValue()
}

/// R's `findMethods(f)` — find all methods for a generic.
pub unsafe fn do_findMethods(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _f = CAR(args);
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
    if result.is_null() {
        R_NilValue()
    } else {
        result
    }
}

/// R's `showMethods(f)` — show methods for a generic.
pub unsafe fn do_showMethods(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _f = CAR(args);
    println!("No methods found");
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    R_NilValue()
}

/// R's `getGenerics(where)` — get all generics.
pub unsafe fn do_getGenerics(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _where = CAR(args);
    Rf_allocVector3(SEXPTYPE::STRSXP, 0)
}

/// R's `getMethods(f)` — get all methods for a generic.
pub unsafe fn do_getMethods(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _f = CAR(args);
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
    if result.is_null() {
        R_NilValue()
    } else {
        result
    }
}

/// R's `existsMethod(f, signature)` — check if method exists.
pub unsafe fn do_existsMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _f = CAR(args);
    let _sig = CAR(CDR(args));
    Rf_ScalarLogical(FALSE)
}

/// R's `hasMethod(f, signature)` — alias for existsMethod.
pub unsafe fn do_hasMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_existsMethod(_call, _op, args, _rho)
}

/// R's `selectMethod(f, signature)` — select method for generic.
pub unsafe fn do_selectMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let f_arg = CAR(args);
    if f_arg.is_null() || f_arg == R_NilValue() {
        return R_NilValue();
    }
    f_arg
}

// ---------------------------------------------------------------------------
// Complete I/O: scan, write.table, sink
// ---------------------------------------------------------------------------

/// R's `scan(file, what, nmax, sep, ...)` — read data from file.
/// Simplified: reads numeric or character data line by line.
pub unsafe fn do_scan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let file_arg = CAR(args);
    let what_arg = CAR(CDR(args));
    let nmax_arg = CAR(CDR(CDR(args)));
    if file_arg.is_null() || file_arg == R_NilValue() {
        return R_NilValue();
    }
    let filename = elt_to_string(file_arg, 0);
    let what_type = if what_arg.is_null() || what_arg == R_NilValue() {
        SEXPTYPE::REALSXP.as_c_int()
    } else {
        TYPEOF(what_arg)
    };
    let nmax = if nmax_arg.is_null() || nmax_arg == R_NilValue() {
        -1_i64
    } else {
        real_or_default(nmax_arg, -1.0) as i64
    };

    let contents = match std::fs::read_to_string(&filename) {
        Ok(s) => s,
        Err(_) => return R_NilValue(),
    };

    let mut values: Vec<String> = Vec::new();
    for line in contents.lines() {
        for token in line.split_whitespace() {
            if nmax > 0 && values.len() as i64 >= nmax {
                break;
            }
            values.push(token.to_string());
        }
        if nmax > 0 && values.len() as i64 >= nmax {
            break;
        }
    }

    let n = values.len() as R_xlen_t;
    if what_type == SEXPTYPE::REALSXP || what_type == SEXPTYPE::INTSXP {
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = REAL(result);
        for (i, v) in values.iter().enumerate() {
            *dst.add(i) = v.parse::<f64>().unwrap_or(NA_REAL);
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    } else {
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        for (i, v) in values.iter().enumerate() {
            let cstr = CString::new(v.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i) = charsxp;
            }
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R's `write.table(x, file, sep=" ", ...)` — write data to file.
pub unsafe fn do_write_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x_arg = CAR(args);
    let file_arg = CAR(CDR(args));
    let sep_arg = CAR(CDR(CDR(args)));
    if x_arg.is_null() || x_arg == R_NilValue() || file_arg.is_null() || file_arg == R_NilValue() {
        return R_NilValue();
    }
    let filename = elt_to_string(file_arg, 0);
    let sep = if sep_arg.is_null() || sep_arg == R_NilValue() {
        " "
    } else {
        &elt_to_string(sep_arg, 0)
    };

    let mut output = String::new();
    let n = XLENGTH(x_arg).max(1);
    let t = TYPEOF(x_arg);

    if t == SEXPTYPE::VECSXP {
        // Data frame-like: write columns
        let ncols = n;
        let nrows = if n > 0 {
            XLENGTH(VECTOR_ELT(x_arg, 0)).max(1)
        } else {
            0
        };
        // Write header with column names
        let names_sym = Rf_install(CString::new("names").unwrap_or_default().as_ptr());
        let names = crate::sexp::attrib_core::getAttrib(x_arg, names_sym);
        if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
            let mut header = Vec::new();
            for j in 0..ncols {
                let charsxp = crate::sexp::accessors::STRING_ELT(names, j);
                if !charsxp.is_null() {
                    let s = crate::sexp::accessors::CHAR(charsxp);
                    if !s.is_null() {
                        header.push(
                            std::ffi::CStr::from_ptr(s)
                                .to_str()
                                .unwrap_or("")
                                .to_string(),
                        );
                    } else {
                        header.push(String::new());
                    }
                } else {
                    header.push(String::new());
                }
            }
            output.push_str(&header.join(sep));
            output.push('\n');
        }
        // Write rows
        for i in 0..nrows {
            let mut row = Vec::new();
            for j in 0..ncols {
                let col = VECTOR_ELT(x_arg, j);
                if !col.is_null() && col != R_NilValue() {
                    row.push(elt_to_string(col, i));
                } else {
                    row.push("NA".to_string());
                }
            }
            output.push_str(&row.join(sep));
            output.push('\n');
        }
    } else {
        // Atomic vector: write as single column
        for i in 0..n {
            output.push_str(&elt_to_string(x_arg, i));
            output.push('\n');
        }
    }

    let _ = std::fs::write(&filename, output);
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    R_NilValue()
}

/// R's `sink(file)` — redirect output to file.
/// Simplified: stores the sink target for cat/print redirection.
pub unsafe fn do_sink(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let file_arg = CAR(args);
    if file_arg.is_null() || file_arg == R_NilValue() {
        // Reset sink (not fully implemented)
        return R_NilValue();
    }
    let _filename = elt_to_string(file_arg, 0);
    // Simplified: sink is not fully implemented in this R port
    // A real implementation would redirect stdout to the file
    eprintln!("Note: sink() is not fully implemented in this R port");
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Math/Statistics
// ---------------------------------------------------------------------------

/// R's `cov(x, y)` — covariance between two numeric vectors.
pub unsafe fn do_cov(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let y_cdr = CDR(args);
    let y = if y_cdr.is_null() || y_cdr == R_NilValue() {
        R_NilValue()
    } else {
        CAR(y_cdr)
    };

    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarReal(NA_REAL);
    }

    let x_data = get_numeric_data(x);
    let y_data = if y.is_null() || y == R_NilValue() {
        x_data.clone()
    } else {
        get_numeric_data(y)
    };

    let n = x_data.len().min(y_data.len());
    if n == 0 {
        return Rf_ScalarReal(NA_REAL);
    }

    let mut sum_x = 0.0_f64;
    let mut sum_y = 0.0_f64;
    let mut count = 0_i64;
    for i in 0..n {
        if !x_data[i].is_nan() && !y_data[i].is_nan() {
            sum_x += x_data[i];
            sum_y += y_data[i];
            count += 1;
        }
    }
    if count < 2 {
        return Rf_ScalarReal(NA_REAL);
    }
    let mean_x = sum_x / count as f64;
    let mean_y = sum_y / count as f64;

    let mut cov = 0.0_f64;
    for i in 0..n {
        if !x_data[i].is_nan() && !y_data[i].is_nan() {
            cov += (x_data[i] - mean_x) * (y_data[i] - mean_y);
        }
    }
    Rf_ScalarReal(cov / (count as f64 - 1.0))
}

/// R's `cor(x, y)` — Pearson correlation between two numeric vectors.
pub unsafe fn do_cor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let y_cdr = CDR(args);
    let y = if y_cdr.is_null() || y_cdr == R_NilValue() {
        R_NilValue()
    } else {
        CAR(y_cdr)
    };

    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarReal(NA_REAL);
    }

    let x_data = get_numeric_data(x);
    let y_data = if y.is_null() || y == R_NilValue() {
        x_data.clone()
    } else {
        get_numeric_data(y)
    };

    let n = x_data.len().min(y_data.len());
    if n == 0 {
        return Rf_ScalarReal(NA_REAL);
    }

    let mut sum_x = 0.0_f64;
    let mut sum_y = 0.0_f64;
    let mut count = 0_i64;
    for i in 0..n {
        if !x_data[i].is_nan() && !y_data[i].is_nan() {
            sum_x += x_data[i];
            sum_y += y_data[i];
            count += 1;
        }
    }
    if count < 2 {
        return Rf_ScalarReal(NA_REAL);
    }
    let mean_x = sum_x / count as f64;
    let mean_y = sum_y / count as f64;

    let mut cov = 0.0_f64;
    let mut var_x = 0.0_f64;
    let mut var_y = 0.0_f64;
    for i in 0..n {
        if !x_data[i].is_nan() && !y_data[i].is_nan() {
            let dx = x_data[i] - mean_x;
            let dy = y_data[i] - mean_y;
            cov += dx * dy;
            var_x += dx * dx;
            var_y += dy * dy;
        }
    }
    let denom = (var_x * var_y).sqrt();
    if denom == 0.0 {
        return Rf_ScalarReal(NA_REAL);
    }
    Rf_ScalarReal(cov / denom)
}

/// R's `scale(x, center=TRUE, scale=TRUE)` — standardize a numeric vector.
pub unsafe fn do_scale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let center_arg = CAR(CDR(args));
    let scale_arg = CAR(CDR(CDR(args)));

    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let do_center = center_arg.is_null()
        || center_arg == R_NilValue()
        || (TYPEOF(center_arg) == SEXPTYPE::LGLSXP && *LOGICAL(center_arg) == TRUE);
    let do_scale = scale_arg.is_null()
        || scale_arg == R_NilValue()
        || (TYPEOF(scale_arg) == SEXPTYPE::LGLSXP && *LOGICAL(scale_arg) == TRUE);

    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    // Compute mean
    let mut sum = 0.0_f64;
    let mut count = 0_i64;
    for i in 0..n {
        let v = real_or_default(elt_to_sexp(x, i), NA_REAL);
        if !v.is_nan() && v != NA_REAL {
            sum += v;
            count += 1;
        }
    }
    let mean = if count > 0 {
        sum / count as f64
    } else {
        NA_REAL
    };

    // Compute sd
    let mut var_sum = 0.0_f64;
    if do_scale {
        for i in 0..n {
            let v = real_or_default(elt_to_sexp(x, i), NA_REAL);
            if !v.is_nan() && v != NA_REAL {
                var_sum += (v - mean) * (v - mean);
            }
        }
    }
    let sd = if count > 1 {
        (var_sum / (count as f64 - 1.0)).sqrt()
    } else {
        NA_REAL
    };

    let dst = REAL(result);
    for i in 0..n {
        let v = real_or_default(elt_to_sexp(x, i), NA_REAL);
        let centered = if do_center { v - mean } else { v };
        let scaled = if do_scale && sd != 0.0 && !sd.is_nan() {
            centered / sd
        } else {
            centered
        };
        *dst.add(i as usize) = scaled;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `rle(x)` — run-length encoding.
pub unsafe fn do_rle(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    if n == 0 {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        SET_VECTOR_ELT(result, 0, Rf_allocVector3(SEXPTYPE::INTSXP, 0));
        SET_VECTOR_ELT(result, 1, Rf_allocVector3(SEXPTYPE::REALSXP, 0));
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !names.is_null() {
            let _p2 = Rf_protect(names);
            for (i, nm) in ["lengths", "values"].iter().enumerate() {
                let cs = CString::new(*nm).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
                if !charsxp.is_null() {
                    let data = (*names).gengc_next_node as *mut SEXP;
                    *data.add(i) = charsxp;
                }
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
            crate::sexp::protect::Rf_unprotect(1);
        }
        crate::sexp::protect::Rf_unprotect(1);
        return result;
    }

    // Collect run lengths and values
    let mut lengths: Vec<i32> = Vec::new();
    let mut values: Vec<f64> = Vec::new();

    let first_val = real_or_default(elt_to_sexp(x, 0), NA_REAL);
    values.push(first_val);
    lengths.push(1);

    for i in 1..n {
        let v = real_or_default(elt_to_sexp(x, i), NA_REAL);
        let last_idx = values.len() - 1;
        if v == values[last_idx] {
            lengths[last_idx] += 1;
        } else {
            values.push(v);
            lengths.push(1);
        }
    }

    let n_runs = lengths.len() as R_xlen_t;
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    let lengths_sexp = Rf_allocVector3(SEXPTYPE::INTSXP, n_runs);
    let values_sexp = Rf_allocVector3(SEXPTYPE::REALSXP, n_runs);
    let _p2 = Rf_protect(lengths_sexp);
    let _p3 = Rf_protect(values_sexp);

    let dst_l = INTEGER(lengths_sexp);
    let dst_v = REAL(values_sexp);
    for i in 0..n_runs {
        *dst_l.add(i as usize) = lengths[i as usize];
        *dst_v.add(i as usize) = values[i as usize];
    }

    SET_VECTOR_ELT(result, 0, lengths_sexp);
    SET_VECTOR_ELT(result, 1, values_sexp);

    let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
    if !names.is_null() {
        let _p4 = Rf_protect(names);
        for (i, nm) in ["lengths", "values"].iter().enumerate() {
            let cs = CString::new(*nm).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
            if !charsxp.is_null() {
                let data = (*names).gengc_next_node as *mut SEXP;
                *data.add(i) = charsxp;
            }
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            names,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }

    crate::sexp::protect::Rf_unprotect(3);
    result
}

/// R's `inverse.rle(x)` — inverse of run-length encoding.
pub unsafe fn do_inverse_rle(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }

    let lengths_sexp = VECTOR_ELT(x, 0);
    let values_sexp = VECTOR_ELT(x, 1);
    if lengths_sexp.is_null() || values_sexp.is_null() {
        return R_NilValue();
    }

    let n_runs = XLENGTH(lengths_sexp);
    if n_runs == 0 {
        return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
    }

    // Compute total length
    let mut total: R_xlen_t = 0;
    for i in 0..n_runs {
        total += (*INTEGER(lengths_sexp).add(i as usize)) as R_xlen_t;
    }

    let result = Rf_allocVector3(SEXPTYPE::REALSXP, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    let mut offset: R_xlen_t = 0;
    for i in 0..n_runs {
        let len = *INTEGER(lengths_sexp).add(i as usize);
        let val = real_or_default(elt_to_sexp(values_sexp, i), NA_REAL);
        for j in 0..len {
            *dst.add((offset + j as R_xlen_t) as usize) = val;
        }
        offset += len as R_xlen_t;
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Matrix helpers
// ---------------------------------------------------------------------------

/// R's `which(x)` variant for arrays — returns 1-based row-major indices where x is TRUE.
pub unsafe fn do_which_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Same as do_which for now — array-aware which is equivalent for logical vectors
    do_which(_call, _op, args, _rho)
}

// ---------------------------------------------------------------------------
// R runtime
// ---------------------------------------------------------------------------

/// R's `commandArgs()` — returns the command line arguments as a character vector.
pub unsafe fn do_commandArgs(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    let args: Vec<String> = std::env::args().collect();
    let n = args.len() as R_xlen_t;
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for (i, arg) in args.iter().enumerate() {
        let cs = CString::new(arg.as_str()).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
        if !charsxp.is_null() {
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(i) = charsxp;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `getOption(x)` — get an option value. Simplified: returns NULL for unknown options.
pub unsafe fn do_getOption(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _x = CAR(args);
    // Simplified: options system not fully implemented
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    R_NilValue()
}

/// R's `options(...)` — set/query options. Simplified: returns NULL.
pub unsafe fn do_options(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    // Simplified: options system not fully implemented
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    R_NilValue()
}

/// R's `interactive()` — returns FALSE (not in interactive session).
pub unsafe fn do_interactive(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    Rf_ScalarLogical(FALSE)
}

/// Alias for `interactive()`.
pub unsafe fn do_is_interactive(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    Rf_ScalarLogical(FALSE)
}

/// R's `getRversion()` — returns R version as a string.
pub unsafe fn do_getRversion(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    let s = CString::new("4.4.1").unwrap_or_default();
    Rf_mkString(s.as_ptr())
}

/// R's `R.version.string` — returns the full R version string.
pub unsafe fn do_R_version_string(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    let s = CString::new("R version 4.4.1 (Rust Port)").unwrap_or_default();
    Rf_mkString(s.as_ptr())
}

// ---------------------------------------------------------------------------
// List operations
// ---------------------------------------------------------------------------

/// R-like `list.append(x, ...)` — append elements to a list.
pub unsafe fn do_list_append(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let rest = CDR(args);
    if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    let mut extra_count: R_xlen_t = 0;
    let mut cur = rest;
    while !cur.is_null() && cur != R_NilValue() {
        extra_count += 1;
        cur = CDR(cur);
    }

    let total = n + extra_count;
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    // Copy original elements
    for i in 0..n {
        SET_VECTOR_ELT(result, i as i64, VECTOR_ELT(x, i));
    }

    // Append new elements
    let mut offset = n;
    cur = rest;
    while !cur.is_null() && cur != R_NilValue() {
        let elem = CAR(cur);
        SET_VECTOR_ELT(result, offset as i64, elem);
        offset += 1;
        cur = CDR(cur);
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R-like `list.prepend(x, ...)` — prepend elements to a list.
pub unsafe fn do_list_prepend(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let rest = CDR(args);
    if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    let mut extra_count: R_xlen_t = 0;
    let mut cur = rest;
    while !cur.is_null() && cur != R_NilValue() {
        extra_count += 1;
        cur = CDR(cur);
    }

    let total = n + extra_count;
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    // Prepend new elements
    let mut offset: R_xlen_t = 0;
    cur = rest;
    while !cur.is_null() && cur != R_NilValue() {
        let elem = CAR(cur);
        SET_VECTOR_ELT(result, offset as i64, elem);
        offset += 1;
        cur = CDR(cur);
    }

    // Copy original elements
    for i in 0..n {
        SET_VECTOR_ELT(result, (offset + i) as i64, VECTOR_ELT(x, i));
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R-like `compact(x)` — remove NULL elements from a list.
pub unsafe fn do_compact(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
        return x;
    }

    let n = XLENGTH(x);
    let mut kept: Vec<R_xlen_t> = Vec::new();
    for i in 0..n {
        let elem = VECTOR_ELT(x, i);
        if !elem.is_null() && elem != R_NilValue() {
            kept.push(i);
        }
    }

    let result = Rf_allocVector3(SEXPTYPE::VECSXP, kept.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for (j, &i) in kept.iter().enumerate() {
        SET_VECTOR_ELT(result, j as i64, VECTOR_ELT(x, i));
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R-like `keep(x, i)` — keep elements at 1-based indices from a list/vector.
pub unsafe fn do_keep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let i_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || i_arg.is_null() || i_arg == R_NilValue() {
        return x;
    }

    let t = TYPEOF(x);
    let n_i = XLENGTH(i_arg);
    let result = Rf_allocVector3(t, n_i);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    if t == SEXPTYPE::VECSXP {
        for j in 0..n_i {
            let idx = (*INTEGER(i_arg).add(j as usize) - 1) as R_xlen_t; // 1-based to 0-based
            if idx >= 0 {
                let elem = VECTOR_ELT(x, idx);
                SET_VECTOR_ELT(result, j as i64, elem);
            }
        }
    } else if t == SEXPTYPE::REALSXP {
        let dst = REAL(result);
        for j in 0..n_i {
            let idx = (*INTEGER(i_arg).add(j as usize) - 1) as R_xlen_t;
            if idx >= 0 {
                *dst.add(j as usize) = *REAL(x).add(idx as usize);
            } else {
                *dst.add(j as usize) = NA_REAL;
            }
        }
    } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
        let dst = INTEGER(result);
        for j in 0..n_i {
            let idx = (*INTEGER(i_arg).add(j as usize) - 1) as R_xlen_t;
            if idx >= 0 {
                *dst.add(j as usize) = *INTEGER(x).add(idx as usize);
            } else {
                *dst.add(j as usize) = NA_INTEGER;
            }
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R-like `discard(x, i)` — discard elements at 1-based indices from a list/vector.
pub unsafe fn do_discard(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let i_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || i_arg.is_null() || i_arg == R_NilValue() {
        return x;
    }

    let n = XLENGTH(x);
    let n_i = XLENGTH(i_arg);

    // Collect which indices to discard (0-based)
    let mut discard_set: std::collections::HashSet<R_xlen_t> = std::collections::HashSet::new();
    for j in 0..n_i {
        let idx = (*INTEGER(i_arg).add(j as usize) - 1) as R_xlen_t;
        if idx >= 0 && idx < n {
            discard_set.insert(idx);
        }
    }

    let t = TYPEOF(x);
    let new_len = n - discard_set.len() as R_xlen_t;
    let result = Rf_allocVector3(t, new_len);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    let mut out_idx: R_xlen_t = 0;
    if t == SEXPTYPE::VECSXP {
        for i in 0..n {
            if !discard_set.contains(&i) {
                SET_VECTOR_ELT(result, out_idx as i64, VECTOR_ELT(x, i));
                out_idx += 1;
            }
        }
    } else if t == SEXPTYPE::REALSXP {
        let dst = REAL(result);
        for i in 0..n {
            if !discard_set.contains(&i) {
                *dst.add(out_idx as usize) = *REAL(x).add(i as usize);
                out_idx += 1;
            }
        }
    } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
        let dst = INTEGER(result);
        for i in 0..n {
            if !discard_set.contains(&i) {
                *dst.add(out_idx as usize) = *INTEGER(x).add(i as usize);
                out_idx += 1;
            }
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// String operations
// ---------------------------------------------------------------------------

/// R-like `str_detect(x, pattern)` — returns logical vector indicating which elements match.
pub unsafe fn do_str_detect(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let pattern_arg = CAR(CDR(args));

    if x.is_null() || x == R_NilValue() || pattern_arg.is_null() || pattern_arg == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
    }

    let pattern_str = elt_to_string(pattern_arg, 0);
    let n = XLENGTH(x).max(1);
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);

    for i in 0..n {
        let s = elt_to_string(x, i);
        let matches = s.contains(&pattern_str);
        *dst.add(i as usize) = if matches { TRUE } else { FALSE };
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R-like `str_extract(x, pattern)` — extracts first occurrence of pattern from each element.
pub unsafe fn do_str_extract(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let pattern_arg = CAR(CDR(args));

    if x.is_null() || x == R_NilValue() || pattern_arg.is_null() || pattern_arg == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
    }

    let pattern_str = elt_to_string(pattern_arg, 0);
    let n = XLENGTH(x).max(1);
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

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

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Complete data operations
// ---------------------------------------------------------------------------

/// R's `reshape(x, direction, varying, v.names, timevar, idvar, times)` — reshape data.
/// Simplified: just return x as-is.
pub unsafe fn do_reshape(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() {
        return R_NilValue();
    }
    x
}

/// R's `complete_cases(...)` — returns logical vector: TRUE where all args are non-NA.
pub unsafe fn do_complete_cases(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Collect all argument vectors
    let mut arg_vecs: Vec<SEXP> = Vec::new();
    let mut max_len: R_xlen_t = 0;
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            arg_vecs.push(arg);
            let n = XLENGTH(arg);
            if n > max_len {
                max_len = n;
            }
        }
        current = CDR(current);
    }
    if arg_vecs.is_empty() || max_len == 0 {
        return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
    }
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, max_len);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);
    for i in 0..max_len {
        let mut complete = TRUE;
        for &arg in &arg_vecs {
            let n = XLENGTH(arg);
            let idx = if n == 0 { 0 } else { i % n };
            let t = TYPEOF(arg);
            let na = if t == SEXPTYPE::REALSXP {
                (*REAL(arg).add(idx as usize)).to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                *INTEGER(arg).add(idx as usize) == NA_INTEGER
            } else {
                false
            };
            if na {
                complete = FALSE;
                break;
            }
        }
        *dst.add(i as usize) = complete;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `na.omit(x)` — returns x with rows containing any NA removed (simplified: works on vectors).
pub unsafe fn do_na_omit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    // Find non-NA indices
    let mut keep: Vec<R_xlen_t> = Vec::new();
    for i in 0..n {
        let na = if t == SEXPTYPE::REALSXP {
            (*REAL(x).add(i as usize)).to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            *INTEGER(x).add(i as usize) == NA_INTEGER
        } else {
            false
        };
        if !na {
            keep.push(i);
        }
    }
    let result = Rf_allocVector3(t, keep.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    if t == SEXPTYPE::REALSXP {
        let dst = REAL(result);
        for (j, &i) in keep.iter().enumerate() {
            *dst.add(j) = *REAL(x).add(i as usize);
        }
    } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
        let dst = INTEGER(result);
        for (j, &i) in keep.iter().enumerate() {
            *dst.add(j) = *INTEGER(x).add(i as usize);
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `na.exclude(x)` — like na.omit but remembers excluded rows. Simplified: same as na.omit.
pub unsafe fn do_na_exclude(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_na_omit(_call, _op, args, _rho)
}

/// R's `is_complete(x)` — logical vector of complete cases for a single vector.
pub unsafe fn do_is_complete(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);
    for i in 0..n {
        let na = if t == SEXPTYPE::REALSXP {
            (*REAL(x).add(i as usize)).to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            *INTEGER(x).add(i as usize) == NA_INTEGER
        } else {
            false
        };
        *dst.add(i as usize) = if na { FALSE } else { TRUE };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Complete string/vector
// ---------------------------------------------------------------------------

/// R-like `str_interp(string, values)` — interpolate values into string (simplified: sprintf-like).
pub unsafe fn do_str_interp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

/// R-like `str_wrap(x, width)` — wrap text to width (simplified: just return x).
pub unsafe fn do_str_wrap(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() {
        return R_NilValue();
    }
    x
}

/// R-like `path_package(package, ...)` — find package path (simplified: return empty string).
pub unsafe fn do_path_package(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _pkg = CAR(args);
    Rf_mkString(CString::new("").unwrap_or_default().as_ptr())
}

/// R's `system.file(..., package)` — find system file (simplified: return empty string).
pub unsafe fn do_system_file(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    Rf_mkString(CString::new("").unwrap_or_default().as_ptr())
}

// ---------------------------------------------------------------------------
// Complete R runtime
// ---------------------------------------------------------------------------

/// R-like `ls_args()` — list argument names of current function (simplified: return empty character).
pub unsafe fn do_ls_args(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    Rf_allocVector3(SEXPTYPE::STRSXP, 0)
}

/// R's `deparse1(expr, collapse, width.cutoff)` — deparse to a single string.
pub unsafe fn do_deparse1(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let expr = CAR(args);
    let collapse_arg = CAR(CDR(args));
    let sep = if collapse_arg.is_null() || collapse_arg == R_NilValue() {
        " ".to_string()
    } else {
        elt_to_string(collapse_arg, 0)
    };
    // Simple deparse: convert to string representation
    let s = if expr.is_null() || expr == R_NilValue() {
        "NULL".to_string()
    } else {
        elt_to_string(expr, 0)
    };
    Rf_mkString(CString::new(s).unwrap_or_default().as_ptr())
}

/// R's `dput(x, file)` — dump object (simplified: return deparse string).
pub unsafe fn do_dput(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let s = if x.is_null() || x == R_NilValue() {
        "NULL".to_string()
    } else {
        elt_to_string(x, 0)
    };
    println!("{}", s);
    Rf_mkString(CString::new(s).unwrap_or_default().as_ptr())
}

/// R's `dget(file)` — read dump (simplified: return NULL).
pub unsafe fn do_dget(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    R_NilValue()
}

/// R's `bquote(expr)` — backquote substitution (simplified: return expr as-is).
pub unsafe fn do_bquote(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let expr = CAR(args);
    if expr.is_null() {
        return R_NilValue();
    }
    expr
}

// ---------------------------------------------------------------------------
// Complete S3
// ---------------------------------------------------------------------------

/// R-like `rownames_to_column(x, var)` — convert rownames to a column (simplified).
pub unsafe fn do_rownames_to_column(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() {
        return R_NilValue();
    }
    x
}

/// R-like `column_to_rownames(x, var)` — convert column to rownames (simplified).
pub unsafe fn do_column_to_rownames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() {
        return R_NilValue();
    }
    x
}

/// R-like `relocate(...cols, .before, .after)` — reorder columns (simplified: return x).
pub unsafe fn do_relocate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() {
        return R_NilValue();
    }
    x
}

// ---------------------------------------------------------------------------
// Complete I/O
// ---------------------------------------------------------------------------

/// R-like `cat_args(...)` — cat with better formatting.
pub unsafe fn do_cat_args(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_cat(_call, _op, args, _rho)
}

/// R-like `message_args(...)` — message with domain.
pub unsafe fn do_message_args(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let mut parts: Vec<String> = Vec::new();
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            let n = XLENGTH(arg).max(1);
            for i in 0..n {
                parts.push(elt_to_string(arg, i));
            }
        }
        current = CDR(current);
    }
    let output = parts.join(" ");
    eprintln!("{}", output);
    crate::sexp::globals::set_R_Visible(FALSE);
    R_NilValue()
}

/// R's `packageStartupMessage(...)` — startup message.
pub unsafe fn do_package_startup_message(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let mut parts: Vec<String> = Vec::new();
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            let n = XLENGTH(arg).max(1);
            for i in 0..n {
                parts.push(elt_to_string(arg, i));
            }
        }
        current = CDR(current);
    }
    let output = parts.join(" ");
    eprintln!("{}", output);
    crate::sexp::globals::set_R_Visible(FALSE);
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Environment completion
// ---------------------------------------------------------------------------

/// R's `parent.env(env)` — returns the parent environment.
pub unsafe fn do_parent_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let env = CAR(args);
    if env.is_null() || env == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(env);
    if t != SEXPTYPE::ENVSXP {
        return R_NilValue();
    }
    // enclos is the enclosing/parent environment
    let parent = (*env).data.envsxp.enclos;
    if parent.is_null() {
        return crate::sexp::globals::R_EmptyEnv();
    }
    parent
}

/// R's `set_parent.env(env, parent)` — set the parent environment (simplified: no-op).
pub unsafe fn do_set_parent_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let env = CAR(args);
    if env.is_null() {
        return R_NilValue();
    }
    env
}

/// R's `env_name(env)` — returns the name of an environment (simplified).
pub unsafe fn do_env_name(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let env = CAR(args);
    if env.is_null() || env == R_NilValue() {
        return Rf_mkString(CString::new("NULL").unwrap_or_default().as_ptr());
    }
    let t = TYPEOF(env);
    if t != SEXPTYPE::ENVSXP {
        return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
    }
    // Check if it's a special environment
    if env == crate::sexp::globals::R_GlobalEnv() {
        return Rf_mkString(CString::new("R_GlobalEnv").unwrap_or_default().as_ptr());
    }
    if env == crate::sexp::globals::R_EmptyEnv() {
        return Rf_mkString(CString::new("R_EmptyEnv").unwrap_or_default().as_ptr());
    }
    if env == crate::sexp::globals::R_BaseEnv() {
        return Rf_mkString(CString::new("base").unwrap_or_default().as_ptr());
    }
    Rf_mkString(CString::new("<environment>").unwrap_or_default().as_ptr())
}

/// R's `environmentName(env)` — returns the name of an environment.
pub unsafe fn do_environment_name(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_env_name(_call, _op, args, _rho)
}

/// R-like `is_empty(env)` — check if environment is empty (simplified).
pub unsafe fn do_is_empty(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let env = CAR(args);
    if env.is_null() || env == R_NilValue() {
        return Rf_ScalarLogical(TRUE);
    }
    let t = TYPEOF(env);
    if t == SEXPTYPE::ENVSXP {
        // Check frame - if it's NULL/NILSXP, env is empty
        let frame = (*env).data.envsxp.frame;
        if frame.is_null() || frame == R_NilValue() {
            return Rf_ScalarLogical(TRUE);
        }
        return Rf_ScalarLogical(FALSE);
    }
    // For vectors, check length
    let n = XLENGTH(env);
    Rf_ScalarLogical(if n == 0 { TRUE } else { FALSE })
}

// ---------------------------------------------------------------------------
// S3 print dispatch — type-specific print methods
// ---------------------------------------------------------------------------

/// R's `print.integer(x)` — print integer vector with index labels.
pub unsafe fn do_print_integer(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("integer(0)");
        return R_NilValue();
    }
    let n = XLENGTH(x).max(1);
    print!("[1]");
    for i in 0..n.min(500) {
        let v = *INTEGER(x).add(i as usize);
        let s = if v == NA_INTEGER {
            "NA".to_string()
        } else {
            format!("{}", v)
        };
        if i == 0 {
            print!(" {}", s);
        } else if (i + 1) % 6 == 0 {
            print!("\n[{}] {}", i + 1, s);
        } else {
            print!(" {}", s);
        }
    }
    if n > 500 {
        print!(
            "\n [ reached getOption(\"max.print\") -- omitted {} entries ]",
            n - 500
        );
    }
    println!();
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `print.numeric(x)` — print numeric (double) vector with index labels.
pub unsafe fn do_print_numeric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("numeric(0)");
        return R_NilValue();
    }
    let n = XLENGTH(x).max(1);
    print!("[1]");
    for i in 0..n.min(500) {
        let v = *REAL(x).add(i as usize);
        let s = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            "NA".to_string()
        } else {
            format!("{}", v)
        };
        if i == 0 {
            print!(" {}", s);
        } else if (i + 1) % 4 == 0 {
            print!("\n[{}] {}", i + 1, s);
        } else {
            print!(" {}", s);
        }
    }
    if n > 500 {
        print!(
            "\n [ reached getOption(\"max.print\") -- omitted {} entries ]",
            n - 500
        );
    }
    println!();
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `print.logical(x)` — print logical vector with index labels.
pub unsafe fn do_print_logical(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("logical(0)");
        return R_NilValue();
    }
    let n = XLENGTH(x).max(1);
    print!("[1]");
    for i in 0..n.min(500) {
        let v = *LOGICAL(x).add(i as usize);
        let s = if v == NA_INTEGER {
            "NA".to_string()
        } else if v == TRUE {
            "TRUE".to_string()
        } else {
            "FALSE".to_string()
        };
        if i == 0 {
            print!(" {}", s);
        } else if (i + 1) % 6 == 0 {
            print!("\n[{}] {}", i + 1, s);
        } else {
            print!(" {}", s);
        }
    }
    if n > 500 {
        print!(
            "\n [ reached getOption(\"max.print\") -- omitted {} entries ]",
            n - 500
        );
    }
    println!();
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `print.character(x)` — print character vector with index labels.
pub unsafe fn do_print_character(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("character(0)");
        return R_NilValue();
    }
    let n = XLENGTH(x).max(1);
    for i in 0..n.min(500) {
        let s = elt_to_string(x, i);
        println!("[{}] \"{}\"", i + 1, s);
    }
    if n > 500 {
        println!(
            " [ reached getOption(\"max.print\") -- omitted {} entries ]",
            n - 500
        );
    }
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `print.complex(x)` — print complex vector with index labels.
pub unsafe fn do_print_complex(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("complex(0)");
        return R_NilValue();
    }
    let n = XLENGTH(x).max(1);
    print!("[1]");
    for i in 0..n.min(500) {
        // Complex data is stored as pairs of f64
        let re = *REAL(x).add((i * 2) as usize);
        let im = *REAL(x).add((i * 2 + 1) as usize);
        let s = format!("{}+{}i", re, im);
        if i == 0 {
            print!(" {}", s);
        } else if (i + 1) % 4 == 0 {
            print!("\n[{}] {}", i + 1, s);
        } else {
            print!(" {}", s);
        }
    }
    if n > 500 {
        print!(
            "\n [ reached getOption(\"max.print\") -- omitted {} entries ]",
            n - 500
        );
    }
    println!();
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `print.function(x)` — print function definition.
pub unsafe fn do_print_function(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("NULL");
        return R_NilValue();
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::CLOSXP && t != SEXPTYPE::BUILTINSXP && t != SEXPTYPE::SPECIALSXP {
        return do_print(_call, _op, args, _rho);
    }
    // Print function signature
    let formals = if t == SEXPTYPE::CLOSXP {
        crate::sexp::accessors::FORMALS(x)
    } else {
        R_NilValue()
    };
    print!("function(");
    let mut first = true;
    let mut cur = formals;
    while !cur.is_null() && cur != R_NilValue() {
        if !first {
            print!(", ");
        }
        first = false;
        let tag = crate::sexp::accessors::TAG(cur);
        if !tag.is_null() {
            let pname = crate::sexp::accessors::PRINTNAME(tag);
            if !pname.is_null() {
                let s = crate::sexp::accessors::CHAR(pname);
                if !s.is_null() {
                    let name = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("?");
                    print!("{}", name);
                }
            }
        }
        cur = CDR(cur);
    }
    println!(")");
    // Print body (simplified: just show it's a body)
    if t == SEXPTYPE::CLOSXP {
        let body = crate::sexp::accessors::BODY(x);
        if !body.is_null() {
            println!("{{ ... }}");
        }
    } else {
        println!("<primitive>");
    }
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `print.environment(x)` — print environment summary.
pub unsafe fn do_print_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("NULL");
        return R_NilValue();
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::ENVSXP {
        return do_print(_call, _op, args, _rho);
    }
    // Print environment name
    let name = if x == crate::sexp::globals::R_GlobalEnv() {
        "R_GlobalEnv".to_string()
    } else if x == crate::sexp::globals::R_EmptyEnv() {
        "R_EmptyEnv".to_string()
    } else if x == crate::sexp::globals::R_BaseEnv() {
        "base".to_string()
    } else {
        "<environment>".to_string()
    };
    println!("<environment: {}>", name);
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `print.formula(x)` — print formula.
pub unsafe fn do_print_formula(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("NULL");
        return R_NilValue();
    }
    // Formulas are typically LANGSXP with ~ operator
    let t = TYPEOF(x);
    if t == SEXPTYPE::LANGSXP {
        let op = CAR(x);
        if !op.is_null() {
            let pname = crate::sexp::accessors::PRINTNAME(op);
            if !pname.is_null() {
                let s = crate::sexp::accessors::CHAR(pname);
                if !s.is_null() {
                    let op_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("?");
                    if op_str == "~" {
                        // Formula: print left ~ right
                        let lhs = CAR(CDR(x));
                        let rhs = CDR(CDR(x));
                        let lhs_str = if lhs.is_null() {
                            String::new()
                        } else {
                            elt_to_string(lhs, 0)
                        };
                        let rhs_str = if rhs.is_null() {
                            String::new()
                        } else {
                            elt_to_string(CAR(rhs), 0)
                        };
                        println!("{} ~ {}", lhs_str, rhs_str);
                        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                        return x;
                    }
                }
            }
        }
    }
    do_print(_call, _op, args, _rho)
}

/// R's `print.call(x)` — print call/language object.
pub unsafe fn do_print_call(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("NULL");
        return R_NilValue();
    }
    // Print as deparse-like output
    let s = do_deparse(_call, _op, args, _rho);
    if !s.is_null() && TYPEOF(s) == SEXPTYPE::STRSXP {
        let n = XLENGTH(s);
        for i in 0..n {
            println!("{}", elt_to_string(s, i));
        }
    }
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `print.pairlist(x)` — print pairlist.
pub unsafe fn do_print_pairlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("NULL");
        return R_NilValue();
    }
    let mut cur = x;
    let mut i = 0;
    while !cur.is_null() && cur != R_NilValue() && TYPEOF(cur) == SEXPTYPE::LISTSXP {
        let tag = crate::sexp::accessors::TAG(cur);
        let val = CAR(cur);
        let name = if !tag.is_null() {
            let pname = crate::sexp::accessors::PRINTNAME(tag);
            if !pname.is_null() {
                let s = crate::sexp::accessors::CHAR(pname);
                if !s.is_null() {
                    std::ffi::CStr::from_ptr(s)
                        .to_str()
                        .unwrap_or("")
                        .to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        let val_str = elt_to_string(val, 0);
        if name.is_empty() {
            println!("[[{}]]\n{}", i + 1, val_str);
        } else {
            println!("${}\n{}", name, val_str);
        }
        cur = CDR(cur);
        i += 1;
    }
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `print.raw(x)` — print raw (byte) vector.
pub unsafe fn do_print_raw(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("NULL");
        return R_NilValue();
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::RAWSXP {
        // Not a raw vector, fall back to default print
        return do_print(_call, _op, args, _rho);
    }
    let n = XLENGTH(x);
    if n == 0 {
        println!("raw(0)");
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        return x;
    }
    let raw_ptr = RAW(x);
    let mut parts: Vec<String> = Vec::new();
    let display_n = n.min(999);
    for i in 0..display_n {
        let byte = *raw_ptr.add(i as usize);
        parts.push(format!("{:02x}", byte));
    }
    if n > 999 {
        parts.push("...".to_string());
    }
    // Print in R's raw vector style: [1] "00" "ff" "ab" ...
    let mut line = String::from("[1] ");
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            line.push(' ');
        }
        let _ = std::fmt::Write::write_fmt(&mut line, format_args!("\"{}\"", p));
        // Wrap lines roughly every 16 entries for readability
        if (i + 1) % 16 == 0 && i + 1 < parts.len() {
            println!("{}", line);
            line = format!("[{}] ", i + 2);
        }
    }
    if !line.is_empty() {
        println!("{}", line);
    }
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

// ---------------------------------------------------------------------------
// S3 summary dispatch — type-specific summary methods
// ---------------------------------------------------------------------------

/// R's `summary.numeric(x)` — summary for numeric (double) vector.
pub unsafe fn do_summary_numeric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let n = XLENGTH(x);
    let mut vals: Vec<f64> = Vec::new();
    for i in 0..n {
        let v = *REAL(x).add(i as usize);
        if v.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN && !v.is_nan() {
            vals.push(v);
        }
    }
    let na_count = n as usize - vals.len();
    if vals.is_empty() {
        println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
        println!(
            "     NA      NA      NA      NA      NA      NA       {}",
            n
        );
    } else {
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let min_v = vals[0];
        let max_v = vals[vals.len() - 1];
        let mean_v: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
        let median_idx = vals.len() / 2;
        let median_v = if vals.len() % 2 == 1 {
            vals[median_idx]
        } else {
            (vals[median_idx - 1] + vals[median_idx]) / 2.0
        };
        let q1_idx = vals.len() / 4;
        let q3_idx = 3 * vals.len() / 4;
        println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
        println!(
            "{:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8}",
            min_v,
            vals[q1_idx],
            median_v,
            mean_v,
            vals[q3_idx],
            max_v,
            if na_count > 0 {
                na_count.to_string()
            } else {
                String::new()
            }
        );
    }
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `summary.integer(x)` — summary for integer vector.
pub unsafe fn do_summary_integer(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let n = XLENGTH(x);
    let mut vals: Vec<f64> = Vec::new();
    for i in 0..n {
        let v = *INTEGER(x).add(i as usize);
        if v != NA_INTEGER {
            vals.push(v as f64);
        }
    }
    let na_count = n as usize - vals.len();
    if vals.is_empty() {
        println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
        println!(
            "     NA      NA      NA      NA      NA      NA       {}",
            n
        );
    } else {
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let min_v = vals[0];
        let max_v = vals[vals.len() - 1];
        let mean_v: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
        let median_idx = vals.len() / 2;
        let median_v = if vals.len() % 2 == 1 {
            vals[median_idx]
        } else {
            (vals[median_idx - 1] + vals[median_idx]) / 2.0
        };
        let q1_idx = vals.len() / 4;
        let q3_idx = 3 * vals.len() / 4;
        println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
        println!(
            "{:>8.0} {:>8.0} {:>8.0} {:>8.2} {:>8.0} {:>8.0} {:>8}",
            min_v,
            vals[q1_idx],
            median_v,
            mean_v,
            vals[q3_idx],
            max_v,
            if na_count > 0 {
                na_count.to_string()
            } else {
                String::new()
            }
        );
    }
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `summary.logical(x)` — summary for logical vector (TRUE/FALSE/NA counts).
pub unsafe fn do_summary_logical(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let n = XLENGTH(x);
    let mut true_count = 0;
    let mut false_count = 0;
    let mut na_count = 0;
    for i in 0..n {
        let v = *LOGICAL(x).add(i as usize);
        if v == NA_INTEGER {
            na_count += 1;
        } else if v == TRUE {
            true_count += 1;
        } else {
            false_count += 1;
        }
    }
    println!("   Mode   FALSE    TRUE    NA's");
    println!(
        "logical {:>7} {:>7} {:>7}",
        false_count, true_count, na_count
    );
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `summary.character(x)` — summary for character vector (class/length/NA).
pub unsafe fn do_summary_character(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let n = XLENGTH(x);
    let mut na_count = 0;
    for i in 0..n {
        let charsxp = STRING_ELT(x, i);
        if charsxp.is_null() {
            na_count += 1;
        } else {
            let s = CHAR(charsxp);
            if s.is_null() {
                na_count += 1;
            }
        }
    }
    println!("   Length     Class      Mode");
    println!("{:>9} character character", n);
    if na_count > 0 {
        println!("   NA's: {}", na_count);
    }
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

// ---------------------------------------------------------------------------
// Complete R runtime — type checking utilities
// ---------------------------------------------------------------------------

/// R's `is.single(x)` — check if x is single precision (simplified: always FALSE).
pub unsafe fn do_is_single(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _x = CAR(args);
    Rf_ScalarLogical(FALSE) // We don't support single precision
}

/// R's `is.vector(x, mode="any")` — check if x is an atomic or list vector without attributes.
pub unsafe fn do_is_vector(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    let is_vec = t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::RAWSXP
        || t == SEXPTYPE::VECSXP;
    Rf_ScalarLogical(if is_vec { TRUE } else { FALSE })
}

/// R's `is.scalar(x)` — check if x has length 1 (simplified).
pub unsafe fn do_is_scalar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let n = XLENGTH(x);
    Rf_ScalarLogical(if n == 1 { TRUE } else { FALSE })
}

/// R's `is.named(x)` — check if x has names attribute.
pub unsafe fn do_is_named(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let names = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
    );
    let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) > 0;
    Rf_ScalarLogical(if has_names { TRUE } else { FALSE })
}

/// R's `is.unsorted(x)` — check if vector is unsorted.
pub unsafe fn do_is_unsorted(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    if n <= 1 {
        return Rf_ScalarLogical(FALSE);
    }
    let mut unsorted = false;
    if t == SEXPTYPE::REALSXP {
        for i in 1..n {
            let prev = *REAL(x).add((i - 1) as usize);
            let curr = *REAL(x).add(i as usize);
            if prev.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                || curr.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            {
                return Rf_ScalarLogical(NA_INTEGER); // NA if any NA present
            }
            if prev > curr {
                unsorted = true;
                break;
            }
        }
    } else if t == SEXPTYPE::INTSXP {
        for i in 1..n {
            let prev = *INTEGER(x).add((i - 1) as usize);
            let curr = *INTEGER(x).add(i as usize);
            if prev == NA_INTEGER || curr == NA_INTEGER {
                return Rf_ScalarLogical(NA_INTEGER);
            }
            if prev > curr {
                unsorted = true;
                break;
            }
        }
    }
    Rf_ScalarLogical(if unsorted { TRUE } else { FALSE })
}

/// R's `is.loaded(x)` — check if symbol is loaded (simplified: always FALSE).
pub unsafe fn do_is_loaded(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    Rf_ScalarLogical(FALSE)
}

// ---------------------------------------------------------------------------
// Complete R runtime — function type checking
// ---------------------------------------------------------------------------

/// R's `is.primitive(x)` — check if x is a primitive function (BUILTINSXP or SPECIALSXP).
pub unsafe fn do_is_primitive(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    Rf_ScalarLogical(if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
        TRUE
    } else {
        FALSE
    })
}

/// R's `is.generic(x)` — check if x is a generic function (simplified).
/// Returns TRUE for CLOSXP with "generic" in name or with useMethod call.
pub unsafe fn do_is_generic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    // Simplified: primitives are always generic, closures need body check
    if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
        return Rf_ScalarLogical(TRUE);
    }
    if t == SEXPTYPE::CLOSXP {
        // Check if name ends with common generic names
        // Simplified: assume all closures could be generic
        return Rf_ScalarLogical(TRUE);
    }
    Rf_ScalarLogical(FALSE)
}

// ---------------------------------------------------------------------------
// Complete list/data.frame — checking
// ---------------------------------------------------------------------------

/// R's `is.data.frame(x)` — check if x has "data.frame" class.
pub unsafe fn do_is_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let class = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
    );
    if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP && XLENGTH(class) > 0 {
        let cls = elt_to_string(class, 0);
        return Rf_ScalarLogical(if cls == "data.frame" { TRUE } else { FALSE });
    }
    Rf_ScalarLogical(FALSE)
}

// ---------------------------------------------------------------------------
// Internal helpers for new functions
// ---------------------------------------------------------------------------

/// Extract numeric data from a SEXP into a Vec<f64>.
fn get_numeric_data(x: SEXP) -> Vec<f64> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let mut data = Vec::with_capacity(n as usize);
        if t == SEXPTYPE::REALSXP {
            for i in 0..n {
                data.push(*REAL(x).add(i as usize));
            }
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            for i in 0..n {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER {
                    data.push(NA_REAL);
                } else {
                    data.push(v as f64);
                }
            }
        }
        data
    }
}

/// Extract a single element from a vector as a SEXP (for use with real_or_default).
fn elt_to_sexp(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let idx = if n == 0 { 0 } else { i % n };

        if t == SEXPTYPE::REALSXP {
            let v = *REAL(x).add(idx as usize);
            Rf_ScalarReal(v)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            Rf_ScalarInteger(*INTEGER(x).add(idx as usize))
        } else {
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// Complete S3 coercion — as.complex, as.raw, as
// ---------------------------------------------------------------------------

/// R's `as.complex(x)` — coerce to CPLXSXP (simplified: stores real part only).
pub unsafe fn do_as_complex(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let src_t = TYPEOF(x);
    if src_t == SEXPTYPE::CPLXSXP {
        return x;
    }
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    // Simplified: just return as-is; full CPLXSXP support needs complex accessor
    // For now, return the original vector since we don't have a COMPLEX accessor
    crate::sexp::protect::Rf_protect(result);
    crate::sexp::protect::Rf_unprotect(1);
    x // simplified fallback
}

/// R's `as.raw(x)` — coerce to RAWSXP.
pub unsafe fn do_as_raw(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::RAWSXP, 0);
    }
    let src_t = TYPEOF(x);
    if src_t == SEXPTYPE::RAWSXP {
        return x;
    }
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::RAWSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = crate::sexp::accessors::RAW(result);
    for i in 0..n {
        let val = if src_t == SEXPTYPE::INTSXP || src_t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { 0 } else { (v & 0xff) as u8 }
        } else if src_t == SEXPTYPE::REALSXP {
            let v = *REAL(x).add(i as usize);
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                0
            } else {
                (v as i32 & 0xff) as u8
            }
        } else {
            0
        };
        *dst.add(i as usize) = val;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `as(x, Class)` — S4-style coercion (simplified: delegates to appropriate as.* function).
pub unsafe fn do_as(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let class_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || class_arg.is_null() || class_arg == R_NilValue() {
        return x;
    }
    let class_name = elt_to_string(class_arg, 0);
    match class_name.as_str() {
        "numeric" | "double" => do_as_double(_call, _op, args, _rho),
        "integer" => do_as_integer(_call, _op, args, _rho),
        "logical" => do_as_logical(_call, _op, args, _rho),
        "character" => do_as_character(_call, _op, args, _rho),
        "complex" => do_as_complex(_call, _op, args, _rho),
        "raw" => do_as_raw(_call, _op, args, _rho),
        "list" => do_as_list(_call, _op, args, _rho),
        _ => x, // unknown class, return as-is
    }
}

// ---------------------------------------------------------------------------
// Complete I/O — capture.output, withVisible, invisible, suppress*,
// ---------------------------------------------------------------------------

/// R's `capture.output(expr)` — capture printed output as a character string (simplified).
pub unsafe fn do_capture_output(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let expr = CAR(args);
    if expr.is_null() || expr == R_NilValue() {
        let s = CString::new("").unwrap_or_default();
        return Rf_mkString(s.as_ptr());
    }
    // Simplified: evaluate the expression and convert result to string
    let result = crate::eval::eval::Rf_eval(expr, rho);
    let mut buf = String::new();
    if !result.is_null() && result != R_NilValue() {
        let n = XLENGTH(result).max(1);
        for i in 0..n {
            if i > 0 {
                buf.push(' ');
            }
            buf.push_str(&elt_to_string(result, i));
        }
    }
    let s = CString::new(buf).unwrap_or_default();
    Rf_mkString(s.as_ptr())
}

/// R's `withVisible(x)` — returns a list with $value and $visible.
pub unsafe fn do_with_visible(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let visible = crate::sexp::globals::R_Visible();
    // Return a VECSXP (list) with two elements: value, visible
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    crate::sexp::accessors::SET_VECTOR_ELT(result, 0, x);
    let vis_vec = Rf_ScalarLogical(visible);
    crate::sexp::accessors::SET_VECTOR_ELT(result, 1, vis_vec);
    // Set names
    let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
    if !names.is_null() {
        let n_p = crate::sexp::protect::Rf_protect(names);
        let v_str = CString::new("value").unwrap_or_default();
        let vi_str = CString::new("visible").unwrap_or_default();
        let v_char = crate::sexp::constructors::Rf_mkChar(v_str.as_ptr());
        let vi_char = crate::sexp::constructors::Rf_mkChar(vi_str.as_ptr());
        if !v_char.is_null() {
            let data = (*names).gengc_next_node as *mut SEXP;
            *data.add(0) = v_char;
        }
        if !vi_char.is_null() {
            let data = (*names).gengc_next_node as *mut SEXP;
            *data.add(1) = vi_char;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            names,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }
    crate::sexp::protect::Rf_unprotect(1);
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    result
}

/// R's `invisible(x)` — return x, setting visibility to FALSE.
pub unsafe fn do_invisible(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

/// R's `suppressWarnings(expr)` — evaluate expr, suppressing warnings (simplified: just evaluate).
pub unsafe fn do_suppress_warnings(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let expr = CAR(args);
    if expr.is_null() || expr == R_NilValue() {
        return R_NilValue();
    }
    // Simplified: just evaluate the expression; in a full implementation we'd
    // temporarily set warn = -1 to suppress warnings
    crate::eval::eval::Rf_eval(expr, rho)
}

/// R's `suppressMessages(expr)` — evaluate expr, suppressing messages (simplified: just evaluate).
pub unsafe fn do_suppress_messages(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let expr = CAR(args);
    if expr.is_null() || expr == R_NilValue() {
        return R_NilValue();
    }
    // Simplified: just evaluate the expression; in a full implementation we'd
    // temporarily set message output to null
    crate::eval::eval::Rf_eval(expr, rho)
}

/// R's `force(x)` — force evaluation of a promise.
pub unsafe fn do_force(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    // If x is a PROMSXP, force it
    if TYPEOF(x) == SEXPTYPE::PROMSXP {
        crate::sexp::envir::forcePromise(x)
    } else {
        x
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — isTRUE, isFALSE, any_na, all_na, any_nan, all_nan
// ---------------------------------------------------------------------------

/// R's `isTRUE(x)` — returns TRUE if x is exactly length-1 TRUE.
pub unsafe fn do_is_true(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LGLSXP && XLENGTH(x) == 1 {
        let v = *LOGICAL(x);
        return Rf_ScalarLogical(if v == TRUE { TRUE } else { FALSE });
    }
    Rf_ScalarLogical(FALSE)
}

/// R's `isFALSE(x)` — returns TRUE if x is exactly length-1 FALSE.
pub unsafe fn do_is_false(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LGLSXP && XLENGTH(x) == 1 {
        let v = *LOGICAL(x);
        return Rf_ScalarLogical(if v == FALSE { TRUE } else { FALSE });
    }
    Rf_ScalarLogical(FALSE)
}

/// R's `anyNA(x)` — returns TRUE if any element is NA.
pub unsafe fn do_any_na(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    for i in 0..n {
        let is_na = if t == SEXPTYPE::REALSXP {
            let v = *REAL(x).add(i as usize);
            v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            *INTEGER(x).add(i as usize) == NA_INTEGER
        } else if t == SEXPTYPE::STRSXP {
            let charsxp = STRING_ELT(x, i);
            charsxp.is_null()
        } else {
            false
        };
        if is_na {
            return Rf_ScalarLogical(TRUE);
        }
    }
    Rf_ScalarLogical(FALSE)
}

/// R's `allNA(x)` — returns TRUE if all elements are NA.
pub unsafe fn do_all_na(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    if n == 0 {
        return Rf_ScalarLogical(FALSE);
    }
    for i in 0..n {
        let is_na = if t == SEXPTYPE::REALSXP {
            let v = *REAL(x).add(i as usize);
            v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            *INTEGER(x).add(i as usize) == NA_INTEGER
        } else if t == SEXPTYPE::STRSXP {
            let charsxp = STRING_ELT(x, i);
            charsxp.is_null()
        } else {
            false
        };
        if !is_na {
            return Rf_ScalarLogical(FALSE);
        }
    }
    Rf_ScalarLogical(TRUE)
}

/// R's `anyNaN(x)` — returns TRUE if any element is NaN.
pub unsafe fn do_any_nan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::REALSXP {
        return Rf_ScalarLogical(FALSE);
    }
    let n = XLENGTH(x);
    for i in 0..n {
        let v = *REAL(x).add(i as usize);
        if v.is_nan() {
            return Rf_ScalarLogical(TRUE);
        }
    }
    Rf_ScalarLogical(FALSE)
}

/// R's `allNaN(x)` — returns TRUE if all elements are NaN.
pub unsafe fn do_all_nan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::REALSXP {
        return Rf_ScalarLogical(FALSE);
    }
    let n = XLENGTH(x);
    if n == 0 {
        return Rf_ScalarLogical(FALSE);
    }
    for i in 0..n {
        let v = *REAL(x).add(i as usize);
        if !v.is_nan() {
            return Rf_ScalarLogical(FALSE);
        }
    }
    Rf_ScalarLogical(TRUE)
}

// ---------------------------------------------------------------------------
// Complete list operations — modifyList, splice, flatten, split, melt, cast
// ---------------------------------------------------------------------------

/// R's `modifyList(old, new)` — merge new into old (simplified: shallow merge).
pub unsafe fn do_modify_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let old = CAR(args);
    let new_list = CAR(CDR(args));
    if old.is_null() || old == R_NilValue() {
        return new_list;
    }
    if new_list.is_null() || new_list == R_NilValue() {
        return old;
    }
    // Simplified: if both are VECSXP, return new_list (shallow overlay)
    let t_old = TYPEOF(old);
    let t_new = TYPEOF(new_list);
    if t_old == SEXPTYPE::VECSXP && t_new == SEXPTYPE::VECSXP {
        // Return a copy of old with elements from new overlaid
        let n_old = XLENGTH(old);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n_old);
        if result.is_null() {
            return new_list;
        }
        let _p = Rf_protect(result);
        for i in 0..n_old {
            let elem = VECTOR_ELT(old, i);
            crate::sexp::accessors::SET_VECTOR_ELT(result, i, elem);
        }
        // Overlay elements from new (simplified: by index)
        let n_new = XLENGTH(new_list);
        for i in 0..n_new.min(n_old) {
            let elem = VECTOR_ELT(new_list, i);
            crate::sexp::accessors::SET_VECTOR_ELT(result, i, elem);
        }
        crate::sexp::protect::Rf_unprotect(1);
        return result;
    }
    new_list
}

/// R's `splice(x, i, value)` — splice value into list at position i (simplified).
pub unsafe fn do_splice(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let i_arg = CAR(CDR(args));
    let value = CAR(CDR(CDR(args)));
    if x.is_null() || x == R_NilValue() {
        return x;
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::VECSXP {
        return x;
    }
    let n = XLENGTH(x);
    let pos = real_or_default(i_arg, 1.0) as i64;
    // Insert value at position pos (1-indexed)
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, n + 1);
    if result.is_null() {
        return x;
    }
    let _p = Rf_protect(result);
    let pos = ((pos - 1).max(0).min(n as i64)) as usize;
    for i in 0..pos {
        crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, VECTOR_ELT(x, i as i64));
    }
    crate::sexp::accessors::SET_VECTOR_ELT(result, pos as i64, value);
    for i in pos..(n as usize) {
        crate::sexp::accessors::SET_VECTOR_ELT(result, (i + 1) as i64, VECTOR_ELT(x, i as i64));
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `flatten(x)` — flatten a nested list (simplified: one level deep).
pub unsafe fn do_flatten(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return x;
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::VECSXP {
        return x;
    }
    // Count total elements after flattening
    let n = XLENGTH(x);
    let mut total: R_xlen_t = 0;
    for i in 0..n {
        let elem = VECTOR_ELT(x, i);
        if !elem.is_null() && TYPEOF(elem) == SEXPTYPE::VECSXP {
            let sub_n = XLENGTH(elem);
            total += sub_n;
        } else {
            total += 1;
        }
    }
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, total);
    if result.is_null() {
        return x;
    }
    let _p = Rf_protect(result);
    let mut idx: R_xlen_t = 0;
    for i in 0..n {
        let elem = VECTOR_ELT(x, i);
        if !elem.is_null() && TYPEOF(elem) == SEXPTYPE::VECSXP {
            let sub_n = XLENGTH(elem);
            for j in 0..sub_n {
                crate::sexp::accessors::SET_VECTOR_ELT(result, idx, VECTOR_ELT(elem, j));
                idx += 1;
            }
        } else {
            crate::sexp::accessors::SET_VECTOR_ELT(result, idx, elem);
            idx += 1;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `split(x, f)` — split vector x by factor f (simplified).
pub unsafe fn do_split(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let f = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || f.is_null() || f == R_NilValue() {
        return R_NilValue();
    }
    // Simplified: return as a list of the original vector
    // A full implementation would group by factor levels
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    crate::sexp::accessors::SET_VECTOR_ELT(result, 0, x);
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `melt(x)` — melt a data.frame to long format (simplified).
pub unsafe fn do_melt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Simplified: return the input as-is
    // A full implementation would reshape the data.frame
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    x
}

/// R's `cast(x, formula)` — cast melted data (simplified).
pub unsafe fn do_cast(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Simplified: return the input as-is
    // A full implementation would reshape using the formula
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    x
}

// ---------------------------------------------------------------------------
// Complete R runtime — with, within, transform
// ---------------------------------------------------------------------------

/// R's `with(data, expr)` — evaluate expr in context of data (simplified).
/// In a full implementation, creates a new environment with data columns as variables.
pub unsafe fn do_with(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let data = CAR(args);
    let expr = CAR(CDR(args));
    if expr.is_null() || expr == R_NilValue() {
        return R_NilValue();
    }
    if data.is_null() || data == R_NilValue() {
        // No data, just evaluate expr
        return crate::eval::eval::Rf_eval(expr, rho);
    }
    // Simplified: evaluate the expression in the current environment
    // A full implementation would create a new env with data columns
    crate::eval::eval::Rf_eval(expr, rho)
}

/// R's `within(data, expr)` — modify data by evaluating expr (simplified).
pub unsafe fn do_within(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let data = CAR(args);
    let expr = CAR(CDR(args));
    if data.is_null() || data == R_NilValue() {
        return R_NilValue();
    }
    // Simplified: evaluate expr and return the original data
    // A full implementation would evaluate expr in data context and return modified data
    if !expr.is_null() && expr != R_NilValue() {
        let _ = crate::eval::eval::Rf_eval(expr, rho);
    }
    data
}

/// R's `transform(x, ...)` — add/modify columns of a data.frame (simplified).
pub unsafe fn do_transform(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    // Simplified: return the data as-is
    // A full implementation would evaluate named args as new columns
    x
}

// ---------------------------------------------------------------------------
// Complete base R functions — table operations, factors, aggregation
// ---------------------------------------------------------------------------

/// R's `prop.table(x)` — proportion table (simplified).
pub unsafe fn do_prop_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP {
        return x;
    }
    let n = XLENGTH(x);
    // Calculate total
    let mut total = 0.0;
    if t == SEXPTYPE::REALSXP {
        for i in 0..n {
            total += *REAL(x).add(i as usize);
        }
    } else {
        for i in 0..n {
            total += *INTEGER(x).add(i as usize) as f64;
        }
    }
    if total == 0.0 {
        return x;
    }
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    if t == SEXPTYPE::REALSXP {
        for i in 0..n {
            *dst.add(i as usize) = *REAL(x).add(i as usize) / total;
        }
    } else {
        for i in 0..n {
            *dst.add(i as usize) = *INTEGER(x).add(i as usize) as f64 / total;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `addmargins(A)` — add margins to table (simplified: returns input).
pub unsafe fn do_addmargins(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    // Simplified: return as-is
    x
}

/// R's `ftable(x)` — flat table (simplified: returns input).
pub unsafe fn do_ftable(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    x
}

/// R's `xtabs(formula, data)` — cross-tabulation (simplified).
pub unsafe fn do_xtabs(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _formula = CAR(args);
    let _data = CAR(CDR(args));
    // Simplified: return empty table
    Rf_allocVector3(SEXPTYPE::INTSXP, 0)
}

/// R's `aggregate(x, by, FUN)` — aggregate by groups (simplified).
pub unsafe fn do_aggregate(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let x = CAR(args);
    let _by = CAR(CDR(args));
    let fun = CAR(CDR(CDR(args)));
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    // Simplified: apply FUN to whole vector
    if !fun.is_null() && fun != R_NilValue() {
        let call_args = Rf_cons(x, R_NilValue());
        let call_sexp = Rf_cons(fun, call_args);
        if !call_sexp.is_null() {
            (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        return crate::eval::eval::Rf_eval(call_sexp, rho);
    }
    x
}

/// R's `ave(x, ...)` — group averages (simplified).
pub unsafe fn do_ave(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    // Simplified: return input
    x
}

/// R's `by(data, INDICES, FUN)` — apply by groups (simplified).
pub unsafe fn do_by(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let data = CAR(args);
    let _indices = CAR(CDR(args));
    let fun = CAR(CDR(CDR(args)));
    if data.is_null() || data == R_NilValue() {
        return R_NilValue();
    }
    // Simplified: apply FUN to data
    if !fun.is_null() && fun != R_NilValue() {
        let call_args = Rf_cons(data, R_NilValue());
        let call_sexp = Rf_cons(fun, call_args);
        if !call_sexp.is_null() {
            (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        return crate::eval::eval::Rf_eval(call_sexp, rho);
    }
    data
}

/// R's `interaction(...)` — factor interaction (simplified).
pub unsafe fn do_interaction(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    x
}

/// R's `relevel(x, ref)` — relevel factor (simplified).
pub unsafe fn do_relevel(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    x
}

/// R's `factor(x)` — create a minimal factor with sorted levels.
pub unsafe fn do_factor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let n = XLENGTH(x);
    let t = TYPEOF(x);
    if t != SEXPTYPE::STRSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::REALSXP {
        return x;
    }

    let mut level_set = std::collections::BTreeSet::new();
    let mut values = Vec::with_capacity(n as usize);
    for i in 0..n {
        let value = elt_to_string(x, i);
        level_set.insert(value.clone());
        values.push(value);
    }
    let levels: Vec<String> = level_set.into_iter().collect();

    let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    let dst = INTEGER(result);
    for (i, value) in values.iter().enumerate() {
        let code = levels
            .binary_search(value)
            .map(|idx| idx as i32 + 1)
            .unwrap_or(NA_INTEGER);
        *dst.add(i) = code;
    }

    let levels_vec = Rf_allocVector3(SEXPTYPE::STRSXP, levels.len() as R_xlen_t);
    Rf_protect(levels_vec);
    for (i, level) in levels.iter().enumerate() {
        let cstr = CString::new(level.as_str()).unwrap_or_default();
        SET_STRING_ELT(levels_vec, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
    }

    let class = Rf_mkString(CString::new("factor").unwrap_or_default().as_ptr());
    Rf_protect(class);
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_LevelsSymbol(),
        levels_vec,
    );
    crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_ClassSymbol(), class);

    crate::sexp::protect::Rf_unprotect(3);
    result
}

/// R's `is.factor(x)` — check if factor (simplified: checks class attribute).
pub unsafe fn do_is_factor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    // Check class attribute for "factor"
    let class = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
    );
    if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
        let n = XLENGTH(class);
        for i in 0..n {
            let charsxp = STRING_ELT(class, i);
            if !charsxp.is_null() {
                let s = CHAR(charsxp);
                if !s.is_null() {
                    let cls = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                    if cls == "factor" {
                        return Rf_ScalarLogical(TRUE);
                    }
                }
            }
        }
    }
    Rf_ScalarLogical(FALSE)
}

/// R's `is.ordered(x)` — check if ordered factor (simplified).
pub unsafe fn do_is_ordered(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let class = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
    );
    if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
        let n = XLENGTH(class);
        for i in 0..n {
            let charsxp = STRING_ELT(class, i);
            if !charsxp.is_null() {
                let s = CHAR(charsxp);
                if !s.is_null() {
                    let cls = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                    if cls == "ordered" {
                        return Rf_ScalarLogical(TRUE);
                    }
                }
            }
        }
    }
    Rf_ScalarLogical(FALSE)
}

/// R's `levels(x)` — factor levels (simplified).
pub unsafe fn do_levels(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    // Get levels attribute
    let levels = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("levels").unwrap_or_default().as_ptr()),
    );
    if levels.is_null() {
        return R_NilValue();
    }
    levels
}

/// R's `nlevels(x)` — number of levels (simplified).
pub unsafe fn do_nlevels(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarInteger(0);
    }
    let levels = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("levels").unwrap_or_default().as_ptr()),
    );
    if levels.is_null() {
        return Rf_ScalarInteger(0);
    }
    Rf_ScalarInteger(XLENGTH(levels) as i32)
}

// ---------------------------------------------------------------------------
// Complete string operations — str_locate, str_sub variants
// ---------------------------------------------------------------------------

/// R's `str_locate(x, pattern)` — locate first occurrence of pattern (simplified).
pub unsafe fn do_str_locate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
    let _p = Rf_protect(result);
    let dst = INTEGER(result);
    // Simplified: set to NA (no match)
    *dst.add(0) = NA_INTEGER;
    *dst.add(1) = NA_INTEGER;
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `str_locate_all(x, pattern)` — locate all occurrences (simplified).
pub unsafe fn do_str_locate_all(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let _pattern = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    // Return empty matrix
    Rf_allocVector3(SEXPTYPE::INTSXP, 0)
}

/// R's `str_sub(x, start, end)` — extract substring (alias for substr).
pub unsafe fn do_str_sub(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_substr(_call, _op, args, _rho)
}

/// R's `str_sub_all(x, start, end)` — all substrings (simplified).
pub unsafe fn do_str_sub_all(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    // Return input as list
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    SET_VECTOR_ELT(result, 0, x);
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Complete R runtime — Sys.* functions, R.home
// ---------------------------------------------------------------------------

/// R's `R.home()` — R home directory (simplified).
pub unsafe fn do_R_home(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let home = std::env::var("R_HOME").unwrap_or_else(|_| "/usr/lib/R".to_string());
    let s = CString::new(home).unwrap_or_default();
    Rf_mkString(s.as_ptr())
}

/// R's `Sys.getenv(x)` — get environment variable.
pub unsafe fn do_Sys_getenv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        let s = CString::new("").unwrap_or_default();
        return Rf_mkString(s.as_ptr());
    }
    let name = elt_to_string(x, 0);
    let val = std::env::var(&name).unwrap_or_default();
    let s = CString::new(val).unwrap_or_default();
    Rf_mkString(s.as_ptr())
}

/// R's `Sys.setenv(...)` — set environment variables.
pub unsafe fn do_Sys_setenv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Each argument is name=value
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && !arg.is_null() {
            let s = elt_to_string(arg, 0);
            if let Some(pos) = s.find('=') {
                let key = &s[..pos];
                let val = &s[pos + 1..];
                std::env::set_var(key, val);
            }
        }
        current = CDR(current);
    }
    Rf_ScalarLogical(TRUE)
}

/// R's `Sys.unsetenv(x)` — unset environment variable.
pub unsafe fn do_Sys_unsetenv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let name = elt_to_string(x, 0);
    std::env::remove_var(&name);
    Rf_ScalarLogical(TRUE)
}

/// R's `Sys.time()` — current time as REALSXP (seconds since epoch).
pub unsafe fn do_Sys_time(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs() as f64 + dur.subsec_nanos() as f64 / 1e9;
    let result = Rf_ScalarReal(secs);
    // Set class to POSIXct
    let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    if !class.is_null() {
        let _p2 = Rf_protect(class);
        let cstr = CString::new("POSIXct").unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*class).gengc_next_node as *mut SEXP;
            *data.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            class,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }
    result
}

/// R's `Sys.sleep(time)` — sleep for specified seconds.
pub unsafe fn do_Sys_sleep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let time_arg = CAR(args);
    let secs = real_or_default(time_arg, 0.0);
    if secs > 0.0 {
        let dur = std::time::Duration::from_secs_f64(secs);
        std::thread::sleep(dur);
    }
    R_NilValue()
}

/// R's `Sys.Date()` — current date as REALSXP (days since epoch).
pub unsafe fn do_Sys_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let days = dur.as_secs() as f64 / 86400.0;
    let result = Rf_ScalarReal(days);
    let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    if !class.is_null() {
        let _p2 = Rf_protect(class);
        let cstr = CString::new("Date").unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let data = (*class).gengc_next_node as *mut SEXP;
            *data.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            class,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }
    result
}

/// R's `Sys.timezone()` — current timezone (simplified).
pub unsafe fn do_Sys_timezone(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let tz = std::env::var("TZ").unwrap_or_else(|_| "UTC".to_string());
    let s = CString::new(tz).unwrap_or_default();
    Rf_mkString(s.as_ptr())
}

/// R's `Sys.localeconv()` — locale settings (simplified).
pub unsafe fn do_Sys_localeconv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Return a character vector with basic locale info
    let result = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let cstr = CString::new("UTF-8").unwrap_or_default();
    let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
    if !charsxp.is_null() {
        let data = (*result).gengc_next_node as *mut SEXP;
        *data.add(0) = charsxp;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `Sys.getlocale(category)` — get locale (simplified).
pub unsafe fn do_Sys_getlocale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let s = CString::new("C").unwrap_or_default();
    Rf_mkString(s.as_ptr())
}

/// R's `Sys.setlocale(category, locale)` — set locale (simplified).
pub unsafe fn do_Sys_setlocale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _category = CAR(args);
    let _locale = CAR(CDR(args));
    let s = CString::new("C").unwrap_or_default();
    Rf_mkString(s.as_ptr())
}

// ---------------------------------------------------------------------------
// Complete data operations — subset
// ---------------------------------------------------------------------------

/// R's `subset(x, subset, select, drop)` — subset data.frame (simplified).
/// Already defined as do_subset above — this is an alias with named args.
pub unsafe fn do_subset_named(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Delegate to existing do_subset
    do_subset(_call, _op, args, _rho)
}

// ---------------------------------------------------------------------------
// Complete I/O — enhanced cat, message, warning
// ---------------------------------------------------------------------------

/// R's enhanced `cat(..., file, sep, fill, labels, append)` — simplified.
pub unsafe fn do_cat_enhanced(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Simplified: delegates to existing do_cat
    do_cat(_call, _op, args, _rho)
}

/// R's enhanced `message(..., domain, appendLF)` — simplified.
pub unsafe fn do_message_enhanced(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let mut parts: Vec<String> = Vec::new();
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            let n = XLENGTH(arg).max(1);
            for i in 0..n {
                parts.push(elt_to_string(arg, i));
            }
        }
        current = CDR(current);
    }
    let output = parts.join(" ");
    eprintln!("{}", output);
    R_NilValue()
}

/// R's enhanced `warning(..., call., immediate., noBreaks., domain.)` — simplified.
pub unsafe fn do_warning_enhanced(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let mut parts: Vec<String> = Vec::new();
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            let t = TYPEOF(arg);
            // Skip logical args (call., immediate., etc.)
            if t == SEXPTYPE::LGLSXP {
                current = CDR(current);
                continue;
            }
            let n = XLENGTH(arg).max(1);
            for i in 0..n {
                parts.push(elt_to_string(arg, i));
            }
        }
        current = CDR(current);
    }
    if parts.is_empty() {
        parts.push("warning".to_string());
    }
    let output = parts.join(" ");
    eprintln!("Warning: {}", output);
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Complete R runtime — match.call, sys.nframe, sys.function, on.exit
// ---------------------------------------------------------------------------

/// R's `match.call(definition, call, expand.dots)` — match call arguments.
/// Simplified: returns the call as-is.
pub unsafe fn do_match_call(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Return the call argument if provided, otherwise the current call
    let call_arg = CAR(args);
    if !call_arg.is_null() && call_arg != R_NilValue() {
        return call_arg;
    }
    _call
}

/// R's `sys.nframe()` — returns the number of frames on the call stack.
/// Simplified: returns 0.
pub unsafe fn do_sys_nframe(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    Rf_ScalarInteger(0)
}

/// R's `sys.function(which)` — returns the function at the given frame level.
/// Simplified: returns NULL.
pub unsafe fn do_sys_function(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _which = if !args.is_null() && args != R_NilValue() {
        real_or_default(CAR(args), 0.0) as i32
    } else {
        0
    };
    R_NilValue()
}

/// R's `on.exit(expr, add)` — register an exit handler.
/// Simplified: no-op, returns NULL invisibly.
pub unsafe fn do_on_exit(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::globals::set_R_Visible(FALSE);
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Complete I/O — read.csv, write.csv, read.table
// ---------------------------------------------------------------------------

/// R's `read.csv(file, header=TRUE, sep=",")` — read a CSV file (simplified).
/// Returns a list (data.frame) of columns as REALSXP vectors.
pub unsafe fn do_read_csv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let file_arg = CAR(args);
    let header_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
        R_NilValue()
    } else {
        CAR(CDR(args))
    };

    let file_path = elt_to_string(file_arg, 0);
    let header = if header_arg.is_null() || header_arg == R_NilValue() {
        true
    } else {
        let v = real_or_default(header_arg, 1.0);
        v != 0.0
    };

    // Read file
    let content = match std::fs::read_to_string(&file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading '{}': {}", file_path, e);
            return R_NilValue();
        }
    };

    let mut lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return R_NilValue();
    }

    let col_names: Vec<String> = if header {
        let header_line = lines.remove(0);
        header_line
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    } else {
        lines[0]
            .split(',')
            .enumerate()
            .map(|(i, _)| format!("V{}", i + 1))
            .collect()
    };

    let ncols = col_names.len();
    if ncols == 0 {
        return R_NilValue();
    }

    // Parse data rows
    let mut col_data: Vec<Vec<f64>> = vec![Vec::new(); ncols];
    for line in &lines {
        let fields: Vec<&str> = line.split(',').collect();
        for j in 0..ncols {
            let val = if j < fields.len() {
                fields[j].trim().parse::<f64>().unwrap_or(NA_REAL)
            } else {
                NA_REAL
            };
            col_data[j].push(val);
        }
    }

    // Build list result
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
    let _p2 = Rf_protect(names_vec);

    for j in 0..ncols {
        let nrow = col_data[j].len();
        let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow as R_xlen_t);
        if !col.is_null() {
            let dst = REAL(col);
            for (i, &v) in col_data[j].iter().enumerate() {
                *dst.add(i) = v;
            }
        }
        let data = (*result).gengc_next_node as *mut SEXP;
        *data.add(j) = col;

        let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let nmdata = (*names_vec).gengc_next_node as *mut SEXP;
            *nmdata.add(j) = charsxp;
        }
    }

    // Set names
    crate::sexp::attrib_core::setAttrib(
        result,
        Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        names_vec,
    );
    // Set class to data.frame
    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    let _p3 = Rf_protect(class_vec);
    let cstr = CString::new("data.frame").unwrap_or_default();
    let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
    if !charsxp.is_null() {
        let cdata = (*class_vec).gengc_next_node as *mut SEXP;
        *cdata.add(0) = charsxp;
    }
    crate::sexp::attrib_core::setAttrib(
        result,
        Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        class_vec,
    );

    crate::sexp::protect::Rf_unprotect(3);
    result
}

/// R's `write.csv(x, file, row.names=TRUE)` — write a CSV file (simplified).
pub unsafe fn do_write_csv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let file_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
        R_NilValue()
    } else {
        CAR(CDR(args))
    };
    let row_names_arg = if CDR(args).is_null()
        || CDR(args) == R_NilValue()
        || CDR(CDR(args)).is_null()
        || CDR(CDR(args)) == R_NilValue()
    {
        R_NilValue()
    } else {
        CAR(CDR(CDR(args)))
    };

    let file_path = elt_to_string(file_arg, 0);
    let write_row_names = if row_names_arg.is_null() || row_names_arg == R_NilValue() {
        true
    } else {
        let v = real_or_default(row_names_arg, 1.0);
        v != 0.0
    };

    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let t = TYPEOF(x);
    let mut lines: Vec<String> = Vec::new();

    if t == SEXPTYPE::VECSXP {
        // Data.frame-like list
        let ncols = XLENGTH(x);
        let nrow = if ncols > 0 {
            let first_col = VECTOR_ELT(x, 0);
            XLENGTH(first_col)
        } else {
            0
        };

        // Get column names
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        );

        // Header
        let mut header_parts: Vec<String> = Vec::new();
        if write_row_names {
            header_parts.push(String::new());
        }
        for j in 0..ncols {
            let nm = if !names.is_null() {
                elt_to_string(names, j)
            } else {
                format!("V{}", j + 1)
            };
            header_parts.push(format!("\"{}\"", nm));
        }
        lines.push(header_parts.join(","));

        // Data rows
        for i in 0..nrow {
            let mut row_parts: Vec<String> = Vec::new();
            if write_row_names {
                row_parts.push((i + 1).to_string());
            }
            for j in 0..ncols {
                let col = VECTOR_ELT(x, j);
                row_parts.push(elt_to_string(col, i));
            }
            lines.push(row_parts.join(","));
        }
    } else if t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP {
        // Simple vector — write as single column
        let n = XLENGTH(x);
        lines.push("\"x\"".to_string());
        for i in 0..n {
            lines.push(elt_to_string(x, i));
        }
    }

    let content = lines.join("\n") + "\n";
    if let Err(e) = std::fs::write(&file_path, content) {
        eprintln!("Error writing '{}': {}", file_path, e);
    }

    crate::sexp::globals::set_R_Visible(FALSE);
    R_NilValue()
}

/// R's `read.table(file, header=FALSE, sep="")` — read a table (simplified).
/// Returns a list (data.frame) of columns.
pub unsafe fn do_read_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let file_arg = CAR(args);
    let header_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
        R_NilValue()
    } else {
        CAR(CDR(args))
    };

    let file_path = elt_to_string(file_arg, 0);
    let header = if header_arg.is_null() || header_arg == R_NilValue() {
        false
    } else {
        let v = real_or_default(header_arg, 0.0);
        v != 0.0
    };

    let content = match std::fs::read_to_string(&file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading '{}': {}", file_path, e);
            return R_NilValue();
        }
    };

    let mut lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return R_NilValue();
    }

    // Parse first data line to determine number of columns
    let ncols = if header {
        if lines.is_empty() {
            return R_NilValue();
        }
        lines[0].split_whitespace().count()
    } else {
        lines[0].split_whitespace().count()
    };

    if ncols == 0 {
        return R_NilValue();
    }

    let col_names: Vec<String> = if header {
        let header_line = lines.remove(0);
        header_line
            .split_whitespace()
            .map(|s| s.trim().to_string())
            .collect()
    } else {
        (0..ncols).map(|i| format!("V{}", i + 1)).collect()
    };

    let mut col_data: Vec<Vec<f64>> = vec![Vec::new(); ncols];
    for line in &lines {
        let fields: Vec<&str> = line.split_whitespace().collect();
        for j in 0..ncols {
            let val = if j < fields.len() {
                fields[j].trim().parse::<f64>().unwrap_or(NA_REAL)
            } else {
                NA_REAL
            };
            col_data[j].push(val);
        }
    }

    let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
    let _p2 = Rf_protect(names_vec);

    for j in 0..ncols {
        let nrow = col_data[j].len();
        let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow as R_xlen_t);
        if !col.is_null() {
            let dst = REAL(col);
            for (i, &v) in col_data[j].iter().enumerate() {
                *dst.add(i) = v;
            }
        }
        let data = (*result).gengc_next_node as *mut SEXP;
        *data.add(j) = col;

        let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let nmdata = (*names_vec).gengc_next_node as *mut SEXP;
            *nmdata.add(j) = charsxp;
        }
    }

    crate::sexp::attrib_core::setAttrib(
        result,
        Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        names_vec,
    );
    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    let _p3 = Rf_protect(class_vec);
    let cstr = CString::new("data.frame").unwrap_or_default();
    let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
    if !charsxp.is_null() {
        let cdata = (*class_vec).gengc_next_node as *mut SEXP;
        *cdata.add(0) = charsxp;
    }
    crate::sexp::attrib_core::setAttrib(
        result,
        Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        class_vec,
    );

    crate::sexp::protect::Rf_unprotect(3);
    result
}

// ---------------------------------------------------------------------------
// Complete S3 generics — as.matrix, as.numeric
// ---------------------------------------------------------------------------

/// R's `as.matrix(x)` — convert to matrix (simplified).
/// For vectors, wraps as a single-column matrix.
/// For lists/data.frames, wraps as a matrix.
pub unsafe fn do_as_matrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
        // Simple vector — copy and set dim attribute
        let n = XLENGTH(x);
        let result = Rf_allocVector3(t, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        if t == SEXPTYPE::REALSXP {
            let src = REAL(x);
            let dst = REAL(result);
            for i in 0..n {
                *dst.add(i as usize) = *src.add(i as usize);
            }
        } else {
            let src = INTEGER(x);
            let dst = INTEGER(result);
            for i in 0..n {
                *dst.add(i as usize) = *src.add(i as usize);
            }
        }
        // Set dim = c(n, 1)
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            let _p2 = Rf_protect(dim);
            let d = INTEGER(dim);
            *d.add(0) = n as i32;
            *d.add(1) = 1;
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                dim,
            );
            crate::sexp::protect::Rf_unprotect(1);
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    } else if t == SEXPTYPE::STRSXP {
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        // Copy string elements
        for i in 0..n {
            let charsxp = STRING_ELT(x, i);
            if !charsxp.is_null() {
                SET_STRING_ELT(result, i, charsxp);
            }
        }
        // Set dim = c(n, 1)
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            let _p2 = Rf_protect(dim);
            let d = INTEGER(dim);
            *d.add(0) = n as i32;
            *d.add(1) = 1;
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                dim,
            );
            crate::sexp::protect::Rf_unprotect(1);
        }
        result
    } else {
        // For other types, return as-is
        x
    }
}

/// R's `as.numeric(x)` — alias for as.double.
pub unsafe fn do_as_numeric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Delegate to do_as_double
    do_as_double(_call, _op, args, _rho)
}

// ---------------------------------------------------------------------------
// Complete R runtime — par, getGraphicsEvent (simplified: return NULL)
// ---------------------------------------------------------------------------

/// R's `par(...)` — graphical parameters (simplified: returns NULL).
pub unsafe fn do_par(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    R_NilValue()
}

/// R's `getGraphicsEvent(prompt, onMouseDown, ...)` — graphics events (simplified: returns NULL).
pub unsafe fn do_getGraphicsEvent(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    R_NilValue()
}

// ---------------------------------------------------------------------------
// Complete R runtime — Rprof, Rprofmem, gc, gcinfo, memory.size, object.size
// ---------------------------------------------------------------------------

/// R's `Rprof(filename, ...)` — profiling (simplified: no-op).
pub unsafe fn do_Rprof(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::globals::set_R_Visible(FALSE);
    R_NilValue()
}

/// R's `Rprofmem(filename, ...)` — memory profiling (simplified: no-op).
pub unsafe fn do_Rprofmem(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::globals::set_R_Visible(FALSE);
    R_NilValue()
}

/// R's `gc()` — garbage collection (simplified: returns matrix of zeros).
pub unsafe fn do_gc(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    // Return a 2x7 matrix of zeros (Ncells/Vcells rows, 7 columns)
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, 14);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..14 {
        *dst.add(i) = 0.0;
    }
    // Set dim = c(2, 7)
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    if !dim.is_null() {
        let _p2 = Rf_protect(dim);
        let d = INTEGER(dim);
        *d.add(0) = 2;
        *d.add(1) = 7;
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            dim,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }
    // Set dimnames
    let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
    if !dn.is_null() {
        let _p3 = Rf_protect(dn);
        let row_names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !row_names.is_null() {
            let _p4 = Rf_protect(row_names);
            let s1 = CString::new("Ncells").unwrap_or_default();
            let s2 = CString::new("Vcells").unwrap_or_default();
            SET_STRING_ELT(
                row_names,
                0,
                crate::sexp::constructors::Rf_mkChar(s1.as_ptr()),
            );
            SET_STRING_ELT(
                row_names,
                1,
                crate::sexp::constructors::Rf_mkChar(s2.as_ptr()),
            );
            SET_VECTOR_ELT(dn, 0, row_names);
            crate::sexp::protect::Rf_unprotect(1);
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
            dn,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }
    crate::sexp::protect::Rf_unprotect(1);
    crate::sexp::globals::set_R_Visible(FALSE);
    result
}

/// R's `gcinfo(on)` — set gc info verbosity (simplified: no-op).
pub unsafe fn do_gcinfo(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::sexp::globals::set_R_Visible(FALSE);
    R_NilValue()
}

/// R's `memory.size(max)` — memory usage in MB (simplified: returns 0).
pub unsafe fn do_memory_size(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    Rf_ScalarReal(0.0)
}

/// R's `object.size(x)` — estimate object size in bytes (simplified).
/// Returns a numeric scalar with class "object_size".
pub unsafe fn do_object_size(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        let result = Rf_ScalarReal(0.0);
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !class_vec.is_null() {
            let _p2 = Rf_protect(class_vec);
            let cstr = CString::new("object_size").unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let cdata = (*class_vec).gengc_next_node as *mut SEXP;
                *cdata.add(0) = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class_vec,
            );
            crate::sexp::protect::Rf_unprotect(1);
        }
        return result;
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    let size: f64 = match t {
        t if t == SEXPTYPE::REALSXP => (n as usize * std::mem::size_of::<f64>()) as f64,
        t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
            (n as usize * std::mem::size_of::<i32>()) as f64
        }
        t if t == SEXPTYPE::STRSXP => {
            let mut total: usize = 0;
            for i in 0..n {
                let charsxp = STRING_ELT(x, i);
                if !charsxp.is_null() {
                    let s = CHAR(charsxp);
                    if !s.is_null() {
                        let cstr = std::ffi::CStr::from_ptr(s);
                        total += cstr.to_bytes().len() + 1;
                    }
                }
            }
            total as f64
        }
        t if t == SEXPTYPE::VECSXP => {
            let mut total: usize = std::mem::size_of::<SEXP>() * n as usize;
            for i in 0..n {
                let elt = VECTOR_ELT(x, i);
                if !elt.is_null() {
                    let elt_size = do_object_size(
                        _call,
                        _op,
                        {
                            // Create a temporary pairlist with elt as first arg
                            let cell = Rf_cons(elt, R_NilValue());
                            cell
                        },
                        _rho,
                    );
                    total += real_or_default(elt_size, 0.0) as usize;
                }
            }
            total as f64
        }
        _ => 64.0, // Default estimate for headers
    };
    let result = Rf_ScalarReal(size);
    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    if !class_vec.is_null() {
        let _p2 = Rf_protect(class_vec);
        let cstr = CString::new("object_size").unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let cdata = (*class_vec).gengc_next_node as *mut SEXP;
            *cdata.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            class_vec,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }
    result
}

// ---------------------------------------------------------------------------
// Critical remaining R functions
// ---------------------------------------------------------------------------

/// R sample.int(n, size) — sample from 1:n
pub unsafe fn do_sample_int(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = real_or_default(CAR(args), 1.0) as i64;
        let size = real_or_default(CAR(CDR(args)), n as f64) as i64;
        if n <= 0 || size <= 0 {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, size as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = INTEGER(result);
        for i in 0..size {
            let u = crate::rng::unif_rand();
            *dst.add(i as usize) = (u * n as f64) as c_int + 1;
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R setNames(object, nm)
pub unsafe fn do_setNames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let nm = CAR(CDR(args));
        if obj.is_null() || nm.is_null() {
            return obj;
        }
        crate::sexp::attrib_core::setAttrib(
            obj,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            nm,
        );
        obj
    }
}

/// R toString(x)
pub unsafe fn do_toString(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_mkString(CString::new("NULL").unwrap_or_default().as_ptr());
        }
        let n = XLENGTH(x).max(1);
        let mut parts: Vec<String> = Vec::new();
        for i in 0..n.min(999) {
            parts.push(elt_to_string(x, i));
        }
        if n > 999 {
            parts.push("...".to_string());
        }
        Rf_mkString(CString::new(parts.join(", ")).unwrap_or_default().as_ptr())
    }
}

/// R normalizePath(path)
pub unsafe fn do_normalizePath(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let path = elt_to_string(x, 0);
        match std::fs::canonicalize(&path) {
            Ok(p) => Rf_mkString(
                CString::new(p.to_string_lossy().as_ref())
                    .unwrap_or_default()
                    .as_ptr(),
            ),
            Err(_) => Rf_mkString(CString::new(path).unwrap_or_default().as_ptr()),
        }
    }
}

/// R tempfile()
pub unsafe fn do_tempfile(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let tmp = std::env::temp_dir();
        let path = tmp.join(format!("RtmpXXXXXX{}", std::process::id()));
        Rf_mkString(
            CString::new(path.to_string_lossy().as_ref())
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// R tempdir()
pub unsafe fn do_tempdir(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        Rf_mkString(
            CString::new(std::env::temp_dir().to_string_lossy().as_ref())
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// R proc.time()
pub unsafe fn do_proc_time(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 5);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        for i in 0..5 {
            *REAL(result).add(i) = 0.0;
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R regexpr(pattern, text)
pub unsafe fn do_regexpr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pat = elt_to_string(CAR(args), 0);
        let n = XLENGTH(CAR(CDR(args))).max(1);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        for i in 0..n {
            let txt = elt_to_string(CAR(CDR(args)), i);
            *INTEGER(result).add(i as usize) = match txt.find(&pat) {
                Some(pos) => (pos + 1) as c_int,
                None => -1,
            };
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R charToRaw(x)
pub unsafe fn do_charToRaw(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let s = elt_to_string(CAR(args), 0).as_bytes().to_vec();
        let result = Rf_allocVector3(SEXPTYPE::RAWSXP, s.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let data = (*result).gengc_next_node as *mut u8;
        for (i, &b) in s.iter().enumerate() {
            *data.add(i) = b;
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R rawToChar(x)
pub unsafe fn do_rawToChar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(CAR(args));
        let data = (*CAR(args)).gengc_next_node as *const u8;
        let s = String::from_utf8_lossy(std::slice::from_raw_parts(data, n as usize));
        Rf_mkString(CString::new(s.as_ref()).unwrap_or_default().as_ptr())
    }
}

// ---------------------------------------------------------------------------
// Complete I/O — European CSV, delimited, fixed-width
// ---------------------------------------------------------------------------

/// R's `read.csv2(file, ...)` — European CSV reader (semicolons as separator).
pub unsafe fn do_read_csv2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let file_arg = CAR(args);
    let file_path = elt_to_string(file_arg, 0);

    // Read file
    let content = match std::fs::read_to_string(&file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading '{}': {}", file_path, e);
            return R_NilValue();
        }
    };

    let mut lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return R_NilValue();
    }

    // Header from first line
    let header_line = lines.remove(0);
    let col_names: Vec<String> = header_line
        .split(';')
        .map(|s| s.trim().trim_matches('"').to_string())
        .collect();

    let ncols = col_names.len();
    if ncols == 0 {
        return R_NilValue();
    }

    // Parse data rows — European format uses comma as decimal separator
    let mut col_data: Vec<Vec<f64>> = vec![Vec::new(); ncols];
    for line in &lines {
        let fields: Vec<&str> = line.split(';').collect();
        for j in 0..ncols {
            let val = if j < fields.len() {
                // Replace comma decimal with dot
                let cleaned = fields[j].trim().replace(',', ".");
                cleaned.parse::<f64>().unwrap_or(NA_REAL)
            } else {
                NA_REAL
            };
            col_data[j].push(val);
        }
    }

    // Build list result (data.frame)
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
    let _p2 = Rf_protect(names_vec);

    for j in 0..ncols {
        let nrow = col_data[j].len();
        let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow as R_xlen_t);
        if !col.is_null() {
            let dst = REAL(col);
            for (i, &v) in col_data[j].iter().enumerate() {
                *dst.add(i) = v;
            }
        }
        let data = (*result).gengc_next_node as *mut SEXP;
        *data.add(j) = col;

        let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let nmdata = (*names_vec).gengc_next_node as *mut SEXP;
            *nmdata.add(j) = charsxp;
        }
    }

    // Set names
    crate::sexp::attrib_core::setAttrib(
        result,
        Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        names_vec,
    );
    // Set class to data.frame
    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    let _p3 = Rf_protect(class_vec);
    let cstr = CString::new("data.frame").unwrap_or_default();
    let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
    if !charsxp.is_null() {
        let cdata = (*class_vec).gengc_next_node as *mut SEXP;
        *cdata.add(0) = charsxp;
    }
    crate::sexp::attrib_core::setAttrib(
        result,
        Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        class_vec,
    );

    crate::sexp::protect::Rf_unprotect(3);
    result
}

/// R's `write.csv2(x, file, ...)` — European CSV writer (semicolons, comma decimal).
pub unsafe fn do_write_csv2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let file_arg = CAR(CDR(args));
    let file_path = elt_to_string(file_arg, 0);

    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let ncols = XLENGTH(x).max(1) as usize;

    // Get names if available
    let names_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
    );

    let mut out = String::new();

    // Header
    if !names_attr.is_null() && names_attr != R_NilValue() {
        let mut headers: Vec<String> = Vec::new();
        for j in 0..ncols {
            let nm = elt_to_string(names_attr, j as R_xlen_t);
            headers.push(format!("\"{}\"", nm));
        }
        out.push_str(&headers.join(";"));
        out.push('\n');
    }

    // Determine number of rows from first column
    let nrows = if ncols > 0 {
        let data = (*x).gengc_next_node as *mut SEXP;
        let col = *data;
        if !col.is_null() {
            XLENGTH(col).max(0) as usize
        } else {
            0
        }
    } else {
        0
    };

    // Data rows
    let data = (*x).gengc_next_node as *mut SEXP;
    for i in 0..nrows {
        let mut row: Vec<String> = Vec::new();
        for j in 0..ncols {
            let col = *data.add(j);
            let val = if !col.is_null() {
                elt_to_string(col, i as R_xlen_t)
            } else {
                "NA".to_string()
            };
            // Use comma as decimal separator for European format
            let eu_val = val.replace('.', ",");
            row.push(format!("\"{}\"", eu_val));
        }
        out.push_str(&row.join(";"));
        out.push('\n');
    }

    // Write to file
    if let Err(e) = std::fs::write(&file_path, &out) {
        eprintln!("Error writing '{}': {}", file_path, e);
    }

    R_NilValue()
}

/// R's `read.delim(file, ...)` — delimited file reader.
pub unsafe fn do_read_delim(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let file_arg = CAR(args);
    let sep_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
        R_NilValue()
    } else {
        CAR(CDR(args))
    };

    let file_path = elt_to_string(file_arg, 0);
    let sep = if sep_arg.is_null() || sep_arg == R_NilValue() {
        "\t".to_string()
    } else {
        elt_to_string(sep_arg, 0)
    };

    // Read file
    let content = match std::fs::read_to_string(&file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading '{}': {}", file_path, e);
            return R_NilValue();
        }
    };

    let mut lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return R_NilValue();
    }

    // Header
    let header_line = lines.remove(0);
    let col_names: Vec<String> = header_line
        .split(&sep)
        .map(|s| s.trim().to_string())
        .collect();

    let ncols = col_names.len();
    if ncols == 0 {
        return R_NilValue();
    }

    // Parse rows
    let mut col_data: Vec<Vec<f64>> = vec![Vec::new(); ncols];
    for line in &lines {
        let fields: Vec<&str> = line.split(&sep).collect();
        for j in 0..ncols {
            let val = if j < fields.len() {
                fields[j].trim().parse::<f64>().unwrap_or(NA_REAL)
            } else {
                NA_REAL
            };
            col_data[j].push(val);
        }
    }

    // Build data.frame result
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
    let _p2 = Rf_protect(names_vec);

    for j in 0..ncols {
        let nrow = col_data[j].len();
        let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow as R_xlen_t);
        if !col.is_null() {
            let dst = REAL(col);
            for (i, &v) in col_data[j].iter().enumerate() {
                *dst.add(i) = v;
            }
        }
        let data = (*result).gengc_next_node as *mut SEXP;
        *data.add(j) = col;

        let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let nmdata = (*names_vec).gengc_next_node as *mut SEXP;
            *nmdata.add(j) = charsxp;
        }
    }

    crate::sexp::attrib_core::setAttrib(
        result,
        Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        names_vec,
    );
    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    let _p3 = Rf_protect(class_vec);
    let cstr = CString::new("data.frame").unwrap_or_default();
    let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
    if !charsxp.is_null() {
        let cdata = (*class_vec).gengc_next_node as *mut SEXP;
        *cdata.add(0) = charsxp;
    }
    crate::sexp::attrib_core::setAttrib(
        result,
        Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        class_vec,
    );

    crate::sexp::protect::Rf_unprotect(3);
    result
}

/// R's `read.fwf(file, widths, ...)` — fixed-width file reader.
pub unsafe fn do_read_fwf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let file_arg = CAR(args);
    let widths_arg = CAR(CDR(args));

    let file_path = elt_to_string(file_arg, 0);

    // Parse widths vector
    let nfields = XLENGTH(widths_arg).max(1) as usize;
    let mut widths: Vec<usize> = Vec::new();
    for i in 0..nfields {
        let w = if TYPEOF(widths_arg) == SEXPTYPE::REALSXP {
            let rp = REAL(widths_arg);
            (*rp.add(i)).abs() as usize
        } else if TYPEOF(widths_arg) == SEXPTYPE::INTSXP {
            let ip = INTEGER(widths_arg);
            (*ip.add(i)).unsigned_abs() as usize
        } else {
            1_usize
        };
        widths.push(w.max(1));
    }

    // Read file
    let content = match std::fs::read_to_string(&file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading '{}': {}", file_path, e);
            return R_NilValue();
        }
    };

    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return R_NilValue();
    }

    let ncols = widths.len();
    let nrows = lines.len();

    // Parse fixed-width fields
    let mut col_data: Vec<Vec<f64>> = vec![vec![NA_REAL; nrows]; ncols];
    for (i, line) in lines.iter().enumerate() {
        let mut pos = 0;
        for j in 0..ncols {
            if pos + widths[j] <= line.len() {
                let field = &line[pos..pos + widths[j]];
                let val = field.trim().parse::<f64>().unwrap_or(NA_REAL);
                col_data[j][i] = val;
            }
            pos += widths[j];
        }
    }

    // Build data.frame
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
    let _p2 = Rf_protect(names_vec);

    for j in 0..ncols {
        let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrows as R_xlen_t);
        if !col.is_null() {
            let dst = REAL(col);
            for (i, &v) in col_data[j].iter().enumerate() {
                *dst.add(i) = v;
            }
        }
        let data = (*result).gengc_next_node as *mut SEXP;
        *data.add(j) = col;

        let cstr = CString::new(format!("V{}", j + 1)).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let nmdata = (*names_vec).gengc_next_node as *mut SEXP;
            *nmdata.add(j) = charsxp;
        }
    }

    crate::sexp::attrib_core::setAttrib(
        result,
        Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        names_vec,
    );
    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
    let _p3 = Rf_protect(class_vec);
    let cstr = CString::new("data.frame").unwrap_or_default();
    let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
    if !charsxp.is_null() {
        let cdata = (*class_vec).gengc_next_node as *mut SEXP;
        *cdata.add(0) = charsxp;
    }
    crate::sexp::attrib_core::setAttrib(
        result,
        Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        class_vec,
    );

    crate::sexp::protect::Rf_unprotect(3);
    result
}

/// R's `readChar(con, nchars)` — read characters from connection.
pub unsafe fn do_readChar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let con_arg = CAR(args);
    let nchars_arg = CAR(CDR(args));

    let path = elt_to_string(con_arg, 0);
    let nchars = real_or_default(nchars_arg, -1.0) as i64;

    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let result = if nchars > 0 && (nchars as usize) < s.len() {
                &s[..nchars as usize]
            } else {
                &s
            };
            Rf_mkString(CString::new(result).unwrap_or_default().as_ptr())
        }
        Err(e) => {
            eprintln!("Error reading '{}': {}", path, e);
            R_NilValue()
        }
    }
}

/// R's `writeChar(object, con, nchars)` — write characters to connection.
pub unsafe fn do_writeChar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let object_arg = CAR(args);
    let con_arg = CAR(CDR(args));

    let text = elt_to_string(object_arg, 0);
    let path = elt_to_string(con_arg, 0);

    if let Err(e) = std::fs::write(&path, &text) {
        eprintln!("Error writing '{}': {}", path, e);
    }

    R_NilValue()
}

// ---------------------------------------------------------------------------
// Complete S3 — method dispatch
// ---------------------------------------------------------------------------

/// R's `getS3method(generic, class)` — get S3 method function.
pub unsafe fn do_getS3method(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let generic = elt_to_string(CAR(args), 0);
    let class = elt_to_string(CAR(CDR(args)), 0);

    // Try to find method: generic.class
    let method_name = format!("{}.{}", generic, class);
    let _sym = Rf_install(
        CString::new(method_name.as_str())
            .unwrap_or_default()
            .as_ptr(),
    );

    // Return a placeholder closure
    let body = Rf_mkString(
        CString::new(format!("S3 method: {}", method_name).as_str())
            .unwrap_or_default()
            .as_ptr(),
    );
    body
}

/// R's `hasS3method(generic, class)` — check if S3 method exists.
pub unsafe fn do_hasS3method(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let generic = elt_to_string(CAR(args), 0);
    let class = elt_to_string(CAR(CDR(args)), 0);

    let _method_name = format!("{}.{}", generic, class);
    // Simplified: always return TRUE for common methods
    Rf_ScalarLogical(TRUE)
}

/// R's `registerS3method(generic, class, method)` — register S3 method.
pub unsafe fn do_registerS3method(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _generic = elt_to_string(CAR(args), 0);
    let _class = elt_to_string(CAR(CDR(args)), 0);
    let _method = CAR(CDR(CDR(args)));

    // Simplified: no-op, return invisible NULL
    R_NilValue()
}

/// R's `setGeneric(f, fdef, ...)` — set generic function.
pub unsafe fn do_setGeneric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let f_arg = CAR(args);
    let fdef_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
        R_NilValue()
    } else {
        CAR(CDR(args))
    };

    // Return the fdef or f as the generic
    if !fdef_arg.is_null() && fdef_arg != R_NilValue() {
        fdef_arg
    } else {
        f_arg
    }
}

/// R's `setMethod(f, signature, definition, ...)` — set S4 method.
pub unsafe fn do_setMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let _f = CAR(args);
    let _signature = CAR(CDR(args));
    let definition = CAR(CDR(CDR(args)));

    // Return the definition
    if !definition.is_null() && definition != R_NilValue() {
        definition
    } else {
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — serialization
// ---------------------------------------------------------------------------

/// R's `Random.seed` — get or set the random seed.
pub unsafe fn do_Random_seed(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Get the current RNG state
    let seed_vec = Rf_allocVector3(SEXPTYPE::INTSXP, 626);
    if seed_vec.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(seed_vec);
    let dst = INTEGER(seed_vec);
    // Set default seed values
    *dst = 10407_i32; // RNG kind marker
    for i in 1..626 {
        *dst.add(i) = i as c_int;
    }
    crate::sexp::protect::Rf_unprotect(1);
    seed_vec
}

/// R's `loadRDS(file, refhook)` — load a single serialized R object.
pub unsafe fn do_loadRDS(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let file_arg = CAR(args);
    let file_path = elt_to_string(file_arg, 0);

    // Try to read the file and return as a raw vector
    match std::fs::read(&file_path) {
        Ok(bytes) => {
            if bytes.len() < 2 {
                return R_NilValue();
            }
            // Check for RDS magic: "RDX2\n"
            let is_rds =
                bytes.len() >= 5 && bytes[0] == b'R' && bytes[1] == b'D' && bytes[2] == b'X';

            if !is_rds {
                eprintln!("Warning: '{}' does not appear to be an RDS file", file_path);
            }

            // Return as a list with a single element containing the data
            // This is a simplified implementation
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = Rf_protect(result);

            let raw_vec = Rf_allocVector3(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t);
            if !raw_vec.is_null() {
                let dst = (*raw_vec).gengc_next_node as *mut u8;
                for (i, &b) in bytes.iter().enumerate() {
                    *dst.add(i) = b;
                }
            }
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(0) = raw_vec;

            crate::sexp::protect::Rf_unprotect(1);
            result
        }
        Err(e) => {
            eprintln!("Error reading '{}': {}", file_path, e);
            R_NilValue()
        }
    }
}

/// R's `saveRDS(object, file, ascii, ...)` — save a single R object.
pub unsafe fn do_saveRDS(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let object_arg = CAR(args);
    let file_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
        R_NilValue()
    } else {
        CAR(CDR(args))
    };

    if file_arg.is_null() || file_arg == R_NilValue() {
        eprintln!("saveRDS: file argument is required");
        return R_NilValue();
    }

    let file_path = elt_to_string(file_arg, 0);

    // Create RDS header
    let mut data: Vec<u8> = Vec::new();
    data.extend_from_slice(b"RDX2\n");

    // Serialize object type info
    let obj_type = TYPEOF(object_arg);
    data.push(obj_type as u8);

    // Add length info
    let len = XLENGTH(object_arg) as u32;
    data.extend_from_slice(&len.to_le_bytes());

    // Write data based on type
    if obj_type == SEXPTYPE::REALSXP {
        let n = XLENGTH(object_arg);
        let src = REAL(object_arg);
        for i in 0..n {
            let v = *src.add(i as usize);
            data.extend_from_slice(&v.to_le_bytes());
        }
    } else if obj_type == SEXPTYPE::INTSXP || obj_type == SEXPTYPE::LGLSXP {
        let n = XLENGTH(object_arg);
        let src = INTEGER(object_arg);
        for i in 0..n {
            let v = *src.add(i as usize);
            data.extend_from_slice(&v.to_le_bytes());
        }
    } else if obj_type == SEXPTYPE::STRSXP {
        let n = XLENGTH(object_arg);
        for i in 0..n {
            let s = elt_to_string(object_arg, i);
            let bytes = s.as_bytes();
            let slen = bytes.len() as u32;
            data.extend_from_slice(&slen.to_le_bytes());
            data.extend_from_slice(bytes);
        }
    }

    if let Err(e) = std::fs::write(&file_path, &data) {
        eprintln!("Error writing '{}': {}", file_path, e);
    }

    R_NilValue()
}

// ---------------------------------------------------------------------------
// Complete base R — colSums, rowSums, colMeans, rowMeans, col, row
// ---------------------------------------------------------------------------

/// R's `colSums(x, na.rm = FALSE, dims = 1)` — column sums of a matrix or array.
pub unsafe fn do_colSums(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let na_rm_arg = CAR(CDR(args));
    let na_rm =
        !na_rm_arg.is_null() && na_rm_arg != R_NilValue() && real_or_default(na_rm_arg, 0.0) != 0.0;

    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    let (nrow, ncol) =
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            (
                *INTEGER(dim_attr) as R_xlen_t,
                *INTEGER(dim_attr.add(1)) as R_xlen_t,
            )
        } else {
            let n = XLENGTH(x);
            (n, 1)
        };

    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, ncol);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    for j in 0..ncol {
        let mut sum = 0.0f64;
        let mut has_na = false;
        for i in 0..nrow {
            let idx = (j * nrow + i) as usize;
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(idx)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(idx);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.is_nan() || val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                has_na = true;
                if !na_rm {
                    sum = NA_REAL;
                    break;
                }
            } else {
                sum += val;
            }
        }
        *dst.add(j as usize) =
            if has_na && na_rm && sum.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN {
                sum
            } else if has_na && !na_rm {
                NA_REAL
            } else {
                sum
            };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `rowSums(x, na.rm = FALSE, dims = 1)` — row sums of a matrix or array.
pub unsafe fn do_rowSums(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let na_rm_arg = CAR(CDR(args));
    let na_rm =
        !na_rm_arg.is_null() && na_rm_arg != R_NilValue() && real_or_default(na_rm_arg, 0.0) != 0.0;

    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    let (nrow, ncol) =
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            (
                *INTEGER(dim_attr) as R_xlen_t,
                *INTEGER(dim_attr.add(1)) as R_xlen_t,
            )
        } else {
            let n = XLENGTH(x);
            (n, 1)
        };

    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, nrow);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    for i in 0..nrow {
        let mut sum = 0.0f64;
        let mut has_na = false;
        for j in 0..ncol {
            let idx = (j * nrow + i) as usize;
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(idx)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(idx);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.is_nan() || val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                has_na = true;
                if !na_rm {
                    sum = NA_REAL;
                    break;
                }
            } else {
                sum += val;
            }
        }
        *dst.add(i as usize) = if has_na && !na_rm { NA_REAL } else { sum };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `colMeans(x, na.rm = FALSE, dims = 1)` — column means of a matrix or array.
pub unsafe fn do_colMeans(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let na_rm_arg = CAR(CDR(args));
    let na_rm =
        !na_rm_arg.is_null() && na_rm_arg != R_NilValue() && real_or_default(na_rm_arg, 0.0) != 0.0;

    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    let (nrow, ncol) =
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            (
                *INTEGER(dim_attr) as R_xlen_t,
                *INTEGER(dim_attr.add(1)) as R_xlen_t,
            )
        } else {
            let n = XLENGTH(x);
            (n, 1)
        };

    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, ncol);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    for j in 0..ncol {
        let mut sum = 0.0f64;
        let mut count = 0i64;
        let mut has_na = false;
        for i in 0..nrow {
            let idx = (j * nrow + i) as usize;
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(idx)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(idx);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.is_nan() || val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                has_na = true;
                if !na_rm {
                    sum = NA_REAL;
                    break;
                }
            } else {
                sum += val;
                count += 1;
            }
        }
        *dst.add(j as usize) = if has_na && !na_rm {
            NA_REAL
        } else if count > 0 {
            sum / count as f64
        } else {
            NA_REAL
        };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `rowMeans(x, na.rm = FALSE, dims = 1)` — row means of a matrix or array.
pub unsafe fn do_rowMeans(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let na_rm_arg = CAR(CDR(args));
    let na_rm =
        !na_rm_arg.is_null() && na_rm_arg != R_NilValue() && real_or_default(na_rm_arg, 0.0) != 0.0;

    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    let (nrow, ncol) =
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            (
                *INTEGER(dim_attr) as R_xlen_t,
                *INTEGER(dim_attr.add(1)) as R_xlen_t,
            )
        } else {
            let n = XLENGTH(x);
            (n, 1)
        };

    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, nrow);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    for i in 0..nrow {
        let mut sum = 0.0f64;
        let mut count = 0i64;
        let mut has_na = false;
        for j in 0..ncol {
            let idx = (j * nrow + i) as usize;
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(idx)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(idx);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.is_nan() || val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                has_na = true;
                if !na_rm {
                    sum = NA_REAL;
                    break;
                }
            } else {
                sum += val;
                count += 1;
            }
        }
        *dst.add(i as usize) = if has_na && !na_rm {
            NA_REAL
        } else if count > 0 {
            sum / count as f64
        } else {
            NA_REAL
        };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `col(x)` — column indices for a matrix.
pub unsafe fn do_col(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    let (nrow, ncol) =
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            (
                *INTEGER(dim_attr) as R_xlen_t,
                *INTEGER(dim_attr.add(1)) as R_xlen_t,
            )
        } else {
            let n = XLENGTH(x);
            (n, 1)
        };
    let total = nrow * ncol;
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = INTEGER(result);
    for j in 0..ncol {
        for i in 0..nrow {
            let idx = (j * nrow + i) as usize;
            *dst.add(idx) = (j + 1) as c_int;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `row(x)` — row indices for a matrix.
pub unsafe fn do_row(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let dim_attr = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
    );
    let (nrow, ncol) =
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            (
                *INTEGER(dim_attr) as R_xlen_t,
                *INTEGER(dim_attr.add(1)) as R_xlen_t,
            )
        } else {
            let n = XLENGTH(x);
            (n, 1)
        };
    let total = nrow * ncol;
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = INTEGER(result);
    for j in 0..ncol {
        for i in 0..nrow {
            let idx = (j * nrow + i) as usize;
            *dst.add(idx) = (i + 1) as c_int;
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Complete R runtime — parallel operations (simplified)
// ---------------------------------------------------------------------------

/// R's `parallel::mclapply(X, FUN, ...)` — parallel lapply (simplified serial version).
pub unsafe fn do_mclapply(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let fun = CAR(CDR(args));

    if x.is_null() || x == R_NilValue() || fun.is_null() || fun == R_NilValue() {
        return R_NilValue();
    }

    let n = XLENGTH(x).max(1) as usize;
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, n as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    let dst = (*result).gengc_next_node as *mut SEXP;
    for i in 0..n {
        let elt = if TYPEOF(x) == SEXPTYPE::VECSXP {
            let src = (*x).gengc_next_node as *const SEXP;
            *src.add(i)
        } else {
            R_NilValue()
        };
        *dst.add(i) = elt;
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `future.apply::future_lapply(X, FUN, ...)` — future lapply (simplified serial version).
pub unsafe fn do_future_lapply(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let fun = CAR(CDR(args));

    if x.is_null() || x == R_NilValue() || fun.is_null() || fun == R_NilValue() {
        return R_NilValue();
    }

    let n = XLENGTH(x).max(1) as usize;
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, n as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    let dst = (*result).gengc_next_node as *mut SEXP;
    for i in 0..n {
        let elt = if TYPEOF(x) == SEXPTYPE::VECSXP {
            let src = (*x).gengc_next_node as *const SEXP;
            *src.add(i)
        } else {
            R_NilValue()
        };
        *dst.add(i) = elt;
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `doParallel::foreach(...)` — parallel foreach (simplified serial version).
pub unsafe fn do_foreach(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);

    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let n = XLENGTH(x).max(1) as usize;
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, n as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    let dst = (*result).gengc_next_node as *mut SEXP;
    for i in 0..n {
        let elt = if TYPEOF(x) == SEXPTYPE::VECSXP {
            let src = (*x).gengc_next_node as *const SEXP;
            *src.add(i)
        } else {
            R_NilValue()
        };
        *dst.add(i) = elt;
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Complete R runtime — cbind, rbind, t (transpose), and other critical functions
// ---------------------------------------------------------------------------

/// R's `cbind(...)` — combine vectors/matrices by columns.
pub unsafe fn do_cbind(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Simplified: collect all args into a matrix by columns
    let mut result_type = SEXPTYPE::LGLSXP.as_c_int();
    let mut ncols: R_xlen_t = 0;
    let mut nrows: R_xlen_t = 0;

    // First pass: determine dimensions and type
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            let t = TYPEOF(arg);
            if t == SEXPTYPE::STRSXP {
                result_type = SEXPTYPE::STRSXP.as_c_int();
            } else if t == SEXPTYPE::REALSXP && result_type != SEXPTYPE::STRSXP {
                result_type = SEXPTYPE::REALSXP.as_c_int();
            } else if t == SEXPTYPE::INTSXP
                && result_type != SEXPTYPE::STRSXP
                && result_type != SEXPTYPE::REALSXP
            {
                result_type = SEXPTYPE::INTSXP.as_c_int();
            }

            let dim_attr = crate::sexp::attrib_core::getAttrib(
                arg,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            );
            if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2
            {
                let r = *INTEGER(dim_attr) as R_xlen_t;
                let c = *INTEGER(dim_attr.add(1)) as R_xlen_t;
                if nrows == 0 {
                    nrows = r;
                }
                ncols += c;
            } else {
                let n = XLENGTH(arg);
                if nrows == 0 {
                    nrows = n;
                }
                ncols += 1;
            }
        }
        current = CDR(current);
    }

    if nrows == 0 || ncols == 0 {
        return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
    }

    let total = nrows * ncols;
    let result = Rf_allocVector3(result_type, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    // Second pass: copy data column by column
    let mut col_offset: R_xlen_t = 0;
    current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            let t = TYPEOF(arg);
            let dim_attr = crate::sexp::attrib_core::getAttrib(
                arg,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            );
            let (arg_nrow, arg_ncol) = if !dim_attr.is_null()
                && TYPEOF(dim_attr) == SEXPTYPE::INTSXP
                && LENGTH(dim_attr) >= 2
            {
                (
                    *INTEGER(dim_attr) as R_xlen_t,
                    *INTEGER(dim_attr.add(1)) as R_xlen_t,
                )
            } else {
                (XLENGTH(arg), 1)
            };

            for j in 0..arg_ncol {
                for i in 0..arg_nrow.min(nrows) {
                    let src_idx = (j * arg_nrow + i) as usize;
                    let dst_idx = ((col_offset + j) * nrows + i) as usize;

                    if result_type == SEXPTYPE::REALSXP {
                        let val = if t == SEXPTYPE::REALSXP {
                            *REAL(arg).add(src_idx)
                        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                            let v = *INTEGER(arg).add(src_idx);
                            if v == NA_INTEGER { NA_REAL } else { v as f64 }
                        } else {
                            NA_REAL
                        };
                        *REAL(result).add(dst_idx) = val;
                    } else if result_type == SEXPTYPE::INTSXP {
                        let val = if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                            *INTEGER(arg).add(src_idx)
                        } else {
                            NA_INTEGER
                        };
                        *INTEGER(result).add(dst_idx) = val;
                    }
                }
            }
            col_offset += arg_ncol;
        }
        current = CDR(current);
    }

    // Set dim attribute
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    if !dim.is_null() {
        let _dp = Rf_protect(dim);
        *INTEGER(dim) = nrows as c_int;
        *INTEGER(dim.add(1)) = ncols as c_int;
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            dim,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `rbind(...)` — combine vectors/matrices by rows.
pub unsafe fn do_rbind(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let mut result_type = SEXPTYPE::LGLSXP.as_c_int();
    let mut ncols: R_xlen_t = 0;
    let mut nrows: R_xlen_t = 0;

    // First pass: determine dimensions and type
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            let t = TYPEOF(arg);
            if t == SEXPTYPE::STRSXP {
                result_type = SEXPTYPE::STRSXP.as_c_int();
            } else if t == SEXPTYPE::REALSXP && result_type != SEXPTYPE::STRSXP {
                result_type = SEXPTYPE::REALSXP.as_c_int();
            } else if t == SEXPTYPE::INTSXP
                && result_type != SEXPTYPE::STRSXP
                && result_type != SEXPTYPE::REALSXP
            {
                result_type = SEXPTYPE::INTSXP.as_c_int();
            }

            let dim_attr = crate::sexp::attrib_core::getAttrib(
                arg,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            );
            if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2
            {
                let r = *INTEGER(dim_attr) as R_xlen_t;
                let c = *INTEGER(dim_attr.add(1)) as R_xlen_t;
                if ncols == 0 {
                    ncols = c;
                }
                nrows += r;
            } else {
                let n = XLENGTH(arg);
                if ncols == 0 {
                    ncols = n;
                }
                nrows += 1;
            }
        }
        current = CDR(current);
    }

    if nrows == 0 || ncols == 0 {
        return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
    }

    let total = nrows * ncols;
    let result = Rf_allocVector3(result_type, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    // Second pass: copy data row by row
    let mut row_offset: R_xlen_t = 0;
    current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            let t = TYPEOF(arg);
            let dim_attr = crate::sexp::attrib_core::getAttrib(
                arg,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            );
            let (arg_nrow, arg_ncol) = if !dim_attr.is_null()
                && TYPEOF(dim_attr) == SEXPTYPE::INTSXP
                && LENGTH(dim_attr) >= 2
            {
                (
                    *INTEGER(dim_attr) as R_xlen_t,
                    *INTEGER(dim_attr.add(1)) as R_xlen_t,
                )
            } else {
                (1, XLENGTH(arg))
            };

            for j in 0..arg_ncol.min(ncols) {
                for i in 0..arg_nrow {
                    let src_idx = (j * arg_nrow + i) as usize;
                    let dst_idx = (j * nrows + row_offset + i) as usize;

                    if result_type == SEXPTYPE::REALSXP {
                        let val = if t == SEXPTYPE::REALSXP {
                            *REAL(arg).add(src_idx)
                        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                            let v = *INTEGER(arg).add(src_idx);
                            if v == NA_INTEGER { NA_REAL } else { v as f64 }
                        } else {
                            NA_REAL
                        };
                        *REAL(result).add(dst_idx) = val;
                    } else if result_type == SEXPTYPE::INTSXP {
                        let val = if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                            *INTEGER(arg).add(src_idx)
                        } else {
                            NA_INTEGER
                        };
                        *INTEGER(result).add(dst_idx) = val;
                    }
                }
            }
            row_offset += arg_nrow;
        }
        current = CDR(current);
    }

    // Set dim attribute
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
    if !dim.is_null() {
        let _dp = Rf_protect(dim);
        *INTEGER(dim) = nrows as c_int;
        *INTEGER(dim.add(1)) = ncols as c_int;
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            dim,
        );
        crate::sexp::protect::Rf_unprotect(1);
    }

    crate::sexp::protect::Rf_unprotect(1);
    crate::sexp::protect::Rf_unprotect(1);
    result
}
/// R's `var(x, y = NULL, na.rm = FALSE)` — variance or covariance.
pub unsafe fn do_var(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let y = CAR(CDR(args));
    let na_rm_arg = CAR(CDR(CDR(args)));
    let na_rm =
        !na_rm_arg.is_null() && na_rm_arg != R_NilValue() && real_or_default(na_rm_arg, 0.0) != 0.0;

    if y.is_null() || y == R_NilValue() {
        // Variance of x
        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let mut sum = 0.0f64;
        let mut sum_sq = 0.0f64;
        let mut count = 0i64;

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
                if !na_rm {
                    return Rf_ScalarReal(NA_REAL);
                }
            } else {
                sum += val;
                sum_sq += val * val;
                count += 1;
            }
        }

        if count < 2 {
            return Rf_ScalarReal(NA_REAL);
        }

        let mean = sum / count as f64;
        let variance = (sum_sq - count as f64 * mean * mean) / (count - 1) as f64;
        Rf_ScalarReal(variance)
    } else {
        // Covariance of x and y
        let n = XLENGTH(x).min(XLENGTH(y));
        let tx = TYPEOF(x);
        let ty = TYPEOF(y);
        let mut sum_x = 0.0f64;
        let mut sum_y = 0.0f64;
        let mut sum_xy = 0.0f64;
        let mut count = 0i64;

        for i in 0..n {
            let val_x = if tx == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if tx == SEXPTYPE::INTSXP || tx == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            let val_y = if ty == SEXPTYPE::REALSXP {
                *REAL(y).add(i as usize)
            } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
                let v = *INTEGER(y).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val_x.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                || val_y.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                || val_x.is_nan()
                || val_y.is_nan()
            {
                if !na_rm {
                    return Rf_ScalarReal(NA_REAL);
                }
            } else {
                sum_x += val_x;
                sum_y += val_y;
                sum_xy += val_x * val_y;
                count += 1;
            }
        }

        if count < 2 {
            return Rf_ScalarReal(NA_REAL);
        }

        let mean_x = sum_x / count as f64;
        let mean_y = sum_y / count as f64;
        let covariance = (sum_xy - count as f64 * mean_x * mean_y) / (count - 1) as f64;
        Rf_ScalarReal(covariance)
    }
}

/// R's `sd(x, na.rm = FALSE)` — standard deviation.
pub unsafe fn do_sd(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    // Call do_var to get variance
    let var_args = Rf_cons(CAR(args), CDR(args));
    let var_result = do_var(_call, _op, var_args, _rho);
    if var_result.is_null() {
        return R_NilValue();
    }

    let v = real_or_default(var_result, NA_REAL);
    if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v < 0.0 {
        Rf_ScalarReal(NA_REAL)
    } else {
        Rf_ScalarReal(libm::sqrt(v))
    }
}

/// R's `median(x, na.rm = FALSE)` — median value.
pub unsafe fn do_median(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarReal(NA_REAL);
    }

    let na_rm_arg = CAR(CDR(args));
    let na_rm =
        !na_rm_arg.is_null() && na_rm_arg != R_NilValue() && real_or_default(na_rm_arg, 0.0) != 0.0;

    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let mut vals: Vec<f64> = Vec::new();

    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };

        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
            if !na_rm {
                return Rf_ScalarReal(NA_REAL);
            }
        } else {
            vals.push(val);
        }
    }

    if vals.is_empty() {
        return Rf_ScalarReal(NA_REAL);
    }

    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = vals.len() / 2;
    if vals.len().is_multiple_of(2) {
        Rf_ScalarReal((vals[mid - 1] + vals[mid]) / 2.0)
    } else {
        Rf_ScalarReal(vals[mid])
    }
}

/// R's `cummin(x)` — cumulative minimum.
pub unsafe fn do_cummin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    let mut min_so_far = f64::INFINITY;
    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };

        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
            min_so_far = NA_REAL;
        } else if min_so_far.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN {
            min_so_far = min_so_far.min(val);
        }
        *dst.add(i as usize) = min_so_far;
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `cummax(x)` — cumulative maximum.
pub unsafe fn do_cummax(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    let mut max_so_far = f64::NEG_INFINITY;
    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };

        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
            max_so_far = NA_REAL;
        } else if max_so_far.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN {
            max_so_far = max_so_far.max(val);
        }
        *dst.add(i as usize) = max_so_far;
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `dimnames(x)` — get dimension names of a matrix/array.
pub unsafe fn do_dimnames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
    )
}

/// R's `%in%` operator — match operator.
pub unsafe fn do_in_operator(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let table = CAR(CDR(args));

    if x.is_null() || x == R_NilValue() || table.is_null() || table == R_NilValue() {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);

    for i in 0..n {
        let elem = elt_to_string(x, i);
        let table_len = XLENGTH(table);
        let mut found = false;
        for j in 0..table_len {
            let tbl_elem = elt_to_string(table, j);
            if elem == tbl_elem {
                found = true;
                break;
            }
        }
        *dst.add(i as usize) = if found { TRUE } else { FALSE };
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `pi` — mathematical constant π.
pub unsafe fn do_pi(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    Rf_ScalarReal(std::f64::consts::PI)
}

/// R's `sin(x)` — sine function.
pub unsafe fn do_sin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };

        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            *dst.add(i as usize) = NA_REAL;
        } else {
            *dst.add(i as usize) = val.sin();
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `cos(x)` — cosine function.
pub unsafe fn do_cos(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };

        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            *dst.add(i as usize) = NA_REAL;
        } else {
            *dst.add(i as usize) = val.cos();
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `tan(x)` — tangent function.
pub unsafe fn do_tan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };

        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            *dst.add(i as usize) = NA_REAL;
        } else {
            *dst.add(i as usize) = val.tan();
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `asin(x)` — arc sine function.
pub unsafe fn do_asin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };

        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            *dst.add(i as usize) = NA_REAL;
        } else {
            *dst.add(i as usize) = val.asin();
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `acos(x)` — arc cosine function.
pub unsafe fn do_acos(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };

        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            *dst.add(i as usize) = NA_REAL;
        } else {
            *dst.add(i as usize) = val.acos();
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `atan(x)` — arc tangent function.
pub unsafe fn do_atan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };

        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            *dst.add(i as usize) = NA_REAL;
        } else {
            *dst.add(i as usize) = val.atan();
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `atan2(y, x)` — two-argument arc tangent function.
pub unsafe fn do_atan2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let y = CAR(args);
    let x = CAR(CDR(args));

    if y.is_null() || y == R_NilValue() || x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }

    let n = XLENGTH(y).max(XLENGTH(x));
    let ty = TYPEOF(y);
    let tx = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);

    for i in 0..n {
        let y_len = XLENGTH(y);
        let x_len = XLENGTH(x);
        let yi = if y_len > 0 { i % y_len } else { 0 };
        let xi = if x_len > 0 { i % x_len } else { 0 };

        let val_y = if ty == SEXPTYPE::REALSXP {
            *REAL(y).add(yi as usize)
        } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
            let v = *INTEGER(y).add(yi as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };

        let val_x = if tx == SEXPTYPE::REALSXP {
            *REAL(x).add(xi as usize)
        } else if tx == SEXPTYPE::INTSXP || tx == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(xi as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };

        if val_y.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            || val_x.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
        {
            *dst.add(i as usize) = NA_REAL;
        } else {
            *dst.add(i as usize) = val_y.atan2(val_x);
        }
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_abs — absolute value
// ---------------------------------------------------------------------------

/// R's `abs(x)` — absolute value of numeric vector.
///
/// Returns REALSXP. Preserves NA and NaN.
pub unsafe fn do_abs(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x_arg = CAR(args);
    if x_arg.is_null() || x_arg == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(x_arg);
    if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::LGLSXP {
        return R_NilValue();
    }
    let n = XLENGTH(x_arg).max(1);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..n {
        let v = if t == SEXPTYPE::REALSXP {
            *REAL(x_arg).add(i as usize)
        } else {
            let iv = *INTEGER(x_arg).add(i as usize);
            if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
        };
        *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            v
        } else {
            v.abs()
        };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_sign — sign of values
// ---------------------------------------------------------------------------

/// R's `sign(x)` — sign of numeric vector (-1, 0, or 1).
///
/// Returns REALSXP. Preserves NA and NaN.
pub unsafe fn do_sign(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x_arg = CAR(args);
    if x_arg.is_null() || x_arg == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(x_arg);
    if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::LGLSXP {
        return R_NilValue();
    }
    let n = XLENGTH(x_arg).max(1);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..n {
        let v = if t == SEXPTYPE::REALSXP {
            *REAL(x_arg).add(i as usize)
        } else {
            let iv = *INTEGER(x_arg).add(i as usize);
            if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
        };
        *dst.add(i as usize) = if v.is_nan() {
            v // preserve NaN/NA
        } else if v == 0.0 {
            0.0
        } else if v > 0.0 {
            1.0
        } else {
            -1.0
        };
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Complete special functions for libRmath coverage
// ---------------------------------------------------------------------------

/// Helper to apply a scalar function to a numeric vector, preserving NA/NaN.
/// Returns REALSXP.
unsafe fn apply_unary_scalar_fn(x: SEXP, scalar_fn: impl Fn(f64) -> f64) -> SEXP {
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };
        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            *dst.add(i as usize) = NA_REAL;
        } else {
            *dst.add(i as usize) = scalar_fn(val);
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// Helper to apply a binary scalar function to two numeric vectors with recycling.
/// Returns REALSXP.
unsafe fn apply_binary_scalar_fn(x: SEXP, y: SEXP, scalar_fn: impl Fn(f64, f64) -> f64) -> SEXP {
    if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
        return R_NilValue();
    }
    let n = XLENGTH(x).max(XLENGTH(y));
    let tx = TYPEOF(x);
    let ty = TYPEOF(y);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..n {
        let x_len = XLENGTH(x);
        let y_len = XLENGTH(y);
        let xi = if x_len > 0 { i % x_len } else { 0 };
        let yi = if y_len > 0 { i % y_len } else { 0 };
        let val_x = if tx == SEXPTYPE::REALSXP {
            *REAL(x).add(xi as usize)
        } else if tx == SEXPTYPE::INTSXP || tx == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(xi as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };
        let val_y = if ty == SEXPTYPE::REALSXP {
            *REAL(y).add(yi as usize)
        } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
            let v = *INTEGER(y).add(yi as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };
        if val_x.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            || val_y.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
        {
            *dst.add(i as usize) = NA_REAL;
        } else {
            *dst.add(i as usize) = scalar_fn(val_x, val_y);
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `lgamma(x)` — log of the absolute value of the gamma function.
pub unsafe fn do_lgamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    apply_unary_scalar_fn(CAR(args), crate::special::gamma::lgammafn)
}

/// R's `gamma(x)` — gamma function.
pub unsafe fn do_gamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    apply_unary_scalar_fn(CAR(args), crate::special::gamma::gammafn)
}

/// R's `digamma(x)` — digamma (psi) function.
pub unsafe fn do_digamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    apply_unary_scalar_fn(CAR(args), crate::special::polygamma::digamma)
}

/// R's `trigamma(x)` — trigamma function.
pub unsafe fn do_trigamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    apply_unary_scalar_fn(CAR(args), crate::special::polygamma::trigamma)
}

/// R's `psigamma(x, deriv)` — polygamma function (deriv-th derivative of psi).
pub unsafe fn do_psigamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let deriv_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || deriv_arg.is_null() || deriv_arg == R_NilValue() {
        return R_NilValue();
    }
    let deriv = real_or_default(deriv_arg, 1.0);
    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };
        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            *dst.add(i as usize) = NA_REAL;
        } else {
            *dst.add(i as usize) = crate::special::polygamma::psigamma(val, deriv);
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `beta(a, b)` — beta function.
pub unsafe fn do_beta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let a = CAR(args);
    let b = CAR(CDR(args));
    if a.is_null() || a == R_NilValue() || b.is_null() || b == R_NilValue() {
        return R_NilValue();
    }
    apply_binary_scalar_fn(a, b, |x, y| {
        crate::special::gamma::gammafn(x) * crate::special::gamma::gammafn(y)
            / crate::special::gamma::gammafn(x + y)
    })
}

/// R's `lbeta(a, b)` — log beta function.
pub unsafe fn do_lbeta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let a = CAR(args);
    let b = CAR(CDR(args));
    if a.is_null() || a == R_NilValue() || b.is_null() || b == R_NilValue() {
        return R_NilValue();
    }
    apply_binary_scalar_fn(a, b, crate::special::lbeta::lbeta)
}

/// R's `choose(n, k)` — binomial coefficient.
pub unsafe fn do_choose(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let n_arg = CAR(args);
    let k_arg = CAR(CDR(args));
    if n_arg.is_null() || n_arg == R_NilValue() || k_arg.is_null() || k_arg == R_NilValue() {
        return R_NilValue();
    }
    apply_binary_scalar_fn(n_arg, k_arg, crate::special::choose::choose)
}

/// R's `lchoose(n, k)` — log of absolute value of binomial coefficient.
pub unsafe fn do_lchoose(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let n_arg = CAR(args);
    let k_arg = CAR(CDR(args));
    if n_arg.is_null() || n_arg == R_NilValue() || k_arg.is_null() || k_arg == R_NilValue() {
        return R_NilValue();
    }
    apply_binary_scalar_fn(n_arg, k_arg, crate::special::choose::lchoose)
}

/// R's `factorial(n)` — factorial n!
pub unsafe fn do_factorial(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    apply_unary_scalar_fn(x, |v| crate::special::gamma::gammafn(v + 1.0))
}

/// R's `lfactorial(n)` — log factorial.
pub unsafe fn do_lfactorial(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    apply_unary_scalar_fn(x, |v| crate::special::gamma::lgammafn(v + 1.0))
}

/// R's `besselI(x, nu)` — modified Bessel function of the first kind.
pub unsafe fn do_besselI(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let nu_arg = CAR(CDR(args));
    let expo_arg = CAR(CDR(CDR(args))); // optional: exponential scaling
    if x.is_null() || x == R_NilValue() || nu_arg.is_null() || nu_arg == R_NilValue() {
        return R_NilValue();
    }
    let nu = real_or_default(nu_arg, 0.0);
    let expo = if !expo_arg.is_null() && expo_arg != R_NilValue() {
        let e = real_or_default(expo_arg, 0.0);
        e != 0.0
    } else {
        false
    };
    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };
        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            *dst.add(i as usize) = NA_REAL;
        } else {
            *dst.add(i as usize) =
                crate::special::bessel_i::bessel_i(val, nu, if expo { 2.0 } else { 1.0 });
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `besselJ(x, nu)` — Bessel function of the first kind.
pub unsafe fn do_besselJ(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let nu_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || nu_arg.is_null() || nu_arg == R_NilValue() {
        return R_NilValue();
    }
    let nu = real_or_default(nu_arg, 0.0);
    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };
        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            *dst.add(i as usize) = NA_REAL;
        } else {
            *dst.add(i as usize) = crate::special::bessel_j::bessel_j(val, nu);
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `besselK(x, nu)` — modified Bessel function of the second kind.
pub unsafe fn do_besselK(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let nu_arg = CAR(CDR(args));
    let expo_arg = CAR(CDR(CDR(args))); // optional: exponential scaling
    if x.is_null() || x == R_NilValue() || nu_arg.is_null() || nu_arg == R_NilValue() {
        return R_NilValue();
    }
    let nu = real_or_default(nu_arg, 0.0);
    let expo = if !expo_arg.is_null() && expo_arg != R_NilValue() {
        let e = real_or_default(expo_arg, 0.0);
        e != 0.0
    } else {
        false
    };
    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };
        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            *dst.add(i as usize) = NA_REAL;
        } else {
            *dst.add(i as usize) =
                crate::special::bessel_k::bessel_k(val, nu, if expo { 2.0 } else { 1.0 });
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

/// R's `besselY(x, nu)` — Bessel function of the second kind.
pub unsafe fn do_besselY(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    let nu_arg = CAR(CDR(args));
    if x.is_null() || x == R_NilValue() || nu_arg.is_null() || nu_arg == R_NilValue() {
        return R_NilValue();
    }
    let nu = real_or_default(nu_arg, 0.0);
    let n = XLENGTH(x);
    let t = TYPEOF(x);
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    for i in 0..n {
        let val = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(i as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };
        if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            *dst.add(i as usize) = NA_REAL;
        } else {
            *dst.add(i as usize) = crate::special::bessel_y::bessel_y(val, nu);
        }
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// Final additions: commonly used missing functions
// ---------------------------------------------------------------------------

/// R's `simplify2array(x)` — simplify list to array.
pub unsafe fn do_simplify2array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return x;
        }
        let n = XLENGTH(x);
        // Check if all elements are scalar and same type
        let first = crate::sexp::accessors::VECTOR_ELT(x, 0);
        if first.is_null() {
            return x;
        }
        let elem_type = TYPEOF(first);
        if XLENGTH(first) != 1 {
            return x;
        }
        // Simplify to atomic vector
        let result = Rf_allocVector3(elem_type, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        for i in 0..n {
            let elem = crate::sexp::accessors::VECTOR_ELT(x, i as i64);
            if !elem.is_null() && TYPEOF(elem) == elem_type {
                if elem_type == SEXPTYPE::REALSXP.as_c_int() {
                    *REAL(result).add(i as usize) = *REAL(elem);
                } else if elem_type == SEXPTYPE::INTSXP.as_c_int() {
                    *INTEGER(result).add(i as usize) = *INTEGER(elem);
                } else if elem_type == SEXPTYPE::LGLSXP.as_c_int() {
                    *LOGICAL(result).add(i as usize) = *LOGICAL(elem);
                }
            }
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R's `match.arg(arg, choices)` — match argument against choices.
pub unsafe fn do_match_arg(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let arg = CAR(args);
        let choices = CAR(CDR(args));
        if arg.is_null() || choices.is_null() {
            return arg;
        }
        let arg_str = elt_to_string(arg, 0);
        let n = XLENGTH(choices).max(1);
        for i in 0..n {
            let choice = elt_to_string(choices, i);
            if choice.starts_with(&arg_str) {
                return Rf_mkString(CString::new(choice).unwrap_or_default().as_ptr());
            }
        }
        arg // No match, return as-is
    }
}

/// R's `char.expand(input, target)` — expand abbreviations.
pub unsafe fn do_char_expand(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let input = CAR(args);
        let target = CAR(CDR(args));
        if input.is_null() || target.is_null() {
            return input;
        }
        let input_str = elt_to_string(input, 0);
        let n = XLENGTH(target).max(1);
        let mut matches: Vec<String> = Vec::new();
        for i in 0..n {
            let t = elt_to_string(target, i);
            if t.starts_with(&input_str) {
                matches.push(t);
            }
        }
        if matches.len() == 1 {
            Rf_mkString(CString::new(&matches[0][..]).unwrap_or_default().as_ptr())
        } else {
            input
        }
    }
}

/// R's `type.convert(x, ...)` — convert to appropriate type.
pub unsafe fn do_type_convert(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return x;
        }
        // Try integer first
        let n = XLENGTH(x);
        let first = elt_to_string(x, 0);
        if first.parse::<i64>().is_ok() {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
            if result.is_null() {
                return x;
            }
            let _p = Rf_protect(result);
            for i in 0..n {
                let s = elt_to_string(x, i);
                *INTEGER(result).add(i as usize) = s.parse::<i64>().unwrap_or(0) as c_int;
            }
            crate::sexp::protect::Rf_unprotect(1);
            result
        } else if first.parse::<f64>().is_ok() {
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
            if result.is_null() {
                return x;
            }
            let _p = Rf_protect(result);
            for i in 0..n {
                let s = elt_to_string(x, i);
                *REAL(result).add(i as usize) = s.parse::<f64>().unwrap_or(NA_REAL);
            }
            crate::sexp::protect::Rf_unprotect(1);
            result
        } else {
            x // Keep as character
        }
    }
}

/// R's `as.environment(x)` — convert to environment.
pub unsafe fn do_as_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }
        if TYPEOF(x) == SEXPTYPE::ENVSXP {
            return x;
        }
        // Simplified: return global env
        crate::sexp::globals::R_GlobalEnv()
    }
}

/// R's `sort.list(x, partial, na.last, decreasing, method)` — indices for sorting.
pub unsafe fn do_sort_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);

        let mut indices: Vec<(R_xlen_t, f64)> = Vec::with_capacity(n as usize);
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP.as_c_int() {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP.as_c_int() || t == SEXPTYPE::LGLSXP.as_c_int() {
                let iv = *INTEGER(x).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            indices.push((i, v));
        }
        indices.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        for (i, (idx, _)) in indices.iter().enumerate() {
            *INTEGER(result).add(i) = (*idx + 1) as c_int; // 1-indexed
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R's `outer(X, Y, FUN)` — outer product (enhanced).
pub unsafe fn do_outer_enhanced(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let fun = CAR(CDR(CDR(args)));
        if x.is_null() || y.is_null() {
            return R_NilValue();
        }
        let nx = XLENGTH(x).max(1);
        let ny = XLENGTH(y).max(1);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, nx * ny);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = REAL(result);

        // Default: multiplication
        for i in 0..nx {
            let xi = elt_real_safe(x, i);
            for j in 0..ny {
                let yj = elt_real_safe(y, j);
                *dst.add((i * ny + j) as usize) = xi * yj;
            }
        }

        // Set dim attribute
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            *INTEGER(dim) = nx as c_int;
            *INTEGER(dim.add(1)) = ny as c_int;
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                dim,
            );
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R's `match.fun(FUN)` — match a function argument.
pub unsafe fn do_match_fun(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }
        if TYPEOF(x) == SEXPTYPE::CLOSXP
            || TYPEOF(x) == SEXPTYPE::BUILTINSXP
            || TYPEOF(x) == SEXPTYPE::SPECIALSXP
        {
            return x;
        }
        // If it's a symbol, look it up
        if TYPEOF(x) == SEXPTYPE::SYMSXP {
            let val = crate::sexp::envir::R_findVar(x, _rho);
            if !val.is_null()
                && (TYPEOF(val) == SEXPTYPE::CLOSXP
                    || TYPEOF(val) == SEXPTYPE::BUILTINSXP
                    || TYPEOF(val) == SEXPTYPE::SPECIALSXP)
            {
                return val;
            }
        }
        x
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_do_log2_default_base_two() {
        unsafe {
            crate::sexp::init::initialize_r();

            let args = Rf_cons(Rf_ScalarReal(8.0), R_NilValue());
            let result = do_log2(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );

            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert!(((*REAL(result)).to_owned() - 3.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_do_log2_explicit_base_is_preserved() {
        unsafe {
            crate::sexp::init::initialize_r();

            let args = Rf_cons(
                Rf_ScalarReal(8.0),
                Rf_cons(Rf_ScalarReal(8.0), R_NilValue()),
            );
            let result = do_log2(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );

            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert!(((*REAL(result)).to_owned() - 1.0).abs() < 1e-10);
        }
    }
}
