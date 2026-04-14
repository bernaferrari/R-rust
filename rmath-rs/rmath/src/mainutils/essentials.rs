//! Essential R built-in functions — c(), seq(), rep(), paste(), cat(), typeof(), is.na(), names().
//!
//! These are the most fundamental R functions that every R program uses.

#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::CString;
use std::os::raw::c_int;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    CAR, CDR, INTEGER, LENGTH, LOGICAL, REAL, SET_STRING_ELT, SET_VECTOR_ELT, TYPEOF,
    VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkString,
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
    let mut result_type = SEXPTYPE::LGLSXP.0;
    let mut total_len: R_xlen_t = 0;
    let mut current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            let t = TYPEOF(arg);
            if t == SEXPTYPE::STRSXP.0 {
                result_type = SEXPTYPE::STRSXP.0;
            } else if t == SEXPTYPE::CPLXSXP.0 && result_type != SEXPTYPE::STRSXP.0 {
                result_type = SEXPTYPE::CPLXSXP.0;
            } else if t == SEXPTYPE::REALSXP.0
                && result_type != SEXPTYPE::STRSXP.0
                && result_type != SEXPTYPE::CPLXSXP.0
            {
                result_type = SEXPTYPE::REALSXP.0;
            } else if t == SEXPTYPE::INTSXP.0
                && result_type != SEXPTYPE::STRSXP.0
                && result_type != SEXPTYPE::CPLXSXP.0
                && result_type != SEXPTYPE::REALSXP.0
            {
                result_type = SEXPTYPE::INTSXP.0;
            }
            total_len += XLENGTH(arg);
        }
        current = CDR(current);
    }

    if total_len == 0 {
        return Rf_allocVector3(SEXPTYPE::LGLSXP.0, 0);
    }

    // Second pass: copy data
    let result = Rf_allocVector3(result_type, total_len);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let mut offset: R_xlen_t = 0;

    current = args;
    while !current.is_null() && current != R_NilValue() {
        let arg = CAR(current);
        if !arg.is_null() && arg != R_NilValue() {
            let t = TYPEOF(arg);
            let n = XLENGTH(arg);

            if result_type == SEXPTYPE::REALSXP.0 {
                let dst = REAL(result);
                for i in 0..n {
                    let val = if t == SEXPTYPE::REALSXP.0 {
                        *REAL(arg).add(i as usize)
                    } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
                        let v = *INTEGER(arg).add(i as usize);
                        if v == NA_INTEGER { NA_REAL } else { v as f64 }
                    } else {
                        NA_REAL
                    };
                    *dst.add((offset + i) as usize) = val;
                }
            } else if result_type == SEXPTYPE::INTSXP.0 {
                let dst = INTEGER(result);
                for i in 0..n {
                    let val = if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
                        *INTEGER(arg).add(i as usize)
                    } else {
                        NA_INTEGER
                    };
                    *dst.add((offset + i) as usize) = val;
                }
            } else if result_type == SEXPTYPE::LGLSXP.0 {
                let dst = LOGICAL(result);
                for i in 0..n {
                    let val = if t == SEXPTYPE::LGLSXP.0 || t == SEXPTYPE::INTSXP.0 {
                        *INTEGER(arg).add(i as usize)
                    } else {
                        NA_INTEGER
                    };
                    *dst.add((offset + i) as usize) = val;
                }
            }
            // STRSXP and CPLXSXP require STRING_ELT/COMPLEX which need more work
            offset += n;
        }
        current = CDR(current);
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

    let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
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
        if t == SEXPTYPE::REALSXP.0 {
            let src = REAL(x);
            let dst = REAL(result);
            for i in 0..n {
                *dst.add((offset + i) as usize) = *src.add(i as usize);
            }
        } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
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
// do_paste / do_paste0 — string concatenation
// ---------------------------------------------------------------------------

/// R's `paste(..., sep=" ")` — concatenates vectors element-wise with recycling.
pub unsafe fn do_paste(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_paste_impl(args, " ")
}

/// R's `paste0(...)` — same as paste with sep="".
pub unsafe fn do_paste0(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_paste_impl(args, "")
}

