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
#[inline(always)]
pub fn r_forceint(x: f64) -> f64 {
    // libm 0.1 doesn't have nearbyint; round is the fallback from C
    libm::round(x)
}

/// R_nonint: check if x is not close to an integer
#[inline(always)]
pub fn r_nonint(x: f64) -> bool {
    let diff = fabs(x - r_forceint(x));
    diff > 1e-9 * fmax2(1.0, fabs(x))
}
