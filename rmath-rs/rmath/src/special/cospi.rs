// Ported from cospi.c: cospi, sinpi, tanpi/Rtanpi

use crate::constants::*;
use crate::error::*;
use libm::*;

const PI: f64 = 3.14159265358979323846264338327950288;

/// cos(pi * x) -- exact when x = k/2 for all integer k.
#[must_use]
pub extern "C" fn cospi(x: f64) -> f64 {
    if isnan(x) {
        return x;
    }
    if !r_finite(x) {
        return ml_warn_return_nan();
    }

    let x = fmod(fabs(x), 2.0);
    if fmod(x, 1.0) == 0.5 {
        return 0.0;
    }
    if x == 1.0 {
        return -1.0;
    }
    if x == 0.0 {
        return 1.0;
    }
    cos(PI * x)
}

/// sin(pi * x) -- exact when x = k/2 for all integer k.
#[must_use]
pub extern "C" fn sinpi(x: f64) -> f64 {
    if isnan(x) {
        return x;
    }
    if !r_finite(x) {
        return ml_warn_return_nan();
    }

    let mut x = fmod(x, 2.0);
    // map (-2,2) --> (-1,1]:
    if x <= -1.0 {
        x += 2.0;
    } else if x > 1.0 {
        x -= 2.0;
    }
    if x == 0.0 || x == 1.0 {
        return 0.0;
    }
    if x == 0.5 {
        return 1.0;
    }
    if x == -0.5 {
        return -1.0;
    }
    sin(PI * x)
}

/// Internal tan(pi * x) implementation (Rtanpi in C).
/// Exact when x = k/4 for all integer k.
fn rtanpi(x: f64) -> f64 {
    if isnan(x) {
        return x;
    }
    if !r_finite(x) {
        return ml_warn_return_nan();
    }

    let mut x = fmod(x, 1.0);
    // map (-1,1] --> (-1/2, 1/2]:
    if x <= -0.5 {
        x += 1.0;
    } else if x > 0.5 {
        x -= 1.0;
    }
    if x == 0.0 {
        0.0
    } else if x == 0.5 {
        ML_NAN
    } else if x == 0.25 {
        1.0
    } else if x == -0.25 {
        -1.0
    } else {
        tan(PI * x)
    }
}

/// tan(pi * x) -- exact when x = k/4 for all integer k.
#[must_use]
pub extern "C" fn tanpi(x: f64) -> f64 {
    rtanpi(x)
}
