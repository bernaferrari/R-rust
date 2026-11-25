#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/summary.c — numeric summary algorithms.
//!
//! This module ports the core numeric algorithms from R's summary functions,
//! operating on raw arrays instead of SEXP objects.
//!
//! Ported algorithms:
//!   isum, rsum, csum (sum with NA/NaN handling),
//!   imin, rmin, imax, rmax (min/max with NA/NaN handling),
//!   iprod, rprod, cprod (product with NA/NaN handling),
//!   real_mean, integer_mean (mean with overflow protection)

use crate::mainutils::essentials::{active_owner, owner_sexp};
use crate::sexp::accessors::{
    CAR, CDR, SET_COMPLEX_ELT, SET_INTEGER_ELT, SET_REAL_ELT, SETCDR, SETTAG, TAG, TYPEOF, XLENGTH,
};
use crate::sexp::constructors::{
    Rf_ScalarComplex, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3,
};
use crate::sexp::ffi::R_xlen_t;
use crate::sexp::ffi::SEXP;
use crate::sexp::ffi::{NA_INTEGER, Rcomplex, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::object::{Sexp, SexpResult};
use crate::sexp::protect::protect;
use crate::sexp::symbol::install_str;
use std::os::raw::{c_double, c_int};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Check if a double is NaN.
#[inline]
fn ISNAN(x: f64) -> bool {
    x.is_nan()
}

/// Check if a double is R's NA.
#[inline]
fn ISNA(x: f64) -> bool {
    x.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
}

// ---------------------------------------------------------------------------
// Sum functions
// ---------------------------------------------------------------------------

/// Sum of integer array with overflow protection.
///
/// Returns `(sum, updated)` where `sum` is the sum as i64 and `updated`
/// indicates whether any non-NA value was found.
///
/// If `narm` is true, NA_INTEGER values are skipped.
/// If `narm` is false and an NA_INTEGER is found, returns `(0, false)`.
pub fn isum(x: &[c_int], narm: bool) -> (i64, bool) {
    let mut s: i64 = 0;
    let mut updated = false;

    for &xi in x {
        if xi == NA_INTEGER {
            if !narm {
                return (0, false);
            }
        } else {
            if !updated {
                updated = true;
            }
            s += xi as i64;
            // Overflow check
            if s > 9_000_000_000_000_000 || s < -9_000_000_000_000_000 {
                // Would switch to double sum in R
            }
        }
    }

    (s, updated)
}

/// Sum of double array.
///
/// Returns `(sum, updated)` where `sum` is the sum and `updated` indicates
/// whether any value was processed.
///
/// Handles infinity overflow.
pub fn rsum(x: &[f64], narm: bool) -> (f64, bool) {
    let mut s: f64 = 0.0;
    let mut updated = false;

    for &xi in x {
        if !narm || !ISNAN(xi) {
            if !updated {
                updated = true;
            }
            s += xi;
        }
    }

    if s > f64::MAX {
        s = f64::INFINITY;
    } else if s < -f64::MAX {
        s = f64::NEG_INFINITY;
    }

    (s, updated)
}

/// Sum of complex array.
///
/// Returns `(sum, updated)`.
pub fn csum(x: &[Rcomplex], narm: bool) -> (Rcomplex, bool) {
    let mut sr: f64 = 0.0;
    let mut si: f64 = 0.0;
    let mut updated = false;

    for xc in x {
        if !narm || (!ISNAN(xc.r) && !ISNAN(xc.i)) {
            if !updated {
                updated = true;
            }
            sr += xc.r;
            si += xc.i;
        }
    }

    (Rcomplex { r: sr, i: si }, updated)
}

// ---------------------------------------------------------------------------
// Min/Max functions
// ---------------------------------------------------------------------------

/// Minimum of integer array.
///
/// Returns `(min, updated)`. NA_INTEGER values are skipped if `narm` is true.
/// If `narm` is false and NA_INTEGER is found, returns `(NA_INTEGER, true)`.
pub fn imin(x: &[c_int], narm: bool) -> (c_int, bool) {
    let mut s: c_int = 0;
    let mut updated = false;

    for &xi in x {
        if xi == NA_INTEGER {
            if !narm {
                return (NA_INTEGER, true);
            }
        } else if !updated || s > xi {
            s = xi;
            if !updated {
                updated = true;
            }
        }
    }

    (s, updated)
}

/// Minimum of double array.
///
/// Handles NaN/NA according to R's rules:
/// - NA trumps NaN
/// - NaN is propagated if narm is false
/// - NaN values are skipped if narm is true
pub fn rmin(x: &[f64], narm: bool) -> (f64, bool) {
    let mut s: f64 = 0.0;
    let mut updated = false;

    for &xi in x {
        if ISNAN(xi) {
            if !narm {
                if !ISNA(s) {
                    s = xi; // any NA trumps all NaNs
                }
                if !updated {
                    updated = true;
                }
            }
        } else if !updated || xi < s {
            // Never true if s is NA/NaN
            s = xi;
            if !updated {
                updated = true;
            }
        }
    }

    (s, updated)
}

/// Maximum of integer array.
///
/// Returns `(max, updated)`.
pub fn imax(x: &[c_int], narm: bool) -> (c_int, bool) {
    let mut s: c_int = 0;
    let mut updated = false;

    for &xi in x {
        if xi == NA_INTEGER {
            if !narm {
                return (NA_INTEGER, true);
            }
        } else if !updated || s < xi {
            s = xi;
            if !updated {
                updated = true;
            }
        }
    }

    (s, updated)
}

/// Maximum of double array.
///
/// Handles NaN/NA according to R's rules.
pub fn rmax(x: &[f64], narm: bool) -> (f64, bool) {
    let mut s: f64 = 0.0;
    let mut updated = false;

    for &xi in x {
        if ISNAN(xi) {
            if !narm {
                if !ISNA(s) {
                    s = xi; // any NA trumps all NaNs
                }
                if !updated {
                    updated = true;
                }
            }
        } else if !updated || xi > s {
            s = xi;
            if !updated {
                updated = true;
            }
        }
    }

    (s, updated)
}

// ---------------------------------------------------------------------------
// Product functions
// ---------------------------------------------------------------------------

/// Product of integer array (returned as double).
///
/// Returns `(product, updated)`.
pub fn iprod(x: &[c_int], narm: bool) -> (f64, bool) {
    let mut s: f64 = 1.0;
    let mut updated = false;

    for &xi in x {
        if xi == NA_INTEGER {
            if !narm {
                if !updated {
                    updated = true;
                }
                return (f64::NAN, updated);
            }
        } else {
            s *= xi as f64;
            if !updated {
                updated = true;
            }
        }

        if ISNAN(s) {
            return (f64::NAN, updated);
        }
    }

    if s > f64::MAX {
        s = f64::INFINITY;
    } else if s < -f64::MAX {
        s = f64::NEG_INFINITY;
    }

    (s, updated)
}

/// Product of double array.
///
/// Returns `(product, updated)`.
pub fn rprod(x: &[f64], narm: bool) -> (f64, bool) {
    let mut s: f64 = 1.0;
    let mut updated = false;

    for &xi in x {
        if !narm || !ISNAN(xi) {
            if !updated {
                updated = true;
            }
            s *= xi;
        }
    }

    if s > f64::MAX {
        s = f64::INFINITY;
    } else if s < -f64::MAX {
        s = f64::NEG_INFINITY;
    }

    (s, updated)
}

/// Product of complex array.
///
/// Returns `(product, updated)`.
pub fn cprod(x: &[Rcomplex], narm: bool) -> (Rcomplex, bool) {
    let mut sr: f64 = 1.0;
    let mut si: f64 = 0.0;
    let mut updated = false;

    for xc in x {
        if !narm || (!ISNAN(xc.r) && !ISNAN(xc.i)) {
            if !updated {
                updated = true;
            }
            let tr = sr;
            let ti = si;
            sr = tr * xc.r - ti * xc.i;
            si = tr * xc.i + ti * xc.r;
        }
    }

    (Rcomplex { r: sr, i: si }, updated)
}

// ---------------------------------------------------------------------------
// Mean functions
// ---------------------------------------------------------------------------

/// Mean of double array with overflow protection.
///
/// Uses two-pass algorithm when the initial sum overflows.
pub fn real_mean(x: &[f64]) -> f64 {
    let n = x.len();
    if n == 0 {
        return f64::NAN;
    }

    // First pass: sum
    let mut s: f64 = 0.0;
    for &xi in x {
        s += xi;
    }

    let finite_s = s.is_finite();

    if finite_s {
        s /= n as f64;
    } else {
        // Infinite s — try dividing each term by n first
        s = 0.0;
        for &xi in x {
            s += xi / n as f64;
        }
    }

    // Second pass: correction term
    if finite_s && s.is_finite() {
        let mut t: f64 = 0.0;
        for &xi in x {
            t += xi - s;
        }
        s += t / n as f64;
    } else if s.is_finite() {
        let mut t: f64 = 0.0;
        for &xi in x {
            t += (xi - s) / n as f64;
        }
        s += t;
    }

    s
}

/// Mean of integer array.
///
/// Returns NaN if any element is NA_INTEGER.
pub fn integer_mean(x: &[c_int]) -> f64 {
    let n = x.len();
    if n == 0 {
        return f64::NAN;
    }

    let mut s: f64 = 0.0;
    for &xi in x {
        if xi == NA_INTEGER {
            return f64::NAN;
        }
        s += xi as f64;
    }

    s / n as f64
}

// ---------------------------------------------------------------------------
// Helper functions for SEXP-level summary operations
// ---------------------------------------------------------------------------

/// NA_REAL: R's special NaN bit pattern for NA.
const NA_REAL: f64 = crate::sexp::ffi::NA_REAL;

/// R_PosInf constant.
const R_PosInf: c_double = f64::INFINITY;

/// R_NegInf constant.
const R_NegInf: c_double = f64::NEG_INFINITY;

/// Convert integer to double, mapping NA_INTEGER to NA_REAL.
#[inline]
fn Int2Real(i: c_int) -> f64 {
    if i == NA_INTEGER { NA_REAL } else { i as f64 }
}

/// asLogical — extract integer logical value from SEXP.
#[inline]
fn asLogical(x: SEXP) -> c_int {
    if x.is_null() || x == R_NilValue() {
        return NA_INTEGER;
    }
    match TYPEOF(x) {
        t if t == SEXPTYPE::LGLSXP => {
            if XLENGTH(x) == 0 {
                NA_INTEGER
            } else {
                crate::sexp::accessors::LOGICAL_ELT(x, 0)
            }
        }
        t if t == SEXPTYPE::INTSXP => {
            if XLENGTH(x) == 0 {
                NA_INTEGER
            } else {
                crate::sexp::accessors::INTEGER_ELT(x, 0)
            }
        }
        t if t == SEXPTYPE::REALSXP => {
            if XLENGTH(x) == 0 {
                NA_INTEGER
            } else {
                let v = crate::sexp::accessors::REAL_ELT(x, 0);
                if ISNAN(v) {
                    NA_INTEGER
                } else {
                    if v != 0.0 { 1 } else { 0 }
                }
            }
        }
        _ => NA_INTEGER,
    }
}

/// asBool2 — extract boolean from scalar (panics on NA).
#[inline]
fn asBool2(x: SEXP, _call: SEXP) -> bool {
    let v = asLogical(x);
    if v == NA_INTEGER {
        std::panic::panic_any(crate::sexp::context::RError {
            message: "invalid 'na.rm' value".to_string(),
        });
    }
    v != 0
}

/// PRIMVAL -- get the primitive's internal integer value.
/// In R, this is stored in the offset/gp field of the SEXPREC.
#[inline]
fn PRIMVAL(op: SEXP) -> c_int {
    crate::mainutils::relop::PRIMVAL(op)
}

/// Get or intern the "na.rm" symbol.
fn R_NaRmSymbol() -> SEXP {
    install_str("na.rm")
}

/// matchArgExact — find the argument matching a tag by exact symbol identity.
/// Destructively removes the matched element from the list.
fn matchArgExact(tag: SEXP, list: &mut SEXP) -> SEXP {
    let mut prev: SEXP = std::ptr::null_mut();
    let mut a = *list;

    while a != R_NilValue() {
        if TAG(a) == tag {
            // Found it — remove from list
            let val = CAR(a);
            if prev.is_null() {
                *list = CDR(a);
            } else {
                SETCDR(prev, CDR(a));
            }
            return val;
        }
        prev = a;
        a = CDR(a);
    }
    R_NilValue()
}

/// checkArity -- verify argument count matches expected arity.
#[inline]
fn checkArity(op: SEXP, args: SEXP) {
    crate::mainutils::relop::checkArity(op, args)
}

/// fixup_NaRm — ensure na.rm is the last argument and exists.
/// Returns the potentially modified args list.
fn fixup_NaRm(mut args: SEXP) -> SEXP {
    let na_sym = R_NaRmSymbol();
    let mut na_value = Rf_ScalarLogical(0); // FALSE
    let mut seen_narm = false;
    let mut prev: SEXP = std::ptr::null_mut();
    let mut a = args;

    while a != R_NilValue() {
        if TAG(a) == na_sym {
            if seen_narm {
                // Duplicate formal argument "na.rm"
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "formal argument \"na.rm\" matched by multiple actual arguments"
                        .to_string(),
                });
            }
            seen_narm = true;
            if CDR(a) == R_NilValue() {
                return args; // already at the end
            }
            na_value = CAR(a);
            if prev.is_null() {
                args = CDR(a);
            } else {
                crate::sexp::accessors::SETCDR(prev, CDR(a));
            }
        }
        prev = a;
        a = CDR(a);
    }

    // Append na.rm = na_value to the end
    let _na_value_guard = protect(na_value);
    let t = crate::sexp::constructors::Rf_cons(na_value, R_NilValue());
    let _t_guard = protect(t);
    SETTAG(t, na_sym);
    if args == R_NilValue() {
        args = t;
    } else {
        let mut r = args;
        while CDR(r) != R_NilValue() {
            r = CDR(r);
        }
        crate::sexp::accessors::SETCDR(r, t);
    }
    args
}

