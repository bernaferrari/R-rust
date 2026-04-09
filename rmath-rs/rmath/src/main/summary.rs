#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

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

use crate::sexp::accessors::{
    CAR, CDR, COMPLEX, INTEGER, LOGICAL, REAL, SETCDR, SETTAG, TAG, TYPEOF, XLENGTH,
};
use crate::sexp::constructors::{
    Rf_ScalarComplex, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3,
};
use crate::sexp::ffi::SEXP;
use crate::sexp::ffi::{NA_INTEGER, Rcomplex, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;
use std::os::raw::{c_char, c_double, c_int};

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
    x.to_bits() == 0x7ff0000000001954
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
const NA_REAL: f64 = f64::NAN;

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
unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return NA_INTEGER;
        }
        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP.0 => {
                if XLENGTH(x) == 0 {
                    NA_INTEGER
                } else {
                    *LOGICAL(x)
                }
            }
            t if t == SEXPTYPE::INTSXP.0 => {
                if XLENGTH(x) == 0 {
                    NA_INTEGER
                } else {
                    *INTEGER(x)
                }
            }
            t if t == SEXPTYPE::REALSXP.0 => {
                if XLENGTH(x) == 0 {
                    NA_INTEGER
                } else {
                    let v = *REAL(x);
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
}

/// asBool2 — extract boolean from scalar (panics on NA).
#[inline]
unsafe fn asBool2(x: SEXP, _call: SEXP) -> bool {
    unsafe {
        let v = asLogical(x);
        if v == NA_INTEGER {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "invalid 'na.rm' value".to_string(),
            });
        }
        v != 0
    }
}

/// PRIMVAL -- get the primitive's internal integer value.
/// In R, this is stored in the offset/gp field of the SEXPREC.
/// Currently returns 0 as the SxpInfo doesn't expose offset.
#[inline]
unsafe fn PRIMVAL(op: SEXP) -> c_int {
    unsafe { crate::main::relop::PRIMVAL(op) }
}

/// Get or intern the "na.rm" symbol.
unsafe fn R_NaRmSymbol() -> SEXP {
    unsafe { Rf_install(b"na.rm\0".as_ptr() as *const c_char) }
}

