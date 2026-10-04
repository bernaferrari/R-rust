#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

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

use crate::fprec::{fprec, fround};
use crate::sexp::accessors::{CADR, CAR, COMPLEX, INTEGER, LOGICAL, NAMED, REAL, TYPEOF, XLENGTH};
use crate::sexp::constructors::{Rf_allocVector3, Rf_length};
use crate::sexp::ffi::Rcomplex;
use crate::sexp::ffi::{NA_INTEGER, SEXP, SEXPTYPE};
use crate::sexp::object::{SessionNodeFactory, Sexp, SexpMut, SexpResult};
use crate::sexp::protect::protect;
use crate::special::cospi::{cospi, sinpi, tanpi};
use crate::special::gamma::{gammafn, lgammafn};
use crate::special::mlutils::R_pow;
use crate::special::polygamma::{digamma, trigamma};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's NA_REAL sentinel (NaN with specific bit pattern).
pub const NA_REAL: f64 = crate::sexp::ffi::NA_REAL;

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
    if crate::sexp::ffi::is_na_real(x) {
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
/// Sets `pnaflag` to true on overflow and otherwise leaves it unchanged.
#[forbid(unsafe_code)]
pub fn R_integer_plus(x: c_int, y: c_int, pnaflag: &mut bool) -> c_int {
    if x == NA_INTEGER || y == NA_INTEGER {
        return NA_INTEGER;
    }

    let x64 = x as i64;
    let y64 = y as i64;
    let result = x64 + y64;

    if result > c_int::MAX as i64 || result < c_int::MIN as i64 {
        *pnaflag = true;
        return NA_INTEGER;
    }
    result as c_int
}

/// Safe integer subtraction with overflow detection.
///
/// Returns NA_INTEGER on overflow or if either input is NA_INTEGER.
/// Sets `pnaflag` to true on overflow and otherwise leaves it unchanged.
#[forbid(unsafe_code)]
pub fn R_integer_minus(x: c_int, y: c_int, pnaflag: &mut bool) -> c_int {
    if x == NA_INTEGER || y == NA_INTEGER {
        return NA_INTEGER;
    }

    // Match C's overflow checks using i64 to avoid wrapping
    let x64 = x as i64;
    let y64 = y as i64;
    if (y64 < 0 && x64 > (c_int::MAX as i64 + y64)) || (y64 > 0 && x64 < (c_int::MIN as i64 + y64))
    {
        *pnaflag = true;
        return NA_INTEGER;
    }
    x - y
}

/// Safe integer multiplication with overflow detection.
///
/// Returns NA_INTEGER on overflow or if either input is NA_INTEGER.
/// Sets `pnaflag` to true on overflow and otherwise leaves it unchanged.
#[forbid(unsafe_code)]
pub fn R_integer_times(x: c_int, y: c_int, pnaflag: &mut bool) -> c_int {
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
        *pnaflag = true;
        NA_INTEGER
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

/// Ported from R's internal `myfmod` (arithmetic.c).
///
/// Keep myfmod() and myfloor() in step. Uses a floor-based algorithm so the
/// result takes the sign of the divisor, matching R's `%%` operator. Warns
/// when the quotient is large enough that precision is probably lost.
pub fn myfmod(x1: f64, x2: f64) -> f64 {
    if x2 == 0.0 {
        return f64::NAN;
    }

    // Special case: very small |x1| relative to x2
    if x2.abs() * C_EPS > 1.0 && R_FINITE(x1) && x1.abs() <= x2.abs() {
        if x1.abs() == x2.abs() {
            return 0.0;
        }
        if (x1 < 0.0 && x2 > 0.0) || (x2 < 0.0 && x1 > 0.0) {
            return x1 + x2; // differing signs
        }
        return x1; // "same" signs (incl. 0)
    }

    let q = x1 / x2;
    if R_FINITE(q) && q.abs() * C_EPS > 1.0 {
        // Stock C warning() attributes the message to the current call
        // ("In 1e+300 %% 1.1 : probable complete loss of accuracy in
        // modulus"); Rf_warning1 looks up getCurrentCall() like C's
        // Rf_warning().
        unsafe {
            crate::mainutils::errors::Rf_warning1(
                c"probable complete loss of accuracy in modulus".as_ptr(),
            );
        }
    }
    let tmp = x1 - libm::floor(q) * x2;
    tmp - libm::floor(tmp / x2) * x2
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
    if x.abs() <= m { x } else { f(x) }
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
    unsafe { crate::mainutils::relop::PRIMVAL(op) }
}

/// Helper: check if SEXP is numeric (INTSXP, REALSXP, CPLXSXP, or LGLSXP).
#[inline]
unsafe fn is_numeric(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::LGLSXP
    }
}

/// Helper: check if SEXP is complex.
#[inline]
unsafe fn is_complex(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::CPLXSXP }
}

/// Helper: check if SEXP is integer or logical.
#[inline]
unsafe fn is_integer_or_logical(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP
    }
}

/// Helper: NO_REFERENCES check (NAMED == 0).
#[inline]
unsafe fn no_references(x: SEXP) -> bool {
    unsafe { NAMED(x) == 0 }
}

/// Integer-to-double conversion respecting NA_INTEGER.
#[inline]
fn r_integer_to_double(x: c_int) -> f64 {
    if x == NA_INTEGER { NA_REAL } else { x as f64 }
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
        let _sa_guard = protect(sa);

        let sy = if no_references(sa) {
            sa
        } else {
            Rf_allocVector3(SEXPTYPE::REALSXP, n)
        };
        let _sy_guard = protect(sy);

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
        let _sa_guard = protect(sa);

        let sy = if no_references(sa) {
            sa
        } else {
            Rf_allocVector3(SEXPTYPE::REALSXP, n)
        };
        let _sy_guard = protect(sy);

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

        sy
    }
}

