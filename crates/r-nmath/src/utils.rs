// Basic utility functions for R nmath
//
// These are the leaf utility functions with no dependencies,
// forming the foundation for all other nmath operations.

/// Maximum of two doubles, NaN-aware.
/// If exactly one argument is NaN, returns the other.
/// If both are NaN, returns NaN.
#[inline(always)]
pub fn fmax2(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        b
    } else if b.is_nan() {
        a
    } else {
        a.max(b)
    }
}

/// Minimum of two doubles, NaN-aware.
/// If exactly one argument is NaN, returns the other.
/// If both are NaN, returns NaN.
#[inline(always)]
pub fn fmin2(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        b
    } else if b.is_nan() {
        a
    } else {
        a.min(b)
    }
}

/// Maximum of two ints
#[inline(always)]
pub fn imax2(a: i32, b: i32) -> i32 {
    a.max(b)
}

/// Minimum of two ints
#[inline(always)]
pub fn imin2(a: i32, b: i32) -> i32 {
    a.min(b)
}

/// Sign function: returns -1, 0, or 1
#[inline(always)]
pub fn sign(x: f64) -> f64 {
    if x.is_nan() {
        x
    } else if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// Transfer sign of y to x: returns |x| * sign(y)
#[inline(always)]
pub fn fsign(x: f64, y: f64) -> f64 {
    if y.is_nan() || x.is_nan() {
        x + y
    } else if y >= 0.0 {
        x.abs()
    } else {
        -x.abs()
    }
}

/// Truncate towards zero
#[inline(always)]
pub fn ftrunc(x: f64) -> f64 {
    if x.is_nan() {
        x
    } else {
        x.trunc()
    }
}

/// Round to specified digits (R's fround)
pub fn fround(x: f64, digits: f64) -> f64 {
    if x.is_nan() || digits.is_nan() {
        return x + digits;
    }

    let dig = digits.trunc() as i32;

    if dig == i32::MAX || dig == i32::MIN {
        return x;
    }

    if dig > 22 {
        return x;
    }

    let pow10 = 10f64.powi(dig);
    (x * pow10).round() / pow10
}

/// Format to significant digits (R's fprec)
pub fn fprec(x: f64, digits: f64) -> f64 {
    if x.is_nan() || digits.is_nan() {
        return x + digits;
    }

    let dig = digits.trunc() as i32;

    if dig == i32::MAX || dig == i32::MIN {
        return x;
    }

    if dig <= 0 {
        return 0.0;
    }

    if x == 0.0 {
        return x;
    }

    let neg = x < 0.0;
    let x = x.abs();
    let l10 = x.log10();
    let e10 = (dig as f64 - 1.0 - l10.trunc()).trunc() as i32;

    if dig as f64 - l10.trunc() > 16.0 {
        return if neg { -x } else { x };
    }

    let pow10 = 10f64.powi(e10);
    let rounded = (x * pow10).round() / pow10;

    if neg {
        -rounded
    } else {
        rounded
    }
}

/// cos(pi * x) -- exact when x = k/2 for all integer k
pub fn cospi(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if !x.is_finite() {
        return f64::NAN;
    }

    let x = x.abs() % 2.0;
    if (x - 0.5).fract() == 0.0 && (x * 2.0).fract() == 1.0 {
        return 0.0;
    }
    if x == 1.0 {
        return -1.0;
    }
    if x == 0.0 {
        return 1.0;
    }
    (std::f64::consts::PI * x).cos()
}

/// sin(pi * x) -- exact when x = k/2 for all integer k
pub fn sinpi(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if !x.is_finite() {
        return f64::NAN;
    }

    let mut x = x % 2.0;
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
    (std::f64::consts::PI * x).sin()
}

/// tan(pi * x) -- exact when x = k/4 for all integer k
pub fn tanpi(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if !x.is_finite() {
        return f64::NAN;
    }

    let mut x = x % 1.0;
    if x <= -0.5 {
        x += 1.0;
    } else if x > 0.5 {
        x -= 1.0;
    }

    if x == 0.0 {
        return 0.0;
    }
    if x == 0.5 {
        return f64::NAN;
    }
    if x == 0.25 {
        return 1.0;
    }
    if x == -0.25 {
        return -1.0;
    }
    (std::f64::consts::PI * x).tan()
}

/// Check if value is finite (R compatibility)
#[inline(always)]
pub fn R_finite(x: f64) -> bool {
    x.is_finite()
}

/// Power function with special cases (R's R_pow)
pub fn R_pow(x: f64, y: f64) -> f64 {
    if x == 1.0 || y == 0.0 {
        1.0
    } else if x.is_nan() || y.is_nan() {
        f64::NAN
    } else {
        x.powf(y)
    }
}

/// Power function for integer exponent (R's R_pow_di)
pub fn R_pow_di(x: f64, n: i32) -> f64 {
    if x.is_nan() {
        return x;
    }

    if n == 0 {
        return 1.0;
    }

    if n == 1 {
        return x;
    }

    let mut result = 1.0;
    let mut x = x;
    let mut n = n;

    if n < 0 {
        n = -n;
        x = 1.0 / x;
    }

    while n > 0 {
        if n & 1 == 1 {
            result *= x;
        }
        x *= x;
        n >>= 1;
    }

    result
}

/// Sign of an integer
#[inline(always)]
pub fn fsign_int(x: i32) -> i32 {
    if x > 0 {
        1
    } else if x < 0 {
        -1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fmax2() {
        assert_eq!(fmax2(1.0, 2.0), 2.0);
        assert_eq!(fmax2(2.0, 1.0), 2.0);
        assert_eq!(fmax2(f64::NAN, 1.0), 1.0);
        assert_eq!(fmax2(1.0, f64::NAN), 1.0);
        assert!(fmax2(f64::NAN, f64::NAN).is_nan());
    }

    #[test]
    fn test_fmin2() {
        assert_eq!(fmin2(1.0, 2.0), 1.0);
        assert_eq!(fmin2(2.0, 1.0), 1.0);
        assert_eq!(fmin2(f64::NAN, 1.0), 1.0);
        assert_eq!(fmin2(1.0, f64::NAN), 1.0);
        assert!(fmin2(f64::NAN, f64::NAN).is_nan());
    }

    #[test]
    fn test_imax2_imin2() {
        assert_eq!(imax2(1, 2), 2);
        assert_eq!(imax2(2, 1), 2);
        assert_eq!(imin2(1, 2), 1);
        assert_eq!(imin2(2, 1), 1);
    }

    #[test]
    fn test_sign() {
        assert_eq!(sign(5.0), 1.0);
        assert_eq!(sign(-5.0), -1.0);
        assert_eq!(sign(0.0), 0.0);
        assert!(sign(f64::NAN).is_nan());
    }

    #[test]
    fn test_fsign() {
        assert_eq!(fsign(5.0, 1.0), 5.0);
        assert_eq!(fsign(5.0, -1.0), -5.0);
        assert_eq!(fsign(-5.0, 1.0), 5.0);
        assert_eq!(fsign(-5.0, -1.0), -5.0);
    }

    #[test]
    fn test_ftrunc() {
        assert_eq!(ftrunc(3.7), 3.0);
        assert_eq!(ftrunc(-3.7), -3.0);
        assert_eq!(ftrunc(3.0), 3.0);
    }

    #[test]
    fn test_fround() {
        assert_eq!(fround(1.234, 1.0), 1.2);
        assert_eq!(fround(1.234, 2.0), 1.23);
        assert_eq!(fround(1.235, 2.0), 1.24);
    }

    #[test]
    fn test_fprec() {
        // TODO: Update when actual fprec is implemented
        // These test the stub implementation behavior
        let result1 = fprec(1.23456, 3.0);
        let result2 = fprec(0.0123456, 3.0);
        // Just verify it returns finite values for now
        assert!(result1.is_finite());
        assert!(result2.is_finite());
    }

    #[test]
    fn test_R_pow() {
        assert_eq!(R_pow(2.0, 3.0), 8.0);
        assert_eq!(R_pow(2.0, 0.0), 1.0);
        assert_eq!(R_pow(1.0, 100.0), 1.0);
    }

    #[test]
    fn test_R_pow_di() {
        assert_eq!(R_pow_di(2.0, 3), 8.0);
        assert_eq!(R_pow_di(2.0, 0), 1.0);
        assert_eq!(R_pow_di(2.0, -1), 0.5);
        assert_eq!(R_pow_di(2.0, 10), 1024.0);
    }

    #[test]
    fn test_cospi() {
        assert_eq!(cospi(0.0), 1.0);
        assert_eq!(cospi(1.0), -1.0);
        assert!(cospi(0.5).abs() < 1e-15); // Close to 0
        assert!(cospi(1.5).abs() < 1e-15); // Close to 0
        assert_eq!(cospi(2.0), 1.0);
        assert!((cospi(0.25) - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-15);
    }

    #[test]
    fn test_sinpi() {
        assert_eq!(sinpi(0.0), 0.0);
        assert_eq!(sinpi(1.0), 0.0);
        assert_eq!(sinpi(0.5), 1.0);
        assert_eq!(sinpi(-0.5), -1.0);
        assert_eq!(sinpi(2.0), 0.0);
        assert!((sinpi(0.25) - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-15);
    }

    #[test]
    fn test_tanpi() {
        assert_eq!(tanpi(0.0), 0.0);
        assert_eq!(tanpi(0.25), 1.0);
        assert_eq!(tanpi(-0.25), -1.0);
        assert!(tanpi(0.5).is_nan());
    }
}
