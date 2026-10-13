use super::*;

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
pub(crate) fn f_x_x(x: f64, f: fn(f64) -> f64, m: f64) -> f64 {
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