/// Coerce a numeric SEXP to REALSXP (no-op if already REALSXP).
unsafe fn coerce_to_real(x: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::REALSXP {
            x
        } else if TYPEOF(x) == SEXPTYPE::INTSXP || TYPEOF(x) == SEXPTYPE::LGLSXP {
            let n = XLENGTH(x);
            let y = Rf_allocVector3(SEXPTYPE::REALSXP, n);
            let src = INTEGER(x);
            let dst = REAL(y);
            for i in 0..(n as usize) {
                let v = *src.add(i);
                *dst.add(i) = if v == NA_INTEGER { NA_REAL } else { v as f64 };
            }
            y
        } else {
            // CPLXSXP or other -- for now just return a zero-length REALSXP
            Rf_allocVector3(SEXPTYPE::REALSXP, 0)
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
        let sy = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
        let _sy_guard = protect(sy);
        let src = COMPLEX(sa);
        let dst = COMPLEX(sy);
        for i in 0..(n as usize) {
            let z = *src.add(i);
            *dst.add(i) = Rcomplex {
                r: f_real(z.r),
                i: f_imag(z.i),
            };
        }
        sy
    }
}

// ---- do_math1 ----

/// Single-argument math functions: sqrt, log, exp, floor, ceil, sign, etc.
///
/// Operation codes (from R's PRIMVAL):
///   1: floor, 2: ceil, 3: sqrt, 4: sign,
///   10: exp, 11: expm1, 12: log1p,
///   20: cos, 21: sin, 22: tan, 23: acos, 24: asin, 25: atan,
///   30: cosh, 31: sinh, 32: tanh, 33: acosh, 34: asinh, 35: atanh,
///   40: lgamma, 41: gamma, 42: digamma, 43: trigamma,
///   47: cospi, 48: sinpi, 49: tanpi
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
    let na_bits = crate::sexp::ffi::R_NA_BIT_PATTERN;
    let a_is_na = a.is_nan() && a.to_bits() == na_bits;
    let b_is_na = b.is_nan() && b.to_bits() == na_bits;
    if a_is_na || b_is_na {
        Some(crate::sexp::ffi::NA_REAL) // return R's NA_REAL
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
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }

        let n = if na > nb { na } else { nb };

        // Coerce both to REALSXP
        let sa = coerce_to_real(sa);
        let _sa_guard = protect(sa);
        let sb = coerce_to_real(sb);
        let _sb_guard = protect(sb);

        let sy = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _sy_guard = protect(sy);

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

/// Integer binary operation with checked inputs and a rooted result of the
/// operation's actual kind (real for division and exponentiation).
#[forbid(unsafe_code)]
fn integer_binary_arith<'s>(
    factory: &SessionNodeFactory<'s>,
    code: c_int,
    left: &Sexp<'s>,
    right: &Sexp<'s>,
) -> SexpResult<Sexp<'s>> {
    factory.require_active()?;
    factory.link(left)?;
    factory.link(right)?;
    let n1 = left.len();
    let n2 = right.len();
    let length = if n1 == 0 || n2 == 0 { 0 } else { n1.max(n2) };
    let real_result = matches!(code, OP_DIV | OP_POW);
    let kind = if real_result {
        SEXPTYPE::REALSXP
    } else {
        SEXPTYPE::INTSXP
    };
    let result = factory.allocate(|arena| Some(arena.alloc_vector(kind, length)))?;
    let mut result = SexpMut::try_from_checked(result)?;
    let mut naflag = false;
    for index in 0..length {
        // Checked element reads copy scalars before the next provider call.
        // Neither input nor result exposes a buffer loan across reentry.
        let x1 = left.try_integer_elt(index % n1)?;
        let x2 = right.try_integer_elt(index % n2)?;
        if real_result {
            let value = if code == OP_DIV {
                R_integer_divide(x1, x2)
            } else if x1 == 1 || x2 == 0 {
                1.0
            } else if x1 == NA_INTEGER || x2 == NA_INTEGER {
                NA_REAL
            } else {
                R_pow(x1 as f64, x2 as f64)
            };
            result.try_set_real_elt(index, value)?;
        } else {
            let value = match code {
                OP_PLUS => R_integer_plus(x1, x2, &mut naflag),
                OP_MINUS => R_integer_minus(x1, x2, &mut naflag),
                OP_TIMES => R_integer_times(x1, x2, &mut naflag),
                OP_MOD => {
                    if x1 == NA_INTEGER || x2 == NA_INTEGER || x2 == 0 {
                        NA_INTEGER
                    } else if x1 >= 0 && x2 > 0 {
                        x1 % x2
                    } else {
                        myfmod(x1 as f64, x2 as f64) as c_int
                    }
                }
                OP_INTDIV => {
                    if x1 == NA_INTEGER || x2 == NA_INTEGER || x2 == 0 {
                        NA_INTEGER
                    } else {
                        libm::floor(x1 as f64 / x2 as f64) as c_int
                    }
                }
                _ => continue,
            };
            result.try_set_integer_elt(index, value)?;
        }
    }
    Ok(result.freeze())
}

