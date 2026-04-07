// Ported from mlutils.c: R_pow, R_pow_di, R_finite, REprintf, NA_REAL, R_PosInf, R_NegInf

use crate::constants::*;
use libm::*;

/// isfinite matching C behavior
#[inline(always)]
fn isfinite(x: f64) -> bool {
    !isnan(x) && x != ML_POSINF && x != ML_NEGINF
}

/// Check if a double is finite (for standalone mode).
#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn R_finite(x: f64) -> i32 {
    if isfinite(x) { 1 } else { 0 }
}

/// Check if a double is NaN (C++ compatibility function).
#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn R_isnancpp(x: f64) -> i32 {
    if isnan(x) { 1 } else { 0 }
}

/// Internal: fmod-like function matching R's internal myfmod.
#[inline]
fn myfmod(x1: f64, x2: f64) -> f64 {
    let q = x1 / x2;
    x1 - floor(q) * x2
}

/// R_pow: compute x^y with full IEEE 754 handling.
#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn R_pow(x: f64, y: f64) -> f64 {
    if x == 1.0 || y == 0.0 {
        return 1.0;
    }
    if x == 0.0 {
        if y > 0.0 {
            return 0.0;
        } else if y < 0.0 {
            return ML_POSINF;
        } else {
            return y;
        } // y is NA or NaN
    }
    if isfinite(x) && isfinite(y) {
        return pow(x, y);
    }
    if isnan(x) || isnan(y) {
        return x + y; // NaN propagation
    }
    if !isfinite(x) {
        if x > 0.0 {
            // Inf ^ y
            return if y < 0.0 { 0.0 } else { ML_POSINF };
        } else {
            // (-Inf) ^ y
            if isfinite(y) && y == floor(y) {
                // (-Inf) ^ n
                if y < 0.0 {
                    return 0.0;
                } else if myfmod(y, 2.0) != 0.0 {
                    return x;
                } else {
                    return -x;
                }
            } else {
                // fall through to return ML_NAN
            }
        }
    }
    if !isfinite(y) {
        if x >= 0.0 && y > 0.0 {
            // y == +Inf
            return if x >= 1.0 { ML_POSINF } else { 0.0 };
        } else if x >= 0.0 {
            // y == -Inf
            return if x < 1.0 { ML_POSINF } else { 0.0 };
        }
    }
    ML_NAN
}

/// R_pow_di: compute x^n for integer n (fast exponentiation by squaring).
#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn R_pow_di(x: f64, n: i32) -> f64 {
    let mut pow = 1.0;
    if isnan(x) {
        return x;
    }
    if n != 0 {
        if !isfinite(x) {
            return R_pow(x, n as f64);
        }
        let mut n_abs = n;
        let mut x_val = x;
        if n_abs < 0 {
            n_abs = -n_abs;
            x_val = 1.0 / x_val;
        }
        loop {
            if (n_abs & 1) != 0 {
                pow *= x_val;
            }
            n_abs >>= 1;
            if n_abs != 0 {
                x_val *= x_val;
            } else {
                break;
            }
        }
    }
    pow
}

/// R's NA_REAL constant (a specific NaN).
/// Immutable: this value is initialized once and never mutated.
#[unsafe(no_mangle)]
pub static NA_REAL: f64 = ML_NAN;

/// R_PosInf constant.
#[unsafe(no_mangle)]
pub static R_PosInf: f64 = ML_POSINF;

/// R_NegInf constant.
#[unsafe(no_mangle)]
pub static R_NegInf: f64 = ML_NEGINF;

/// REprintf: print to stderr (varargs-like, simplified for Rust).
/// In standalone mode, this just prints to stderr.
/// For the C FFI compatibility, we provide a simple version.
#[unsafe(no_mangle)]
pub extern "C" fn REprintf(format: *const i8) {
    unsafe {
        if format.is_null() {
            return;
        }
        let c_str = std::ffi::CStr::from_ptr(format);
        if let Ok(s) = c_str.to_str() {
            eprint!("{}", s);
        }
    }
}
