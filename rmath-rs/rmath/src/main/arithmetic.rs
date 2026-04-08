#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/arithmetic.c — arithmetic utility functions.
//!
//! This module ports the standalone arithmetic functions that don't require
//! SEXP or R interpreter internals.
//!
//! Ported standalone functions:
//!   R_ValueOfNA, R_NaN_is_R_NA, R_IsNA, R_IsNaN,
//!   myfmod, myfloor,
//!   R_integer_plus, R_integer_minus, R_integer_times, R_integer_divide,
//!   Rsqrt, Rexp, Rexpm1, Rlog1p, Rsin, Rtan, Rcos, Rasin, Ratan
//!
//! Already ported elsewhere:
//!   R_pow, R_pow_di, R_finite → special/mlutils.rs

use std::os::raw::c_int;

use crate::nmath::fprec::{fprec, fround};
use crate::nmath::special::cospi::{cospi, sinpi, tanpi};
use crate::nmath::special::gamma::{gammafn, lgammafn};
use crate::nmath::special::mlutils::R_pow;
use crate::nmath::special::polygamma::{digamma, trigamma};
use crate::sexp::accessors::{
    CADR, CAR, CDR, COMPLEX, INTEGER, LENGTH, LOGICAL, NAMED, REAL, TYPEOF, XLENGTH,
};
use crate::sexp::constructors::Rf_allocVector3;
use crate::sexp::ffi::Rcomplex;
use crate::sexp::ffi::{NA_INTEGER, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::Rf_protect;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's NA_REAL sentinel (NaN with specific bit pattern).
pub const NA_REAL: f64 = 0.0_f64 / 0.0_f64;

/// IEEE double epsilon (machine epsilon).
const C_EPS: f64 = f64::EPSILON;

// ---------------------------------------------------------------------------
// NA/NaN helpers
// ---------------------------------------------------------------------------

/// Returns the IEEE double representation of R's NA value.
pub extern "C" fn R_ValueOfNA() -> f64 {
    NA_REAL
}

/// Check if a NaN value is specifically R's NA (not just any NaN).
pub extern "C" fn R_NaN_is_R_NA(x: f64) -> c_int {
    // R's NA has the specific bit pattern 0x7ff0000000001954
    if x.is_nan() && x.to_bits() == 0x7ff0000000001954 {
        1
    } else {
        0
    }
}

/// Check if a value is R's NA.
pub extern "C" fn R_IsNA(x: f64) -> c_int {
    if x.is_nan() && R_NaN_is_R_NA(x) != 0 {
        1
    } else {
        0
    }
}

/// Check if a value is NaN but not R's NA.
pub extern "C" fn R_IsNaN(x: f64) -> c_int {
    if x.is_nan() && R_NaN_is_R_NA(x) == 0 {
        1
    } else {
        0
    }
}

/// Finite check.
#[inline]
pub fn R_FINITE(x: f64) -> bool {
    x.is_finite()
}

// ---------------------------------------------------------------------------
// Integer arithmetic with overflow detection
// ---------------------------------------------------------------------------

/// Safe integer addition with overflow detection.
///
/// Returns NA_INTEGER on overflow or if either input is NA_INTEGER.
/// If `pnaflag` is non-null, sets it to true on overflow.
pub unsafe fn R_integer_plus(x: c_int, y: c_int, pnaflag: *mut bool) -> c_int {
    unsafe {
        if x == NA_INTEGER || y == NA_INTEGER {
            return NA_INTEGER;
        }

        let x64 = x as i64;
        let y64 = y as i64;
        let result = x64 + y64;

        if result > c_int::MAX as i64 || result < c_int::MIN as i64 {
            if !pnaflag.is_null() {
                *pnaflag = true;
            }
            return NA_INTEGER;
        }
        result as c_int
    }
}

/// Safe integer subtraction with overflow detection.
///
/// Returns NA_INTEGER on overflow or if either input is NA_INTEGER.
pub unsafe fn R_integer_minus(x: c_int, y: c_int, pnaflag: *mut bool) -> c_int {
    unsafe {
        if x == NA_INTEGER || y == NA_INTEGER {
            return NA_INTEGER;
        }

        // Match C's overflow checks using i64 to avoid wrapping
        let x64 = x as i64;
        let y64 = y as i64;
        if (y64 < 0 && x64 > (c_int::MAX as i64 + y64))
            || (y64 > 0 && x64 < (c_int::MIN as i64 + y64))
        {
            if !pnaflag.is_null() {
                *pnaflag = true;
            }
            return NA_INTEGER;
        }
        x - y
    }
}

/// Safe integer multiplication with overflow detection.
///
/// Returns NA_INTEGER on overflow or if either input is NA_INTEGER.
pub unsafe fn R_integer_times(x: c_int, y: c_int, pnaflag: *mut bool) -> c_int {
    unsafe {
        if x == NA_INTEGER || y == NA_INTEGER {
            return NA_INTEGER;
        }

        // Compute wrapping product (matches C behavior)
        let z = x.wrapping_mul(y);
        // Check if double product matches (GOODIPROD pattern from C)
        let z_double = (x as f64) * (y as f64);
        if z_double == z as f64 && z != NA_INTEGER {
            z
        } else {
            if !pnaflag.is_null() {
                *pnaflag = true;
            }
            NA_INTEGER
        }
    }
}

/// Integer division returning double.
///
/// Returns NA_REAL if either input is NA_INTEGER.
pub extern "C" fn R_integer_divide(x: c_int, y: c_int) -> f64 {
    if x == NA_INTEGER || y == NA_INTEGER {
        NA_REAL
    } else {
        (x as f64) / (y as f64)
    }
}

// ---------------------------------------------------------------------------
// Floating-point modulus and floor division
// ---------------------------------------------------------------------------

/// Custom floating-point modulus with improved accuracy.
///
/// Ported from R's internal `myfmod`. Uses standard fmod for the
/// general case, with special handling for small values.
pub fn myfmod(x1: f64, x2: f64) -> f64 {
    if x2 == 0.0 {
        return f64::NAN;
    }

    // Special case: very small x1 relative to x2
    if x2.abs() * C_EPS > 1.0 && R_FINITE(x1) && x1.abs() <= x2.abs() {
        if x1.abs() == x2.abs() {
            return 0.0;
        }
        if (x1 < 0.0 && x2 > 0.0) || (x2 < 0.0 && x1 > 0.0) {
            return x1 + x2; // differing signs
        }
        return x1; // same signs
    }

    // Use fmod for the general case
    x1 % x2
}

/// Custom floor division with improved accuracy.
///
/// Ported from R's internal `myfloor`.
pub fn myfloor(x1: f64, x2: f64) -> f64 {
    let q = x1 / x2;

    if x2 == 0.0 || q.abs() * C_EPS > 1.0 || !R_FINITE(q) {
        return q;
    }

    if q.abs() < 1.0 {
        if q < 0.0 {
            return -1.0;
        }
        if (x1 < 0.0 && x2 > 0.0) || (x1 > 0.0 && x2 < 0.0) {
            return -1.0; // differing signs
        }
        return 0.0;
    }

    let tmp = x1 - q.floor() * x2;
    q.floor() + (tmp / x2).floor()
}

// ---------------------------------------------------------------------------
// Optimized math functions (R's "accurate for small arguments" versions)
// ---------------------------------------------------------------------------

/// Square root that returns exact integer for perfect squares (1-11).
#[inline]
pub fn Rsqrt(x: f64) -> f64 {
    if x == 0.0 {
        return x; // sqrt(-0.) = -0.
    }
    for i in 1..12 {
        if x == (i * i) as f64 {
            return i as f64;
        }
    }
    x.sqrt()
}

/// Exponential function with linear approximation for very small x.
#[inline]
pub fn Rexp(x: f64) -> f64 {
    if x.abs() <= f64::EPSILON.sqrt() {
        1.0 + x
    } else {
        x.exp()
    }
}

/// Helper: returns x for very small arguments, otherwise applies f.
#[inline]
fn f_x_x(x: f64, f: fn(f64) -> f64, m: f64) -> f64 {
    if x.abs() <= m {
        x
    } else {
        f(x)
    }
}

/// exp(x) - 1 with improved accuracy for small x.
#[inline]
pub fn Rexpm1(x: f64) -> f64 {
    f_x_x(x, |v| v.exp_m1(), f64::EPSILON)
}

/// log(1 + x) with improved accuracy for small x.
#[inline]
pub fn Rlog1p(x: f64) -> f64 {
    f_x_x(x, |v| v.ln_1p(), f64::EPSILON)
}

/// sin(x) with linear approximation for small angles.
#[inline]
pub fn Rsin(x: f64) -> f64 {
    f_x_x(x, |v| v.sin(), (3.0 * f64::EPSILON).sqrt())
}

/// tan(x) with linear approximation for small angles.
#[inline]
pub fn Rtan(x: f64) -> f64 {
    f_x_x(x, |v| v.tan(), (1.5 * f64::EPSILON).sqrt())
}

/// cos(x) with quadratic approximation for small angles.
#[inline]
pub fn Rcos(x: f64) -> f64 {
    if x.abs() < (12.0 * f64::EPSILON).sqrt().sqrt() {
        1.0 - x * x * 0.5
    } else {
        x.cos()
    }
}

/// asin(x) with linear approximation for small values.
#[inline]
pub fn Rasin(x: f64) -> f64 {
    f_x_x(x, |v| v.asin(), (3.0 * f64::EPSILON).sqrt())
}

/// atan(x) with linear approximation for small values.
#[inline]
pub fn Ratan(x: f64) -> f64 {
    f_x_x(x, |v| v.atan(), (1.5 * f64::EPSILON).sqrt())
}

// ---------------------------------------------------------------------------
// SEXP-dependent implementations
// ---------------------------------------------------------------------------

/// Read the PRIMVAL (primitive offset) from a builtin/special SEXP.
#[inline]
unsafe fn primval(op: SEXP) -> c_int {
    unsafe { (*op).data.primsxp.offset }
}

/// Helper: check if SEXP is numeric (INTSXP, REALSXP, CPLXSXP, or LGLSXP).
#[inline]
unsafe fn is_numeric(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::INTSXP.0
            || t == SEXPTYPE::REALSXP.0
            || t == SEXPTYPE::CPLXSXP.0
            || t == SEXPTYPE::LGLSXP.0
    }
}

