//! Complex number arithmetic operations for CPLXSXP.
//!
//! Ports the essential complex arithmetic from R's complex.c.
//! Supports vectorized binary operations (+, -, *, /) with recycling,
//! and complex constructors.

use crate::sexp::accessors::{COMPLEX, INTEGER, REAL, TYPEOF, XLENGTH};
#[allow(unused_imports)]
use crate::sexp::constructors::{Rf_ScalarComplex, Rf_allocVector3};
use crate::sexp::ffi::{NA_INTEGER, R_xlen_t, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

/// NA sentinel for complex: both real and imaginary parts are NA_REAL.
pub const NA_COMPLEX: Rcomplex = Rcomplex {
    r: crate::sexp::ffi::NA_REAL,
    i: crate::sexp::ffi::NA_REAL,
};

/// Check if a complex value is NA.
#[inline]
pub fn is_na_complex(z: Rcomplex) -> bool {
    z.r.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
        || z.i.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
}

// ---------------------------------------------------------------------------
// Element access with recycling
// ---------------------------------------------------------------------------

/// Get a complex value at index i with recycling from a CPLXSXP vector.
#[inline]
unsafe fn elt_complex(x: SEXP, i: R_xlen_t) -> Rcomplex {
    unsafe {
        let n = XLENGTH(x);
        let idx = if n == 0 { 0 } else { i % n };
        *COMPLEX(x).add(idx as usize)
    }
}

/// Get a complex value from any numeric SEXP (REALSXP, INTSXP, LGLSXP, CPLXSXP).
#[inline]
unsafe fn elt_complex_coerce(x: SEXP, i: R_xlen_t) -> Rcomplex {
    unsafe {
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return elt_complex(x, i);
        }
        let n = XLENGTH(x);
        let idx = if n == 0 { 0 } else { i % n };
        let r = if t == SEXPTYPE::REALSXP {
            *REAL(x).add(idx as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(idx as usize);
            if v == NA_INTEGER {
                crate::sexp::ffi::NA_REAL
            } else {
                v as f64
            }
        } else {
            crate::sexp::ffi::NA_REAL
        };
        Rcomplex { r, i: 0.0 }
    }
}

// ---------------------------------------------------------------------------
// Coercion helpers
// ---------------------------------------------------------------------------

/// Coerce a numeric SEXP to CPLXSXP.
pub unsafe fn coerce_to_complex(x: SEXP) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return x;
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let dst = COMPLEX(result);
        for i in 0..n {
            *dst.add(i as usize) = elt_complex_coerce(x, i);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Binary operations
// ---------------------------------------------------------------------------

/// Apply a binary complex operation with recycling.
///
/// If both inputs are REALSXP/INTSXP/LGLSXP, the result is REALSXP if possible.
/// If either input is CPLXSXP, the result is CPLXSXP.
pub unsafe fn complex_binary(op: &str, sa: SEXP, sb: SEXP) -> SEXP {
    unsafe {
        let na = XLENGTH(sa);
        let nb = XLENGTH(sb);
        let n = if na == 0 || nb == 0 {
            0
        } else if na >= nb {
            na
        } else {
            nb
        };
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::CPLXSXP, 0);
        }

        let a_is_complex = TYPEOF(sa) == SEXPTYPE::CPLXSXP;
        let b_is_complex = TYPEOF(sb) == SEXPTYPE::CPLXSXP;
        let result_is_complex = a_is_complex || b_is_complex;

        // If both are real, delegate to real_binary
        if !result_is_complex {
            // Use the real arithmetic path
            return crate::eval::arithmetic::real_binary(op, sa, sb);
        }

        let result = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = COMPLEX(result);
        super::arithmetic::warn_if_non_multiple_recycling(na, nb);

        for i in 0..n {
            let x = elt_complex_coerce(sa, i);
            let y = elt_complex_coerce(sb, i);

            if is_na_complex(x) || is_na_complex(y) {
                *dst.add(i as usize) = NA_COMPLEX;
                continue;
            }

            let val = match op {
                "+" => Rcomplex {
                    r: x.r + y.r,
                    i: x.i + y.i,
                },
                "-" => Rcomplex {
                    r: x.r - y.r,
                    i: x.i - y.i,
                },
                "*" => Rcomplex {
                    r: x.r * y.r - x.i * y.i,
                    i: x.r * y.i + x.i * y.r,
                },
                "/" => {
                    let denom = y.r * y.r + y.i * y.i;
                    if denom == 0.0 {
                        NA_COMPLEX
                    } else {
                        Rcomplex {
                            r: (x.r * y.r + x.i * y.i) / denom,
                            i: (x.i * y.r - x.r * y.i) / denom,
                        }
                    }
                }
                "^" => complex_pow(x, y),
                _ => NA_COMPLEX,
            };

            *dst.add(i as usize) = val;
        }

        super::arithmetic::propagate_binary_vector_attributes(result, sa, sb, n);
        result
    }
}

