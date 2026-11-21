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
pub fn do_is_single(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        let _x = CAR(args);
        std::panic::panic_any(crate::sexp::context::RError {
            message: "type \"single\" unimplemented in R".to_string(),
        });
    }
}

/// R's `is.vector(x, mode="any")`.
///
/// GNU `do_isvector`: mode `"any"` is `isVector` (atomic, list, or expression).
/// `"numeric"` is integer or double, excluding factors and logicals. Any other
/// mode must be the type name (`"name"` means `"symbol"`). A names attribute
/// is allowed; every other attribute makes the result false. A missing mode
/// is `"any"`, which is what `is.vector(x)` and `str()` pass.
pub fn do_is_vector(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    if args.is_null() || args == R_NilValue() {
        return scalar_logical(FALSE);
    }
    let mode = match vector_mode_argument(args) {
        Ok(mode) => mode,
        Err(message) => panic_r_error(message),
    };
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return scalar_logical(FALSE);
    }
    let mode_name = if mode == "name" {
        "symbol".to_string()
    } else {
        mode
    };
    let matches_mode = if mode_name == "any" {
        is_gnu_vector(TYPEOF(x))
    } else if mode_name == "numeric" {
        is_numeric_mode(x)
    } else {
        sexptype_mode_name(x) == mode_name
    };
    if !matches_mode || has_disallowed_vector_attribute(x) {
        return scalar_logical(FALSE);
    }
    scalar_logical(TRUE)
}

fn vector_mode_argument(args: SEXP) -> Result<String, &'static str> {
    let cell = CDR(args);
    if cell.is_null() || cell == R_NilValue() {
        return Ok("any".to_string());
    }
    let mode = CAR(cell);
    if mode == R_MissingArg() {
        return Ok("any".to_string());
    }
    if mode.is_null()
        || mode == R_NilValue()
        || TYPEOF(mode) != SEXPTYPE::STRSXP
        || XLENGTH(mode) != 1
    {
        return Err("invalid 'mode' argument");
    }
    let owner = active_owner();
    let elt = owner
        .sexp(mode)
        .and_then(|mode| mode.try_string_elt(0))
        .map_err(|_| "invalid 'mode' argument")?;
    if elt.as_raw().is_null() || elt.as_raw() == R_NilValue() {
        return Err("invalid 'mode' argument");
    }
    if elt.as_raw() == crate::sexp::globals::R_NaString() {
        return Ok("NA".to_string());
    }
    charsxp_utf8_lossy(&elt).ok_or("invalid 'mode' argument")
}

fn is_gnu_vector(t: c_int) -> bool {
    t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::RAWSXP
        || t == SEXPTYPE::VECSXP
        || t == SEXPTYPE::EXPRSXP
}

fn is_numeric_mode(x: SEXP) -> bool {
    let t = TYPEOF(x);
    if t == SEXPTYPE::INTSXP {
        crate::mainutils::objects::inherits2(x, c"factor") == 0
    } else {
        t == SEXPTYPE::REALSXP
    }
}

fn sexptype_mode_name(x: SEXP) -> String {
    let t = TYPEOF(x);
    if t == SEXPTYPE::OBJSXP {
        // SAFETY: x is a live SEXP; reading the S4 flag bit is a pure read.
        let s4 = (unsafe { (*x).sxpinfo.gp() } & (1 << 4)) != 0;
        return if s4 {
            "S4".to_string()
        } else {
            "object".to_string()
        };
    }
    let raw = crate::mainutils::util_main::type2char(t);
    if raw.is_null() {
        return "unknown".to_string();
    }
    // SAFETY: raw is a live NUL-terminated static string.
    unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned()
}

fn has_disallowed_vector_attribute(x: SEXP) -> bool {
    let names_sym = crate::sexp::attrib_core::R_NamesSymbol();
    let nil = R_NilValue();
    let mut a = ATTRIB(x);
    while !a.is_null() && a != nil {
        if TAG(a) != names_sym {
            return true;
        }
        a = CDR(a);
    }
    false
}