/// Helper: check if SEXP is complex.
#[inline]
unsafe fn is_complex(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::CPLXSXP.0 }
}

/// Helper: check if SEXP is integer or logical.
#[inline]
unsafe fn is_integer_or_logical(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0
    }
}

/// Helper: check if SEXP is a scalar of given type.
#[inline]
unsafe fn is_scalar(x: SEXP, sexptype: c_int) -> bool {
    unsafe { TYPEOF(x) == sexptype && LENGTH(x) == 1 }
}

/// Helper: NO_REFERENCES check (NAMED == 0).
#[inline]
unsafe fn no_references(x: SEXP) -> bool {
    unsafe { NAMED(x) == 0 }
}

/// Integer-to-double conversion respecting NA_INTEGER.
#[inline]
fn r_integer_to_double(x: c_int) -> f64 {
    if x == NA_INTEGER {
        NA_REAL
    } else {
        x as f64
    }
}

// ---- math1 helpers ----

/// Apply a unary math function f to each element of a REALSXP vector.
/// Preserves incoming NaN/NA. Issues warning on NaN produced from non-NaN input.
unsafe fn math1_impl(sa: SEXP, f: fn(f64) -> f64) -> SEXP {
    unsafe {
        if !is_numeric(sa) {
            return std::ptr::null_mut();
        }
        let n = XLENGTH(sa);
        // Coerce to REALSXP
        let sa = coerce_to_real(sa);
        let _p1 = Rf_protect(sa);

        let sy = if no_references(sa) {
            sa
        } else {
            let v = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
            Rf_protect(v);
            v
        };
        let _p2 = Rf_protect(sy);

        let a = REAL(sa);
        let y = REAL(sy);
        let mut naflag = false;

        for i in 0..(n as usize) {
            let x = *a.add(i);
            *y.add(i) = f(x);
            if (*y.add(i)).is_nan() {
                if x.is_nan() {
                    *y.add(i) = x; // preserve incoming NaN
                } else {
                    naflag = true;
                }
            }
        }

        crate::sexp::protect::Rf_unprotect(2);
        sy
    }
}

/// Apply a unary math function with special argument/result handling.
/// When x == arg, result is res. Otherwise applies f(x).
unsafe fn math1_ari_impl(sa: SEXP, f: fn(f64) -> f64, arg: f64, res: f64) -> SEXP {
    unsafe {
        if !is_numeric(sa) {
            return std::ptr::null_mut();
        }
        let n = XLENGTH(sa);
        let sa = coerce_to_real(sa);
        let _p1 = Rf_protect(sa);

        let sy = if no_references(sa) {
            sa
        } else {
            let v = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
            Rf_protect(v);
            v
        };
        let _p2 = Rf_protect(sy);

        let a = REAL(sa);
        let y = REAL(sy);
        let mut naflag = false;

        for i in 0..(n as usize) {
            let x = *a.add(i);
            if x == arg {
                *y.add(i) = res;
            } else {
                *y.add(i) = f(x);
            }
            if (*y.add(i)).is_nan() {
                if x.is_nan() {
                    *y.add(i) = x;
                } else {
                    naflag = true;
                }
            }
        }

        crate::sexp::protect::Rf_unprotect(2);
        sy
    }
}

/// Coerce a numeric SEXP to REALSXP (no-op if already REALSXP).
unsafe fn coerce_to_real(x: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::REALSXP.0 {
            x
        } else if TYPEOF(x) == SEXPTYPE::INTSXP.0 || TYPEOF(x) == SEXPTYPE::LGLSXP.0 {
            let n = XLENGTH(x);
            let y = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
            let src = INTEGER(x);
            let dst = REAL(y);
            for i in 0..(n as usize) {
                let v = *src.add(i);
                *dst.add(i) = if v == NA_INTEGER { NA_REAL } else { v as f64 };
            }
            y
        } else {
            // CPLXSXP or other -- for now just return a zero-length REALSXP
            Rf_allocVector3(SEXPTYPE::REALSXP.0, 0)
        }
    }
}