/// Complex power: z^w = exp(w * log(z))
fn complex_pow(z: Rcomplex, w: Rcomplex) -> Rcomplex {
    let rho = complex_abs(z);
    if rho == 0.0 {
        if w.r == 0.0 && w.i == 0.0 {
            return Rcomplex { r: 1.0, i: 0.0 }; // 0^0 = 1
        }
        if w.r > 0.0 {
            return Rcomplex { r: 0.0, i: 0.0 }; // 0^positive = 0
        }
        return NA_COMPLEX; // 0^negative = NA
    }
    let theta = z.i.atan2(z.r);
    let log_rho = rho.ln();
    // log(z) = ln|z| + i*arg(z)
    // w * log(z) = (a+ib)(ln|z| + i*theta) = (a*ln|z| - b*theta) + i(b*ln|z| + a*theta)
    let re = w.r * log_rho - w.i * theta;
    let im = w.i * log_rho + w.r * theta;
    // exp(re + i*im) = exp(re) * (cos(im) + i*sin(im))
    let abs = re.exp();
    Rcomplex {
        r: abs * im.cos(),
        i: abs * im.sin(),
    }
}

/// Complex absolute value |z| = sqrt(r^2 + i^2)
#[inline]
fn complex_abs(z: Rcomplex) -> f64 {
    (z.r * z.r + z.i * z.i).sqrt()
}

// ---------------------------------------------------------------------------
// Unary operations
// ---------------------------------------------------------------------------

/// Complex absolute value (Modulus) — returns REALSXP.
pub unsafe fn complex_abs_vec(sa: SEXP) -> SEXP {
    unsafe {
        if sa.is_null() || TYPEOF(sa) != SEXPTYPE::CPLXSXP {
            return R_NilValue();
        }
        let n = XLENGTH(sa);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let z = *COMPLEX(sa).add(i as usize);
            if is_na_complex(z) {
                *dst.add(i as usize) = crate::sexp::ffi::NA_REAL;
            } else {
                *dst.add(i as usize) = complex_abs(z);
            }
        }
        result
    }
}

/// Apply a unary complex function element-wise.
pub unsafe fn complex_unary_vec(sa: SEXP, f: fn(Rcomplex) -> Rcomplex) -> SEXP {
    unsafe {
        if sa.is_null() || TYPEOF(sa) != SEXPTYPE::CPLXSXP {
            return R_NilValue();
        }
        let n = XLENGTH(sa);
        let result = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = COMPLEX(result);
        for i in 0..n {
            let z = *COMPLEX(sa).add(i as usize);
            if is_na_complex(z) {
                *dst.add(i as usize) = NA_COMPLEX;
            } else {
                *dst.add(i as usize) = f(z);
            }
        }
        result
    }
}

/// Complex square root.
pub fn complex_sqrt(z: Rcomplex) -> Rcomplex {
    let rho = complex_abs(z);
    let a = ((rho + z.r) / 2.0).sqrt();
    let b = ((rho - z.r) / 2.0).sqrt() * if z.i < 0.0 { -1.0 } else { 1.0 };
    Rcomplex { r: a, i: b }
}

/// Complex exponential.
pub fn complex_exp(z: Rcomplex) -> Rcomplex {
    let abs = z.r.exp();
    Rcomplex {
        r: abs * z.i.cos(),
        i: abs * z.i.sin(),
    }
}

/// Complex natural logarithm.
pub fn complex_log(z: Rcomplex) -> Rcomplex {
    let rho = complex_abs(z);
    let theta = z.i.atan2(z.r);
    Rcomplex {
        r: rho.ln(),
        i: theta,
    }
}

/// Complex sine.
pub fn complex_sin(z: Rcomplex) -> Rcomplex {
    Rcomplex {
        r: z.r.sin() * z.i.cosh(),
        i: z.r.cos() * z.i.sinh(),
    }
}

/// Complex cosine.
pub fn complex_cos(z: Rcomplex) -> Rcomplex {
    Rcomplex {
        r: z.r.cos() * z.i.cosh(),
        i: -(z.r.sin() * z.i.sinh()),
    }
}

/// Complex tangent.
pub fn complex_tan(z: Rcomplex) -> Rcomplex {
    let s = complex_sin(z);
    let c = complex_cos(z);
    let d = c.r * c.r + c.i * c.i;
    if d == 0.0 {
        return NA_COMPLEX;
    }
    Rcomplex {
        r: (s.r * c.r + s.i * c.i) / d,
        i: (s.i * c.r - s.r * c.i) / d,
    }
}

