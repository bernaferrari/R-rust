// Ported from R's nmath/fprec.c and nmath/fround.c
//
// fprec: Returns the value of x rounded to "digits" significant decimal digits.
// fround: Rounds "x" to "digits" decimal digits.
//
// Original fprec by W. Fullerton of Los Alamos Scientific Laboratory.
// Improvements by Martin Maechler, May 1997; further ones, Feb.2000.

use crate::constants::*;
use crate::special::mlutils::R_pow_di;
use libm::{ceil, fabs, floor, fmod, log10, round};

const MAX_DIGITS_FP: i32 = 22;
// DBL_MAX_10_EXP is 308 for IEEE 754
const MAX10E: i32 = 308;

/// Returns the value of x rounded to "digits" significant decimal digits.
///
/// This is R's signif(x, digits) via Math2(args, fprec).
pub fn fprec(x: f64, digits: f64) -> f64 {
    fprec_inner(x, digits)
}

/// Approximate logb(x) = floor(log2(|x|)) for f64.
/// libm 0.1 doesn't have logb, so we implement it.
#[inline]
fn ilogb_approx(x: f64) -> f64 {
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if !r_finite(x) {
        return x;
    }
    let ax = fabs(x);
    let l2 = libm::log2(ax);
    floor(l2)
}

#[inline]
fn fprec_inner(x: f64, digits: f64) -> f64 {
    if isnan(x) || isnan(digits) {
        return x + digits;
    }
    if !r_finite(x) {
        return x;
    }
    if !r_finite(digits) {
        return if digits > 0.0 { x } else { digits };
    }
    if x == 0.0 {
        return x;
    }

    let mut dig = round(digits) as i32;
    if dig > MAX_DIGITS_FP {
        return x;
    } else if dig < 1 {
        dig = 1;
    }

    let mut sgn = 1.0_f64;
    let mut x = x;
    if x < 0.0 {
        sgn = -sgn;
        x = -x;
    }
    let l10 = log10(x);
    let mut e10 = dig - 1 - floor(l10) as i32;

    if fabs(l10) < (MAX10E - 2) as f64 {
        let mut p10 = 1.0_f64;
        if e10 > MAX10E {
            // numbers less than 10^(dig-1) * 1e-308
            p10 = R_pow_di(10.0, e10 - MAX10E);
            e10 = MAX10E;
        }
        if e10 > 0 {
            // Try always to have pow >= 1 and so exactly representable
            let pow10 = R_pow_di(10.0, e10);
            sgn * libm::round((x * pow10) * p10) / pow10 / p10
        } else {
            let pow10 = R_pow_di(10.0, -e10);
            sgn * libm::round(x / pow10) * pow10
        }
    } else {
        // -- LARGE or small --
        let do_round = log10(f64::MAX) - l10 >= R_pow_di(10.0, -dig) as f64;
        // e.g. signif(1.09e308, 2)
        let e2 = dig + if e10 > 0 { 1 } else { -1 } * MAX_DIGITS_FP;
        let p10 = R_pow_di(10.0, e2);
        let p10_large = R_pow_di(10.0, e10 - e2);
        x *= p10;
        x *= p10_large;
        // p10 * P10 = 10 ^ e10
        if do_round {
            x += 0.5;
        }
        x = floor(x) / p10;
        sgn * x / p10_large
    }
}

/// Rounds "x" to "digits" decimal digits.
pub fn fround(x: f64, digits: f64) -> f64 {
    fround_inner(x, digits)
}

#[inline]
fn fround_inner(x: f64, digits: f64) -> f64 {
    // MAX_DIGITS = DBL_MAX_10_EXP + DBL_DIG = 308 + 15 = 323
    const MAX_DIGITS: i32 = 323;

    // Note that large digits make sense for very small numbers
    if isnan(x) || isnan(digits) {
        return x + digits;
    }
    if !r_finite(x) {
        return x;
    }
    if digits > MAX_DIGITS as f64 || x == 0.0 {
        return x;
    } else if digits < -(MAX10E as f64) {
        // includes -Inf
        return 0.0;
    } else if digits == 0.0 {
        // common case
        return libm::round(x);
    }

    let dig = floor(digits + 0.5) as i32;
    let mut sgn = 1.0_f64;
    let mut x = x;
    if x < 0.0 {
        sgn = -1.0;
        x = -x;
    }
    // now x > 0
    let l10x = std::f64::consts::LOG10_2 * (0.5 + ilogb_approx(x)); // ~= log10(x), but cheaper

    if l10x + dig as f64 > 15.0 {
        // rounding to so many digits that no rounding is needed
        sgn * x
    } else if dig <= MAX10E {
        // both pow10 := 10^d and x10 := x * pow10 do *not* overflow
        let pow10 = R_pow_di(10.0, dig);
        let x10 = x * pow10;
        let i10 = floor(x10);
        let xd = i10 / pow10;
        let xu = ceil(x10) / pow10;

        let du = xu - x;
        let dd = x - xd;
        if du < dd || (fmod(i10, 2.0) == 1.0 && du == dd) {
            sgn * xu
        } else {
            sgn * xd
        }
    } else {
        // DBL_MAX_10_EXP =: max10e < dig <= DBL_DIG - l10x: case of |x| << 1; ~ 10^-305
        let e10 = dig - MAX10E; // > 0
        let p10 = R_pow_di(10.0, e10);
        let pow10 = R_pow_di(10.0, MAX10E);
        let x10 = (x * pow10) * p10;
        let i10 = floor(x10);
        let xd = i10 / pow10 / p10;
        let xu = ceil(x10) / pow10 / p10;

        let du = xu - x;
        let dd = x - xd;
        if du < dd || (fmod(i10, 2.0) == 1.0 && du == dd) {
            sgn * xu
        } else {
            sgn * xd
        }
    }
}

// =====================================================================
// C FFI shims
// =====================================================================

#[unsafe(no_mangle)]
pub extern "C" fn Rf_fprec(x: f64, digits: f64) -> f64 {
    fprec_inner(x, digits)
}

#[unsafe(no_mangle)]
pub extern "C" fn fprec_c(x: f64, digits: f64) -> f64 {
    fprec_inner(x, digits)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_fround(x: f64, digits: f64) -> f64 {
    fround_inner(x, digits)
}

#[unsafe(no_mangle)]
pub extern "C" fn fround_c(x: f64, digits: f64) -> f64 {
    fround_inner(x, digits)
}