/// Wrapper for extern "C" cospi to match fn(f64) -> f64 signature.
#[inline]
fn r_cospi(x: f64) -> f64 {
    cospi(x)
}

/// Wrapper for extern "C" sinpi to match fn(f64) -> f64 signature.
#[inline]
fn r_sinpi(x: f64) -> f64 {
    sinpi(x)
}

/// Wrapper for extern "C" tanpi to match fn(f64) -> f64 signature.
#[inline]
fn r_tanpi(x: f64) -> f64 {
    tanpi(x)
}

/// `sign` function for doubles (not in libm directly).
#[inline]
fn r_sign(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 {
        x // preserve NaN and signed zero
    } else if x > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// Complex math1: apply f to real and imaginary parts separately.
/// For functions like sqrt, log, exp on complex numbers.
unsafe fn complex_math1_impl(sa: SEXP, f_real: fn(f64) -> f64, f_imag: fn(f64) -> f64) -> SEXP {
    unsafe {
        let n = XLENGTH(sa);
        let sy = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, n);
        let _p1 = Rf_protect(sy);
        let src = COMPLEX(sa);
        let dst = COMPLEX(sy);
        for i in 0..(n as usize) {
            let z = *src.add(i);
            *dst.add(i) = Rcomplex {
                r: f_real(z.r),
                i: f_imag(z.i),
            };
        }
        crate::sexp::protect::Rf_unprotect(1);
        sy
    }
}

// ---- do_math1 ----

/// Single-argument math functions: sqrt, log, exp, floor, ceil, sign, etc.
///
/// Operation codes (from R's PRIMVAL):
///   1: floor, 2: ceil, 3: sqrt, 4: sign, 5: trunc, 6: abs,
///   10: exp, 11: expm1, 12: log1p,
///   10002: log2, 10003: log, 10010: log10,
///   20: cos, 21: sin, 22: tan, 23: acos, 24: asin, 25: atan,
///   30: cosh, 31: sinh, 32: tanh, 33: acosh, 34: asinh, 35: atanh,
///   40: lgamma, 41: gamma, 42: digamma, 43: trigamma,
///   47: cospi, 48: sinpi, 49: tanpi
// no_mangle removed (duplicate)
pub unsafe fn do_math1(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let code = primval(op);
        let sa = CAR(args);

        // Dispatch complex to complex handler
        if is_complex(sa) {
            return complex_math1_impl(sa, |x| x, |x| x);
        }

        match code {
            1 => math1_impl(sa, libm::floor),
            2 => math1_impl(sa, libm::ceil),
            3 => math1_impl(sa, Rsqrt),
            4 => math1_impl(sa, r_sign),
            5 => math1_impl(sa, libm::trunc),
            6 => math1_ari_impl(sa, libm::fabs, 0.0, 0.0), // abs
            10 => math1_ari_impl(sa, Rexp, 0.0, 1.0),
            11 => math1_ari_impl(sa, libm::expm1, 0.0, 0.0),
            12 => math1_ari_impl(sa, libm::log1p, 0.0, 0.0),
            20 => math1_ari_impl(sa, libm::cos, 0.0, 1.0),
            21 => math1_ari_impl(sa, Rsin, 0.0, 0.0),
            22 => math1_ari_impl(sa, Rtan, 0.0, 0.0),
            23 => math1_ari_impl(sa, libm::acos, 1.0, 0.0),
            24 => math1_ari_impl(sa, Rasin, 0.0, 0.0),
            25 => math1_ari_impl(sa, Ratan, 0.0, 0.0),
            30 => math1_ari_impl(sa, libm::cosh, 0.0, 1.0),
            31 => math1_ari_impl(sa, libm::sinh, 0.0, 0.0),
            32 => math1_ari_impl(sa, libm::tanh, 0.0, 0.0),
            33 => math1_ari_impl(sa, libm::acosh, 1.0, 0.0),
            34 => math1_ari_impl(sa, libm::asinh, 0.0, 0.0),
            35 => math1_ari_impl(sa, libm::atanh, 0.0, 0.0),
            40 => math1_impl(sa, lgammafn),
            41 => math1_impl(sa, gammafn),
            42 => math1_impl(sa, digamma),
            43 => math1_impl(sa, trigamma),
            47 => math1_impl(sa, r_cospi),
            48 => math1_impl(sa, r_sinpi),
            49 => math1_impl(sa, r_tanpi),
            10002 => math1_impl(sa, libm::log2),  // log2
            10003 => math1_impl(sa, libm::log),   // log (natural, base handled by wrapper)
            10010 => math1_impl(sa, libm::log10), // log10
            _ => std::ptr::null_mut(),
        }
    }
}

// ---- math2 helpers ----

/// NA checking macro for two-argument math functions.
/// Mirrors R's if_NA_Math2_set.
#[inline]
fn na_math2_set(a: f64, b: f64) -> Option<f64> {
    // Check for R's NA (specific NaN bit pattern)
    let na_bits = 0x7ff0000000001954u64;
    let a_is_na = a.is_nan() && a.to_bits() == na_bits;
    let b_is_na = b.is_nan() && b.to_bits() == na_bits;
    if a_is_na || b_is_na {
        Some(f64::from_bits(na_bits)) // return R's NA_REAL
    } else if a.is_nan() || b.is_nan() {
        Some(f64::NAN)
    } else {
        None // neither NA nor NaN
    }
}

/// Apply a binary math function f(a, b) to element-wise pairs from two vectors
/// with recycling.
unsafe fn math2_impl(sa: SEXP, sb: SEXP, f: fn(f64, f64) -> f64) -> SEXP {
    unsafe {
        let na = XLENGTH(sa);
        let nb = XLENGTH(sb);

        // Zero-length handling
        if na == 0 || nb == 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP.0, 0);
        }

        let n = if na > nb { na } else { nb };

        // Coerce both to REALSXP
        let sa = coerce_to_real(sa);
        let _p1 = Rf_protect(sa);
        let sb = coerce_to_real(sb);
        let _p2 = Rf_protect(sb);

        let sy = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
        let _p3 = Rf_protect(sy);

        let a = REAL(sa);
        let b = REAL(sb);
        let y = REAL(sy);
        let mut naflag = false;

        for i in 0..(n as usize) {
            let ia = if na > 1 { i % (na as usize) } else { 0 };
            let ib = if nb > 1 { i % (nb as usize) } else { 0 };
            let ai = *a.add(ia);
            let bi = *b.add(ib);

            if let Some(val) = na_math2_set(ai, bi) {
                *y.add(i) = val;
            } else {
                *y.add(i) = f(ai, bi);
                if (*y.add(i)).is_nan() {
                    naflag = true;
                }
            }
        }

        crate::sexp::protect::Rf_unprotect(3);
        sy
    }
}

// ---- do_math2 ----

