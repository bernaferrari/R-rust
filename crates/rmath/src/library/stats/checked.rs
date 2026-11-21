//! Checked-access boundary helpers shared by the stats `.Call`/`.Internal`
//! adapters.
//!
//! Stats adapters run inside evaluated R calls, so every raw SEXP they handle
//! is live under the active runtime. The one operation that still needs
//! `unsafe` -- admitting the ambient runtime owner -- is kept here as a small
//! audited boundary so the adapters themselves can stay on checked element
//! access.

use std::os::raw::{c_double, c_int};

use crate::sexp::context::r_error;
use crate::sexp::ffi::{R_xlen_t, SEXP};
use crate::sexp::object::{SessionNodeFactory, Sexp, SexpMut, SexpResult};
use crate::sexp::owner::OwnerToken;

/// Unwrap a checked result, raising an R error on failure.
pub(crate) fn checked<T>(result: SexpResult<T>) -> T {
    result.unwrap_or_else(|error| r_error(error.to_string()))
}

/// The runtime owner retained by the active R call.
pub(crate) fn current_owner() -> OwnerToken<'static> {
    // SAFETY: stats adapters only run inside evaluated R calls, where the
    // interpreter retains the current runtime owner across the dispatch.
    checked(unsafe { OwnerToken::current() })
}

/// Checked node factory bound to the active runtime session.
pub(crate) fn node_factory() -> SessionNodeFactory<'static> {
    SessionNodeFactory::new(current_owner())
}

/// Admit a live raw SEXP into the checked object view.
pub(crate) fn wrapped(x: SEXP) -> Sexp<'static> {
    checked(node_factory().wrap(x))
}

/// Admit a live raw SEXP as a mutable checked vector view.
pub(crate) fn mutable(x: SEXP) -> SexpMut<'static> {
    checked(node_factory().wrap(x).and_then(SexpMut::try_from_checked))
}

/// Checked CHARSXP from Rust text; matches `Rf_mkChar` on the session heap.
pub(crate) fn mk_char(text: &str) -> SEXP {
    crate::sexp::constructors::mk_char_str(text)
}

/// Checked STRSXP(1) scalar; matches `Rf_mkString` on the session heap.
pub(crate) fn mk_string(text: &str) -> SEXP {
    crate::sexp::constructors::mk_string_str(text)
}

/// Copy a REALSXP payload through checked element reads.
pub(crate) fn real_vec(x: SEXP) -> Vec<c_double> {
    let value = wrapped(x);
    (0..value.len())
        .map(|i| checked(value.try_real_elt(i)))
        .collect()
}

/// Copy an INTSXP payload through checked element reads.
pub(crate) fn int_vec(x: SEXP) -> Vec<c_int> {
    let value = wrapped(x);
    (0..value.len())
        .map(|i| checked(value.try_integer_elt(i)))
        .collect()
}

/// Copy an LGLSXP payload through checked element reads.
pub(crate) fn logical_vec(x: SEXP) -> Vec<c_int> {
    let value = wrapped(x);
    (0..value.len())
        .map(|i| checked(value.try_logical_elt(i)))
        .collect()
}

/// Write `values` into the leading cells of a REALSXP through checked writes.
pub(crate) fn set_reals(x: SEXP, values: &[c_double]) {
    let mut out = mutable(x);
    for (i, &v) in values.iter().enumerate() {
        checked(out.try_set_real_elt(i as R_xlen_t, v));
    }
}

/// Write `values` into the leading cells of an INTSXP through checked writes.
pub(crate) fn set_integers(x: SEXP, values: &[c_int]) {
    let mut out = mutable(x);
    for (i, &v) in values.iter().enumerate() {
        checked(out.try_set_integer_elt(i as R_xlen_t, v));
    }
}

/// Write `values` into the leading cells of an LGLSXP through checked writes.
pub(crate) fn set_logicals(x: SEXP, values: &[c_int]) {
    let mut out = mutable(x);
    for (i, &v) in values.iter().enumerate() {
        checked(out.try_set_logical_elt(i as R_xlen_t, v));
    }
}

/// Evaluate a live expression in a live environment.
pub(crate) fn eval(expr: SEXP, env: SEXP) -> SEXP {
    crate::eval::eval::eval_inner(expr, env)
}

/// Deep-duplicate a live object.
pub(crate) fn duplicate(x: SEXP) -> SEXP {
    crate::mainutils::seq::Rf_duplicate(x)
}

