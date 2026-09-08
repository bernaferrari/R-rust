// Utility functions ported from fmax2.c, fmin2.c, imax2.c, imin2.c,
// sign.c, fsign.c, ftrunc.c

use crate::constants::*;
use libm::{fabs, trunc};

/// Maximum of two doubles, propagating NaN (like C's fmax2).
pub fn fmax2(x: f64, y: f64) -> f64 {
    if isnan(x) || isnan(y) {
        return x + y; // NaN propagation
    }
    if x < y { y } else { x }
}

/// Minimum of two doubles, propagating NaN (like C's fmin2).
pub fn fmin2(x: f64, y: f64) -> f64 {
    if isnan(x) || isnan(y) {
        return x + y; // NaN propagation
    }
    if x < y { x } else { y }
}

/// Maximum of two integers.
pub fn imax2(x: i32, y: i32) -> i32 {
    if x < y { y } else { x }
}

/// Minimum of two integers.
pub fn imin2(x: i32, y: i32) -> i32 {
    if x < y { x } else { y }
}

/// Sign function: returns 1 if x > 0, 0 if x == 0, -1 if x < 0.
/// Propagates NaN.
pub fn sign(x: f64) -> f64 {
    if isnan(x) {
        return x;
    }
    if x > 0.0 {
        1.0
    } else if x == 0.0 {
        0.0
    } else {
        -1.0
    }
}

/// Transfer of sign: |x| * signum(y). Propagates NaN.
pub fn fsign(x: f64, y: f64) -> f64 {
    if isnan(x) || isnan(y) {
        return x + y; // NaN propagation
    }
    if y >= 0.0 { fabs(x) } else { -fabs(x) }
}

/// Truncation toward zero.
pub fn ftrunc(x: f64) -> f64 {
    trunc(x)
}

/// R_forceint: force to integer (like C's nearbyint)
/// Uses round-half-to-even (banker's rounding) to match C's nearbyint.
#[inline(always)]
pub fn r_forceint(x: f64) -> f64 {
    nearbyint_impl(x)
}

/// Round-half-to-even (banker's rounding) — matches C99 nearbyint.
/// libm 0.1 lacks nearbyint, so we implement it manually.
fn nearbyint_impl(x: f64) -> f64 {
    // Handle special cases
    if x.is_infinite() || x.is_nan() {
        return x;
    }
    // Values within [-2^52, 2^52] where rounding matters
    let abs_x = x.abs();
    if abs_x >= 4503599627370496.0f64 {
        // Already an integer (all f64 values >= 2^52 are integers)
        return x;
    }
    let truncated = trunc(x);
    let diff = (x - truncated).abs();
    if diff < 0.5 {
        truncated
    } else if diff > 0.5 {
        truncated + x.signum()
    } else {
        // Exactly 0.5 — round to even
        if truncated % 2.0 == 0.0 {
            truncated
        } else {
            truncated + x.signum()
        }
    }
}

/// R_nonint: check if x is not close to an integer
#[inline(always)]
pub fn r_nonint(x: f64) -> bool {
    let diff = fabs(x - r_forceint(x));
    diff > 1e-9 * fmax2(1.0, fabs(x))
}