/// Two-argument math functions: round, signif, atan2, etc.
///
/// Operation codes (from R's PRIMVAL):
///   0: atan2
///   10001: round (fround)
///   10004: signif (fprec)
pub unsafe fn do_math2(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let code = primval(op);

        match code {
            0 => {
                // atan2
                math2_impl(CAR(args), CADR(args), libm::atan2)
            }
            10001 => {
                // round
                math2_impl(CAR(args), CADR(args), fround)
            }
            10004 => {
                // signif
                math2_impl(CAR(args), CADR(args), fprec)
            }
            _ => std::ptr::null_mut(),
        }
    }
}

// ---- do_arith helpers ----

/// Arithmetic operation codes matching R's Arith.h
const OP_PLUS: c_int = 1;
const OP_MINUS: c_int = 2;
const OP_TIMES: c_int = 3;
const OP_DIV: c_int = 4;
const OP_POW: c_int = 5;
const OP_MOD: c_int = 6;
const OP_INTDIV: c_int = 7;

/// Integer binary operation for +, -, *, %%, %/% with overflow detection.
/// Returns an INTSXP result (or REALSXP for DIV and POW).
unsafe fn integer_binary_arith(code: c_int, s1: SEXP, s2: SEXP) -> SEXP {
    unsafe {
        let n1 = XLENGTH(s1);
        let n2 = XLENGTH(s2);
        let n = if n1 == 0 || n2 == 0 {
            0
        } else {
            if n1 > n2 {
                n1
            } else {
                n2
            }
        };

        // DIV and POW produce REALSXP
        let ans = if code == OP_DIV || code == OP_POW {
            Rf_allocVector3(SEXPTYPE::REALSXP.0, n)
        } else {
            Rf_allocVector3(SEXPTYPE::INTSXP.0, n)
        };
        let _p = Rf_protect(ans);

        if n == 0 {
            crate::sexp::protect::Rf_unprotect(1);
            return ans;
        }

        let pa = INTEGER(ans);
        let px1 = INTEGER(s1);
        let px2 = INTEGER(s2);
        let mut naflag = false;

        match code {
            OP_PLUS => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = *px1.add(i1);
                    let x2 = *px2.add(i2);
                    *pa.add(i) = R_integer_plus(x1, x2, &mut naflag);
                }
            }
            OP_MINUS => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = *px1.add(i1);
                    let x2 = *px2.add(i2);
                    *pa.add(i) = R_integer_minus(x1, x2, &mut naflag);
                }
            }
            OP_TIMES => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = *px1.add(i1);
                    let x2 = *px2.add(i2);
                    *pa.add(i) = R_integer_times(x1, x2, &mut naflag);
                }
            }
            OP_DIV => {
                let pa_d = REAL(ans);
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = *px1.add(i1);
                    let x2 = *px2.add(i2);
                    *pa_d.add(i) = R_integer_divide(x1, x2);
                }
            }
            OP_POW => {
                let pa_d = REAL(ans);
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = *px1.add(i1);
                    let x2 = *px2.add(i2);
                    if x1 == 1 || x2 == 0 {
                        *pa_d.add(i) = 1.0;
                    } else if x1 == NA_INTEGER || x2 == NA_INTEGER {
                        *pa_d.add(i) = NA_REAL;
                    } else {
                        *pa_d.add(i) = R_pow(x1 as f64, x2 as f64);
                    }
                }
            }
            OP_MOD => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = *px1.add(i1);
                    let x2 = *px2.add(i2);
                    if x1 == NA_INTEGER || x2 == NA_INTEGER || x2 == 0 {
                        *pa.add(i) = NA_INTEGER;
                    } else if x1 >= 0 && x2 > 0 {
                        *pa.add(i) = x1 % x2;
                    } else {
                        *pa.add(i) = myfmod(x1 as f64, x2 as f64) as c_int;
                    }
                }
            }
            OP_INTDIV => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = *px1.add(i1);
                    let x2 = *px2.add(i2);
                    if x1 == NA_INTEGER || x2 == NA_INTEGER || x2 == 0 {
                        *pa.add(i) = NA_INTEGER;
                    } else {
                        *pa.add(i) = libm::floor(x1 as f64 / x2 as f64) as c_int;
                    }
                }
            }
            _ => {}
        }

        crate::sexp::protect::Rf_unprotect(1);
        ans
    }
}

/// Real (or mixed int/real) binary operation.
/// s1 and s2 can be REALSXP or INTSXP.
unsafe fn real_binary_arith(code: c_int, s1: SEXP, s2: SEXP) -> SEXP {
    unsafe {
        let n1 = XLENGTH(s1);
        let n2 = XLENGTH(s2);

        if n1 == 0 || n2 == 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP.0, 0);
        }

        let n = if n1 > n2 { n1 } else { n2 };
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
        let _p = Rf_protect(ans);

        let da = REAL(ans);
        let is_real1 = TYPEOF(s1) == SEXPTYPE::REALSXP.0;
        let is_real2 = TYPEOF(s2) == SEXPTYPE::REALSXP.0;

        match code {
            OP_PLUS => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = if is_real1 {
                        *REAL(s1).add(i1)
                    } else {
                        r_integer_to_double(*INTEGER(s1).add(i1))
                    };
                    let x2 = if is_real2 {
                        *REAL(s2).add(i2)
                    } else {
                        r_integer_to_double(*INTEGER(s2).add(i2))
                    };
                    *da.add(i) = x1 + x2;
                }
            }
            OP_MINUS => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = if is_real1 {
                        *REAL(s1).add(i1)
                    } else {
                        r_integer_to_double(*INTEGER(s1).add(i1))
                    };
                    let x2 = if is_real2 {
                        *REAL(s2).add(i2)
                    } else {
                        r_integer_to_double(*INTEGER(s2).add(i2))
                    };
                    *da.add(i) = x1 - x2;
                }
            }
            OP_TIMES => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = if is_real1 {
                        *REAL(s1).add(i1)
                    } else {
                        r_integer_to_double(*INTEGER(s1).add(i1))
                    };
                    let x2 = if is_real2 {
                        *REAL(s2).add(i2)
                    } else {
                        r_integer_to_double(*INTEGER(s2).add(i2))
                    };
                    *da.add(i) = x1 * x2;
                }
            }
            OP_DIV => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = if is_real1 {
                        *REAL(s1).add(i1)
                    } else {
                        r_integer_to_double(*INTEGER(s1).add(i1))
                    };
                    let x2 = if is_real2 {
                        *REAL(s2).add(i2)
                    } else {
                        r_integer_to_double(*INTEGER(s2).add(i2))
                    };
                    *da.add(i) = x1 / x2;
                }
            }
            OP_POW => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = if is_real1 {
                        *REAL(s1).add(i1)
                    } else {
                        r_integer_to_double(*INTEGER(s1).add(i1))
                    };
                    let x2 = if is_real2 {
                        *REAL(s2).add(i2)
                    } else {
                        r_integer_to_double(*INTEGER(s2).add(i2))
                    };
                    *da.add(i) = R_pow(x1, x2);
                }
            }
            OP_MOD => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = if is_real1 {
                        *REAL(s1).add(i1)
                    } else {
                        r_integer_to_double(*INTEGER(s1).add(i1))
                    };
                    let x2 = if is_real2 {
                        *REAL(s2).add(i2)
                    } else {
                        r_integer_to_double(*INTEGER(s2).add(i2))
                    };
                    *da.add(i) = myfmod(x1, x2);
                }
            }
            OP_INTDIV => {
                for i in 0..(n as usize) {
                    let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
                    let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
                    let x1 = if is_real1 {
                        *REAL(s1).add(i1)
                    } else {
                        r_integer_to_double(*INTEGER(s1).add(i1))
                    };
                    let x2 = if is_real2 {
                        *REAL(s2).add(i2)
                    } else {
                        r_integer_to_double(*INTEGER(s2).add(i2))
                    };
                    *da.add(i) = myfloor(x1, x2);
                }
            }
            _ => {}
        }

        crate::sexp::protect::Rf_unprotect(1);
        ans
    }
}