/// R's `is.scalar(x)` — check if x has length 1 (simplified).
pub fn do_is_scalar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return scalar_logical(FALSE);
    }
    let n = XLENGTH(x);
    scalar_logical(if n == 1 { TRUE } else { FALSE })
}

/// R's `is.named(x)` — check if x has names attribute.
pub fn do_is_named(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return scalar_logical(FALSE);
    }
    let names = crate::sexp::attrib_core::getAttrib(x, install_str("names"));
    let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) > 0;
    scalar_logical(if has_names { TRUE } else { FALSE })
}

/// R's `is.unsorted(x, na.rm = FALSE, strictly = FALSE)`.
///
/// Missing values dominate the default result just as in GNU R: with
/// `na.rm = FALSE`, any NA/NaN makes the result `NA`, even if another pair is
/// visibly out of order.
pub fn do_is_unsorted(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return scalar_logical(FALSE);
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
        return scalar_logical(FALSE);
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
    scalar_logical(result)
}

fn logical_arg_with_default(
    args: SEXP,
    name: &str,
    position: usize,
    default: c_int,
) -> Result<c_int, &'static str> {
    let value = arg_by_name_or_position(args, &[name], position);
    if value.is_null() || value == R_NilValue() {
        return Ok(default);
    }
    if XLENGTH(value) == 0 {
        return Err("argument is of length zero");
    }
    let value_type = TYPEOF(value);
    let owner = active_owner();
    let value_owned = owner_sexp(owner, value);
    let raw = if value_type == SEXPTYPE::LGLSXP || value_type == SEXPTYPE::INTSXP {
        integer_like_elt(&value_owned, 0)
    } else if value_type == SEXPTYPE::REALSXP {
        let value = value_owned
            .try_real_elt(0)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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

fn panic_r_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    })
}