/// Complex hyperbolic sine.
pub fn complex_sinh(z: Rcomplex) -> Rcomplex {
    Rcomplex {
        r: z.r.sinh() * z.i.cos(),
        i: z.r.cosh() * z.i.sin(),
    }
}

/// Complex hyperbolic cosine.
pub fn complex_cosh(z: Rcomplex) -> Rcomplex {
    Rcomplex {
        r: z.r.cosh() * z.i.cos(),
        i: z.r.sinh() * z.i.sin(),
    }
}

/// Complex hyperbolic tangent.
pub fn complex_tanh(z: Rcomplex) -> Rcomplex {
    if z.r.is_finite() && z.r.abs() > 20.0 {
        return Rcomplex {
            r: if z.r.is_sign_negative() { -1.0 } else { 1.0 },
            i: 0.0,
        };
    }

    let denominator = (2.0 * z.r).cosh() + (2.0 * z.i).cos();
    if denominator == 0.0 {
        return NA_COMPLEX;
    }
    Rcomplex {
        r: (2.0 * z.r).sinh() / denominator,
        i: (2.0 * z.i).sin() / denominator,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complex_add() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let a = Rf_ScalarComplex(Rcomplex { r: 1.0, i: 2.0 });
            let b = Rf_ScalarComplex(Rcomplex { r: 3.0, i: 4.0 });
            let result = complex_binary("+", a, b);
            let z = *COMPLEX(result);
            assert!((z.r - 4.0).abs() < 1e-10);
            assert!((z.i - 6.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_complex_mul() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // (1+2i) * (3+4i) = (3-8) + (4+6)i = -5 + 10i
            let a = Rf_ScalarComplex(Rcomplex { r: 1.0, i: 2.0 });
            let b = Rf_ScalarComplex(Rcomplex { r: 3.0, i: 4.0 });
            let result = complex_binary("*", a, b);
            let z = *COMPLEX(result);
            assert!((z.r - (-5.0)).abs() < 1e-10);
            assert!((z.i - 10.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_complex_div() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // (1+2i) / (1+i) = ((1+2i)(1-i)) / 2 = (3+i)/2 = 1.5 + 0.5i
            let a = Rf_ScalarComplex(Rcomplex { r: 1.0, i: 2.0 });
            let b = Rf_ScalarComplex(Rcomplex { r: 1.0, i: 1.0 });
            let result = complex_binary("/", a, b);
            let z = *COMPLEX(result);
            assert!((z.r - 1.5).abs() < 1e-10);
            assert!((z.i - 0.5).abs() < 1e-10);
        }
    }

    #[test]
    fn test_complex_sub() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let a = Rf_ScalarComplex(Rcomplex { r: 5.0, i: 3.0 });
            let b = Rf_ScalarComplex(Rcomplex { r: 2.0, i: 1.0 });
            let result = complex_binary("-", a, b);
            let z = *COMPLEX(result);
            assert!((z.r - 3.0).abs() < 1e-10);
            assert!((z.i - 2.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_complex_abs() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let a = Rf_ScalarComplex(Rcomplex { r: 3.0, i: 4.0 });
            let result = complex_abs_vec(a);
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            let v = *REAL(result);
            assert!((v - 5.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_complex_exp() {
        let _session = crate::sexp::session::RSession::new();
        // exp(i*pi) = -1
        let z = complex_exp(Rcomplex {
            r: 0.0,
            i: std::f64::consts::PI,
        });
        assert!((z.r - (-1.0)).abs() < 1e-10);
        assert!(z.i.abs() < 1e-10);
    }

    #[test]
    fn test_complex_sqrt() {
        let _session = crate::sexp::session::RSession::new();
        // sqrt(-1) = i
        let z = complex_sqrt(Rcomplex { r: -1.0, i: 0.0 });
        assert!(z.r.abs() < 1e-10);
        assert!((z.i - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_complex_hyperbolic_identity() {
        let _session = crate::sexp::session::RSession::new();
        let z = Rcomplex { r: 1.25, i: -0.5 };
        let cosh_z = complex_cosh(z);
        let cos_iz = complex_cos(Rcomplex { r: -z.i, i: z.r });
        assert!((cosh_z.r - cos_iz.r).abs() < 1e-12);
        assert!((cosh_z.i - cos_iz.i).abs() < 1e-12);

        let tanh_large = complex_tanh(Rcomplex { r: 356.0, i: 0.0 });
        assert_eq!(tanh_large.r, 1.0);
        assert_eq!(tanh_large.i, 0.0);
    }
}
