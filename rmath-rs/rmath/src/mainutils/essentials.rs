//! Essential R built-in functions — c(), seq(), rep(), paste(), cat(), typeof(), is.na(), names().
//!
//! These are the most fundamental R functions that every R program uses.

use std::ffi::CString;
use std::os::raw::c_int;

use crate::sexp::accessors::{CAR, CDR, INTEGER, LOGICAL, REAL, TYPEOF, XLENGTH};
#[allow(unused_imports)]
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarLogical, Rf_allocVector3, Rf_cons, Rf_mkString};
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
    unsafe {
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
    unsafe {
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
}

// ---------------------------------------------------------------------------
// do_rep — repeat elements
// ---------------------------------------------------------------------------

/// R's `rep(x, times)` — repeats a vector `times` times.
pub unsafe fn do_rep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// do_paste / do_paste0 — string concatenation
// ---------------------------------------------------------------------------

/// R's `paste(..., sep=" ")` — concatenates vectors element-wise with recycling.
pub unsafe fn do_paste(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_paste_impl(args, " ") }
}

/// R's `paste0(...)` — same as paste with sep="".
pub unsafe fn do_paste0(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_paste_impl(args, "") }
}

unsafe fn do_paste_impl(args: SEXP, sep: &str) -> SEXP {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// do_cat — print to stdout
// ---------------------------------------------------------------------------

/// R's `cat(..., sep=" ")` — prints args to stdout without trailing newline.
pub unsafe fn do_cat(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// do_print — basic print
// ---------------------------------------------------------------------------

/// R's `print(x)` — basic print with newline. Returns x invisibly.
pub unsafe fn do_print(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// do_typeof — type name
// ---------------------------------------------------------------------------

/// R's `typeof(x)` — returns the type name as STRSXP.
pub unsafe fn do_typeof(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// do_is_na — check for NA
// ---------------------------------------------------------------------------

/// R's `is.na(x)` — returns LGLSXP with TRUE for NA elements.
pub unsafe fn do_is_na(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// do_names — get/set names attribute
// ---------------------------------------------------------------------------

/// R's `names(x)` — returns the names attribute.
pub unsafe fn do_names(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// do_which — find TRUE indices
// ---------------------------------------------------------------------------

/// R's `which(x)` — returns indices of TRUE elements in a logical vector.
pub unsafe fn do_which(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// do_ifelse — vectorized conditional
// ---------------------------------------------------------------------------

/// R's `ifelse(test, yes, no)` — vectorized if/else with recycling.
pub unsafe fn do_ifelse(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// do_table — frequency table
// ---------------------------------------------------------------------------

/// R's `table(...)` — counts occurrences of each unique value.
pub unsafe fn do_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
            *dst.add(i) = count as c_int;
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

// ---------------------------------------------------------------------------
// do_as_* — type coercion
// ---------------------------------------------------------------------------

/// R's `as.integer(x)` — coerce to INTSXP.
pub unsafe fn do_as_integer(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { coerce_to_type(args, SEXPTYPE::INTSXP.0) }
}

/// R's `as.double(x)` — coerce to REALSXP.
pub unsafe fn do_as_double(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { coerce_to_type(args, SEXPTYPE::REALSXP.0) }
}

/// R's `as.character(x)` — coerce to STRSXP.
pub unsafe fn do_as_character(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { coerce_to_type(args, SEXPTYPE::STRSXP.0) }
}

/// R's `as.logical(x)` — coerce to LGLSXP.
pub unsafe fn do_as_logical(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { coerce_to_type(args, SEXPTYPE::LGLSXP.0) }
}

/// R's `as.vector(x)` — strips attributes, returns simple vector.
pub unsafe fn do_as_vector(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { CAR(args) } // simplified: just return as-is
}

/// R's `as.list(x)` — converts to VECSXP (list).
pub unsafe fn do_as_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

unsafe fn coerce_to_type(args: SEXP, target: c_int) -> SEXP {
    unsafe {
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
}

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
}

// ---------------------------------------------------------------------------
// Register essentials builtins
// ---------------------------------------------------------------------------

/// Register essential builtins in the base environment.
pub unsafe fn register_essentials_builtins(env: SEXP) {
    unsafe {
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
    unsafe {
        let x = CAR(args);
        let fun = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || fun.is_null() {
            return Rf_allocVector3(SEXPTYPE::VECSXP.0, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, n);
        if result.is_null() { return R_NilValue(); }
        let _p = Rf_protect(result);
        for i in 0..n {
            let elem = extract_element(x, i);
            let call_args = Rf_cons(elem, R_NilValue());
            let call_sexp = Rf_cons(fun, call_args);
            if !call_sexp.is_null() { (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP); }
            let val = crate::eval::eval::Rf_eval(call_sexp, rho);
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R's `sapply(X, FUN)` — like lapply but simplifies to vector.
pub unsafe fn do_sapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let list = do_lapply(_call, _op, args, rho);
        if list.is_null() || TYPEOF(list) != SEXPTYPE::VECSXP.0 { return list; }
        let n = XLENGTH(list);
        if n == 0 { return list; }
        let first = crate::sexp::accessors::VECTOR_ELT(list, 0);
        if first.is_null() || XLENGTH(first) != 1 { return list; }
        let elem_type = TYPEOF(first);
        if elem_type != SEXPTYPE::REALSXP.0 && elem_type != SEXPTYPE::INTSXP.0 && elem_type != SEXPTYPE::LGLSXP.0 {
            return list;
        }
        let result = Rf_allocVector3(elem_type, n);
        if result.is_null() { return list; }
        let _p = Rf_protect(result);
        for i in 0..n {
            let elem = crate::sexp::accessors::VECTOR_ELT(list, i as i64);
            if !elem.is_null() && TYPEOF(elem) == elem_type {
                if elem_type == SEXPTYPE::REALSXP.0 { *REAL(result).add(i as usize) = *REAL(elem); }
                else if elem_type == SEXPTYPE::INTSXP.0 { *INTEGER(result).add(i as usize) = *INTEGER(elem); }
                else if elem_type == SEXPTYPE::LGLSXP.0 { *LOGICAL(result).add(i as usize) = *LOGICAL(elem); }
            }
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R's `vapply(X, FUN, FUN.VALUE)` — simplified as lapply.
pub unsafe fn do_vapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_lapply(_call, _op, args, rho) }
}

/// R's `Map(f, ...)` — apply f element-wise.
pub unsafe fn do_map(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fun = CAR(args);
        let x = CAR(CDR(args));
        if fun.is_null() || x.is_null() || x == R_NilValue() { return R_NilValue(); }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP.0, n);
        if result.is_null() { return R_NilValue(); }
        let _p = Rf_protect(result);
        for i in 0..n {
            let elem = extract_element(x, i);
            let call_args = Rf_cons(elem, R_NilValue());
            let call_sexp = Rf_cons(fun, call_args);
            if !call_sexp.is_null() { (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP); }
            let val = crate::eval::eval::Rf_eval(call_sexp, rho);
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R's `Filter(f, x)` — keep elements where f returns TRUE.
pub unsafe fn do_filter(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fun = CAR(args);
        let x = CAR(CDR(args));
        if fun.is_null() || x.is_null() || x == R_NilValue() { return R_NilValue(); }
        let n = XLENGTH(x);
        let mut kept: Vec<R_xlen_t> = Vec::new();
        for i in 0..n {
            let elem = extract_element(x, i);
            let call_args = Rf_cons(elem, R_NilValue());
            let call_sexp = Rf_cons(fun, call_args);
            if !call_sexp.is_null() { (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP); }
            let val = crate::eval::eval::Rf_eval(call_sexp, rho);
            if !val.is_null() && TYPEOF(val) == SEXPTYPE::LGLSXP.0 && *LOGICAL(val) != 0 { kept.push(i); }
        }
        let result = Rf_allocVector3(TYPEOF(x), kept.len() as R_xlen_t);
        if result.is_null() { return R_NilValue(); }
        let _p = Rf_protect(result);
        for (new_i, &old_i) in kept.iter().enumerate() {
            if TYPEOF(x) == SEXPTYPE::REALSXP.0 { *REAL(result).add(new_i) = *REAL(x).add(old_i as usize); }
            else if TYPEOF(x) == SEXPTYPE::INTSXP.0 { *INTEGER(result).add(new_i) = *INTEGER(x).add(old_i as usize); }
        }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

/// R's `do.call(what, args)` — call function with list of args.
pub unsafe fn do_do_call(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fun = CAR(args);
        let arg_list = CAR(CDR(args));
        if fun.is_null() || arg_list.is_null() { return R_NilValue(); }
        let n = if TYPEOF(arg_list) == SEXPTYPE::VECSXP.0 { XLENGTH(arg_list) } else { 0 };
        let mut call_args = R_NilValue();
        for i in (0..n).rev() {
            call_args = Rf_cons(crate::sexp::accessors::VECTOR_ELT(arg_list, i as i64), call_args);
        }
        let call_sexp = Rf_cons(fun, call_args);
        if !call_sexp.is_null() { (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP); }
        crate::eval::eval::Rf_eval(call_sexp, rho)
    }
}

fn extract_element(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        if t == SEXPTYPE::VECSXP.0 { return crate::sexp::accessors::VECTOR_ELT(x, i as i64); }
        let elem = Rf_allocVector3(t, 1);
        if elem.is_null() { return R_NilValue(); }
        if t == SEXPTYPE::REALSXP.0 { *REAL(elem) = *REAL(x).add(i as usize); }
        else if t == SEXPTYPE::INTSXP.0 { *INTEGER(elem) = *INTEGER(x).add(i as usize); }
        else if t == SEXPTYPE::LGLSXP.0 { *LOGICAL(elem) = *LOGICAL(x).add(i as usize); }
        elem
    }
}

// ---------------------------------------------------------------------------
// Error handling: stop, warning, message, tryCatch, inherits, exists, get, assign
// ---------------------------------------------------------------------------

/// R's `stop(...)` — raise error.
pub unsafe fn do_stop(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let s = elt_to_string(CAR(args), 0);
        std::panic::panic_any(crate::sexp::context::RError { message: s });
    }
}

/// R's `warning(...)` — issue warning.
pub unsafe fn do_warning(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { eprintln!("Warning: {}", elt_to_string(CAR(args), 0)); R_NilValue() }
}

/// R's `message(...)` — print message.
pub unsafe fn do_message(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { eprintln!("{}", elt_to_string(CAR(args), 0)); R_NilValue() }
}

/// R's `inherits(x, what)` — check class.
pub unsafe fn do_inherits(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let what = CAR(CDR(args));
        if x.is_null() || what.is_null() { return Rf_ScalarLogical(FALSE); }
        let class_attr = crate::sexp::attrib_core::getAttrib(x, Rf_install(CString::new("class").unwrap_or_default().as_ptr()));
        if class_attr.is_null() || TYPEOF(class_attr) != SEXPTYPE::STRSXP.0 { return Rf_ScalarLogical(FALSE); }
        let target = elt_to_string(what, 0);
        let n = XLENGTH(class_attr);
        for i in 0..n {
            if elt_to_string(class_attr, i) == target { return Rf_ScalarLogical(TRUE); }
        }
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `tryCatch(expr)` — basic try/catch.
pub unsafe fn do_tryCatch(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() { return R_NilValue(); }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::eval::eval::Rf_eval(expr, rho)));
        match result { Ok(val) => val, Err(_) => { eprintln!("Error caught"); R_NilValue() } }
    }
}

/// R's `exists(x, envir)` — check name exists.
pub unsafe fn do_exists(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name = elt_to_string(CAR(args), 0);
        let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
        let val = crate::sexp::envir::R_findVar(sym, rho);
        Rf_ScalarLogical(if !val.is_null() && val != crate::sexp::globals::R_UnboundValue() { TRUE } else { FALSE })
    }
}

/// R's `get(x, envir)` — get value.
pub unsafe fn do_get(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name = elt_to_string(CAR(args), 0);
        crate::sexp::envir::R_findVar(Rf_install(CString::new(name).unwrap_or_default().as_ptr()), rho)
    }
}

/// R's `assign(x, value, envir)` — assign value.
pub unsafe fn do_assign(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name = elt_to_string(CAR(args), 0);
        let val = CAR(CDR(args));
        if val.is_null() { return R_NilValue(); }
        crate::sexp::envir::defineVar(Rf_install(CString::new(name).unwrap_or_default().as_ptr()), val, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        val
    }
}

/// R's `ls(envir)` — list objects.
pub unsafe fn do_ls(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: return empty string vector
        // Full implementation needs R_ls which isn't ported yet
        Rf_allocVector3(SEXPTYPE::STRSXP.0, 0)
    }
}

/// R's `rm(list, envir)` — remove objects.
pub unsafe fn do_rm(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let list = CAR(args);
        if list.is_null() || TYPEOF(list) != SEXPTYPE::STRSXP.0 { return R_NilValue(); }
        for i in 0..XLENGTH(list) {
            let sym = Rf_install(CString::new(elt_to_string(list, i)).unwrap_or_default().as_ptr());
            crate::sexp::envir::defineVar(sym, crate::sexp::globals::R_UnboundValue(), rho);
        }
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Distribution functions: dnorm, pnorm, qnorm, dpois, ppois
// ---------------------------------------------------------------------------

/// R's `dnorm(x, mean=0, sd=1)` — normal density.
pub unsafe fn do_dnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_dist_unary(args, 0.0, 1.0, |x, m, s| crate::dist::normal::dnorm4_inner(x, m, s, false)) }
}

/// R's `pnorm(q, mean=0, sd=1)` — normal CDF.
pub unsafe fn do_pnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_dist_unary(args, 0.0, 1.0, |q, m, s| crate::dist::normal::pnorm5_inner(q, m, s, true, false)) }
}

/// R's `qnorm(p, mean=0, sd=1)` — normal quantile.
pub unsafe fn do_qnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_dist_unary(args, 0.0, 1.0, |p, m, s| crate::dist::normal::qnorm5_inner(p, m, s, true, false)) }
}

/// R's `dpois(x, lambda)` — Poisson density.
pub unsafe fn do_dpois(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_dist_unary(args, 1.0, 0.0, |x, lam, _| crate::dist::poisson::dpois_inner(x, lam, false)) }
}