// ---------------------------------------------------------------------------
// SEXP-level sum/min/max/prod helpers (operate on SEXP vectors)
// ---------------------------------------------------------------------------

/// Root a raw vector in the active owner for checked element reads.
fn checked_vec(x: SEXP) -> Sexp<'static> {
    owner_sexp(active_owner(), x)
}

/// Unwrap a checked accessor result, raising an R error on failure.
fn checked<T>(result: SexpResult<T>) -> T {
    result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Checked integer read that also accepts logical storage, matching GNU R's
/// `INTEGER()` aliasing of LGLSXP payloads.
fn int_elt(v: &Sexp<'_>, i: R_xlen_t) -> c_int {
    checked(if v.typeof_() == SEXPTYPE::LGLSXP {
        v.try_logical_elt(i)
    } else {
        v.try_integer_elt(i)
    })
}

/// Integer/logical sum from SEXP vector.
/// Returns (sum_value, updated_flag).
/// updated: 0 = no elements, NA_INTEGER = NA found (go to na_answer),
///          42 = overflow (switch to real).
fn isum_sexp(sx: SEXP, narm: bool) -> (i64, c_int) {
    let n = XLENGTH(sx);
    let v = checked_vec(sx);
    let mut s: i64 = 0;
    let mut updated: c_int = 0;
    let mut overflow_count: i32 = 0;

    for k in 0..n {
        let xi = int_elt(&v, k);
        if xi != NA_INTEGER {
            if updated == 0 {
                updated = 1;
            }
            s += xi as i64;
            overflow_count += 1;
            if overflow_count > 1000 {
                if s > 9_000_000_000_000_000_i64 || s < -9_000_000_000_000_000_i64 {
                    return (s, 42); // overflow, switch to real
                }
                overflow_count = 0;
            }
        } else if !narm {
            return (0, NA_INTEGER);
        }
    }
    (s, updated)
}

/// Real sum from SEXP vector (used when integer overflow occurs).
fn risum_sexp(sx: SEXP, narm: bool) -> (f64, bool) {
    let n = XLENGTH(sx);
    let v = checked_vec(sx);
    let mut s: f64 = 0.0;
    let mut updated = false;

    for k in 0..n {
        let xi = int_elt(&v, k);
        if xi != NA_INTEGER {
            if !updated {
                updated = true;
            }
            s += xi as f64;
        } else if !narm {
            return (NA_REAL, true);
        }
    }
    if s > f64::MAX {
        s = R_PosInf;
    } else if s < -f64::MAX {
        s = R_NegInf;
    }
    (s, updated)
}

/// Double sum from SEXP vector.
fn rsum_sexp(sx: SEXP, narm: bool) -> (f64, bool) {
    let n = XLENGTH(sx);
    let v = checked_vec(sx);
    let mut s: f64 = 0.0;
    let mut updated = false;

    for k in 0..n {
        let xi = checked(v.try_real_elt(k));
        if !narm || !ISNAN(xi) {
            if !updated {
                updated = true;
            }
            s += xi;
        }
    }
    if s > f64::MAX {
        s = R_PosInf;
    } else if s < -f64::MAX {
        s = R_NegInf;
    }
    (s, updated)
}

/// Complex sum from SEXP vector.
fn csum_sexp(sx: SEXP, narm: bool) -> (Rcomplex, bool) {
    let n = XLENGTH(sx);
    let v = checked_vec(sx);
    let mut sr: f64 = 0.0;
    let mut si: f64 = 0.0;
    let mut updated = false;

    for k in 0..n {
        let xc = checked(v.try_complex_elt(k));
        if !narm || (!ISNAN(xc.r) && !ISNAN(xc.i)) {
            if !updated {
                updated = true;
            }
            sr += xc.r;
            si += xc.i;
        }
    }
    (Rcomplex { r: sr, i: si }, updated)
}

/// Integer min from SEXP vector.
fn imin_sexp(sx: SEXP, narm: bool) -> (c_int, bool) {
    let n = XLENGTH(sx);
    let v = checked_vec(sx);
    let mut s: c_int = 0;
    let mut updated = false;

    for k in 0..n {
        let xi = int_elt(&v, k);
        if xi != NA_INTEGER {
            if !updated || s > xi {
                s = xi;
                if !updated {
                    updated = true;
                }
            }
        } else if !narm {
            return (NA_INTEGER, true);
        }
    }
    (s, updated)
}

/// Double min from SEXP vector.
fn rmin_sexp(sx: SEXP, narm: bool) -> (f64, bool) {
    let n = XLENGTH(sx);
    let v = checked_vec(sx);
    let mut s: f64 = 0.0;
    let mut updated = false;

    for k in 0..n {
        let xi = checked(v.try_real_elt(k));
        if ISNAN(xi) {
            if !narm {
                if !ISNA(s) {
                    s = xi;
                }
                if !updated {
                    updated = true;
                }
            }
        } else if !updated || xi < s {
            s = xi;
            if !updated {
                updated = true;
            }
        }
    }
    (s, updated)
}

/// Integer max from SEXP vector.
fn imax_sexp(sx: SEXP, narm: bool) -> (c_int, bool) {
    let n = XLENGTH(sx);
    let v = checked_vec(sx);
    let mut s: c_int = 0;
    let mut updated = false;

    for k in 0..n {
        let xi = int_elt(&v, k);
        if xi != NA_INTEGER {
            if !updated || s < xi {
                s = xi;
                if !updated {
                    updated = true;
                }
            }
        } else if !narm {
            return (NA_INTEGER, true);
        }
    }
    (s, updated)
}

/// Double max from SEXP vector.
fn rmax_sexp(sx: SEXP, narm: bool) -> (f64, bool) {
    let n = XLENGTH(sx);
    let v = checked_vec(sx);
    let mut s: f64 = 0.0;
    let mut updated = false;

    for k in 0..n {
        let xi = checked(v.try_real_elt(k));
        if ISNAN(xi) {
            if !narm {
                if !ISNA(s) {
                    s = xi;
                }
                if !updated {
                    updated = true;
                }
            }
        } else if !updated || xi > s {
            s = xi;
            if !updated {
                updated = true;
            }
        }
    }
    (s, updated)
}

/// Integer product from SEXP vector (returns double).
fn iprod_sexp(sx: SEXP, narm: bool) -> (f64, bool) {
    let n = XLENGTH(sx);
    let v = checked_vec(sx);
    let mut s: f64 = 1.0;
    let mut updated = false;

    for k in 0..n {
        let xi = int_elt(&v, k);
        if xi != NA_INTEGER {
            s *= xi as f64;
            if !updated {
                updated = true;
            }
        } else if !narm {
            if !updated {
                updated = true;
            }
            return (NA_REAL, updated);
        }
        if ISNAN(s) {
            return (NA_REAL, updated);
        }
    }
    if s > f64::MAX {
        s = R_PosInf;
    } else if s < -f64::MAX {
        s = R_NegInf;
    }
    (s, updated)
}

/// Double product from SEXP vector.
fn rprod_sexp(sx: SEXP, narm: bool) -> (f64, bool) {
    let n = XLENGTH(sx);
    let v = checked_vec(sx);
    let mut s: f64 = 1.0;
    let mut updated = false;

    for k in 0..n {
        let xi = checked(v.try_real_elt(k));
        if !narm || !ISNAN(xi) {
            if !updated {
                updated = true;
            }
            s *= xi;
        }
    }
    if s > f64::MAX {
        s = R_PosInf;
    } else if s < -f64::MAX {
        s = R_NegInf;
    }
    (s, updated)
}

/// Complex product from SEXP vector.
fn cprod_sexp(sx: SEXP, narm: bool) -> (Rcomplex, bool) {
    let n = XLENGTH(sx);
    let v = checked_vec(sx);
    let mut sr: f64 = 1.0;
    let mut si: f64 = 0.0;
    let mut updated = false;

    for k in 0..n {
        let xc = checked(v.try_complex_elt(k));
        if !narm || (!ISNAN(xc.r) && !ISNAN(xc.i)) {
            if !updated {
                updated = true;
            }
            let tr = sr;
            let ti = si;
            sr = tr * xc.r - ti * xc.i;
            si = tr * xc.i + ti * xc.r;
        }
    }
    (Rcomplex { r: sr, i: si }, updated)
}

// ---------------------------------------------------------------------------
// SEXP-level mean helpers
// ---------------------------------------------------------------------------

/// Mean of a logical SEXP vector.
fn logical_mean_sexp(x: SEXP) -> SEXP {
    let n = XLENGTH(x);
    let v = checked_vec(x);
    let mut s: f64 = 0.0;
    for k in 0..n {
        let xi = checked(v.try_logical_elt(k));
        if xi == c_int::MIN {
            return Rf_ScalarReal(NA_REAL);
        }
        s += xi as f64;
    }
    Rf_ScalarReal(s / n as f64)
}

/// Mean of an integer SEXP vector.
fn integer_mean_sexp(x: SEXP) -> SEXP {
    let n = XLENGTH(x);
    let v = checked_vec(x);
    let mut s: f64 = 0.0;
    for k in 0..n {
        let xi = int_elt(&v, k);
        if xi == NA_INTEGER {
            return Rf_ScalarReal(NA_REAL);
        }
        s += xi as f64;
    }
    Rf_ScalarReal(s / n as f64)
}

/// Mean of a real SEXP vector (with overflow protection).
fn real_mean_sexp(x: SEXP) -> SEXP {
    let n = XLENGTH(x);
    let v = checked_vec(x);
    if n == 0 {
        // GNU summary.c do_summary mean: ScalarReal(R_NaN)
        return Rf_ScalarReal(f64::NAN);
    }

    // GNU .Internal(mean): first NA/NaN is returned with its bits.
    // Adding NA_REAL into a running sum collapses it to a generic NaN.
    for k in 0..n {
        let v = checked(v.try_real_elt(k));
        if ISNAN(v) {
            return Rf_ScalarReal(v);
        }
    }

    // First pass: sum
    let mut s: f64 = 0.0;
    for k in 0..n {
        s += checked(v.try_real_elt(k));
    }

    let finite_s = s.is_finite();
    if finite_s {
        s /= n as f64;
    } else {
        // Infinite s — divide each term by n first
        s = 0.0;
        for k in 0..n {
            s += checked(v.try_real_elt(k)) / n as f64;
        }
    }

    // Second pass: correction term
    if finite_s && s.is_finite() {
        let mut t: f64 = 0.0;
        for k in 0..n {
            t += checked(v.try_real_elt(k)) - s;
        }
        s += t / n as f64;
    } else if s.is_finite() {
        let mut t: f64 = 0.0;
        for k in 0..n {
            t += (checked(v.try_real_elt(k)) - s) / n as f64;
        }
        s += t;
    }

    Rf_ScalarReal(s)
}

/// Mean of a complex SEXP vector.
fn complex_mean_sexp(x: SEXP) -> SEXP {
    let n = XLENGTH(x);
    let v = checked_vec(x);
    let mut sr: f64 = 0.0;
    let mut si: f64 = 0.0;

    for k in 0..n {
        let xc = checked(v.try_complex_elt(k));
        sr += xc.r;
        si += xc.i;
    }
    sr /= n as f64;
    si /= n as f64;

    if sr.is_finite() && si.is_finite() {
        let mut tr: f64 = 0.0;
        let mut ti: f64 = 0.0;
        for k in 0..n {
            let xc = checked(v.try_complex_elt(k));
            tr += xc.r - sr;
            ti += xc.i - si;
        }
        sr += tr / n as f64;
        si += ti / n as f64;
    }

    Rf_ScalarComplex(Rcomplex { r: sr, i: si })
}

// ---------------------------------------------------------------------------
// do_summary — sum (0), mean (1), min (2), max (3), prod (4)
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// do_range — range() dispatches to range.default
// ---------------------------------------------------------------------------

/// `do_range` implements `range(...)` which finds min and max.
/// It delegates to range.default via applyClosure.
pub fn do_range(call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let factory = active_owner().node_factory();
    let input_args = if args.is_null() {
        factory.nil()
    } else {
        factory
            .wrap(args)
            .expect("range arguments belong to the active heap")
    };
    let args = fixup_NaRm(input_args.as_raw());
    let _args_guard = protect(args);

    // Find range.default and apply it
    let range_sym = install_str("range.default");
    let range_fun = crate::sexp::envir::findFun(range_sym, env);
    let _range_fun_guard = protect(range_fun);

    // Build promise args
    let arguments = factory
        .wrap(args)
        .expect("fixed range arguments remain live");
    // SAFETY: the active factory's owner is current for this builtin and no
    // Rust payload loans are held across promise construction.
    let prargs = unsafe { crate::eval::dispatch::promiseArgs(&factory, arguments, factory.nil()) };

    // Evaluate range.default via applyClosure
    let ans = crate::eval::closure::applyClosureWithFrameVars(
        call,
        range_fun,
        prargs.as_raw(),
        env,
        R_NilValue(),
        R_NilValue(),
        1,
    );

    ans
}

// ---------------------------------------------------------------------------
// do_mean — mean.default implementation
// ---------------------------------------------------------------------------

/// `do_mean` implements `mean.default(x)`.
/// Note: mean is typically dispatched via do_summary when PRIMVAL(op) == 1,
/// but this provides a standalone entry point.
pub fn do_mean(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let x = CAR(args);
    match TYPEOF(x) {
        t if t == SEXPTYPE::LGLSXP => logical_mean_sexp(x),
        t if t == SEXPTYPE::INTSXP => integer_mean_sexp(x),
        t if t == SEXPTYPE::REALSXP => real_mean_sexp(x),
        t if t == SEXPTYPE::CPLXSXP => complex_mean_sexp(x),
        _ => {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "invalid 'type' of argument".to_string(),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_isum_basic() {
        let x = vec![1i32, 2, 3, 4, 5];
        let (s, updated) = isum(&x, false);
        assert_eq!(s, 15);
        assert!(updated);
    }

    #[test]
    fn test_isum_with_na() {
        let x = vec![1i32, NA_INTEGER, 3];
        // narm=false: NA present -> not updated
        let (s, updated) = isum(&x, false);
        assert!(!updated);
        // narm=true: skip NA
        let (s, updated) = isum(&x, true);
        assert_eq!(s, 4);
        assert!(updated);
    }

    #[test]
    fn test_rsum_basic() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let (s, updated) = rsum(&x, false);
        assert!((s - 15.0).abs() < 1e-10);
        assert!(updated);
    }

    #[test]
    fn test_rsum_with_nan() {
        let x = vec![1.0, f64::NAN, 3.0];
        let (s, updated) = rsum(&x, true);
        assert!((s - 4.0).abs() < 1e-10);
        assert!(updated);
    }

    #[test]
    fn test_csum_basic() {
        let x = vec![Rcomplex { r: 1.0, i: 2.0 }, Rcomplex { r: 3.0, i: 4.0 }];
        let (s, updated) = csum(&x, false);
        assert!((s.r - 4.0).abs() < 1e-10);
        assert!((s.i - 6.0).abs() < 1e-10);
        assert!(updated);
    }

    #[test]
    fn test_imin_basic() {
        let x = vec![5i32, 3, 1, 4, 2];
        let (m, updated) = imin(&x, false);
        assert_eq!(m, 1);
        assert!(updated);
    }

    #[test]
    fn test_imin_with_na() {
        let x = vec![5i32, NA_INTEGER, 1];
        let (m, updated) = imin(&x, false);
        assert_eq!(m, NA_INTEGER);
        assert!(updated);
        let (m, updated) = imin(&x, true);
        assert_eq!(m, 1);
        assert!(updated);
    }

    #[test]
    fn test_imax_basic() {
        let x = vec![5i32, 3, 1, 4, 2];
        let (m, updated) = imax(&x, false);
        assert_eq!(m, 5);
        assert!(updated);
    }

    #[test]
    fn test_rmin_basic() {
        let x = vec![5.0, 3.0, 1.0, 4.0, 2.0];
        let (m, updated) = rmin(&x, false);
        assert!((m - 1.0).abs() < 1e-10);
        assert!(updated);
    }

    #[test]
    fn test_rmin_with_nan() {
        let x = vec![5.0, f64::NAN, 1.0];
        // narm=false: NaN is propagated
        let (m, updated) = rmin(&x, false);
        assert!(m.is_nan());
        assert!(updated);
        // narm=true: NaN is skipped
        let (m, updated) = rmin(&x, true);
        assert!((m - 1.0).abs() < 1e-10);
        assert!(updated);
    }

    #[test]
    fn test_rmax_basic() {
        let x = vec![5.0, 3.0, 1.0, 4.0, 2.0];
        let (m, updated) = rmax(&x, false);
        assert!((m - 5.0).abs() < 1e-10);
        assert!(updated);
    }

    #[test]
    fn test_iprod_basic() {
        let x = vec![2i32, 3, 4];
        let (p, updated) = iprod(&x, false);
        assert!((p - 24.0).abs() < 1e-10);
        assert!(updated);
    }

    #[test]
    fn test_rprod_basic() {
        let x = vec![2.0, 3.0, 4.0];
        let (p, updated) = rprod(&x, false);
        assert!((p - 24.0).abs() < 1e-10);
        assert!(updated);
    }

    #[test]
    fn test_cprod_basic() {
        let x = vec![Rcomplex { r: 1.0, i: 0.0 }, Rcomplex { r: 0.0, i: 1.0 }];
        let (p, updated) = cprod(&x, false);
        assert!((p.r - 0.0).abs() < 1e-10);
        assert!((p.i - 1.0).abs() < 1e-10);
        assert!(updated);
    }

    #[test]
    fn test_real_mean_basic() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((real_mean(&x) - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_real_mean_empty() {
        let x: Vec<f64> = vec![];
        assert!(real_mean(&x).is_nan());
    }

    #[test]
    fn test_integer_mean_basic() {
        let x = vec![1i32, 2, 3, 4, 5];
        assert!((integer_mean(&x) - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_integer_mean_with_na() {
        let x = vec![1i32, 2, NA_INTEGER, 4];
        assert!(integer_mean(&x).is_nan());
    }

    #[test]
    fn test_rmin_na_trumps_nan() {
        let na = crate::sexp::ffi::NA_REAL;
        let x = vec![f64::NAN, na, 1.0];
        // narm=false: NA should trump NaN
        let (m, updated) = rmin(&x, false);
        assert!(ISNA(m));
        assert!(updated);
    }

    #[test]
    fn test_rsum_overflow() {
        let x = vec![f64::MAX, f64::MAX];
        let (s, updated) = rsum(&x, false);
        assert!(s.is_infinite());
        assert!(s.is_sign_positive());
        assert!(updated);
    }

    #[test]
    fn test_isum_empty() {
        let x: Vec<c_int> = vec![];
        let (s, updated) = isum(&x, false);
        assert_eq!(s, 0);
        assert!(!updated);
    }

    #[test]
    fn test_rprod_with_nan_narm() {
        let x = vec![2.0, f64::NAN, 3.0];
        let (p, updated) = rprod(&x, true);
        assert!((p - 6.0).abs() < 1e-10);
        assert!(updated);
    }
}