unsafe fn do_paste_impl(args: SEXP, sep: &str) -> SEXP {
    // Collect all args, find max length
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

    if arg_vecs.is_empty() {
        let s = CString::new("").unwrap_or_default();
        return Rf_mkString(s.as_ptr());
    }
    if max_len == 0 {
        max_len = 1;
    }

    let result = Rf_allocVector3(SEXPTYPE::STRSXP.0, max_len);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    for i in 0..max_len {
        let mut parts: Vec<String> = Vec::new();
        for &arg in &arg_vecs {
            let n = XLENGTH(arg);
            let idx = if n == 0 { 0 } else { i % n };
            let s = elt_to_string(arg, idx);
            parts.push(s);
        }
        let joined = parts.join(sep);
        let cstr = CString::new(joined).unwrap_or_default();
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
// do_cat — print to stdout
// ---------------------------------------------------------------------------

/// R's `cat(..., sep=" ")` — prints args to stdout without trailing newline.
pub unsafe fn do_cat(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
    print!("{}", output);
    R_NilValue()
}

// ---------------------------------------------------------------------------
// do_print — basic print
// ---------------------------------------------------------------------------

/// R's `print(x)` — basic print with newline. Returns x invisibly.
pub unsafe fn do_print(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        println!("NULL");
        return R_NilValue();
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x).max(1);
    for i in 0..n {
        let s = elt_to_string(x, i);
        if i == 0 {
            println!("[1] {}", s);
        } else {
            println!("[{}] {}", i + 1, s);
        }
    }
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    x
}

// ---------------------------------------------------------------------------
// do_typeof — type name
// ---------------------------------------------------------------------------

/// R's `typeof(x)` — returns the type name as STRSXP.
pub unsafe fn do_typeof(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        let s = CString::new("NULL").unwrap_or_default();
        return Rf_mkString(s.as_ptr());
    }
    let name = match TYPEOF(x) {
        t if t == SEXPTYPE::LGLSXP.0 => "logical",
        t if t == SEXPTYPE::INTSXP.0 => "integer",
        t if t == SEXPTYPE::REALSXP.0 => "double",
        t if t == SEXPTYPE::CPLXSXP.0 => "complex",
        t if t == SEXPTYPE::STRSXP.0 => "character",
        t if t == SEXPTYPE::VECSXP.0 => "list",
        t if t == SEXPTYPE::LISTSXP.0 => "pairlist",
        t if t == SEXPTYPE::LANGSXP.0 => "language",
        t if t == SEXPTYPE::SYMSXP.0 => "symbol",
        t if t == SEXPTYPE::CLOSXP.0 => "closure",
        t if t == SEXPTYPE::BUILTINSXP.0 => "builtin",
        t if t == SEXPTYPE::SPECIALSXP.0 => "special",
        t if t == SEXPTYPE::ENVSXP.0 => "environment",
        t if t == SEXPTYPE::NILSXP.0 => "NULL",
        t if t == SEXPTYPE::CHARSXP.0 => "character",
        _ => "unknown",
    };
    let s = CString::new(name).unwrap_or_default();
    Rf_mkString(s.as_ptr())
}

// ---------------------------------------------------------------------------
// do_is_na — check for NA
// ---------------------------------------------------------------------------

/// R's `is.na(x)` — returns LGLSXP with TRUE for NA elements.
pub unsafe fn do_is_na(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP.0, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);

    for i in 0..n {
        let is_na = if t == SEXPTYPE::REALSXP.0 {
            let v = *REAL(x).add(i as usize);
            v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
        } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
            *INTEGER(x).add(i as usize) == NA_INTEGER
        } else {
            false
        };
        *dst.add(i as usize) = if is_na { TRUE } else { FALSE };
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_names — get/set names attribute
// ---------------------------------------------------------------------------

/// R's `names(x)` — returns the names attribute.
pub unsafe fn do_names(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    // Get names attribute
    let names = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
    );
    if names.is_null() {
        return R_NilValue();
    }
    names
}

// ---------------------------------------------------------------------------
// do_which — find TRUE indices
// ---------------------------------------------------------------------------