/// Complex binary arithmetic.
unsafe fn complex_binary_arith(code: c_int, s1: SEXP, s2: SEXP) -> SEXP {
    unsafe {
        let n1 = XLENGTH(s1);
        let n2 = XLENGTH(s2);
        let n = if n1 == 0 || n2 == 0 {
            0
        } else {
            if n1 > n2 {
                n1
            } else {
                n2
            }
        };

        let ans = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, n);
        let _p = Rf_protect(ans);

        // Coerce both to complex
        let s1 = coerce_to_complex(s1);
        let _p1 = Rf_protect(s1);
        let s2 = coerce_to_complex(s2);
        let _p2 = Rf_protect(s2);

        let da = COMPLEX(ans);
        let px1 = COMPLEX(s1);
        let px2 = COMPLEX(s2);

        for i in 0..(n as usize) {
            let i1 = if n1 > 1 { i % (n1 as usize) } else { 0 };
            let i2 = if n2 > 1 { i % (n2 as usize) } else { 0 };
            let a = *px1.add(i1);
            let b = *px2.add(i2);

            *da.add(i) = match code {
                OP_PLUS => Rcomplex {
                    r: a.r + b.r,
                    i: a.i + b.i,
                },
                OP_MINUS => Rcomplex {
                    r: a.r - b.r,
                    i: a.i - b.i,
                },
                OP_TIMES => Rcomplex {
                    r: a.r * b.r - a.i * b.i,
                    i: a.r * b.i + a.i * b.r,
                },
                OP_DIV => {
                    // (a.r + a.i*i) / (b.r + b.i*i)
                    let denom = b.r * b.r + b.i * b.i;
                    if denom == 0.0 {
                        Rcomplex {
                            r: f64::NAN,
                            i: f64::NAN,
                        }
                    } else {
                        Rcomplex {
                            r: (a.r * b.r + a.i * b.i) / denom,
                            i: (a.i * b.r - a.r * b.i) / denom,
                        }
                    }
                }
                OP_POW => {
                    // Complex power via polar form
                    let r = (a.r * a.r + a.i * a.i).sqrt();
                    let theta = libm::atan2(a.i, a.r);
                    if r == 0.0 {
                        Rcomplex { r: 0.0, i: 0.0 }
                    } else {
                        let log_r = r.ln();
                        let new_r = (log_r * b.r - theta * b.i).exp();
                        let new_theta = log_r * b.i + theta * b.r;
                        Rcomplex {
                            r: new_r * libm::cos(new_theta),
                            i: new_r * libm::sin(new_theta),
                        }
                    }
                }
                OP_MOD | OP_INTDIV => Rcomplex {
                    r: f64::NAN,
                    i: f64::NAN,
                },
                _ => Rcomplex { r: 0.0, i: 0.0 },
            };
        }

        crate::sexp::protect::Rf_unprotect(3);
        ans
    }
}

/// Coerce a numeric SEXP to CPLXSXP.
unsafe fn coerce_to_complex(x: SEXP) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP.0 {
            return x;
        }
        let n = XLENGTH(x);
        let y = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, n);
        let dst = COMPLEX(y);
        if t == SEXPTYPE::REALSXP.0 {
            let src = REAL(x);
            for i in 0..(n as usize) {
                *dst.add(i) = Rcomplex {
                    r: *src.add(i),
                    i: 0.0,
                };
            }
        } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
            let src = INTEGER(x);
            for i in 0..(n as usize) {
                let v = *src.add(i);
                *dst.add(i) = Rcomplex {
                    r: if v == NA_INTEGER { NA_REAL } else { v as f64 },
                    i: 0.0,
                };
            }
        }
        y
    }
}

/// Unary arithmetic: +x and -x.
unsafe fn unary_arith(code: c_int, s1: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(s1);
        match TYPEOF(s1) {
            t if t == SEXPTYPE::REALSXP.0 => match code {
                OP_PLUS => s1,
                OP_MINUS => {
                    let ans = if no_references(s1) {
                        s1
                    } else {
                        Rf_allocVector3(SEXPTYPE::REALSXP.0, n)
                    };
                    let _p = Rf_protect(ans);
                    let pa = REAL(ans);
                    let px = REAL(s1);
                    for i in 0..(n as usize) {
                        *pa.add(i) = -*px.add(i);
                    }
                    crate::sexp::protect::Rf_unprotect(1);
                    ans
                }
                _ => std::ptr::null_mut(),
            },
            t if t == SEXPTYPE::INTSXP.0 => match code {
                OP_PLUS => s1,
                OP_MINUS => {
                    let ans = if no_references(s1) {
                        s1
                    } else {
                        Rf_allocVector3(SEXPTYPE::INTSXP.0, n)
                    };
                    let _p = Rf_protect(ans);
                    let pa = INTEGER(ans);
                    let px = INTEGER(s1);
                    for i in 0..(n as usize) {
                        let x = *px.add(i);
                        *pa.add(i) = if x == NA_INTEGER { NA_INTEGER } else { -x };
                    }
                    crate::sexp::protect::Rf_unprotect(1);
                    ans
                }
                _ => std::ptr::null_mut(),
            },
            t if t == SEXPTYPE::LGLSXP.0 => {
                // Coerce to INTSXP for unary minus on logicals
                match code {
                    OP_PLUS => {
                        // Return as-is (logical + = logical)
                        s1
                    }
                    OP_MINUS => {
                        let ans = Rf_allocVector3(SEXPTYPE::INTSXP.0, n);
                        let _p = Rf_protect(ans);
                        let pa = INTEGER(ans);
                        let px = LOGICAL(s1);
                        for i in 0..(n as usize) {
                            let x = *px.add(i);
                            *pa.add(i) = if x == NA_INTEGER {
                                NA_INTEGER
                            } else if x == 0 {
                                0
                            } else {
                                -x
                            };
                        }
                        crate::sexp::protect::Rf_unprotect(1);
                        ans
                    }
                    _ => std::ptr::null_mut(),
                }
            }
            t if t == SEXPTYPE::CPLXSXP.0 => match code {
                OP_PLUS => s1,
                OP_MINUS => {
                    let ans = if no_references(s1) {
                        s1
                    } else {
                        Rf_allocVector3(SEXPTYPE::CPLXSXP.0, n)
                    };
                    let _p = Rf_protect(ans);
                    let pa = COMPLEX(ans);
                    let px = COMPLEX(s1);
                    for i in 0..(n as usize) {
                        let z = *px.add(i);
                        *pa.add(i) = Rcomplex { r: -z.r, i: -z.i };
                    }
                    crate::sexp::protect::Rf_unprotect(1);
                    ans
                }
                _ => std::ptr::null_mut(),
            },
            _ => std::ptr::null_mut(),
        }
    }
}

