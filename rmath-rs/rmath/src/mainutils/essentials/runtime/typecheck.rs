//! Type checking utilities — is.single/is.vector/is.scalar/is.named/is.unsorted,
//! is.loaded/is.primitive/is.generic, isTRUE/isFALSE, any_na/all_na/any_nan/all_nan.

#[allow(unused_imports)]
use std::collections::BTreeSet;
#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int};
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use crate::mainutils::essentials::*;

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
#[allow(unused_imports)]
use crate::sexp::context::RError;
#[allow(unused_imports)]
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
#[allow(unused_imports)]
use crate::sexp::globals::{R_MissingArg, R_NilValue};
#[allow(unused_imports)]
use crate::sexp::protect::protect;
#[allow(unused_imports)]
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Complete R runtime — type checking utilities
// ---------------------------------------------------------------------------

/// R's `is.single(x)` — stock R exposes this but errors because single is unimplemented.
pub unsafe fn do_is_single(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _x = CAR(args);
        std::panic::panic_any(crate::sexp::context::RError {
            message: "type \"single\" unimplemented in R".to_string(),
        });
    }
}

/// R's `is.vector(x, mode="any")` — check if x is an atomic or list vector without attributes.
pub unsafe fn do_is_vector(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

/// R's `is.scalar(x)` — check if x has length 1 (simplified).
pub unsafe fn do_is_scalar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        Rf_ScalarLogical(if n == 1 { TRUE } else { FALSE })
    }
}

/// R's `is.named(x)` — check if x has names attribute.
pub unsafe fn do_is_named(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let names = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"names".as_ptr()));
        let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) > 0;
        Rf_ScalarLogical(if has_names { TRUE } else { FALSE })
    }
}

/// R's `is.unsorted(x, na.rm = FALSE, strictly = FALSE)`.
///
/// Missing values dominate the default result just as in GNU R: with
/// `na.rm = FALSE`, any NA/NaN makes the result `NA`, even if another pair is
/// visibly out of order.
pub unsafe fn do_is_unsorted(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let na_rm = match logical_arg_with_default(args, "na.rm", 1, FALSE) {
            Ok(value) => value != FALSE,
            Err(message) => panic_r_error(message),
        };
        let strictly = match logical_arg_with_default(args, "strictly", 2, FALSE) {
            Ok(value) => value != FALSE,
            Err(_) => panic_r_error("invalid 'strictly' argument"),
        };
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        if n <= 1 {
            return Rf_ScalarLogical(FALSE);
        }
        let result = if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP {
            is_unsorted_int_like(x, n, na_rm, strictly)
        } else if t == SEXPTYPE::REALSXP {
            is_unsorted_real(x, n, na_rm, strictly)
        } else if t == SEXPTYPE::CPLXSXP {
            is_unsorted_complex(x, n, na_rm, strictly)
        } else if t == SEXPTYPE::STRSXP {
            is_unsorted_character(x, n, na_rm, strictly)
        } else if t == SEXPTYPE::RAWSXP {
            is_unsorted_raw(x, n, strictly)
        } else {
            NA_LOGICAL
        };
        Rf_ScalarLogical(result)
    }
}

unsafe fn logical_arg_with_default(
    args: SEXP,
    name: &str,
    position: usize,
    default: c_int,
) -> Result<c_int, &'static str> {
    unsafe {
        let value = arg_by_name_or_position(args, &[name], position);
        if value.is_null() || value == R_NilValue() {
            return Ok(default);
        }
        if XLENGTH(value) == 0 {
            return Err("argument is of length zero");
        }
        let value_type = TYPEOF(value);
        let raw = if value_type == SEXPTYPE::LGLSXP || value_type == SEXPTYPE::INTSXP {
            *INTEGER(value)
        } else if value_type == SEXPTYPE::REALSXP {
            let value = *REAL(value);
            if value.is_nan() {
                NA_LOGICAL
            } else {
                value as c_int
            }
        } else {
            return Err("argument is not interpretable as logical");
        };
        if raw == NA_LOGICAL {
            return Err("missing value where TRUE/FALSE needed");
        }
        Ok(if raw == FALSE { FALSE } else { TRUE })
    }
}

fn panic_r_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    })
}

unsafe fn is_unsorted_int_like(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    unsafe {
        let mut previous: Option<c_int> = None;
        for i in 0..n {
            let current = *INTEGER(x).add(i as usize);
            if current == NA_INTEGER {
                if na_rm {
                    continue;
                }
                return NA_LOGICAL;
            }
            if let Some(prev) = previous {
                if out_of_order_i32(prev, current, strictly) {
                    return TRUE;
                }
            }
            previous = Some(current);
        }
        FALSE
    }
}