/// R's `ppois(q, lambda)` — Poisson CDF.
pub unsafe fn do_ppois(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_dist_unary(args, 1.0, 0.0, |q, lam, _| crate::dist::poisson::ppois_inner(q, lam, true, false)) }
}

/// R's `dbinom(x, size, prob)` — binomial density.
pub unsafe fn do_dbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_dist_unary(args, 1.0, 0.5, |x, n, p| crate::dist::binomial::dbinom_inner(x, n, p, false)) }
}

/// R's `pbinom(q, size, prob)` — binomial CDF.
pub unsafe fn do_pbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_dist_unary(args, 1.0, 0.5, |q, n, p| crate::dist::binomial::pbinom_inner(q, n, p, true, false)) }
}

/// R's `dexp(x, rate)` — exponential density.
pub unsafe fn do_dexp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_dist_unary(args, 1.0, 0.0, |x, rate, _| crate::dist::exponential::dexp(x, 1.0 / rate, 1)) }
}

/// R's `pexp(q, rate)` — exponential CDF.
pub unsafe fn do_pexp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_dist_unary(args, 1.0, 0.0, |q, rate, _| crate::dist::exponential::pexp_inner(q, 1.0 / rate, true, false)) }
}

/// Generic vectorized distribution function with 2 parameters.
fn do_dist_unary(args: SEXP, default_p1: f64, default_p2: f64, f: fn(f64, f64, f64) -> f64) -> SEXP {
    unsafe {
        let x = CAR(args);
        let p1 = real_or_default(CAR(CDR(args)), default_p1);
        let p2_arg = CAR(CDR(CDR(args)));
        let p2 = if p2_arg.is_null() || p2_arg == R_NilValue() { default_p2 } else { real_or_default(p2_arg, default_p2) };
        if x.is_null() { return R_NilValue(); }
        let n = XLENGTH(x).max(1);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
        if result.is_null() { return R_NilValue(); }
        let _p = Rf_protect(result);
        let dst = REAL(result);
        for i in 0..n { *dst.add(i as usize) = f(elt_real_safe(x, i), p1, p2); }
        crate::sexp::protect::Rf_unprotect(1);
        result
    }
}

fn elt_real_safe(x: SEXP, i: R_xlen_t) -> f64 {
    unsafe {
        if x.is_null() { return NA_REAL; }
        let n = XLENGTH(x);
        let idx = if n == 0 { 0 } else { i % n };
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP.0 { *REAL(x).add(idx as usize) }
        else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
            let v = *INTEGER(x).add(idx as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else { NA_REAL }
    }
}