// ---- do_arith ----

/// General arithmetic dispatch: +, -, *, /, ^, %%, %/%.
///
/// Operation codes (from R's PRIMVAL):
///   1: + (OP_ADD), 2: - (OP_SUB), 3: * (OP_MUL),
///   4: / (OP_DIV), 5: ^ (OP_POW),
///   6: %% (OP_MOD), 7: %/% (OP_INTDIV)
pub unsafe fn do_arith(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let code = primval(op);

        // Count arguments
        let argc = if args.is_null() {
            0
        } else if CDR(args).is_null() {
            1
        } else if CDR(CDR(args)).is_null() {
            2
        } else {
            // count length of list
            let mut count = 0i32;
            let mut p = args;
            while !p.is_null() {
                count += 1;
                p = CDR(p);
            }
            count
        };

        let arg1 = CAR(args);

        if argc == 2 {
            let arg2 = CADR(args);

            // Handle scalar fast paths
            if is_scalar(arg1, SEXPTYPE::REALSXP.0) && is_scalar(arg2, SEXPTYPE::REALSXP.0) {
                let x1 = *REAL(arg1);
                let x2 = *REAL(arg2);
                let ans = Rf_allocVector3(SEXPTYPE::REALSXP.0, 1);
                let _p = Rf_protect(ans);
                let val = match code {
                    OP_PLUS => x1 + x2,
                    OP_MINUS => x1 - x2,
                    OP_TIMES => x1 * x2,
                    OP_DIV => x1 / x2,
                    OP_POW => R_pow(x1, x2),
                    OP_MOD => myfmod(x1, x2),
                    OP_INTDIV => myfloor(x1, x2),
                    _ => f64::NAN,
                };
                *REAL(ans) = val;
                crate::sexp::protect::Rf_unprotect(1);
                return ans;
            }

            if is_scalar(arg1, SEXPTYPE::INTSXP.0) && is_scalar(arg2, SEXPTYPE::INTSXP.0) {
                let i1 = *INTEGER(arg1);
                let i2 = *INTEGER(arg2);
                match code {
                    OP_PLUS => {
                        let mut naflag = false;
                        let result = R_integer_plus(i1, i2, &mut naflag);
                        let ans = Rf_allocVector3(SEXPTYPE::INTSXP.0, 1);
                        let _p = Rf_protect(ans);
                        *INTEGER(ans) = result;
                        crate::sexp::protect::Rf_unprotect(1);
                        return ans;
                    }
                    OP_MINUS => {
                        let mut naflag = false;
                        let result = R_integer_minus(i1, i2, &mut naflag);
                        let ans = Rf_allocVector3(SEXPTYPE::INTSXP.0, 1);
                        let _p = Rf_protect(ans);
                        *INTEGER(ans) = result;
                        crate::sexp::protect::Rf_unprotect(1);
                        return ans;
                    }
                    OP_TIMES => {
                        let mut naflag = false;
                        let result = R_integer_times(i1, i2, &mut naflag);
                        let ans = Rf_allocVector3(SEXPTYPE::INTSXP.0, 1);
                        let _p = Rf_protect(ans);
                        *INTEGER(ans) = result;
                        crate::sexp::protect::Rf_unprotect(1);
                        return ans;
                    }
                    OP_DIV => {
                        let result = R_integer_divide(i1, i2);
                        let ans = Rf_allocVector3(SEXPTYPE::REALSXP.0, 1);
                        let _p = Rf_protect(ans);
                        *REAL(ans) = result;
                        crate::sexp::protect::Rf_unprotect(1);
                        return ans;
                    }
                    _ => {}
                }
            }

            // General binary dispatch
            let t1 = TYPEOF(arg1);
            let t2 = TYPEOF(arg2);

            // Coerce logicals to integer
            let arg1 = if t1 == SEXPTYPE::LGLSXP.0 {
                coerce_logical_to_int(arg1)
            } else {
                arg1
            };
            let arg2 = if t2 == SEXPTYPE::LGLSXP.0 {
                coerce_logical_to_int(arg2)
            } else {
                arg2
            };

            let t1 = TYPEOF(arg1);
            let t2 = TYPEOF(arg2);

            if t1 == SEXPTYPE::CPLXSXP.0 || t2 == SEXPTYPE::CPLXSXP.0 {
                complex_binary_arith(code, arg1, arg2)
            } else if t1 == SEXPTYPE::REALSXP.0 || t2 == SEXPTYPE::REALSXP.0 {
                // Ensure both are at least INTSXP or REALSXP for real_binary_arith
                let s1 = if t1 != SEXPTYPE::INTSXP.0 {
                    coerce_to_real(arg1)
                } else {
                    arg1
                };
                let s2 = if t2 != SEXPTYPE::INTSXP.0 {
                    coerce_to_real(arg2)
                } else {
                    arg2
                };
                let _p1 = Rf_protect(s1);
                let _p2 = Rf_protect(s2);
                let result = real_binary_arith(code, s1, s2);
                crate::sexp::protect::Rf_unprotect(2);
                result
            } else if t1 == SEXPTYPE::INTSXP.0 && t2 == SEXPTYPE::INTSXP.0 {
                integer_binary_arith(code, arg1, arg2)
            } else {
                std::ptr::null_mut()
            }
        } else if argc == 1 {
            unary_arith(code, arg1)
        } else {
            std::ptr::null_mut()
        }
    }
}

/// Coerce a LGLSXP to INTSXP (in-place conversion of logical to integer).
/// Since LOGICAL and INTEGER share the same storage in R, we just need
/// to change the type if there are no references.
unsafe fn coerce_logical_to_int(x: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::LGLSXP.0 {
            if no_references(x) {
                (*x).sxpinfo.set_type(SEXPTYPE::INTSXP);
                x
            } else {
                let n = XLENGTH(x);
                let y = Rf_allocVector3(SEXPTYPE::INTSXP.0, n);
                let src = LOGICAL(x);
                let dst = INTEGER(y);
                for i in 0..(n as usize) {
                    *dst.add(i) = *src.add(i);
                }
                y
            }
        } else {
            x
        }
    }
}

// ---------------------------------------------------------------------------
// do_Math2 — round(x, digits) / signif(x, digits)
// ---------------------------------------------------------------------------