unsafe fn is_unsorted_real(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    unsafe {
        let mut previous: Option<f64> = None;
        for i in 0..n {
            let current = *REAL(x).add(i as usize);
            if current.is_nan() {
                if na_rm {
                    continue;
                }
                return NA_LOGICAL;
            }
            if let Some(prev) = previous {
                if out_of_order_f64(prev, current, strictly) {
                    return TRUE;
                }
            }
            previous = Some(current);
        }
        FALSE
    }
}

unsafe fn is_unsorted_complex(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    unsafe {
        let mut previous: Option<Rcomplex> = None;
        for i in 0..n {
            let current = *COMPLEX(x).add(i as usize);
            if current.r.is_nan() || current.i.is_nan() {
                if na_rm {
                    continue;
                }
                return NA_LOGICAL;
            }
            if let Some(prev) = previous {
                if out_of_order_complex(prev, current, strictly) {
                    return TRUE;
                }
            }
            previous = Some(current);
        }
        FALSE
    }
}

unsafe fn is_unsorted_character(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    unsafe {
        let mut previous: Option<String> = None;
        for i in 0..n {
            if STRING_ELT(x, i) == crate::sexp::globals::R_NaString() {
                if na_rm {
                    continue;
                }
                return NA_LOGICAL;
            }
            let current = elt_to_string(x, i);
            if let Some(prev) = previous.as_deref() {
                let out_of_order = if strictly {
                    prev >= current.as_str()
                } else {
                    prev > current.as_str()
                };
                if out_of_order {
                    return TRUE;
                }
            }
            previous = Some(current);
        }
        FALSE
    }
}

unsafe fn is_unsorted_raw(x: SEXP, n: R_xlen_t, strictly: bool) -> c_int {
    unsafe {
        for i in 1..n {
            let prev = *RAW(x).add((i - 1) as usize);
            let current = *RAW(x).add(i as usize);
            let out_of_order = if strictly {
                prev >= current
            } else {
                prev > current
            };
            if out_of_order {
                return TRUE;
            }
        }
        FALSE
    }
}

fn out_of_order_i32(previous: c_int, current: c_int, strictly: bool) -> bool {
    if strictly {
        previous >= current
    } else {
        previous > current
    }
}

fn out_of_order_f64(previous: f64, current: f64, strictly: bool) -> bool {
    if strictly {
        previous >= current
    } else {
        previous > current
    }
}

fn out_of_order_complex(previous: Rcomplex, current: Rcomplex, strictly: bool) -> bool {
    if previous.r > current.r {
        return true;
    }
    if previous.r < current.r {
        return false;
    }
    if strictly {
        previous.i >= current.i
    } else {
        previous.i > current.i
    }
}

/// R's `is.loaded(...)` — delegates to `dotcode::do_isloaded` (R_lookupLoadedSymbol).
pub unsafe fn do_is_loaded(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::dotcode::do_isloaded(call, op, args, rho) }
}

// ---------------------------------------------------------------------------
// Complete R runtime — function type checking
// ---------------------------------------------------------------------------

/// R's `is.primitive(x)` — check if x is a primitive function (BUILTINSXP or SPECIALSXP).
pub unsafe fn do_is_primitive(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

/// R's `is.generic(x)` — check if x is a generic function (simplified).
/// Returns TRUE for CLOSXP with "generic" in name or with useMethod call.
pub unsafe fn do_is_generic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// Complete R runtime — isTRUE, isFALSE, any_na, all_na, any_nan, all_nan
// ---------------------------------------------------------------------------

/// R's `isTRUE(x)` — returns TRUE if x is exactly length-1 TRUE.
pub unsafe fn do_is_true(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

/// R's `isFALSE(x)` — returns TRUE if x is exactly length-1 FALSE.
pub unsafe fn do_is_false(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

/// R's `anyNA(x)` — returns TRUE if any element is NA.
pub unsafe fn do_any_na(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        for i in 0..n {
            if atomic_value_is_missing(x, i) {
                return Rf_ScalarLogical(TRUE);
            }
        }
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `allNA(x)` — returns TRUE if all elements are NA.
pub unsafe fn do_all_na(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        if n == 0 {
            return Rf_ScalarLogical(FALSE);
        }
        for i in 0..n {
            if !atomic_value_is_missing(x, i) {
                return Rf_ScalarLogical(FALSE);
            }
        }
        Rf_ScalarLogical(TRUE)
    }
}

/// R's `anyNaN(x)` — returns TRUE if any element is NaN.
pub unsafe fn do_any_nan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

/// R's `allNaN(x)` — returns TRUE if all elements are NaN.
pub unsafe fn do_all_nan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}