/// R's `which(x)` — returns indices of TRUE elements in a logical vector.
pub unsafe fn do_which(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::INTSXP.0, 0);
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::LGLSXP.0 && t != SEXPTYPE::INTSXP.0 {
        return Rf_allocVector3(SEXPTYPE::INTSXP.0, 0);
    }

    let n = XLENGTH(x);
    let mut indices: Vec<i32> = Vec::new();
    for i in 0..n {
        let v = *INTEGER(x).add(i as usize);
        if v != 0 && v != NA_INTEGER {
            indices.push((i + 1) as i32); // R is 1-indexed
        }
    }

    let result = Rf_allocVector3(SEXPTYPE::INTSXP.0, indices.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = INTEGER(result);
    for (i, &idx) in indices.iter().enumerate() {
        *dst.add(i) = idx;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_ifelse — vectorized conditional
// ---------------------------------------------------------------------------

/// R's `ifelse(test, yes, no)` — vectorized if/else with recycling.
pub unsafe fn do_ifelse(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let test = CAR(args);
    let yes = CAR(CDR(args));
    let no = CAR(CDR(CDR(args)));

    if test.is_null() || yes.is_null() || no.is_null() {
        return R_NilValue();
    }

    let n = XLENGTH(test).max(XLENGTH(yes)).max(XLENGTH(no));
    if n == 0 {
        return R_NilValue();
    }

    // Use REALSXP for result (can handle all types)
    let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = REAL(result);
    let test_n = XLENGTH(test);
    let yes_n = XLENGTH(yes);
    let no_n = XLENGTH(no);

    for i in 0..n {
        let test_idx = if test_n == 0 { 0 } else { i % test_n };
        let cond = if TYPEOF(test) == SEXPTYPE::LGLSXP.0 {
            *LOGICAL(test).add(test_idx as usize) != 0
        } else if TYPEOF(test) == SEXPTYPE::INTSXP.0 {
            *INTEGER(test).add(test_idx as usize) != 0
        } else {
            false
        };

        let src = if cond { yes } else { no };
        let src_n = if cond { yes_n } else { no_n };
        let src_idx = if src_n == 0 { 0 } else { i % src_n };

        let val = if TYPEOF(src) == SEXPTYPE::REALSXP.0 {
            *REAL(src).add(src_idx as usize)
        } else if TYPEOF(src) == SEXPTYPE::INTSXP.0 || TYPEOF(src) == SEXPTYPE::LGLSXP.0 {
            let v = *INTEGER(src).add(src_idx as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        };
        *dst.add(i as usize) = val;
    }

    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_table — frequency table
// ---------------------------------------------------------------------------

/// R's `table(...)` — counts occurrences of each unique value.
pub unsafe fn do_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::INTSXP.0 && t != SEXPTYPE::REALSXP.0 && t != SEXPTYPE::LGLSXP.0 {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    let mut counts: std::collections::BTreeMap<i64, i64> = std::collections::BTreeMap::new();

    for i in 0..n {
        let key = if t == SEXPTYPE::REALSXP.0 {
            (*REAL(x).add(i as usize)).to_bits() as i64
        } else {
            *INTEGER(x).add(i as usize) as i64
        };
        *counts.entry(key).or_insert(0) += 1;
    }

    let len = counts.len() as R_xlen_t;
    let result = Rf_allocVector3(SEXPTYPE::INTSXP.0, len);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = INTEGER(result);
    for (i, (_, &count)) in counts.iter().enumerate() {
        *dst.add(i) = count.min(c_int::MAX as i64) as c_int;
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

// ---------------------------------------------------------------------------
// do_as_* — type coercion
// ---------------------------------------------------------------------------

/// R's `as.integer(x)` — coerce to INTSXP.
pub unsafe fn do_as_integer(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    coerce_to_type(args, SEXPTYPE::INTSXP.0)
}

/// R's `as.double(x)` — coerce to REALSXP.
pub unsafe fn do_as_double(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    coerce_to_type(args, SEXPTYPE::REALSXP.0)
}

/// R's `as.character(x)` — coerce to STRSXP.
pub unsafe fn do_as_character(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    coerce_to_type(args, SEXPTYPE::STRSXP.0)
}

/// R's `as.logical(x)` — coerce to LGLSXP.
pub unsafe fn do_as_logical(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    coerce_to_type(args, SEXPTYPE::LGLSXP.0)
}

/// R's `as.vector(x)` — strips attributes, returns simple vector.
pub unsafe fn do_as_vector(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    CAR(args) // simplified: just return as-is
}

/// R's `as.list(x)` — converts to VECSXP (list).
pub unsafe fn do_as_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::VECSXP.0, 0);
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::VECSXP.0 {
        return x;
    }
    // Convert atomic vector to list
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for i in 0..n {
        // Create a length-1 vector for each element
        let elem = Rf_allocVector3(t, 1);
        if !elem.is_null() {
            if t == SEXPTYPE::REALSXP.0 {
                *REAL(elem) = *REAL(x).add(i as usize);
            } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
                *INTEGER(elem) = *INTEGER(x).add(i as usize);
            }
        }
        crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, elem);
    }
    crate::sexp::protect::Rf_unprotect(1);
    result
}

unsafe fn coerce_to_type(args: SEXP, target: c_int) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return R_NilValue();
    }
    let src_t = TYPEOF(x);
    let n = XLENGTH(x);

    if src_t == target {
        return x; // Already the right type
    }

    if target == SEXPTYPE::REALSXP.0 {
        let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = REAL(result);
        for i in 0..n {
            if src_t == SEXPTYPE::INTSXP.0 || src_t == SEXPTYPE::LGLSXP.0 {
                let v = *INTEGER(x).add(i as usize);
                *dst.add(i as usize) = if v == NA_INTEGER { NA_REAL } else { v as f64 };
            } else {
                *dst.add(i as usize) = NA_REAL;
            }
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    } else if target == SEXPTYPE::INTSXP.0 {
        let result = Rf_allocVector3(SEXPTYPE::INTSXP.0, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = INTEGER(result);
        for i in 0..n {
            if src_t == SEXPTYPE::REALSXP.0 {
                let v = *REAL(x).add(i as usize);
                if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || !v.is_finite() {
                    *dst.add(i as usize) = NA_INTEGER;
                } else {
                    *dst.add(i as usize) = v as c_int;
                }
            } else if src_t == SEXPTYPE::LGLSXP.0 {
                let v = *LOGICAL(x).add(i as usize);
                *dst.add(i as usize) = if v == NA_INTEGER { NA_INTEGER } else { v };
            } else {
                *dst.add(i as usize) = NA_INTEGER;
            }
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    } else if target == SEXPTYPE::LGLSXP.0 {
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP.0, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            if src_t == SEXPTYPE::INTSXP.0 {
                let v = *INTEGER(x).add(i as usize);
                *dst.add(i as usize) = if v == NA_INTEGER {
                    NA_INTEGER
                } else if v != 0 {
                    TRUE
                } else {
                    FALSE
                };
            } else if src_t == SEXPTYPE::REALSXP.0 {
                let v = *REAL(x).add(i as usize);
                if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    *dst.add(i as usize) = NA_INTEGER;
                } else {
                    *dst.add(i as usize) = if v != 0.0 { TRUE } else { FALSE };
                }
            } else {
                *dst.add(i as usize) = NA_INTEGER;
            }
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    } else {
        x // Unsupported coercion, return as-is
    }
}

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
    let result = Rf_allocVector3(SEXPTYPE::INTSXP.0, n);
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
    let result = Rf_allocVector3(SEXPTYPE::STRSXP.0, n);
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
    let result = Rf_allocVector3(SEXPTYPE::STRSXP.0, n);
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
    let result = Rf_allocVector3(SEXPTYPE::STRSXP.0, n);
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
    let result = Rf_allocVector3(SEXPTYPE::STRSXP.0, n);
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
        return Rf_allocVector3(SEXPTYPE::VECSXP.0, 0);
    }
    let split = elt_to_string(split_arg, 0);
    let n = XLENGTH(x_arg).max(1);
    let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, n);
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
        let vec = Rf_allocVector3(SEXPTYPE::STRSXP.0, parts.len() as R_xlen_t);
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
        return Rf_allocVector3(SEXPTYPE::REALSXP.0, 0);
    }
    let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, max_len);
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
    let t = if TYPEOF(values) == SEXPTYPE::STRSXP.0 || TYPEOF(x) == SEXPTYPE::STRSXP.0 {
        SEXPTYPE::STRSXP.0
    } else if TYPEOF(x) == SEXPTYPE::REALSXP.0 || TYPEOF(values) == SEXPTYPE::REALSXP.0 {
        SEXPTYPE::REALSXP.0
    } else {
        SEXPTYPE::INTSXP.0
    };
    let result = Rf_allocVector3(t, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    if t == SEXPTYPE::REALSXP.0 {
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
    } else if t == SEXPTYPE::INTSXP.0 {
        let dst = INTEGER(result);
        for i in 0..after {
            *dst.add(i as usize) = if TYPEOF(x) == SEXPTYPE::INTSXP.0 {
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
            *dst.add((after + i) as usize) = if TYPEOF(values) == SEXPTYPE::INTSXP.0 {
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
            *dst.add((i + vlen) as usize) = if TYPEOF(x) == SEXPTYPE::INTSXP.0 {
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
        if t == SEXPTYPE::REALSXP.0 {
            *REAL(result).add(i as usize) = *REAL(x).add(i as usize);
        } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
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
        if t == SEXPTYPE::REALSXP.0 {
            *REAL(result).add(i as usize) = *REAL(x).add((start + i) as usize);
        } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
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
        if t == SEXPTYPE::REALSXP.0 {
            *REAL(result).add(j as usize) = *REAL(x).add(src);
        } else if t == SEXPTYPE::INTSXP.0 {
            *INTEGER(result).add(j as usize) = *INTEGER(x).add(src);
        } else if t == SEXPTYPE::LGLSXP.0 {
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
        let key = if t == SEXPTYPE::REALSXP.0 {
            (*REAL(y).add(i as usize)).to_bits() as i64
        } else {
            *INTEGER(y).add(i as usize) as i64
        };
        y_keys.insert(key);
    }
    let mut result_keys: Vec<i64> = Vec::new();
    let mut seen: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    for i in 0..xn {
        let key = if t == SEXPTYPE::REALSXP.0 {
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
        if t == SEXPTYPE::REALSXP.0 {
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
        SEXPTYPE::INTSXP.0
    };
    let mut seen: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    let mut result_keys: Vec<i64> = Vec::new();
    let mut add_from = |src: SEXP| {
        if !src.is_null() && src != R_NilValue() {
            let n = XLENGTH(src);
            for i in 0..n {
                let key = if t == SEXPTYPE::REALSXP.0 {
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
        if t == SEXPTYPE::REALSXP.0 {
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
        let key = if t == SEXPTYPE::REALSXP.0 {
            (*REAL(x).add(i as usize)).to_bits() as i64
        } else {
            *INTEGER(x).add(i as usize) as i64
        };
        x_keys.insert(key);
    }
    let mut seen: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    let mut result_keys: Vec<i64> = Vec::new();
    for i in 0..yn {
        let key = if t == SEXPTYPE::REALSXP.0 {
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
        if t == SEXPTYPE::REALSXP.0 {
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
        let key = if tx == SEXPTYPE::REALSXP.0 {
            (*REAL(x).add(i as usize)).to_bits() as i64
        } else {
            *INTEGER(x).add(i as usize) as i64
        };
        x_set.insert(key);
    }
    for i in 0..yn {
        let key = if ty == SEXPTYPE::REALSXP.0 {
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
    if t != SEXPTYPE::REALSXP.0 && t != SEXPTYPE::INTSXP.0 {
        return Rf_ScalarLogical(TRUE);
    }
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP.0, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);
    for i in 0..n {
        let is_fin = if t == SEXPTYPE::REALSXP.0 {
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
    if t != SEXPTYPE::REALSXP.0 {
        return Rf_ScalarLogical(FALSE);
    }
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP.0, n);
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
    if t != SEXPTYPE::REALSXP.0 {
        return Rf_ScalarLogical(FALSE);
    }
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP.0, n);
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
        !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP.0 && LENGTH(dim_attr) == 2;
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
    Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::VECSXP.0 {
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
    let result = Rf_allocVector3(SEXPTYPE::STRSXP.0, n);
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
    let result = Rf_allocVector3(SEXPTYPE::STRSXP.0, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for i in 0..n {
        let s = if TYPEOF(x) == SEXPTYPE::REALSXP.0 {
            let v = *REAL(x).add(i as usize);
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                "NA".to_string()
            } else if nsmall > 0 {
                format!("{:.*}", nsmall, v)
            } else {
                format!("{}", v)
            }
        } else if TYPEOF(x) == SEXPTYPE::INTSXP.0 {
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
        "[",
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

/// Read a scalar real from a numeric SEXP, with default.
fn real_or_default(x: SEXP, default: f64) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return default;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP.0 {
            *REAL(x)
        } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
            let v = *INTEGER(x);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            default
        }
    }
}

/// Convert an element of a vector to a String.
fn elt_to_string(x: SEXP, i: R_xlen_t) -> String {
    unsafe {
        if x.is_null() {
            return "NULL".to_string();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let idx = if n == 0 { 0 } else { i % n };

        if t == SEXPTYPE::REALSXP.0 {
            let v = *REAL(x).add(idx as usize);
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                "NA".to_string()
            } else {
                format!("{}", v)
            }
        } else if t == SEXPTYPE::INTSXP.0 {
            let v = *INTEGER(x).add(idx as usize);
            if v == NA_INTEGER {
                "NA".to_string()
            } else {
                format!("{}", v)
            }
        } else if t == SEXPTYPE::LGLSXP.0 {
            let v = *LOGICAL(x).add(idx as usize);
            if v == NA_INTEGER {
                "NA".to_string()
            } else if v == TRUE {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        } else if t == SEXPTYPE::STRSXP.0 {
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
        } else if t == SEXPTYPE::SYMSXP.0 {
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
        return Rf_allocVector3(SEXPTYPE::VECSXP.0, 0);
    }
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, n);
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
    if list.is_null() || TYPEOF(list) != SEXPTYPE::VECSXP.0 {
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
    if elem_type != SEXPTYPE::REALSXP.0
        && elem_type != SEXPTYPE::INTSXP.0
        && elem_type != SEXPTYPE::LGLSXP.0
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
            if elem_type == SEXPTYPE::REALSXP.0 {
                *REAL(result).add(i as usize) = *REAL(elem);
            } else if elem_type == SEXPTYPE::INTSXP.0 {
                *INTEGER(result).add(i as usize) = *INTEGER(elem);
            } else if elem_type == SEXPTYPE::LGLSXP.0 {
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
    let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, n);
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
        if !val.is_null() && TYPEOF(val) == SEXPTYPE::LGLSXP.0 && *LOGICAL(val) != 0 {
            kept.push(i);
        }
    }
    let result = Rf_allocVector3(TYPEOF(x), kept.len() as R_xlen_t);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for (new_i, &old_i) in kept.iter().enumerate() {
        if TYPEOF(x) == SEXPTYPE::REALSXP.0 {
            *REAL(result).add(new_i) = *REAL(x).add(old_i as usize);
        } else if TYPEOF(x) == SEXPTYPE::INTSXP.0 {
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
    let n = if TYPEOF(arg_list) == SEXPTYPE::VECSXP.0 {
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
        if t == SEXPTYPE::VECSXP.0 {
            return crate::sexp::accessors::VECTOR_ELT(x, i as i64);
        }
        let elem = Rf_allocVector3(t, 1);
        if elem.is_null() {
            return R_NilValue();
        }
        if t == SEXPTYPE::REALSXP.0 {
            *REAL(elem) = *REAL(x).add(i as usize);
        } else if t == SEXPTYPE::INTSXP.0 {
            *INTEGER(elem) = *INTEGER(x).add(i as usize);
        } else if t == SEXPTYPE::LGLSXP.0 {
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
        if t == SEXPTYPE::REALSXP.0 {
            *REAL(result).add(j as usize) = *REAL(x).add(src);
        } else if t == SEXPTYPE::INTSXP.0 {
            *INTEGER(result).add(j as usize) = *INTEGER(x).add(src);
        } else if t == SEXPTYPE::LGLSXP.0 {
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
    if t == SEXPTYPE::REALSXP.0 {
        for i in 0..nrow {
            *REAL(result).add(i as usize) = *REAL(x).add(offset + i as usize);
        }
    } else if t == SEXPTYPE::INTSXP.0 {
        for i in 0..nrow {
            *INTEGER(result).add(i as usize) = *INTEGER(x).add(offset + i as usize);
        }
    } else if t == SEXPTYPE::LGLSXP.0 {
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
    if dim_attr.is_null() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP.0 || LENGTH(dim_attr) < 2 {
        return R_NilValue(); // not a matrix/array
    }
    let nrow = *INTEGER(dim_attr) as R_xlen_t;
    let ncol = *INTEGER(dim_attr.add(1)) as R_xlen_t;
    let margin = real_or_default(margin_arg, 1.0) as i64;

    if margin == 1 {
        // Apply over rows
        let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, nrow);
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
        let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, ncol);
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
        let key = if idx_t == SEXPTYPE::INTSXP.0 || idx_t == SEXPTYPE::LGLSXP.0 {
            *INTEGER(index).add(idx_i as usize) as i64
        } else if idx_t == SEXPTYPE::REALSXP.0 {
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
    let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, num_groups);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    for (g, indices) in groups.iter().enumerate() {
        let group_vec = Rf_allocVector3(TYPEOF(x), indices.len() as R_xlen_t);
        if !group_vec.is_null() {
            let t = TYPEOF(x);
            for (j, &src_i) in indices.iter().enumerate() {
                if t == SEXPTYPE::REALSXP.0 {
                    *REAL(group_vec).add(j) = *REAL(x).add(src_i as usize);
                } else if t == SEXPTYPE::INTSXP.0 {
                    *INTEGER(group_vec).add(j) = *INTEGER(x).add(src_i as usize);
                } else if t == SEXPTYPE::LGLSXP.0 {
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

    let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, max_len);
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
    } else if TYPEOF(fun_arg) == SEXPTYPE::STRSXP.0 {
        elt_to_string(fun_arg, 0) == "*"
    } else if TYPEOF(fun_arg) == SEXPTYPE::SYMSXP.0 {
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

    let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, nx * ny);
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
                let v = if !val.is_null() && TYPEOF(val) == SEXPTYPE::REALSXP.0 {
                    *REAL(val)
                } else if !val.is_null()
                    && (TYPEOF(val) == SEXPTYPE::INTSXP.0 || TYPEOF(val) == SEXPTYPE::LGLSXP.0)
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
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP.0, 2);
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
    } else if TYPEOF(fun_arg) == SEXPTYPE::STRSXP.0 {
        elt_to_string(fun_arg, 0)
    } else if TYPEOF(fun_arg) == SEXPTYPE::SYMSXP.0 {
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
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP.0 && LENGTH(dim_attr) >= 2 {
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
                let src_val = if t == SEXPTYPE::REALSXP.0 {
                    *REAL(x).add(src_idx)
                } else if t == SEXPTYPE::INTSXP.0 {
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
                if t == SEXPTYPE::REALSXP.0 {
                    *REAL(result).add(src_idx) = res;
                } else if t == SEXPTYPE::INTSXP.0 {
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
                let src_val = if t == SEXPTYPE::REALSXP.0 {
                    *REAL(x).add(src_idx)
                } else if t == SEXPTYPE::INTSXP.0 {
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
                if t == SEXPTYPE::REALSXP.0 {
                    *REAL(result).add(src_idx) = res;
                } else if t == SEXPTYPE::INTSXP.0 {
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
    if class_attr.is_null() || TYPEOF(class_attr) != SEXPTYPE::STRSXP.0 {
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
    Rf_allocVector3(SEXPTYPE::STRSXP.0, 0)
}

/// R's `rm(list, envir)` — remove objects.
pub unsafe fn do_rm(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let list = CAR(args);
    if list.is_null() || TYPEOF(list) != SEXPTYPE::STRSXP.0 {
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
        let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
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
        let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
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
        if t == SEXPTYPE::REALSXP.0 {
            *REAL(x).add(idx as usize)
        } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
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
        TYPEOF(byrow_arg) == SEXPTYPE::LGLSXP.0 && *LOGICAL(byrow_arg) != 0
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

        if t == SEXPTYPE::REALSXP.0 {
            *REAL(result).add(i as usize) = *REAL(data).add(src_idx as usize);
        } else if t == SEXPTYPE::INTSXP.0 {
            *INTEGER(result).add(i as usize) = *INTEGER(data).add(src_idx as usize);
        } else if t == SEXPTYPE::LGLSXP.0 {
            *LOGICAL(result).add(i as usize) = *LOGICAL(data).add(src_idx as usize);
        }
    }

    // Set dim attribute
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP.0, 2);
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
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP.0 && LENGTH(dim_attr) >= 2 {
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
            if t == SEXPTYPE::REALSXP.0 {
                *REAL(result).add(dst) = *REAL(x).add(src);
            } else if t == SEXPTYPE::INTSXP.0 {
                *INTEGER(result).add(dst) = *INTEGER(x).add(src);
            } else if t == SEXPTYPE::LGLSXP.0 {
                *LOGICAL(result).add(dst) = *LOGICAL(x).add(src);
            }
        }
    }

    // Set transposed dim attribute
    let dim = Rf_allocVector3(SEXPTYPE::INTSXP.0, 2);
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
    if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP.0 && LENGTH(dim_attr) >= 1 {
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
    if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP.0 && LENGTH(dim_attr) >= 2 {
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
    if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP.0 && LENGTH(dim_attr) >= 2 {
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
            if TYPEOF(x) == SEXPTYPE::REALSXP.0 {
                *REAL(result).add(i) = *REAL(x).add(src);
            } else if TYPEOF(x) == SEXPTYPE::INTSXP.0 {
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
            if t == SEXPTYPE::REALSXP.0 {
                *REAL(result).add(i) = 0.0;
            } else if t == SEXPTYPE::INTSXP.0 {
                *INTEGER(result).add(i) = 0;
            }
        }
        for i in 0..n {
            let dst = i * n + i;
            if t == SEXPTYPE::REALSXP.0 {
                *REAL(result).add(dst) = *REAL(x).add(i);
            } else if t == SEXPTYPE::INTSXP.0 {
                *INTEGER(result).add(dst) = *INTEGER(x).add(i);
            }
        }

        // Set dim
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP.0, 2);
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
    if t != SEXPTYPE::INTSXP.0 && t != SEXPTYPE::REALSXP.0 {
        return x; // Simplified: non-numeric returns as-is
    }
    let n = XLENGTH(x);
    let mut seen: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    let mut unique_indices: Vec<R_xlen_t> = Vec::new();

    for i in 0..n {
        let key = if t == SEXPTYPE::REALSXP.0 {
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
        if t == SEXPTYPE::REALSXP.0 {
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
        TYPEOF(dec_arg) == SEXPTYPE::LGLSXP.0 && *LOGICAL(dec_arg) != 0
    };

    let t = TYPEOF(x);
    let n = XLENGTH(x);
    let result = Rf_allocVector3(t, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    // Copy and sort
    if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
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
    } else if t == SEXPTYPE::REALSXP.0 {
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
        if t == SEXPTYPE::REALSXP.0 {
            *REAL(result).add(dst) = *REAL(x).add(src);
        } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
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
    if t != SEXPTYPE::LGLSXP.0 && t != SEXPTYPE::INTSXP.0 {
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
    if t != SEXPTYPE::LGLSXP.0 && t != SEXPTYPE::INTSXP.0 {
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
        return Rf_allocVector3(SEXPTYPE::INTSXP.0, 0);
    }
    let result = Rf_allocVector3(SEXPTYPE::INTSXP.0, n as R_xlen_t);
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
        return Rf_allocVector3(SEXPTYPE::INTSXP.0, 0);
    }
    let result = Rf_allocVector3(SEXPTYPE::INTSXP.0, n);
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
    let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
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
    let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
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
        return Rf_allocVector3(SEXPTYPE::REALSXP.0, 0);
    }
    let result_len = n - lag as R_xlen_t;
    let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, result_len);
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

    let n = if TYPEOF(text) == SEXPTYPE::STRSXP.0 {
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

    let result = Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t);
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
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP.0, n);
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
    let result = Rf_allocVector3(SEXPTYPE::STRSXP.0, n as R_xlen_t);
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
            && TYPEOF(cond) == SEXPTYPE::LGLSXP.0
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
        if dir_arg.is_null() { return R_NilValue(); }
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
        if x.is_null() || x == R_NilValue() { return Rf_ScalarLogical(FALSE); }
        let n = XLENGTH(x).max(1);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP.0, n);
        if result.is_null() { return R_NilValue(); }
        let _p = Rf_protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let path = elt_to_string(x, i);
            *dst.add(i as usize) = if std::path::Path::new(&path).is_dir() { TRUE } else { FALSE };
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R's `file.create(...)` — create empty files.
pub unsafe fn do_file_create(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() { return Rf_ScalarLogical(FALSE); }
        let n = XLENGTH(x).max(1);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP.0, n);
        if result.is_null() { return R_NilValue(); }
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
        if x.is_null() || x == R_NilValue() { return Rf_ScalarInteger(0); }
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
            if result.is_ok() { count += 1; }
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
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP.0, n);
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
    if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP.0 && LENGTH(dim_attr) >= 1 {
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
    if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP.0 && LENGTH(dim_attr) >= 2 {
        Rf_ScalarInteger(*INTEGER(dim_attr.add(1)))
    } else {
        Rf_ScalarInteger(1)
    }
}

/// R's `lengths(x)` — length of each element in a list/vector.
pub unsafe fn do_lengths(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::INTSXP.0, 0);
    }
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::INTSXP.0, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = INTEGER(result);
    let t = TYPEOF(x);
    if t == SEXPTYPE::VECSXP.0 {
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
    if !dimnames.is_null() && TYPEOF(dimnames) == SEXPTYPE::VECSXP.0 && LENGTH(dimnames) >= 2 {
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
        let name = if t == SEXPTYPE::REALSXP.0 {
            "numeric"
        } else if t == SEXPTYPE::INTSXP.0 {
            "integer"
        } else if t == SEXPTYPE::LGLSXP.0 {
            "logical"
        } else if t == SEXPTYPE::STRSXP.0 {
            "character"
        } else if t == SEXPTYPE::VECSXP.0 {
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
    let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, n);
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
        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP.0, n);
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
    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP.0, 1);
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
            let rn = Rf_allocVector3(SEXPTYPE::INTSXP.0, 2);
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
    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP.0, 1);
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
            if t == SEXPTYPE::STRSXP.0 {
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
    if idx < 1 || TYPEOF(x) != SEXPTYPE::VECSXP.0 {
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
    if idx < 1 || TYPEOF(x) != SEXPTYPE::VECSXP.0 {
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
        return Rf_allocVector3(SEXPTYPE::VECSXP.0, 0);
    }
    let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, total_len);
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
            if TYPEOF(arg) == SEXPTYPE::VECSXP.0 {
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
    if TYPEOF(x) != SEXPTYPE::VECSXP.0 {
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
            if t == SEXPTYPE::REALSXP.0 {
                all_values.push(*REAL(elem).add(j as usize));
            } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
                let v = *INTEGER(elem).add(j as usize);
                all_ints.push(v);
            } else if t == SEXPTYPE::STRSXP.0 {
                all_strs.push(elt_to_string(elem, j));
                saw_str = true;
            } else if t == SEXPTYPE::VECSXP.0 {
                // Nested list — recurse via extraction
                let inner = VECTOR_ELT(elem, j as i64);
                if !inner.is_null() && TYPEOF(inner) == SEXPTYPE::REALSXP.0 {
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
        SEXPTYPE::STRSXP.0
    } else if !all_values.is_empty() {
        SEXPTYPE::REALSXP.0
    } else {
        SEXPTYPE::INTSXP.0
    };

    let total: R_xlen_t = if result_type == SEXPTYPE::STRSXP.0 {
        all_strs.len() as R_xlen_t
    } else if result_type == SEXPTYPE::REALSXP.0 {
        (all_values.len() + all_ints.len()) as R_xlen_t
    } else {
        all_ints.len() as R_xlen_t
    };

    let result = Rf_allocVector3(result_type, total);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);

    if result_type == SEXPTYPE::REALSXP.0 {
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
    } else if result_type == SEXPTYPE::INTSXP.0 {
        let dst = INTEGER(result);
        for (idx, &v) in all_ints.iter().enumerate() {
            *dst.add(idx) = v;
        }
    } else if result_type == SEXPTYPE::STRSXP.0 {
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
    let is_atomic = t == SEXPTYPE::LGLSXP.0
        || t == SEXPTYPE::INTSXP.0
        || t == SEXPTYPE::REALSXP.0
        || t == SEXPTYPE::CPLXSXP.0
        || t == SEXPTYPE::STRSXP.0
        || t == SEXPTYPE::RAWSXP.0
        || t == SEXPTYPE::CHARSXP.0
        || t == SEXPTYPE::NILSXP.0;
    Rf_ScalarLogical(if is_atomic { TRUE } else { FALSE })
}

/// R's `is.recursive(x)` — TRUE for recursive types (list, pairlist, language, etc.).
pub unsafe fn do_is_recursive(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_ScalarLogical(FALSE);
    }
    let t = TYPEOF(x);
    let is_rec = t == SEXPTYPE::VECSXP.0
        || t == SEXPTYPE::LISTSXP.0
        || t == SEXPTYPE::LANGSXP.0
        || t == SEXPTYPE::CLOSXP.0
        || t == SEXPTYPE::BUILTINSXP.0
        || t == SEXPTYPE::SPECIALSXP.0
        || t == SEXPTYPE::ENVSXP.0
        || t == SEXPTYPE::EXPRSXP.0;
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
    let (nrow, ncol) = if !dim_attr.is_null()
        && TYPEOF(dim_attr) == SEXPTYPE::INTSXP.0
        && LENGTH(dim_attr) >= 2
    {
        (*INTEGER(dim_attr) as R_xlen_t, *INTEGER(dim_attr.add(1)) as R_xlen_t)
    } else {
        let n = XLENGTH(x).max(1);
        (n, 1)
    };

    // Get colnames
    let colnames = crate::sexp::attrib_core::getAttrib(
        x,
        Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
    );
    let col_names_vec: Vec<String> = if !colnames.is_null()
        && TYPEOF(colnames) == SEXPTYPE::VECSXP.0
        && LENGTH(colnames) >= 2
    {
        let cn = VECTOR_ELT(colnames, 1);
        if !cn.is_null() && TYPEOF(cn) == SEXPTYPE::STRSXP.0 {
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
        header.push_str(&format!("{:>12}", name));
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
    let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP.0;

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
                t if t == SEXPTYPE::REALSXP.0 => "num".to_string(),
                t if t == SEXPTYPE::INTSXP.0 => "int".to_string(),
                t if t == SEXPTYPE::LGLSXP.0 => "logi".to_string(),
                t if t == SEXPTYPE::STRSXP.0 => "chr".to_string(),
                t if t == SEXPTYPE::VECSXP.0 => "list".to_string(),
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
    if t != SEXPTYPE::REALSXP.0 && t != SEXPTYPE::INTSXP.0 {
        // For non-numeric, just return type info
        return do_typeof(_call, _op, args, _rho);
    }
    let n = XLENGTH(x);
    if n == 0 {
        return R_NilValue();
    }
    let mut vals: Vec<f64> = Vec::new();
    for i in 0..n {
        let v = if t == SEXPTYPE::REALSXP.0 {
            *REAL(x).add(i as usize)
        } else {
            let iv = *INTEGER(x).add(i as usize);
            if iv == NA_INTEGER {
                NA_REAL
            } else {
                iv as f64
            }
        };
        if v.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN && !v.is_nan() {
            vals.push(v);
        }
    }
    if vals.is_empty() {
        println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
        println!("     NA      NA      NA      NA      NA      NA       {}", n);
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
        min_v, q1_v, median_v, mean_v, q3_v, max_v, if na_count > 0 { na_count.to_string() } else { String::new() }
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

    if t == SEXPTYPE::VECSXP.0 {
        // List
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        );
        let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP.0;

        // Check for data.frame class
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        let is_df = if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP.0 {
            elt_to_string(class, 0) == "data.frame"
        } else {
            false
        };

        if is_df {
            let ncol = n;
            let nrow = if ncol > 0 {
                let first = VECTOR_ELT(x, 0);
                if first.is_null() { 0 } else { XLENGTH(first) }
            } else { 0 };
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
                        t if t == SEXPTYPE::REALSXP.0 => format!("num [1:{}]", m),
                        t if t == SEXPTYPE::INTSXP.0 => format!("int [1:{}]", m),
                        t if t == SEXPTYPE::LGLSXP.0 => format!("logi [1:{}]", m),
                        t if t == SEXPTYPE::STRSXP.0 => format!("chr [1:{}]", m),
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
                        t if t == SEXPTYPE::REALSXP.0 => format!("num [1:{}]", m),
                        t if t == SEXPTYPE::INTSXP.0 => format!("int [1:{}]", m),
                        t if t == SEXPTYPE::LGLSXP.0 => format!("logi [1:{}]", m),
                        t if t == SEXPTYPE::STRSXP.0 => format!("chr [1:{}]", m),
                        t if t == SEXPTYPE::VECSXP.0 => format!("list [1:{}]", m),
                        _ => format!("? [1:{}]", m),
                    }
                };
                println!(" $ {}: {}", name, elem_type);
            }
        }
    } else {
        // Atomic vector or other
        let type_name = match t {
            t if t == SEXPTYPE::REALSXP.0 => "num",
            t if t == SEXPTYPE::INTSXP.0 => "int",
            t if t == SEXPTYPE::LGLSXP.0 => "logi",
            t if t == SEXPTYPE::STRSXP.0 => "chr",
            t if t == SEXPTYPE::CPLXSXP.0 => "cplx",
            t if t == SEXPTYPE::RAWSXP.0 => "raw",
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
    if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP.0 {
        let cls_name = elt_to_string(class, 0);
        if cls_name == "data.frame" {
            return x;
        }
    }
    // Wrap in a single-element list and set class
    let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, 1);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    SET_VECTOR_ELT(result, 0, x);

    let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP.0, 1);
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
    let rn = Rf_allocVector3(SEXPTYPE::INTSXP.0, 2);
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
    let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP.0, 1);
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