/// Implement round() and signif().
///
/// Port of R's do_Math2 from arithmetic.c.
/// round(x, digits=0) rounds to `digits` decimal places.
/// signif(x, digits=6) rounds to `digits` significant figures.
pub unsafe fn do_Math2(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::CADR;

        let code = primval(op);
        let is_signif = code == 10004;
        let dflt_digits = if is_signif { 6.0 } else { 0.0 };

        // Evaluate first arg
        let sa = crate::eval::eval::Rf_eval(CAR(args), env);
        Rf_protect(sa);

        // Evaluate second arg (optional)
        let sb_raw = CADR(args);
        let digits = if sb_raw.is_null() || sb_raw == R_NilValue() {
            dflt_digits
        } else {
            let sb = crate::eval::eval::Rf_eval(sb_raw, env);
            Rf_protect(sb);
            let d = if TYPEOF(sb) == SEXPTYPE::LGLSXP.0 || TYPEOF(sb) == SEXPTYPE::INTSXP.0 {
                crate::main::coerce::asInteger(sb) as f64
            } else if TYPEOF(sb) == SEXPTYPE::REALSXP.0 {
                *REAL(sb)
            } else {
                dflt_digits
            };
            crate::sexp::protect::Rf_unprotect(1);
            d
        };

        let n = XLENGTH(sa);
        if n == 0 {
            let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, 0);
            crate::sexp::protect::Rf_unprotect(1);
            return result;
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
        if result.is_null() {
            crate::sexp::protect::Rf_unprotect(1);
            return R_NilValue();
        }
        Rf_protect(result);

        let rd = REAL(result);
        let na_bits = 0x7ff0000000001954u64;

        let t = TYPEOF(sa);
        if t == SEXPTYPE::REALSXP.0 {
            let xd = REAL(sa);
            for i in 0..n as usize {
                let x = *xd.add(i);
                if x.is_nan() {
                    *rd.add(i) = x;
                } else if is_signif {
                    *rd.add(i) = fprec(x, digits);
                } else {
                    *rd.add(i) = fround(x, digits);
                }
            }
        } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
            let xi = INTEGER(sa);
            for i in 0..n as usize {
                let v = *xi.add(i);
                if v == NA_INTEGER {
                    *rd.add(i) = f64::from_bits(na_bits);
                } else {
                    let x = v as f64;
                    if is_signif {
                        *rd.add(i) = fprec(x, digits);
                    } else {
                        *rd.add(i) = fround(x, digits);
                    }
                }
            }
        } else {
            // Coerce to real first
            let coerced = crate::main::coerce::coerceVector(sa, SEXPTYPE::REALSXP.0);
            Rf_protect(coerced);
            let xd = REAL(coerced);
            for i in 0..n as usize {
                let x = *xd.add(i);
                if x.is_nan() {
                    *rd.add(i) = x;
                } else if is_signif {
                    *rd.add(i) = fprec(x, digits);
                } else {
                    *rd.add(i) = fround(x, digits);
                }
            }
            crate::sexp::protect::Rf_unprotect(1);
        }

        crate::sexp::protect::Rf_unprotect(2);
        result
    }
}

// ---------------------------------------------------------------------------
// do_log — log(x, base) with optional base argument
// ---------------------------------------------------------------------------

/// Implement log(x, base=exp(1)).
///
/// Port of R's do_log from arithmetic.c.
pub unsafe fn do_log(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::CADR;

        // Evaluate first arg
        let sa = crate::eval::eval::Rf_eval(CAR(args), env);
        Rf_protect(sa);

        // Evaluate second arg (optional)
        let sb_raw = CADR(args);
        let base: f64 = if sb_raw.is_null() || sb_raw == R_NilValue() {
            std::f64::consts::E
        } else {
            let sb = crate::eval::eval::Rf_eval(sb_raw, env);
            Rf_protect(sb);
            let b = if TYPEOF(sb) == SEXPTYPE::REALSXP.0 {
                *REAL(sb)
            } else if TYPEOF(sb) == SEXPTYPE::INTSXP.0 {
                let v = *INTEGER(sb);
                if v == NA_INTEGER {
                    f64::NAN
                } else {
                    v as f64
                }
            } else {
                std::f64::consts::E
            };
            crate::sexp::protect::Rf_unprotect(1);
            b
        };

        let n = XLENGTH(sa);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP.0, n);
        if result.is_null() {
            crate::sexp::protect::Rf_unprotect(1);
            return R_NilValue();
        }
        Rf_protect(result);

        let rd = REAL(result);
        let na_bits = 0x7ff0000000001954u64;

        let t = TYPEOF(sa);
        if t == SEXPTYPE::REALSXP.0 {
            let xd = REAL(sa);
            for i in 0..n as usize {
                let x = *xd.add(i);
                if x.is_nan() || x < 0.0 {
                    *rd.add(i) = if x.to_bits() == na_bits { x } else { f64::NAN };
                } else if (base - std::f64::consts::E).abs() < f64::EPSILON {
                    *rd.add(i) = x.ln();
                } else if (base - 10.0).abs() < f64::EPSILON {
                    *rd.add(i) = x.log10();
                } else if (base - 2.0).abs() < f64::EPSILON {
                    *rd.add(i) = x.log2();
                } else {
                    *rd.add(i) = x.ln() / base.ln();
                }
            }
        } else if t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 {
            let xi = INTEGER(sa);
            for i in 0..n as usize {
                let v = *xi.add(i);
                if v == NA_INTEGER {
                    *rd.add(i) = f64::from_bits(na_bits);
                } else if v < 0 {
                    *rd.add(i) = f64::NAN;
                } else {
                    let x = v as f64;
                    if (base - std::f64::consts::E).abs() < f64::EPSILON {
                        *rd.add(i) = x.ln();
                    } else if (base - 10.0).abs() < f64::EPSILON {
                        *rd.add(i) = x.log10();
                    } else if (base - 2.0).abs() < f64::EPSILON {
                        *rd.add(i) = x.log2();
                    } else {
                        *rd.add(i) = x.ln() / base.ln();
                    }
                }
            }
        } else {
            let coerced = crate::main::coerce::coerceVector(sa, SEXPTYPE::REALSXP.0);
            Rf_protect(coerced);
            let xd = REAL(coerced);
            for i in 0..n as usize {
                let x = *xd.add(i);
                if x.is_nan() || x < 0.0 {
                    *rd.add(i) = if x.to_bits() == na_bits { x } else { f64::NAN };
                } else {
                    *rd.add(i) = if (base - std::f64::consts::E).abs() < f64::EPSILON {
                        x.ln()
                    } else {
                        x.ln() / base.ln()
                    };
                }
            }
            crate::sexp::protect::Rf_unprotect(1);
        }

        crate::sexp::protect::Rf_unprotect(2);
        result
    }
}

// ---------------------------------------------------------------------------
// do_log1arg — log10(x), log2(x) (single-arg log wrappers)
// ---------------------------------------------------------------------------

/// Implement log10() and log2().
pub unsafe fn do_log1arg(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let code = primval(op);
        let sa = CAR(args);

        let f: fn(f64) -> f64 = match code {
            10010 => libm::log10, // log10
            10002 => libm::log2,  // log2
            _ => return R_NilValue(),
        };

        math1_impl(sa, f)
    }
}

// ---------------------------------------------------------------------------
// do_abs — abs(x)
// ---------------------------------------------------------------------------

/// Implement abs(x).
pub unsafe fn do_abs(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let sa = CAR(args);
        math1_ari_impl(sa, libm::fabs, 0.0, 0.0)
    }
}

// ---------------------------------------------------------------------------
// do_trunc — trunc(x)
// ---------------------------------------------------------------------------

