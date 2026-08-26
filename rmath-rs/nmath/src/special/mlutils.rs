// Ported from mlutils.c: R_pow, R_pow_di, R_finite, REprintf, NA_REAL, R_PosInf, R_NegInf

use crate::constants::*;
use libm::*;

/// isfinite matching C behavior
#[inline(always)]
fn isfinite(x: f64) -> bool {
    !isnan(x) && x != ML_POSINF && x != ML_NEGINF
}

/// Check if a double is finite (for standalone mode).
pub fn R_finite(x: f64) -> i32 {
    if isfinite(x) { 1 } else { 0 }
}

/// Check if a double is NaN (C++ compatibility function).
pub fn R_isnancpp(x: f64) -> i32 {
    if isnan(x) { 1 } else { 0 }
}

/// Internal: fmod-like function matching R's internal myfmod.
#[inline]
fn myfmod(x1: f64, x2: f64) -> f64 {
    let q = x1 / x2;
    x1 - floor(q) * x2
}

/// R_pow: compute x^y with full IEEE 754 handling.
pub fn R_pow(x: f64, y: f64) -> f64 {
    // Squaring is the most common of the specially handled cases so
    // check for it first (arithmetic.c).
    if y == 2.0 {
        return x * x;
    }
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
    if x >= -11.0 && x <= 11.0 {
        // Small-magnitude bases: multiply chains for y == 3/4 differ from
        // pow() by 1 ulp intentionally (stock arithmetic.c R_pow).
        if y == 4.0 {
            return x * x * x * x;
        }
        if y == 3.0 {
            return x * x * x;
        }
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
        if x >= 0.0 {
            if y > 0.0 {
                // y == +Inf
                return if x >= 1.0 { ML_POSINF } else { 0.0 };
            } else {
                // y == -Inf
                return if x < 1.0 { ML_POSINF } else { 0.0 };
            }
        }
    }
    ML_NAN
}

pub fn R_pow_di(x: f64, n: i32) -> f64 {
    let mut pow = 1.0;
    if isnan(x) {
        return x;
    }
    if n != 0 {
        if !isfinite(x) {
            return R_pow(x, n as f64);
        }
        // arithmetic.c R_pow_di: square-multiply on x, then invert once at
        // the end for negative n (avoids accumulating error on 1/x).
        let is_neg = n < 0;
        let mut n_abs = if is_neg { -n } else { n };
        let mut x_val = x;
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
        if is_neg {
            pow = 1.0 / pow;
        }
    }
    pow
}

/// R's NA_REAL constant (a specific NaN).
/// Immutable: this value is initialized once and never mutated.
pub static NA_REAL: f64 = ML_NAN;

/// R_PosInf constant.
pub static R_PosInf: f64 = ML_POSINF;

/// R_NegInf constant.
pub static R_NegInf: f64 = ML_NEGINF;

/// REprintf: print to stderr (C FFI compatibility shim).
pub fn REprintf(format: *const std::os::raw::c_char) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r_pow_fast_paths_match_stock_arithmetic_c() {
        // Stock R_pow (main/arithmetic.c) special-cases y == 2 before
        // anything else, and y == 4 / y == 3 for |x| <= 11 via multiply
        // chains, which differ from pow() by rounding on purpose.
        assert_eq!(R_pow(1.1, 3.0), 1.3310000000000004); // 1.1^3 in R
        assert_eq!(R_pow(1.1, 4.0), 1.4641000000000006);
        assert_eq!(R_pow(9.0, 2.0), 81.0);
        assert_eq!(R_pow(-2.5, 2.0), 6.25);
        assert_eq!(R_pow(10.5, 3.0), 10.5 * 10.5 * 10.5);
        // |x| > 11 keeps the pow() path for y == 3 / y == 4.
        assert_eq!(R_pow(12.0, 3.0), pow(12.0, 3.0));
        assert_eq!(R_pow(1.0, 7.0), 1.0);
        assert_eq!(R_pow(12.0, 4.0), pow(12.0, 4.0));
        assert_eq!(R_pow(0.0, 2.0), 0.0);
        assert_eq!(R_pow(0.0, -1.0), ML_POSINF);
    }

    #[test]
    fn r_pow_di_uses_repeated_squaring_like_stock() {
        // rbinom's np < 30 path computes qn = R_pow_di(q, n); the
        // square-and-multiply chain differs from pow() by ulps.
        assert_eq!(R_pow_di(0.7, 40), 6.36680576090901e-07);
        assert_ne!(R_pow_di(0.7, 40), pow(0.7, 40.0));
        assert_eq!(R_pow_di(2.0, -2), 0.25);
        assert_eq!(R_pow_di(-2.0, 3), -8.0);
        assert_eq!(R_pow_di(1.5, 0), 1.0);
        assert!(isnan(R_pow_di(ML_NAN, 5)));
        assert_eq!(R_pow_di(ML_POSINF, 3), ML_POSINF);
    }
}