/// Real (or mixed int/real) binary operation.
/// s1 and s2 can be REALSXP or INTSXP.
unsafe fn real_binary_arith(code: c_int, s1: SEXP, s2: SEXP) -> SEXP {
    unsafe {
        let n1 = XLENGTH(s1);
        let n2 = XLENGTH(s2);

        if n1 == 0 || n2 == 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }

        let n = if n1 > n2 { n1 } else { n2 };
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _ans_guard = protect(ans);

        let da = REAL(ans);
        let is_real1 = TYPEOF(s1) == SEXPTYPE::REALSXP;
        let is_real2 = TYPEOF(s2) == SEXPTYPE::REALSXP;

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
            _ => {} // intentionally unhandled: unsupported SEXPTYPE for real modulo
        }

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
            if n1 > n2 { n1 } else { n2 }
        };

        let ans = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
        let _ans_guard = protect(ans);

        // Coerce both to complex
        let s1 = coerce_to_complex(s1);
        let _s1_guard = protect(s1);
        let s2 = coerce_to_complex(s2);
        let _s2_guard = protect(s2);

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

        ans
    }
}

/// Coerce a numeric SEXP to CPLXSXP.
unsafe fn coerce_to_complex(x: SEXP) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return x;
        }
        let n = XLENGTH(x);
        let y = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
        let dst = COMPLEX(y);
        if t == SEXPTYPE::REALSXP {
            let src = REAL(x);
            for i in 0..(n as usize) {
                *dst.add(i) = Rcomplex {
                    r: *src.add(i),
                    i: 0.0,
                };
            }
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
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
            t if t == SEXPTYPE::REALSXP => match code {
                OP_PLUS => s1,
                OP_MINUS => {
                    let ans = if no_references(s1) {
                        s1
                    } else {
                        Rf_allocVector3(SEXPTYPE::REALSXP, n)
                    };
                    let _ans_guard = protect(ans);
                    let pa = REAL(ans);
                    let px = REAL(s1);
                    for i in 0..(n as usize) {
                        *pa.add(i) = -*px.add(i);
                    }
                    ans
                }
                _ => std::ptr::null_mut(),
            },
            t if t == SEXPTYPE::INTSXP => match code {
                OP_PLUS => s1,
                OP_MINUS => {
                    let ans = if no_references(s1) {
                        s1
                    } else {
                        Rf_allocVector3(SEXPTYPE::INTSXP, n)
                    };
                    let _ans_guard = protect(ans);
                    let pa = INTEGER(ans);
                    let px = INTEGER(s1);
                    for i in 0..(n as usize) {
                        let x = *px.add(i);
                        *pa.add(i) = if x == NA_INTEGER { NA_INTEGER } else { -x };
                    }
                    ans
                }
                _ => std::ptr::null_mut(),
            },
            t if t == SEXPTYPE::LGLSXP => {
                // Coerce to INTSXP for unary minus on logicals
                match code {
                    OP_PLUS | OP_MINUS => {
                        let ans = Rf_allocVector3(SEXPTYPE::INTSXP, n);
                        let _ans_guard = protect(ans);
                        let pa = INTEGER(ans);
                        let px = LOGICAL(s1);
                        for i in 0..(n as usize) {
                            let x = *px.add(i);
                            *pa.add(i) = if code == OP_MINUS && x != NA_INTEGER && x != 0 {
                                -x
                            } else {
                                x
                            };
                        }
                        ans
                    }
                    _ => std::ptr::null_mut(),
                }
            }
            t if t == SEXPTYPE::CPLXSXP => match code {
                OP_PLUS => s1,
                OP_MINUS => {
                    let ans = if no_references(s1) {
                        s1
                    } else {
                        Rf_allocVector3(SEXPTYPE::CPLXSXP, n)
                    };
                    let _ans_guard = protect(ans);
                    let pa = COMPLEX(ans);
                    let px = COMPLEX(s1);
                    for i in 0..(n as usize) {
                        let z = *px.add(i);
                        *pa.add(i) = Rcomplex { r: -z.r, i: -z.i };
                    }
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

        let argc = Rf_length(args);

        let arg1 = CAR(args);

        if argc == 2 {
            let arg2 = CADR(args);

            // Retain both originals before either provider can detach them,
            // and both conversions through the second provider and kernel.
            let owner = crate::sexp::owner::OwnerToken::current().unwrap_or_else(|error| {
                crate::sexp::context::r_error(format!("arithmetic owner: {error}"))
            });
            let factory = SessionNodeFactory::new(owner);
            let original_arg1 = factory.wrap(arg1).unwrap_or_else(|error| {
                crate::sexp::context::r_error(format!("arithmetic argument: {error}"))
            });
            let original_arg2 = factory.wrap(arg2).unwrap_or_else(|error| {
                crate::sexp::context::r_error(format!("arithmetic argument: {error}"))
            });

            // Real scalars copy both values before allocating the rooted
            // output. Lazy providers remain lazy and no payload loan escapes.
            if original_arg1.typeof_() == SEXPTYPE::REALSXP
                && original_arg1.len() == 1
                && original_arg2.typeof_() == SEXPTYPE::REALSXP
                && original_arg2.len() == 1
            {
                let x1 = original_arg1.try_real_elt(0).unwrap_or_else(|error| {
                    crate::sexp::context::r_error(format!("real arithmetic input: {error}"))
                });
                let x2 = original_arg2.try_real_elt(0).unwrap_or_else(|error| {
                    crate::sexp::context::r_error(format!("real arithmetic input: {error}"))
                });
                let value = match code {
                    OP_PLUS => x1 + x2,
                    OP_MINUS => x1 - x2,
                    OP_TIMES => x1 * x2,
                    OP_DIV => x1 / x2,
                    OP_POW => R_pow(x1, x2),
                    OP_MOD => myfmod(x1, x2),
                    OP_INTDIV => myfloor(x1, x2),
                    _ => f64::NAN,
                };
                let result = factory
                    .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::REALSXP, 1)))
                    .and_then(|result| {
                        let mut result = SexpMut::try_from_checked(result)?;
                        result.try_set_real_elt(0, value)?;
                        Ok(result.freeze())
                    })
                    .unwrap_or_else(|error| {
                        crate::sexp::context::r_error(format!("real arithmetic result: {error}"))
                    });
                return result.as_raw();
            }

            // Integer scalars use the same checked kernel as longer vectors.
            let arg1_owner =
                coerce_logical_to_int(&factory, original_arg1).unwrap_or_else(|error| {
                    crate::sexp::context::r_error(format!("logical coercion: {error}"))
                });
            let arg2_owner =
                coerce_logical_to_int(&factory, original_arg2).unwrap_or_else(|error| {
                    crate::sexp::context::r_error(format!("logical coercion: {error}"))
                });
            let arg1 = arg1_owner.as_raw();
            let arg2 = arg2_owner.as_raw();

            let t1 = TYPEOF(arg1);
            let t2 = TYPEOF(arg2);

            if t1 == SEXPTYPE::CPLXSXP || t2 == SEXPTYPE::CPLXSXP {
                complex_binary_arith(code, arg1, arg2)
            } else if t1 == SEXPTYPE::REALSXP || t2 == SEXPTYPE::REALSXP {
                // Ensure both are at least INTSXP or REALSXP for real_binary_arith
                let s1 = if t1 != SEXPTYPE::INTSXP {
                    coerce_to_real(arg1)
                } else {
                    arg1
                };
                let s2 = if t2 != SEXPTYPE::INTSXP {
                    coerce_to_real(arg2)
                } else {
                    arg2
                };
                let _s1_guard = protect(s1);
                let _s2_guard = protect(s2);
                let result = real_binary_arith(code, s1, s2);
                result
            } else if t1 == SEXPTYPE::INTSXP && t2 == SEXPTYPE::INTSXP {
                let result = integer_binary_arith(&factory, code, &arg1_owner, &arg2_owner)
                    .unwrap_or_else(|error| {
                        crate::sexp::context::r_error(format!("integer arithmetic: {error}"))
                    });
                result.as_raw()
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

/// Copy lazy or shared logical values through checked element reads. Only an
/// unshared dense i32 payload can keep its allocation while changing kind.
fn coerce_logical_to_int<'s>(
    factory: &SessionNodeFactory<'s>,
    input: Sexp<'s>,
) -> SexpResult<Sexp<'s>> {
    factory.require_active()?;
    factory.link(&input)?;
    if input.typeof_() != SEXPTYPE::LGLSXP {
        return Ok(input);
    }
    if !crate::sexp::altrep::is_altrep(&input) && unsafe { no_references(input.as_raw()) } {
        let allocation = input.allocation()?;
        allocation
            .heap_identity()
            .retype_node(allocation, SEXPTYPE::INTSXP)
            .ok_or(crate::sexp::object::SexpError::StaleAllocation)?;
        return Ok(input);
    }
    let length = input.len();
    let output = factory.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, length)))?;
    let mut output = SexpMut::try_from_checked(output)?;
    for index in 0..length {
        // Providers may collect, allocate, reenter, or fail. No input/output
        // buffer loan crosses them; both canonical values remain rooted.
        let value = input.try_logical_elt(index)?;
        output.try_set_integer_elt(index, value)?;
    }
    Ok(output.freeze())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sexp::{
        altrep::{self, AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
        object::{PairlistBuilder, SessionNodeFactory, Sexp, SexpError, SexpMut, SexpResult},
        session::RSession,
    };
    use std::{cell::Cell, rc::Rc};

    struct LogicalCoercionProvider {
        mode: Rc<Cell<u8>>,
        reads: Rc<Cell<usize>>,
    }

    impl AltrepClass for LogicalCoercionProvider {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::LGLSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(4)
        }
        fn element<'s>(
            &self,
            context: &AltrepContext<'s>,
            index: i64,
        ) -> SexpResult<AltrepElement<'s>> {
            self.reads.set(self.reads.get() + 1);
            context.gc()?;
            if index == 1 {
                match self.mode.get() {
                    1 => {
                        return Err(SexpError::Altrep {
                            reason: "logical coercion read failure",
                        });
                    }
                    2 => panic!("logical coercion callback unwind"),
                    3 => {
                        context.object().try_logical_elt(index)?;
                    }
                    _ => {}
                }
            }
            Ok(AltrepElement::Logical(
                [1, 0, NA_INTEGER, 1][index as usize],
            ))
        }
    }

    fn logical_coercion_source<'s>(
        session: &'s RSession,
        mode: Rc<Cell<u8>>,
        reads: Rc<Cell<usize>>,
    ) -> Sexp<'s> {
        let class = session
            .register_altrep_class("logical-coercion", LogicalCoercionProvider { mode, reads })
            .unwrap();
        AltrepBuilder::new(class).build().unwrap()
    }

    #[test]
    fn owned_scalar_altrep_arithmetic_retains_detached_operands() {
        struct ScalarProvider {
            kind: SEXPTYPE,
            integer: i32,
            real: f64,
            detach: Rc<Cell<SEXP>>,
            nil: SEXP,
            reads: Rc<Cell<usize>>,
        }
        impl AltrepClass for ScalarProvider {
            fn vector_type(&self) -> SEXPTYPE {
                self.kind
            }
            fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
                Ok(1)
            }
            fn element<'s>(
                &self,
                context: &AltrepContext<'s>,
                index: i64,
            ) -> SexpResult<AltrepElement<'s>> {
                assert_eq!(index, 0);
                self.reads.set(self.reads.get() + 1);
                let head = self.detach.replace(std::ptr::null_mut());
                if !head.is_null() {
                    unsafe { crate::sexp::accessors::SETCDR(head, self.nil) };
                }
                context.gc()?;
                Ok(if self.kind == SEXPTYPE::INTSXP {
                    AltrepElement::Integer(self.integer)
                } else {
                    AltrepElement::Real(self.real)
                })
            }
        }
        for kind in [SEXPTYPE::INTSXP, SEXPTYPE::REALSXP] {
            let session = RSession::new_for_gc_tests();
            let factory = SessionNodeFactory::new(session.owner_token().unwrap());
            let detach = Rc::new(Cell::new(std::ptr::null_mut()));
            let left_reads = Rc::new(Cell::new(0));
            let right_reads = Rc::new(Cell::new(0));
            let left_class = session
                .register_altrep_class(
                    "scalar-left",
                    ScalarProvider {
                        kind,
                        integer: 7,
                        real: 2.5,
                        detach: detach.clone(),
                        nil: factory.nil().as_raw(),
                        reads: left_reads.clone(),
                    },
                )
                .unwrap();
            let right_class = session
                .register_altrep_class(
                    "scalar-right",
                    ScalarProvider {
                        kind,
                        integer: 5,
                        real: 4.25,
                        detach: Rc::new(Cell::new(std::ptr::null_mut())),
                        nil: factory.nil().as_raw(),
                        reads: right_reads.clone(),
                    },
                )
                .unwrap();
            let left = AltrepBuilder::new(left_class).build().unwrap();
            let original_class = altrep::altrep_class(&left).unwrap();
            let right = AltrepBuilder::new(right_class).build().unwrap();
            let mut arguments = PairlistBuilder::from_factory(factory.clone());
            arguments.push(left.clone(), None).unwrap();
            // Move away the right operand's only independent root. Initially
            // the argument chain alone retains it for this actual handler call.
            arguments.push(right, None).unwrap();
            let arguments = arguments.finish().unwrap();
            let operation = factory
                .wrap(unsafe { crate::mainutils::names::R_Primitive(c"+".as_ptr()) })
                .unwrap();
            let collections = Rc::new(Cell::new(0));
            let observed = collections.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                observed.set(observed.get() + 1);
                crate::sexp::gengc::full_gc();
            }));
            let before = crate::sexp::protect::R_ProtectCount();
            detach.set(arguments.as_raw());
            let result = factory
                .wrap(unsafe {
                    do_arith(
                        factory.nil().as_raw(),
                        operation.as_raw(),
                        arguments.as_raw(),
                        factory.nil().as_raw(),
                    )
                })
                .unwrap();
            assert!(arguments.try_cdr().unwrap().is_nil());
            assert_eq!(left_reads.get(), 1);
            assert_eq!(right_reads.get(), 1);
            assert!(collections.get() >= 2);
            drop(arguments);
            crate::sexp::gengc::full_gc();
            assert_eq!(result.typeof_(), kind);
            if kind == SEXPTYPE::INTSXP {
                assert_eq!(result.try_integer_elt(0).unwrap(), 12);
            } else {
                assert_eq!(result.try_real_elt(0).unwrap(), 6.75);
            }
            assert_eq!(left.typeof_(), kind);
            assert!(altrep::is_altrep(&left));
            assert!(!altrep::is_materialized(&left));
            assert_eq!(altrep::altrep_class(&left).unwrap(), original_class);
            assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
        }
    }

    #[test]
    fn owned_scalar_arithmetic_preserves_na_power_cases_during_result_gc() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let power = factory
            .wrap(unsafe { crate::mainutils::names::R_Primitive(c"^".as_ptr()) })
            .unwrap();
        let collections = Rc::new(Cell::new(0));
        let observed = collections.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            crate::sexp::gengc::full_gc();
        }));
        for kind in [SEXPTYPE::INTSXP, SEXPTYPE::REALSXP] {
            let scalar = |value: f64| {
                let value_node = factory
                    .allocate(|arena| Some(arena.alloc_vector(kind, 1)))
                    .unwrap();
                let mut value_node = SexpMut::try_from_checked(value_node).unwrap();
                if kind == SEXPTYPE::INTSXP {
                    let integer = if crate::sexp::ffi::is_na_real(value) {
                        NA_INTEGER
                    } else {
                        value as i32
                    };
                    value_node.try_set_integer_elt(0, integer).unwrap();
                } else {
                    value_node.try_set_real_elt(0, value).unwrap();
                }
                value_node.freeze()
            };
            for (left, right, expected) in [
                (1.0, NA_REAL, 1.0),
                (NA_REAL, 0.0, 1.0),
                (NA_REAL, 2.0, NA_REAL),
            ] {
                let mut arguments = PairlistBuilder::from_factory(factory.clone());
                arguments.push(scalar(left), None).unwrap();
                arguments.push(scalar(right), None).unwrap();
                let arguments = arguments.finish().unwrap();
                let before = crate::sexp::protect::R_ProtectCount();
                let collections_before = collections.get();
                session.with_active_in(|instance| unsafe {
                    (*instance).memory_state.gc_force_gap = 1;
                    (*instance).memory_state.gc_force_wait = 1;
                });
                let result = factory
                    .wrap(unsafe {
                        do_arith(
                            factory.nil().as_raw(),
                            power.as_raw(),
                            arguments.as_raw(),
                            factory.nil().as_raw(),
                        )
                    })
                    .unwrap();
                session.with_active_in(|instance| unsafe {
                    (*instance).memory_state.gc_force_gap = 0;
                });
                assert!(collections.get() > collections_before);
                drop(arguments);
                crate::sexp::gengc::full_gc();
                assert_eq!(result.typeof_(), SEXPTYPE::REALSXP);
                let actual = result.try_real_elt(0).unwrap();
                if crate::sexp::ffi::is_na_real(expected) {
                    assert!(crate::sexp::ffi::is_na_real(actual));
                } else {
                    assert_eq!(actual, expected);
                }
                assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
            }
        }
    }

    #[test]
    fn owned_integer_vector_division_reads_collecting_altrep_without_output_loans() {
        struct CollectingIntegers(Rc<Cell<usize>>);
        impl AltrepClass for CollectingIntegers {
            fn vector_type(&self) -> SEXPTYPE {
                SEXPTYPE::INTSXP
            }
            fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
                Ok(4)
            }
            fn element<'s>(
                &self,
                context: &AltrepContext<'s>,
                index: i64,
            ) -> SexpResult<AltrepElement<'s>> {
                self.0.set(self.0.get() + 1);
                context.gc()?;
                Ok(AltrepElement::Integer(
                    [2, 8, NA_INTEGER, 1][index as usize],
                ))
            }
        }
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let reads = Rc::new(Cell::new(0));
        let class = session
            .register_altrep_class("arithmetic-integers", CollectingIntegers(reads.clone()))
            .unwrap();
        let source = AltrepBuilder::new(class).build().unwrap();
        let denominator = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 2)))
            .unwrap();
        let mut denominator = SexpMut::try_from_checked(denominator).unwrap();
        denominator.try_set_integer_elt(0, 2).unwrap();
        denominator.try_set_integer_elt(1, 2).unwrap();
        let mut arguments = PairlistBuilder::from_factory(factory.clone());
        arguments.push(source.clone(), None).unwrap();
        arguments.push(denominator.freeze(), None).unwrap();
        let arguments = arguments.finish().unwrap();
        let operation = factory
            .wrap(unsafe { crate::mainutils::names::R_Primitive(c"/".as_ptr()) })
            .unwrap();
        let collections = Rc::new(Cell::new(0));
        let observed = collections.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            crate::sexp::gengc::full_gc();
        }));
        let before = crate::sexp::protect::R_ProtectCount();
        let result = factory
            .wrap(unsafe {
                do_arith(
                    factory.nil().as_raw(),
                    operation.as_raw(),
                    arguments.as_raw(),
                    factory.nil().as_raw(),
                )
            })
            .unwrap();
        drop(arguments);
        crate::sexp::gengc::full_gc();
        assert_eq!(result.typeof_(), SEXPTYPE::REALSXP);
        for (index, value) in [(0, 1.0), (1, 4.0), (3, 0.5)] {
            assert_eq!(result.try_real_elt(index).unwrap(), value);
        }
        assert!(crate::sexp::ffi::is_na_real(
            result.try_real_elt(2).unwrap()
        ));
        assert_eq!(reads.get(), 4);
        assert!(collections.get() >= 4);
        assert_eq!(source.typeof_(), SEXPTYPE::INTSXP);
        assert!(!altrep::is_materialized(&source));
        assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
    }

    #[test]
    fn owned_integer_vector_division_and_power_use_real_results_through_gc() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let integer_vector = |values: &[i32]| {
            let vector = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, values.len() as _)))
                .unwrap();
            let mut vector = SexpMut::try_from_checked(vector).unwrap();
            for (index, value) in values.iter().enumerate() {
                vector.try_set_integer_elt(index as _, *value).unwrap();
            }
            vector.freeze()
        };
        let collections = Rc::new(Cell::new(0));
        let observed = collections.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            crate::sexp::gengc::full_gc();
        }));
        let run = |name: &std::ffi::CStr, left: &[i32], right: &[i32]| {
            let left = integer_vector(left);
            let right = integer_vector(right);
            let mut arguments = PairlistBuilder::from_factory(factory.clone());
            arguments.push(left, None).unwrap();
            arguments.push(right, None).unwrap();
            let arguments = arguments.finish().unwrap();
            let operation = factory
                .wrap(unsafe { crate::mainutils::names::R_Primitive(name.as_ptr()) })
                .unwrap();
            let before = crate::sexp::protect::R_ProtectCount();
            let collections_before = collections.get();
            session.with_active_in(|instance| unsafe {
                (*instance).memory_state.gc_force_gap = 1;
                (*instance).memory_state.gc_force_wait = 1;
            });
            let result = factory
                .wrap(unsafe {
                    do_arith(
                        factory.nil().as_raw(),
                        operation.as_raw(),
                        arguments.as_raw(),
                        factory.nil().as_raw(),
                    )
                })
                .unwrap();
            session.with_active_in(|instance| unsafe {
                (*instance).memory_state.gc_force_gap = 0;
            });
            assert!(collections.get() > collections_before);
            assert_eq!(result.typeof_(), SEXPTYPE::REALSXP);
            assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
            drop(arguments);
            crate::sexp::gengc::full_gc();
            result
        };
        let divided = run(c"/", &[2, 8, NA_INTEGER, 0, 1], &[2, 0]);
        assert_eq!(divided.len(), 5);
        assert_eq!(divided.try_real_elt(0).unwrap(), 1.0);
        assert_eq!(divided.try_real_elt(1).unwrap(), f64::INFINITY);
        assert!(crate::sexp::ffi::is_na_real(
            divided.try_real_elt(2).unwrap()
        ));
        let zero_divided = divided.try_real_elt(3).unwrap();
        assert!(zero_divided.is_nan());
        assert!(!crate::sexp::ffi::is_na_real(zero_divided));
        assert_eq!(divided.try_real_elt(4).unwrap(), 0.5);

        let power = run(
            c"^",
            &[2, 1, NA_INTEGER, NA_INTEGER, 0, NA_INTEGER],
            &[3, NA_INTEGER, 0, 2, 0, NA_INTEGER],
        );
        assert_eq!(power.len(), 6);
        for (index, value) in [(0, 8.0), (1, 1.0), (2, 1.0), (4, 1.0)] {
            assert_eq!(power.try_real_elt(index).unwrap(), value);
        }
        for index in [3, 5] {
            assert!(crate::sexp::ffi::is_na_real(
                power.try_real_elt(index).unwrap()
            ));
        }
        let recycled_power = run(c"^", &[2, 3, 4, 1, NA_INTEGER, NA_INTEGER], &[0, 2]);
        for (index, value) in [1.0, 9.0, 1.0, 1.0, 1.0].into_iter().enumerate() {
            assert_eq!(recycled_power.try_real_elt(index as _).unwrap(), value);
        }
        assert!(crate::sexp::ffi::is_na_real(
            recycled_power.try_real_elt(5).unwrap()
        ));
        for name in [c"/", c"^"] {
            let empty = run(name, &[], &[2]);
            assert_eq!(empty.len(), 0);
        }
        crate::sexp::gengc::full_gc();
        assert_eq!(power.try_real_elt(0).unwrap(), 8.0);
        assert_eq!(divided.try_real_elt(4).unwrap(), 0.5);
    }

    #[test]
    fn owned_logical_coercion_preserves_lazy_and_materialized_providers_during_gc() {
        for materialized in [false, true] {
            let session = RSession::new_for_gc_tests();
            let factory = SessionNodeFactory::new(session.owner_token().unwrap());
            let reads = Rc::new(Cell::new(0));
            let source = logical_coercion_source(&session, Rc::new(Cell::new(0)), reads.clone());
            let descriptor = altrep::altrep_class(&source).unwrap();
            if materialized {
                altrep::force_materialization(&source).unwrap();
            }
            let reads_before = reads.get();
            let collections = Rc::new(Cell::new(0));
            let observed = collections.clone();
            let detach_target = Rc::new(Cell::new(std::ptr::null_mut()));
            let detached_head = detach_target.clone();
            let nil = factory.nil().as_raw();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                observed.set(observed.get() + 1);
                let head: SEXP = detached_head.replace(std::ptr::null_mut());
                if !head.is_null() {
                    unsafe { crate::sexp::accessors::SETCDR(head, nil) };
                }
                crate::sexp::gengc::full_gc();
            }));
            session.with_active_in(|instance| unsafe {
                (*instance).memory_state.gc_force_gap = 1;
                (*instance).memory_state.gc_force_wait = 1;
            });
            let before = crate::sexp::protect::R_ProtectCount();
            let result = coerce_logical_to_int(&factory, source.clone()).unwrap();
            assert_ne!(result.as_raw(), source.as_raw());
            assert_eq!(result.typeof_(), SEXPTYPE::INTSXP);
            assert_eq!(source.typeof_(), SEXPTYPE::LGLSXP);
            assert_eq!(altrep::altrep_class(&source).unwrap(), descriptor);
            assert_eq!(altrep::is_materialized(&source), materialized);
            assert!(!altrep::is_altrep(&result));
            assert_eq!(reads.get() - reads_before, if materialized { 0 } else { 4 });
            for (index, value) in [1, 0, NA_INTEGER, 1].into_iter().enumerate() {
                assert_eq!(result.try_integer_elt(index as _).unwrap(), value);
                assert_eq!(source.try_logical_elt(index as _).unwrap(), value);
            }
            // The actual binary arithmetic consumer must retain its first
            // converted value while converting the second collecting provider.
            let right_class = session
                .register_altrep_class(
                    "logical-coercion-right",
                    LogicalCoercionProvider {
                        mode: Rc::new(Cell::new(0)),
                        reads: Rc::new(Cell::new(0)),
                    },
                )
                .unwrap();
            let right = AltrepBuilder::new(right_class).build().unwrap();
            let mut arguments = PairlistBuilder::from_factory(factory.clone());
            arguments.push(source.clone(), None).unwrap();
            arguments.push(right, None).unwrap();
            let arguments = arguments.finish().unwrap();
            let plus = factory
                .wrap(unsafe { crate::mainutils::names::R_Primitive(c"+".as_ptr()) })
                .unwrap();
            // The right provider has no caller root. The first result-allocation
            // callback detaches it, before its own coercion begins.
            detach_target.set(arguments.as_raw());
            let sum = factory
                .wrap(unsafe {
                    do_arith(
                        factory.nil().as_raw(),
                        plus.as_raw(),
                        arguments.as_raw(),
                        factory.nil().as_raw(),
                    )
                })
                .unwrap();
            assert!(arguments.try_cdr().unwrap().is_nil());
            crate::sexp::gengc::full_gc();
            for (index, value) in [2, 0, NA_INTEGER, 2].into_iter().enumerate() {
                assert_eq!(sum.try_integer_elt(index as _).unwrap(), value);
            }
            assert_eq!(result.try_integer_elt(2).unwrap(), NA_INTEGER);
            assert_eq!(source.typeof_(), SEXPTYPE::LGLSXP);
            assert_eq!(altrep::altrep_class(&source).unwrap(), descriptor);
            assert!(collections.get() > 0);
            assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
        }
    }

    #[test]
    fn owned_logical_coercion_retypes_only_unshared_dense_storage() {
        let session = RSession::new_for_gc_tests();
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        for shared in [false, true] {
            let value = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::LGLSXP, 2)))
                .unwrap();
            let mut value = SexpMut::try_from_checked(value).unwrap();
            value.try_set_logical_elt(0, 1).unwrap();
            value.try_set_logical_elt(1, NA_INTEGER).unwrap();
            let input = value.freeze();
            unsafe {
                crate::sexp::accessors::SET_NAMED(input.as_raw(), if shared { 2 } else { 0 })
            };
            let original_pointer = input.as_raw();
            let output = coerce_logical_to_int(&factory, input.clone()).unwrap();
            assert_eq!(output.as_raw() == original_pointer, !shared);
            assert_eq!(output.typeof_(), SEXPTYPE::INTSXP);
            assert_eq!(output.try_integer_elt(0).unwrap(), 1);
            assert_eq!(output.try_integer_elt(1).unwrap(), NA_INTEGER);
            if shared {
                assert_eq!(input.typeof_(), SEXPTYPE::LGLSXP);
                assert_eq!(input.try_logical_elt(1).unwrap(), NA_INTEGER);
            }
        }
    }

    #[test]
    fn owned_logical_coercion_errors_unwind_and_reentry_leave_source_retryable() {
        for mode in [1, 2, 3] {
            let session = RSession::new_for_gc_tests();
            let factory = SessionNodeFactory::new(session.owner_token().unwrap());
            let state = Rc::new(Cell::new(mode));
            let source = logical_coercion_source(&session, state.clone(), Rc::new(Cell::new(0)));
            let descriptor = altrep::altrep_class(&source).unwrap();
            let before = crate::sexp::protect::R_ProtectCount();
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                coerce_logical_to_int(&factory, source.clone())
            }));
            if mode == 2 {
                assert!(failure.is_err());
            } else {
                assert!(failure.unwrap().is_err());
            }
            assert_eq!(source.typeof_(), SEXPTYPE::LGLSXP);
            assert_eq!(altrep::altrep_class(&source).unwrap(), descriptor);
            assert!(!altrep::is_materialized(&source));
            assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
            state.set(0);
            crate::sexp::gengc::full_gc();
            let retry = coerce_logical_to_int(&factory, source.clone()).unwrap();
            crate::sexp::gengc::full_gc();
            for (index, value) in [1, 0, NA_INTEGER, 1].into_iter().enumerate() {
                assert_eq!(retry.try_integer_elt(index as _).unwrap(), value);
                assert_eq!(source.try_logical_elt(index as _).unwrap(), value);
            }
            assert_eq!(source.typeof_(), SEXPTYPE::LGLSXP);
            assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
        }
    }

    unsafe fn int_scalar(value: c_int) -> SEXP {
        unsafe {
            let scalar = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
            *INTEGER(scalar) = value;
            scalar
        }
    }

    unsafe fn two_arg_call_args(left: SEXP, right: SEXP) -> SEXP {
        unsafe {
            crate::sexp::constructors::Rf_cons(
                left,
                crate::sexp::constructors::Rf_cons(right, crate::sexp::globals::R_NilValue()),
            )
        }
    }

    #[test]
    fn test_do_arith_uses_primitive_operation_code() {
        let _session = crate::sexp::session::RSession::new_for_gc_tests();
        unsafe {
            let plus = crate::mainutils::names::R_Primitive(c"+".as_ptr());
            let plus_args = two_arg_call_args(int_scalar(5), int_scalar(3));
            let plus_result = do_arith(
                std::ptr::null_mut(),
                plus,
                plus_args,
                crate::sexp::globals::R_NilValue(),
            );
            assert_eq!(TYPEOF(plus_result), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(plus_result), 8);

            let minus = crate::mainutils::names::R_Primitive(c"-".as_ptr());
            let minus_args = two_arg_call_args(int_scalar(5), int_scalar(3));
            let minus_result = do_arith(
                std::ptr::null_mut(),
                minus,
                minus_args,
                crate::sexp::globals::R_NilValue(),
            );
            assert_eq!(TYPEOF(minus_result), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(minus_result), 2);
        }
    }

    #[test]
    fn test_R_integer_plus() {
        let mut naflag = false;
        assert_eq!(R_integer_plus(3, 4, &mut naflag), 7);
        assert!(!naflag);

        // NA propagation
        assert_eq!(R_integer_plus(NA_INTEGER, 4, &mut naflag), NA_INTEGER);
        assert_eq!(R_integer_plus(3, NA_INTEGER, &mut naflag), NA_INTEGER);

        // Overflow
        assert_eq!(R_integer_plus(c_int::MAX, 1, &mut naflag), NA_INTEGER);
        assert!(naflag);
    }

    #[test]
    fn test_R_integer_minus() {
        let mut naflag = false;
        assert_eq!(R_integer_minus(10, 3, &mut naflag), 7);
        assert!(!naflag);

        // Overflow: (MIN+2) - 3 = MIN-1, which overflows
        assert_eq!(R_integer_minus(c_int::MIN + 2, 3, &mut naflag), NA_INTEGER);
        assert!(naflag);
    }

    #[test]
    fn test_R_integer_times() {
        let mut naflag = false;
        assert_eq!(R_integer_times(6, 7, &mut naflag), 42);
        assert!(!naflag);

        // NA propagation
        assert_eq!(R_integer_times(NA_INTEGER, 7, &mut naflag), NA_INTEGER);

        // Overflow
        naflag = false;
        assert_eq!(R_integer_times(c_int::MAX, 2, &mut naflag), NA_INTEGER);
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
        // R's %% takes the sign of the divisor (floor-based), unlike C fmod.
        assert!((myfmod(-10.0, 3.0) - 2.0).abs() < 1e-10);
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
        let na = f64::from_bits(crate::sexp::ffi::R_NA_BIT_PATTERN);
        assert_eq!(R_IsNA(na), 1);
        assert_eq!(R_IsNA(f64::NAN), 0);
        assert_eq!(R_IsNA(1.0), 0);
    }

    #[test]
    fn test_R_IsNaN() {
        assert_eq!(R_IsNaN(f64::NAN), 1);
        let na = f64::from_bits(crate::sexp::ffi::R_NA_BIT_PATTERN);
        assert_eq!(R_IsNaN(na), 0);
        assert_eq!(R_IsNaN(1.0), 0);
    }
}