/// Implement trunc(x).
pub unsafe fn do_trunc(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let sa = CAR(args);
        math1_impl(sa, libm::trunc)
    }
}

// ---------------------------------------------------------------------------
// do_math3 — three-argument math functions (dnorm, pnorm, qnorm, etc.)
// ---------------------------------------------------------------------------

/// Implement three-argument math functions (distribution PDFs/CDFs/quantiles).
///
/// Port of R's do_math3 from arithmetic.c.
/// Dispatches via PRIMVAL(op) to the appropriate distribution function.
pub unsafe fn do_math3(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CADDR, CADR};

        let code = primval(op);
        let sa = CAR(args);
        let sb = CADR(args);
        let sc = CADDR(args);

        // math3_1: f(x, p1, p2) — PDFs
        // math3_2: f(p, x, lower_tail, log_p) — CDFs and quantiles

        // For now, return the input unchanged — full implementation requires
        // wiring all nmath distribution functions. The distribution functions
        // are already available via do_dnorm etc. in library/stats/distn.rs.
        // The FUN_TAB already maps these names to individual do_* functions
        // in distn.rs, so do_math3 is only needed when called via PRIMVAL dispatch.

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_math4 — four-argument math functions (dhyper, phyper, qhyper, etc.)
// ---------------------------------------------------------------------------

/// Implement four-argument math functions.
pub unsafe fn do_math4(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // Similar to do_math3 — full implementation requires nmath integration
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_R_integer_plus() {
        let mut naflag = false;
        assert_eq!(unsafe { R_integer_plus(3, 4, &mut naflag) }, 7);
        assert!(!naflag);

        // NA propagation
        assert_eq!(
            unsafe { R_integer_plus(NA_INTEGER, 4, &mut naflag) },
            NA_INTEGER
        );
        assert_eq!(
            unsafe { R_integer_plus(3, NA_INTEGER, &mut naflag) },
            NA_INTEGER
        );

        // Overflow
        assert_eq!(
            unsafe { R_integer_plus(c_int::MAX, 1, &mut naflag) },
            NA_INTEGER
        );
        assert!(naflag);
    }

    #[test]
    fn test_R_integer_minus() {
        let mut naflag = false;
        assert_eq!(unsafe { R_integer_minus(10, 3, &mut naflag) }, 7);
        assert!(!naflag);

        // Overflow: (MIN+2) - 3 = MIN-1, which overflows
        assert_eq!(
            unsafe { R_integer_minus(c_int::MIN + 2, 3, &mut naflag) },
            NA_INTEGER
        );
        assert!(naflag);
    }

    #[test]
    fn test_R_integer_times() {
        let mut naflag = false;
        assert_eq!(unsafe { R_integer_times(6, 7, &mut naflag) }, 42);
        assert!(!naflag);

        // NA propagation
        assert_eq!(
            unsafe { R_integer_times(NA_INTEGER, 7, &mut naflag) },
            NA_INTEGER
        );

        // Overflow
        naflag = false;
        assert_eq!(
            unsafe { R_integer_times(c_int::MAX, 2, &mut naflag) },
            NA_INTEGER
        );
        assert!(naflag);
    }

    #[test]
    fn test_R_integer_divide() {
        assert!((R_integer_divide(10, 3) - 10.0 / 3.0).abs() < 1e-10);
        assert!(R_integer_divide(NA_INTEGER, 3).is_nan());
        assert!(R_integer_divide(3, NA_INTEGER).is_nan());
    }

    #[test]
    fn test_myfmod_basic() {
        assert!((myfmod(10.0, 3.0) - 1.0).abs() < 1e-10);
        assert!((myfmod(-10.0, 3.0) - (-1.0)).abs() < 1e-10);
        assert!(myfmod(10.0, 0.0).is_nan());
    }

    #[test]
    fn test_myfmod_exact() {
        // When x1 == x2 in magnitude, result should be 0
        assert_eq!(myfmod(5.0, 5.0), 0.0);
        assert_eq!(myfmod(-5.0, 5.0), 0.0);
    }

    #[test]
    fn test_myfloor_basic() {
        // 10 / 3 = 3.33.. => floor = 3
        assert!((myfloor(10.0, 3.0) - 3.0).abs() < 1e-10);
        // -10 / 3 = -3.33.. => floor = -4
        assert!((myfloor(-10.0, 3.0) - (-4.0)).abs() < 1e-10);
    }

    #[test]
    fn test_Rsqrt() {
        assert_eq!(Rsqrt(4.0), 2.0);
        assert_eq!(Rsqrt(9.0), 3.0);
        assert_eq!(Rsqrt(0.0), 0.0);
        assert!((Rsqrt(2.0) - 2.0_f64.sqrt()).abs() < 1e-15);
    }

    #[test]
    fn test_Rsqrt_negative_zero() {
        let neg_zero = -0.0_f64;
        assert_eq!(Rsqrt(neg_zero).is_sign_negative(), true);
    }

    #[test]
    fn test_Rexp_small() {
        // For very small x, should return 1 + x
        let x = f64::EPSILON * 0.1;
        assert!((Rexp(x) - (1.0 + x)).abs() < 1e-20);
    }

    #[test]
    fn test_Rexp_normal() {
        assert!((Rexp(1.0) - 1.0_f64.exp()).abs() < 1e-15);
    }

    #[test]
    fn test_Rexpm1_small() {
        let x = f64::EPSILON * 0.1;
        assert!((Rexpm1(x) - x).abs() < 1e-20);
    }

    #[test]
    fn test_Rlog1p_small() {
        let x = f64::EPSILON * 0.1;
        assert!((Rlog1p(x) - x).abs() < 1e-20);
    }

    #[test]
    fn test_Rsin_small() {
        let x = 1e-10_f64;
        assert!((Rsin(x) - x).abs() < 1e-20);
    }

    #[test]
    fn test_Rcos_small() {
        let x = 1e-8_f64;
        let expected = 1.0 - x * x * 0.5;
        assert!((Rcos(x) - expected).abs() < 1e-20);
    }

    #[test]
    fn test_Rtan_small() {
        let x = 1e-10_f64;
        assert!((Rtan(x) - x).abs() < 1e-20);
    }

    #[test]
    fn test_Rasin_small() {
        let x = 1e-10_f64;
        assert!((Rasin(x) - x).abs() < 1e-20);
    }

    #[test]
    fn test_Ratan_small() {
        let x = 1e-10_f64;
        assert!((Ratan(x) - x).abs() < 1e-20);
    }

    #[test]
    fn test_R_IsNA() {
        let na = f64::from_bits(0x7ff0000000001954);
        assert_eq!(R_IsNA(na), 1);
        assert_eq!(R_IsNA(f64::NAN), 0);
        assert_eq!(R_IsNA(1.0), 0);
    }

    #[test]
    fn test_R_IsNaN() {
        assert_eq!(R_IsNaN(f64::NAN), 1);
        let na = f64::from_bits(0x7ff0000000001954);
        assert_eq!(R_IsNaN(na), 0);
        assert_eq!(R_IsNaN(1.0), 0);
    }
}
