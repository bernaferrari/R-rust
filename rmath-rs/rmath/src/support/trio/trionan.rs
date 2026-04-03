/*!
 * Port of R's trionan.c - NaN and special floating-point quantity handling.
 *
 * Original copyright (C) 2001 Bjorn Reese <breese@users.sourceforge.net>
 * BSD-style license.
 *
 * Uses Rust's built-in f64 support for NaN/Inf detection which is
 * IEEE 754 compliant on all supported platforms.
 */

use std::os::raw::c_int;

/// Floating-point classification constants (matching C's trio enum).
pub const TRIO_FP_INFINITE: c_int = 0;
pub const TRIO_FP_NAN: c_int = 1;
pub const TRIO_FP_NORMAL: c_int = 2;
pub const TRIO_FP_SUBNORMAL: c_int = 3;
pub const TRIO_FP_ZERO: c_int = 4;

/// Classify a floating-point number and determine its sign.
///
/// Returns one of the TRIO_FP_* constants and sets `is_negative` to non-zero
/// if the number has its sign bit set.
pub fn trio_fpclassify_and_signbit(number: f64, is_negative: &mut c_int) -> c_int {
    if number.is_nan() {
        *is_negative = 0;
        return TRIO_FP_NAN;
    } else if number.is_infinite() {
        *is_negative = if number < 0.0 { 1 } else { 0 };
        return TRIO_FP_INFINITE;
    } else if number == 0.0 {
        // In IEEE 754 the sign of zero is ignored in comparisons,
        // so we handle this as a special case by examining the sign bit.
        *is_negative = if number.is_sign_negative() { 1 } else { 0 };
        return TRIO_FP_ZERO;
    } else if number.abs() < f64::MIN_POSITIVE {
        *is_negative = if number < 0.0 { 1 } else { 0 };
        return TRIO_FP_SUBNORMAL;
    } else {
        *is_negative = if number < 0.0 { 1 } else { 0 };
        return TRIO_FP_NORMAL;
    }
}

/// Check if a number is NaN.
///
/// Returns non-zero if `number` is NaN, zero otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trio_isnan(number: f64) -> c_int {
    let mut dummy: c_int = 0;
    if trio_fpclassify_and_signbit(number, &mut dummy) == TRIO_FP_NAN {
        1
    } else {
        0
    }
}

/// Check if a number is infinite.
///
/// Returns 1 if positive infinity, -1 if negative infinity, 0 otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trio_isinf(number: f64) -> c_int {
    let mut is_negative: c_int = 0;
    if trio_fpclassify_and_signbit(number, &mut is_negative) == TRIO_FP_INFINITE {
        if is_negative != 0 { -1 } else { 1 }
    } else {
        0
    }
}

/// Check if a number is finite.
///
/// Returns non-zero if finite, zero otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trio_isfinite(number: f64) -> c_int {
    let mut dummy: c_int = 0;
    match trio_fpclassify_and_signbit(number, &mut dummy) {
        TRIO_FP_INFINITE | TRIO_FP_NAN => 0,
        _ => 1,
    }
}

/// Examine the sign of a number.
///
/// Returns non-zero if the number has the sign bit set (i.e. is negative).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trio_signbit(number: f64) -> c_int {
    let mut is_negative: c_int = 0;
    let _ = trio_fpclassify_and_signbit(number, &mut is_negative);
    is_negative
}

/// Examine the class of a number.
///
/// Returns one of the TRIO_FP_* constants.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trio_fpclassify(number: f64) -> c_int {
    let mut dummy: c_int = 0;
    trio_fpclassify_and_signbit(number, &mut dummy)
}

/// Generate NaN (Not-a-Number).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trio_nan() -> f64 {
    f64::NAN
}

/// Generate positive infinity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trio_pinf() -> f64 {
    f64::INFINITY
}

/// Generate negative infinity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trio_ninf() -> f64 {
    f64::NEG_INFINITY
}

/// Generate negative zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trio_nzero() -> f64 {
    -0.0f64
}
