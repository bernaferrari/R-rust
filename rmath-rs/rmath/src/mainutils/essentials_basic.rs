#![allow(unsafe_op_in_unsafe_fn)]

use std::collections::BTreeMap;
use std::ffi::CString;
use std::os::raw::c_int;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    CAR, CDR, CHAR, INTEGER, LENGTH, LOGICAL, RAW, REAL, SET_STRING_ELT, SET_VECTOR_ELT,
    STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH,
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

use crate::mainutils::essentials::elt_to_string;

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

    let result = Rf_allocVector3(SEXPTYPE::STRSXP, max_len);
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
        t if t == SEXPTYPE::BUILTINSXP => "builtin",
        t if t == SEXPTYPE::SPECIALSXP => "special",
        t if t == SEXPTYPE::ENVSXP => "environment",
        t if t == SEXPTYPE::NILSXP => "NULL",
        t if t == SEXPTYPE::CHARSXP => "character",
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
    let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    let dst = LOGICAL(result);

    for i in 0..n {
        let is_na = if t == SEXPTYPE::REALSXP {
            let v = *REAL(x).add(i as usize);
            v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
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
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::LGLSXP && t != SEXPTYPE::INTSXP {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }

    let n = XLENGTH(x);
    let mut indices: Vec<i32> = Vec::new();
    for i in 0..n {
        let v = *INTEGER(x).add(i as usize);
        if v != 0 && v != NA_INTEGER {
            indices.push((i + 1) as i32); // R is 1-indexed
        }
    }

    let result = Rf_allocVector3(SEXPTYPE::INTSXP, indices.len() as R_xlen_t);
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
    let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
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
        let cond = if TYPEOF(test) == SEXPTYPE::LGLSXP {
            *LOGICAL(test).add(test_idx as usize) != 0
        } else if TYPEOF(test) == SEXPTYPE::INTSXP {
            *INTEGER(test).add(test_idx as usize) != 0
        } else {
            false
        };

        let src = if cond { yes } else { no };
        let src_n = if cond { yes_n } else { no_n };
        let src_idx = if src_n == 0 { 0 } else { i % src_n };

        let val = if TYPEOF(src) == SEXPTYPE::REALSXP {
            *REAL(src).add(src_idx as usize)
        } else if TYPEOF(src) == SEXPTYPE::INTSXP || TYPEOF(src) == SEXPTYPE::LGLSXP {
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
    if t != SEXPTYPE::INTSXP && t != SEXPTYPE::REALSXP && t != SEXPTYPE::LGLSXP {
        return R_NilValue();
    }

    let n = XLENGTH(x);
    let mut counts: BTreeMap<i64, i64> = BTreeMap::new();

    for i in 0..n {
        let key = if t == SEXPTYPE::REALSXP {
            (*REAL(x).add(i as usize)).to_bits() as i64
        } else {
            *INTEGER(x).add(i as usize) as i64
        };
        *counts.entry(key).or_insert(0) += 1;
    }

    let len = counts.len() as R_xlen_t;
    let result = Rf_allocVector3(SEXPTYPE::INTSXP, len);
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
    coerce_to_type(args, SEXPTYPE::INTSXP.as_c_int())
}

/// R's `as.double(x)` — coerce to REALSXP.
pub unsafe fn do_as_double(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    coerce_to_type(args, SEXPTYPE::REALSXP.as_c_int())
}

/// R's `as.character(x)` — coerce to STRSXP.
pub unsafe fn do_as_character(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    coerce_to_type(args, SEXPTYPE::STRSXP.as_c_int())
}

/// R's `as.logical(x)` — coerce to LGLSXP.
pub unsafe fn do_as_logical(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    coerce_to_type(args, SEXPTYPE::LGLSXP.as_c_int())
}

/// R's `as.vector(x)` — strips attributes, returns simple vector.
pub unsafe fn do_as_vector(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    CAR(args) // simplified: just return as-is
}

/// R's `as.list(x)` — converts to VECSXP (list).
pub unsafe fn do_as_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::VECSXP {
        return x;
    }
    // Convert atomic vector to list
    let n = XLENGTH(x);
    let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
    if result.is_null() {
        return R_NilValue();
    }
    let _p = Rf_protect(result);
    for i in 0..n {
        // Create a length-1 vector for each element
        let elem = Rf_allocVector3(t, 1);
        if !elem.is_null() {
            if t == SEXPTYPE::REALSXP {
                *REAL(elem) = *REAL(x).add(i as usize);
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
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

    if target == SEXPTYPE::REALSXP {
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = REAL(result);
        for i in 0..n {
            if src_t == SEXPTYPE::INTSXP || src_t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                *dst.add(i as usize) = if v == NA_INTEGER { NA_REAL } else { v as f64 };
            } else {
                *dst.add(i as usize) = NA_REAL;
            }
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    } else if target == SEXPTYPE::INTSXP {
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = INTEGER(result);
        for i in 0..n {
            if src_t == SEXPTYPE::REALSXP {
                let v = *REAL(x).add(i as usize);
                if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || !v.is_finite() {
                    *dst.add(i as usize) = NA_INTEGER;
                } else {
                    *dst.add(i as usize) = v as c_int;
                }
            } else if src_t == SEXPTYPE::LGLSXP {
                let v = *LOGICAL(x).add(i as usize);
                *dst.add(i as usize) = if v == NA_INTEGER { NA_INTEGER } else { v };
            } else {
                *dst.add(i as usize) = NA_INTEGER;
            }
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    } else if target == SEXPTYPE::LGLSXP {
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = Rf_protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            if src_t == SEXPTYPE::INTSXP {
                let v = *INTEGER(x).add(i as usize);
                *dst.add(i as usize) = if v == NA_INTEGER {
                    NA_INTEGER
                } else if v != 0 {
                    TRUE
                } else {
                    FALSE
                };
            } else if src_t == SEXPTYPE::REALSXP {
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