/// UTF-8 bytes of a live CHARSXP (`CHAR(x)` without the NUL); like GNU,
/// `NA_STRING` reads as `"NA"`.
pub(crate) fn char_bytes(x: SEXP) -> Vec<u8> {
    if x == crate::sexp::globals::R_NaString() {
        return b"NA".to_vec();
    }
    crate::sexp::accessors::charsxp_as_utf8(x)
}

/// UTF-8 text of a live CHARSXP, lossily decoded.
pub(crate) fn char_string(x: SEXP) -> String {
    String::from_utf8_lossy(&char_bytes(x)).into_owned()
}

/// Print text through `Rprintf`, the routing GNU's native code uses.
pub(crate) fn rprintf(text: &str) {
    let line = std::ffi::CString::new(text).unwrap_or_default();
    // SAFETY: `line` is a live NUL-terminated buffer for the whole call, and
    // Rprintf copies it before returning; no varargs are consumed.
    unsafe { crate::mainutils::printutils::Rprintf(line.as_ptr(), std::ptr::null_mut()) }
}

/// Allocate an `nrow x ncol` matrix (GNU `allocMatrix`).
pub(crate) fn alloc_matrix(mode: c_int, nrow: c_int, ncol: c_int) -> SEXP {
    // SAFETY: allocMatrix only validates its extents and allocates a fresh
    // vector with a `dim` attribute; it has no caller-side preconditions.
    unsafe { crate::mainutils::array::allocMatrix(mode, nrow, ncol) }
}

/// Bind `symbol` to `value` in `rho`'s frame (GNU `defineVar`).
pub(crate) fn define_var(symbol: SEXP, value: SEXP, rho: SEXP) {
    let _ = crate::sexp::envir::define_var_updates(symbol, value, rho);
}

/// Find a function binding for `symbol` from `rho` (GNU `findFun`), or
/// `R_UnboundValue` when there is none.
pub(crate) fn find_fun(symbol: SEXP, rho: SEXP) -> SEXP {
    lookup(symbol, rho, crate::sexp::envir::find_fun_safe)
}

/// Find a variable binding for `symbol` from `rho` (GNU `findVar`), or
/// `R_UnboundValue` when there is none.
pub(crate) fn find_var(symbol: SEXP, rho: SEXP) -> SEXP {
    lookup(symbol, rho, crate::sexp::envir::find_var_safe)
}

fn lookup(
    symbol: SEXP,
    rho: SEXP,
    find: fn(Sexp<'static>, Sexp<'static>) -> Option<Sexp<'static>>,
) -> SEXP {
    let unbound = crate::sexp::globals::R_UnboundValue();
    if symbol.is_null() {
        return unbound;
    }
    let factory = node_factory();
    match (factory.wrap(symbol), factory.wrap(rho)) {
        (Ok(symbol), Ok(rho)) => find(symbol, rho)
            .map(|value| value.as_raw())
            .unwrap_or(unbound),
        _ => unbound,
    }
}

/// Build a closure over live `formals`, `body` and `env` (GNU `mkCLOSXP`).
pub(crate) fn mk_closure(formals: SEXP, body: SEXP, env: SEXP) -> SEXP {
    // SAFETY: the three nodes are live for the call; mkCLOSXP only allocates
    // the closure cell that captures them.
    unsafe { crate::mainutils::dstruct::mkCLOSXP(formals, body, env) }
}

/// Number of rows of a matrix-like object (GNU `nrows`).
pub(crate) fn nrows(x: SEXP) -> c_int {
    use crate::sexp::accessors::{INTEGER_ELT, LENGTH};
    if x.is_null() || x == crate::sexp::globals::R_NilValue() {
        return -1;
    }
    let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
    if dim.is_null() || dim == crate::sexp::globals::R_NilValue() {
        return LENGTH(x);
    }
    if LENGTH(dim) >= 1 {
        INTEGER_ELT(dim, 0)
    } else {
        -1
    }
}

/// Number of columns of a matrix-like object (GNU `ncols`).
pub(crate) fn ncols(x: SEXP) -> c_int {
    use crate::sexp::accessors::{INTEGER_ELT, LENGTH};
    if x.is_null() || x == crate::sexp::globals::R_NilValue() {
        return -1;
    }
    let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
    if dim.is_null() || dim == crate::sexp::globals::R_NilValue() {
        return 1;
    }
    if LENGTH(dim) >= 2 {
        INTEGER_ELT(dim, 1)
    } else {
        1
    }
}