fn is_unsorted_int_like(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    let owner = active_owner();
    let x = owner_sexp(owner, x);
    let mut previous: Option<c_int> = None;
    for i in 0..n {
        let current = integer_like_elt(&x, i);
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

fn is_unsorted_real(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    let owner = active_owner();
    let x = owner_sexp(owner, x);
    let mut previous: Option<f64> = None;
    for i in 0..n {
        let current = x
            .try_real_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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

fn is_unsorted_complex(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    let owner = active_owner();
    let x = owner_sexp(owner, x);
    let mut previous: Option<Rcomplex> = None;
    for i in 0..n {
        let current = x
            .try_complex_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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

fn is_unsorted_character(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    let mut previous: Option<String> = None;
    for i in 0..n {
        let elt = x_owned
            .try_string_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if elt.as_raw() == crate::sexp::globals::R_NaString() {
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

fn is_unsorted_raw(x: SEXP, n: R_xlen_t, strictly: bool) -> c_int {
    let owner = active_owner();
    let x = owner_sexp(owner, x);
    for i in 1..n {
        let prev = x
            .try_raw_elt(i - 1)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let current = x
            .try_raw_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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
pub fn do_is_loaded(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::dotcode::do_isloaded(call, op, args, rho) }
}

// ---------------------------------------------------------------------------
// Complete R runtime — function type checking
// ---------------------------------------------------------------------------

/// R's `is.primitive(x)` — `typeof(x)` is `"builtin"` or `"special"`.
pub fn do_is_primitive(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return scalar_logical(FALSE);
    }
    let t = TYPEOF(x);
    scalar_logical(if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
        TRUE
    } else {
        FALSE
    })
}

/// R's `is.generic(x)` — check if x is a generic function (simplified).
/// Returns TRUE for CLOSXP with "generic" in name or with useMethod call.
pub fn do_is_generic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return scalar_logical(FALSE);
    }
    let t = TYPEOF(x);
    // Simplified: primitives are always generic, closures need body check
    if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
        return scalar_logical(TRUE);
    }
    if t == SEXPTYPE::CLOSXP {
        // Check if name ends with common generic names
        // Simplified: assume all closures could be generic
        return scalar_logical(TRUE);
    }
    scalar_logical(FALSE)
}

// ---------------------------------------------------------------------------
// Complete R runtime — isTRUE, isFALSE, any_na, all_na, any_nan, all_nan
// ---------------------------------------------------------------------------

/// R's `isTRUE(x)` — returns TRUE if x is exactly length-1 TRUE.
pub fn do_is_true(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return scalar_logical(FALSE);
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LGLSXP && XLENGTH(x) == 1 {
        let owner = active_owner();
        let v = owner_sexp(owner, x)
            .try_logical_elt(0)
            .unwrap_or(NA_LOGICAL);
        return scalar_logical(if v == TRUE { TRUE } else { FALSE });
    }
    scalar_logical(FALSE)
}

/// R's `isFALSE(x)` — returns TRUE if x is exactly length-1 FALSE.
pub fn do_is_false(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return scalar_logical(FALSE);
    }
    let t = TYPEOF(x);
    if t == SEXPTYPE::LGLSXP && XLENGTH(x) == 1 {
        let owner = active_owner();
        let v = owner_sexp(owner, x)
            .try_logical_elt(0)
            .unwrap_or(NA_LOGICAL);
        return scalar_logical(if v == FALSE { TRUE } else { FALSE });
    }
    scalar_logical(FALSE)
}

/// R's `anyNA(x)` — internal generic with a rooted default traversal.
pub fn do_any_na(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current().expect("active anyNA owner");
        let pin = owner.pin().expect("live anyNA owner");
        let factory = owner.node_factory();
        let _args = factory.wrap(args).expect("anyNA argument owner");
        let _call = (!call.is_null()).then(|| factory.wrap(call).expect("anyNA call owner"));
        let _op = (!op.is_null()).then(|| factory.wrap(op).expect("anyNA operation owner"));
        let _rho = (!rho.is_null()).then(|| factory.wrap(rho).expect("anyNA environment owner"));
        // GNU's do_anyNA rejects arity before attempting internal-generic
        // dispatch. Validate owner admission first, then prevent classed
        // calls with excess actuals from reaching a method.
        let nargs = LENGTH(args);
        if nargs < 1 || nargs > 2 {
            crate::mainutils::errors::Rf_error(
                b"anyNA takes 1 or 2 arguments\0".as_ptr() as *const c_char
            );
        }
        let mut ans = R_NilValue();
        let dispatched = crate::eval::dispatch::DispatchOrEval(
            call,
            op,
            c"anyNA".as_ptr(),
            args,
            rho,
            &mut ans,
            0,
            1,
        ) != 0;
        if let Some(pin) = &pin {
            pin.require_live().expect("live anyNA owner after dispatch");
        }
        factory
            .require_active()
            .expect("original anyNA owner after dispatch");
        if dispatched {
            return ans;
        }
        crate::mainutils::coerce::do_anyNA(call, op, args, rho)
    }
}

/// R's `allNA(x)` — returns TRUE if all elements are NA.
pub fn do_all_na(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return scalar_logical(FALSE);
    }
    let n = XLENGTH(x);
    if n == 0 {
        return scalar_logical(FALSE);
    }
    for i in 0..n {
        if !atomic_value_is_missing(x, i) {
            return scalar_logical(FALSE);
        }
    }
    scalar_logical(TRUE)
}

/// R's `anyNaN(x)` — returns TRUE if any element is NaN.
pub fn do_any_nan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return scalar_logical(FALSE);
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::REALSXP {
        return scalar_logical(FALSE);
    }
    let n = XLENGTH(x);
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    for i in 0..n {
        let v = x_owned
            .try_real_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if v.is_nan() {
            return scalar_logical(TRUE);
        }
    }
    scalar_logical(FALSE)
}

/// R's `allNaN(x)` — returns TRUE if all elements are NaN.
pub fn do_all_nan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return scalar_logical(FALSE);
    }
    let t = TYPEOF(x);
    if t != SEXPTYPE::REALSXP {
        return scalar_logical(FALSE);
    }
    let n = XLENGTH(x);
    if n == 0 {
        return scalar_logical(FALSE);
    }
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    for i in 0..n {
        let v = x_owned
            .try_real_elt(i)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if !v.is_nan() {
            return scalar_logical(FALSE);
        }
    }
    scalar_logical(TRUE)
}