/// matchArgExact — find the argument matching a tag by exact symbol identity.
/// Destructively removes the matched element from the list.
unsafe fn matchArgExact(tag: SEXP, list: *mut SEXP) -> SEXP {
    unsafe {
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
}

/// checkArity -- delegates to Rf_checkArityCall.
#[inline]
unsafe fn checkArity(op: SEXP, args: SEXP) {
    crate::main::errors::Rf_checkArityCall(op, args, crate::main::errors::getCurrentCall());
}

/// fixup_NaRm — ensure na.rm is the last argument and exists.
/// Returns the potentially modified args list.
unsafe fn fixup_NaRm(mut args: SEXP) -> SEXP {
    unsafe {
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
        na_value = Rf_protect(na_value);
        let t = crate::sexp::constructors::Rf_cons(na_value, R_NilValue());
        Rf_unprotect(1);
        let t = Rf_protect(t);
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
        Rf_unprotect(1);
        args
    }
}

// ---------------------------------------------------------------------------
// SEXP-level sum/min/max/prod helpers (operate on SEXP vectors)
// ---------------------------------------------------------------------------

/// Integer/logical sum from SEXP vector.
/// Returns (sum_value, updated_flag).
/// updated: 0 = no elements, NA_INTEGER = NA found (go to na_answer),
///          42 = overflow (switch to real).
unsafe fn isum_sexp(sx: SEXP, narm: bool) -> (i64, c_int) {
    unsafe {
        let n = XLENGTH(sx);
        let ptr = INTEGER(sx);
        let mut s: i64 = 0;
        let mut updated: c_int = 0;
        let mut overflow_count: i32 = 0;

        for k in 0..n {
            let xi = *ptr.add(k as usize);
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
}

/// Real sum from SEXP vector (used when integer overflow occurs).
unsafe fn risum_sexp(sx: SEXP, narm: bool) -> (f64, bool) {
    unsafe {
        let n = XLENGTH(sx);
        let ptr = INTEGER(sx);
        let mut s: f64 = 0.0;
        let mut updated = false;

        for k in 0..n {
            let xi = *ptr.add(k as usize);
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
}

/// Double sum from SEXP vector.
unsafe fn rsum_sexp(sx: SEXP, narm: bool) -> (f64, bool) {
    unsafe {
        let n = XLENGTH(sx);
        let ptr = REAL(sx);
        let mut s: f64 = 0.0;
        let mut updated = false;

        for k in 0..n {
            let xi = *ptr.add(k as usize);
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
}

/// Complex sum from SEXP vector.
unsafe fn csum_sexp(sx: SEXP, narm: bool) -> (Rcomplex, bool) {
    unsafe {
        let n = XLENGTH(sx);
        let ptr = COMPLEX(sx);
        let mut sr: f64 = 0.0;
        let mut si: f64 = 0.0;
        let mut updated = false;

        for k in 0..n {
            let xc = *ptr.add(k as usize);
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
}

/// Integer min from SEXP vector.
unsafe fn imin_sexp(sx: SEXP, narm: bool) -> (c_int, bool) {
    unsafe {
        let n = XLENGTH(sx);
        let ptr = INTEGER(sx);
        let mut s: c_int = 0;
        let mut updated = false;

        for k in 0..n {
            let xi = *ptr.add(k as usize);
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
}

/// Double min from SEXP vector.
unsafe fn rmin_sexp(sx: SEXP, narm: bool) -> (f64, bool) {
    unsafe {
        let n = XLENGTH(sx);
        let ptr = REAL(sx);
        let mut s: f64 = 0.0;
        let mut updated = false;

        for k in 0..n {
            let xi = *ptr.add(k as usize);
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
}

/// Integer max from SEXP vector.
unsafe fn imax_sexp(sx: SEXP, narm: bool) -> (c_int, bool) {
    unsafe {
        let n = XLENGTH(sx);
        let ptr = INTEGER(sx);
        let mut s: c_int = 0;
        let mut updated = false;

        for k in 0..n {
            let xi = *ptr.add(k as usize);
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
}

/// Double max from SEXP vector.
unsafe fn rmax_sexp(sx: SEXP, narm: bool) -> (f64, bool) {
    unsafe {
        let n = XLENGTH(sx);
        let ptr = REAL(sx);
        let mut s: f64 = 0.0;
        let mut updated = false;

        for k in 0..n {
            let xi = *ptr.add(k as usize);
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
}

/// Integer product from SEXP vector (returns double).
unsafe fn iprod_sexp(sx: SEXP, narm: bool) -> (f64, bool) {
    unsafe {
        let n = XLENGTH(sx);
        let ptr = INTEGER(sx);
        let mut s: f64 = 1.0;
        let mut updated = false;

        for k in 0..n {
            let xi = *ptr.add(k as usize);
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
}

/// Double product from SEXP vector.
unsafe fn rprod_sexp(sx: SEXP, narm: bool) -> (f64, bool) {
    unsafe {
        let n = XLENGTH(sx);
        let ptr = REAL(sx);
        let mut s: f64 = 1.0;
        let mut updated = false;

        for k in 0..n {
            let xi = *ptr.add(k as usize);
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
}

/// Complex product from SEXP vector.
unsafe fn cprod_sexp(sx: SEXP, narm: bool) -> (Rcomplex, bool) {
    unsafe {
        let n = XLENGTH(sx);
        let ptr = COMPLEX(sx);
        let mut sr: f64 = 1.0;
        let mut si: f64 = 0.0;
        let mut updated = false;

        for k in 0..n {
            let xc = *ptr.add(k as usize);
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
}

// ---------------------------------------------------------------------------
// SEXP-level mean helpers
// ---------------------------------------------------------------------------

/// Mean of a logical SEXP vector.
unsafe fn logical_mean_sexp(x: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(x);
        let ptr = LOGICAL(x);
        let mut s: f64 = 0.0;
        for k in 0..n {
            let xi = *ptr.add(k as usize);
            if xi == c_int::MIN {
                return Rf_ScalarReal(NA_REAL);
            }
            s += xi as f64;
        }
        Rf_ScalarReal(s / n as f64)
    }
}

/// Mean of an integer SEXP vector.
unsafe fn integer_mean_sexp(x: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(x);
        let ptr = INTEGER(x);
        let mut s: f64 = 0.0;
        for k in 0..n {
            let xi = *ptr.add(k as usize);
            if xi == NA_INTEGER {
                return Rf_ScalarReal(NA_REAL);
            }
            s += xi as f64;
        }
        Rf_ScalarReal(s / n as f64)
    }
}

/// Mean of a real SEXP vector (with overflow protection).
unsafe fn real_mean_sexp(x: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(x);
        let ptr = REAL(x);
        if n == 0 {
            return Rf_ScalarReal(NA_REAL);
        }

        // First pass: sum
        let mut s: f64 = 0.0;
        for k in 0..n {
            s += *ptr.add(k as usize);
        }

        let finite_s = s.is_finite();
        if finite_s {
            s /= n as f64;
        } else {
            // Infinite s — divide each term by n first
            s = 0.0;
            for k in 0..n {
                s += *ptr.add(k as usize) / n as f64;
            }
        }

        // Second pass: correction term
        if finite_s && s.is_finite() {
            let mut t: f64 = 0.0;
            for k in 0..n {
                t += *ptr.add(k as usize) - s;
            }
            s += t / n as f64;
        } else if s.is_finite() {
            let mut t: f64 = 0.0;
            for k in 0..n {
                t += (*ptr.add(k as usize) - s) / n as f64;
            }
            s += t;
        }

        Rf_ScalarReal(s)
    }
}

/// Mean of a complex SEXP vector.
unsafe fn complex_mean_sexp(x: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(x);
        let ptr = COMPLEX(x);
        let mut sr: f64 = 0.0;
        let mut si: f64 = 0.0;

        for k in 0..n {
            let xc = *ptr.add(k as usize);
            sr += xc.r;
            si += xc.i;
        }
        sr /= n as f64;
        si /= n as f64;

        if sr.is_finite() && si.is_finite() {
            let mut tr: f64 = 0.0;
            let mut ti: f64 = 0.0;
            for k in 0..n {
                let xc = *ptr.add(k as usize);
                tr += xc.r - sr;
                ti += xc.i - si;
            }
            sr += tr / n as f64;
            si += ti / n as f64;
        }

        Rf_ScalarComplex(Rcomplex { r: sr, i: si })
    }
}

// ---------------------------------------------------------------------------
// do_summary — sum (0), mean (1), min (2), max (3), prod (4)
// ---------------------------------------------------------------------------

/// `do_summary` provides a variety of data summaries.
///
/// op (PRIMVAL): 0 = sum, 1 = mean, 2 = min, 3 = max, 4 = prod
pub unsafe fn do_summary(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let iop = PRIMVAL(op);

        // Mean is special: only one arg, no na.rm dispatch
        if iop == 1 {
            let x = CAR(args);
            match TYPEOF(x) {
                t if t == SEXPTYPE::LGLSXP.0 => return logical_mean_sexp(x),
                t if t == SEXPTYPE::INTSXP.0 => return integer_mean_sexp(x),
                t if t == SEXPTYPE::REALSXP.0 => return real_mean_sexp(x),
                t if t == SEXPTYPE::CPLXSXP.0 => return complex_mean_sexp(x),
                _ => {
                    std::panic::panic_any(crate::sexp::context::RError {
                        message: format!("invalid 'type' of argument"),
                    });
                }
            }
        }

        // For sum/min/max/prod: fixup na.rm to be the last argument
        let args = Rf_protect(fixup_NaRm(args));
        let _call2 = Rf_protect(crate::main::duplicate::shallow_duplicate(call));

        // Extract na.rm value
        let na_rm_sym = R_NaRmSymbol();
        let mut args_mut = args;
        let na_rm_val = matchArgExact(na_rm_sym, &mut args_mut);
        let narm = asBool2(na_rm_val, call);

        // Determine ans_type by scanning all arguments
        let mut complex_a = false;
        let mut real_a = false;
        let mut a = args_mut;
        while a != R_NilValue() {
            match TYPEOF(CAR(a)) {
                t if t == SEXPTYPE::INTSXP.0
                    || t == SEXPTYPE::LGLSXP.0
                    || t == SEXPTYPE::NILSXP.0 => {}
                t if t == SEXPTYPE::REALSXP.0 => {
                    real_a = true;
                }
                t if t == SEXPTYPE::CPLXSXP.0 => {
                    complex_a = true;
                }
                _ => {
                    let bad = CAR(a);
                    Rf_unprotect(2);
                    std::panic::panic_any(crate::sexp::context::RError {
                        message: format!("invalid 'type' of argument"),
                    });
                }
            }
            a = CDR(a);
        }

        let mut ans_type: c_int;
        let mut icum: c_int = 0;
        let mut iLcum: i64 = 0;
        let mut zcum = Rcomplex { r: 0.0, i: 0.0 };
        let mut use_isum = true;

        match iop {
            0 => {
                // sum
                if complex_a {
                    ans_type = SEXPTYPE::CPLXSXP.0;
                } else if real_a {
                    ans_type = SEXPTYPE::REALSXP.0;
                } else {
                    ans_type = SEXPTYPE::INTSXP.0;
                    iLcum = 0;
                }
            }
            2 => {
                // min
                ans_type = SEXPTYPE::INTSXP.0;
                zcum.r = R_PosInf;
                icum = c_int::MAX;
            }
            3 => {
                // max
                ans_type = SEXPTYPE::INTSXP.0;
                zcum.r = R_NegInf;
                icum = 1 + c_int::MIN; // R_INT_MIN
            }
            4 => {
                // prod
                ans_type = SEXPTYPE::REALSXP.0;
                zcum.r = 1.0;
                zcum.i = 0.0;
            }
            _ => {
                Rf_unprotect(2);
                return R_NilValue();
            }
        }

        let mut empty = true;
        let mut args_iter = args_mut;

        // Loop over all arguments
        while args_iter != R_NilValue() {
            a = CAR(args_iter);
            let xtype = TYPEOF(a);
            let len = XLENGTH(a);

            if len > 0 {
                let mut updated: c_int = 0;
                let mut int_a = false;
                let mut real_a_local = false;

                match iop {
                    2 | 3 => {
                        // min / max
                        match xtype {
                            t if t == SEXPTYPE::LGLSXP.0 || t == SEXPTYPE::INTSXP.0 => {
                                int_a = true;
                                let (itmp, upd) = if iop == 2 {
                                    imin_sexp(a, narm)
                                } else {
                                    imax_sexp(a, narm)
                                };
                                updated = if upd { 1 } else { 0 };

                                if updated != 0 {
                                    if ans_type == SEXPTYPE::INTSXP.0 {
                                        if icum != NA_INTEGER {
                                            if itmp == NA_INTEGER
                                                || (iop == 2 && itmp < icum)
                                                || (iop == 3 && itmp > icum)
                                            {
                                                icum = itmp;
                                            }
                                        }
                                    } else if ans_type == SEXPTYPE::REALSXP.0 {
                                        let tmp = Int2Real(itmp);
                                        if ISNA(zcum.r) {
                                            // NA trumps anything
                                        } else if ISNAN(tmp) {
                                            if ISNA(tmp) {
                                                zcum.r = tmp;
                                            } else {
                                                zcum.r += tmp;
                                            }
                                        } else if (iop == 2 && tmp < zcum.r)
                                            || (iop == 3 && tmp > zcum.r)
                                        {
                                            zcum.r = tmp;
                                        }
                                    }
                                }
                            }
                            t if t == SEXPTYPE::REALSXP.0 => {
                                real_a_local = true;
                                if ans_type == SEXPTYPE::INTSXP.0 && !empty {
                                    ans_type = SEXPTYPE::REALSXP.0;
                                    zcum.r = Int2Real(icum);
                                }
                                let (tmp, upd) = if iop == 2 {
                                    rmin_sexp(a, narm)
                                } else {
                                    rmax_sexp(a, narm)
                                };
                                updated = if upd { 1 } else { 0 };

                                if updated != 0 && ans_type == SEXPTYPE::REALSXP.0 {
                                    if ISNA(zcum.r) {
                                        // NA trumps anything
                                    } else if ISNAN(tmp) {
                                        if ISNA(tmp) {
                                            zcum.r = tmp;
                                        } else {
                                            zcum.r += tmp;
                                        }
                                    } else if (iop == 2 && tmp < zcum.r)
                                        || (iop == 3 && tmp > zcum.r)
                                    {
                                        zcum.r = tmp;
                                    }
                                }
                            }
                            _ => {
                                Rf_unprotect(2);
                                std::panic::panic_any(crate::sexp::context::RError {
                                    message: "invalid 'type' of argument".to_string(),
                                });
                            }
                        }
                    }

                    0 => {
                        // sum
                        match xtype {
                            t if t == SEXPTYPE::LGLSXP.0 || t == SEXPTYPE::INTSXP.0 => {
                                let (iLtmp, upd) = isum_sexp(a, narm);
                                updated = upd;

                                if updated == NA_INTEGER {
                                    // NA found, na_answer
                                    let ans = Rf_allocVector3(ans_type, 1);
                                    match ans_type {
                                        t2 if t2 == SEXPTYPE::INTSXP.0 => {
                                            *INTEGER(ans) = NA_INTEGER;
                                        }
                                        t2 if t2 == SEXPTYPE::REALSXP.0 => {
                                            *REAL(ans) = NA_REAL;
                                        }
                                        t2 if t2 == SEXPTYPE::CPLXSXP.0 => {
                                            *COMPLEX(ans) = Rcomplex {
                                                r: NA_REAL,
                                                i: NA_REAL,
                                            };
                                        }
                                        _ => {}
                                    }
                                    Rf_unprotect(2);
                                    return ans;
                                } else if use_isum && updated == 42 {
                                    // Impending integer overflow — switch to real
                                    use_isum = false;
                                    if ans_type == SEXPTYPE::INTSXP.0 {
                                        ans_type = SEXPTYPE::REALSXP.0;
                                    }
                                    let (tmp, _upd) = risum_sexp(a, narm);
                                    zcum.r = iLcum as f64 + tmp;
                                } else if updated != 0 {
                                    if ans_type == SEXPTYPE::INTSXP.0 {
                                        let s = iLcum as f64 + iLtmp as f64;
                                        if s > c_int::MAX as f64 || s < (1 + c_int::MIN) as f64 {
                                            ans_type = SEXPTYPE::REALSXP.0;
                                            zcum.r = s;
                                        } else {
                                            iLcum += iLtmp;
                                        }
                                    } else {
                                        zcum.r += iLtmp as f64;
                                    }
                                }
                            }
                            t if t == SEXPTYPE::REALSXP.0 => {
                                if ans_type == SEXPTYPE::INTSXP.0 {
                                    ans_type = SEXPTYPE::REALSXP.0;
                                    if !empty {
                                        zcum.r = Int2Real(icum as c_int);
                                    }
                                }
                                let (tmp, upd) = rsum_sexp(a, narm);
                                updated = if upd { 1 } else { 0 };
                                if updated != 0 {
                                    zcum.r += tmp;
                                }
                            }
                            t if t == SEXPTYPE::CPLXSXP.0 => {
                                if ans_type == SEXPTYPE::INTSXP.0 {
                                    ans_type = SEXPTYPE::CPLXSXP.0;
                                    if !empty {
                                        zcum.r = Int2Real(icum as c_int);
                                    }
                                } else if ans_type == SEXPTYPE::REALSXP.0 {
                                    ans_type = SEXPTYPE::CPLXSXP.0;
                                }
                                let (ztmp, upd) = csum_sexp(a, narm);
                                updated = if upd { 1 } else { 0 };
                                if updated != 0 {
                                    zcum.r += ztmp.r;
                                    zcum.i += ztmp.i;
                                }
                            }
                            _ => {
                                Rf_unprotect(2);
                                std::panic::panic_any(crate::sexp::context::RError {
                                    message: "invalid 'type' of argument".to_string(),
                                });
                            }
                        }
                    }

                    4 => {
                        // prod
                        match xtype {
                            t if t == SEXPTYPE::LGLSXP.0 || t == SEXPTYPE::INTSXP.0 => {
                                let (tmp, upd) = iprod_sexp(a, narm);
                                updated = if upd { 1 } else { 0 };
                                if updated != 0 {
                                    zcum.r *= tmp;
                                    zcum.i *= tmp;
                                }
                            }
                            t if t == SEXPTYPE::REALSXP.0 => {
                                let (tmp, upd) = rprod_sexp(a, narm);
                                updated = if upd { 1 } else { 0 };
                                if updated != 0 {
                                    zcum.r *= tmp;
                                    zcum.i *= tmp;
                                }
                            }
                            t if t == SEXPTYPE::CPLXSXP.0 => {
                                ans_type = SEXPTYPE::CPLXSXP.0;
                                let (ztmp, upd) = cprod_sexp(a, narm);
                                updated = if upd { 1 } else { 0 };
                                if updated != 0 {
                                    let z = Rcomplex {
                                        r: zcum.r,
                                        i: zcum.i,
                                    };
                                    zcum.r = z.r * ztmp.r - z.i * ztmp.i;
                                    zcum.i = z.r * ztmp.i + z.i * ztmp.r;
                                }
                            }
                            _ => {
                                Rf_unprotect(2);
                                std::panic::panic_any(crate::sexp::context::RError {
                                    message: "invalid 'type' of argument".to_string(),
                                });
                            }
                        }
                    }

                    _ => {}
                }

                if empty && updated != 0 {
                    empty = false;
                }
            } else {
                // Zero-length argument — update ans_type if needed
                match xtype {
                    t if t == SEXPTYPE::LGLSXP.0
                        || t == SEXPTYPE::INTSXP.0
                        || t == SEXPTYPE::REALSXP.0
                        || t == SEXPTYPE::NILSXP.0 => {}
                    t if t == SEXPTYPE::CPLXSXP.0 => {
                        if iop == 2 || iop == 3 {
                            Rf_unprotect(2);
                            std::panic::panic_any(crate::sexp::context::RError {
                                message: "invalid 'type' of argument".to_string(),
                            });
                        }
                    }
                    _ => {
                        Rf_unprotect(2);
                        std::panic::panic_any(crate::sexp::context::RError {
                            message: "invalid 'type' of argument".to_string(),
                        });
                    }
                }
                if ans_type < xtype && ans_type != SEXPTYPE::CPLXSXP.0 {
                    if !empty && ans_type == SEXPTYPE::INTSXP.0 {
                        zcum.r = Int2Real(icum);
                    }
                    ans_type = xtype;
                }
            }

            args_iter = CDR(args_iter);
        }

        // Handle empty min/max
        if empty && (iop == 2 || iop == 3) {
            ans_type = SEXPTYPE::REALSXP.0;
        }

        let ans = Rf_allocVector3(ans_type, 1);
        match ans_type {
            t if t == SEXPTYPE::INTSXP.0 => {
                if iop == 0 {
                    *INTEGER(ans) = iLcum as c_int;
                } else {
                    *INTEGER(ans) = icum;
                }
            }
            t if t == SEXPTYPE::REALSXP.0 => {
                *REAL(ans) = zcum.r;
            }
            t if t == SEXPTYPE::CPLXSXP.0 => {
                *COMPLEX(ans) = Rcomplex {
                    r: zcum.r,
                    i: zcum.i,
                };
            }
            _ => {}
        }
        Rf_unprotect(2);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_range — range() dispatches to range.default
// ---------------------------------------------------------------------------

/// `do_range` implements `range(...)` which finds min and max.
/// It delegates to range.default via applyClosure.
pub unsafe fn do_range(call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let args = Rf_protect(fixup_NaRm(args));

        // Find range.default and apply it
        let range_sym = Rf_install(b"range.default\0".as_ptr() as *const c_char);
        let range_fun = Rf_protect(crate::sexp::envir::findFun(range_sym, env));

        // Build promise args
        let prargs = Rf_protect(crate::eval::dispatch::promiseArgs(args, R_NilValue()));

        // Evaluate range.default via applyClosure
        let ans = crate::eval::closure::applyClosure(call, range_fun, prargs, env, R_NilValue(), 1);

        Rf_unprotect(3);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_mean — mean.default implementation
// ---------------------------------------------------------------------------

/// `do_mean` implements `mean.default(x)`.
/// Note: mean is typically dispatched via do_summary when PRIMVAL(op) == 1,
/// but this provides a standalone entry point.
pub unsafe fn do_mean(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP.0 => logical_mean_sexp(x),
            t if t == SEXPTYPE::INTSXP.0 => integer_mean_sexp(x),
            t if t == SEXPTYPE::REALSXP.0 => real_mean_sexp(x),
            t if t == SEXPTYPE::CPLXSXP.0 => complex_mean_sexp(x),
            _ => {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: format!("invalid 'type' of argument"),
                });
            }
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
        let na = f64::from_bits(0x7ff0000000001954);
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
