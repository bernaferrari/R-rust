//! Complex number arithmetic operations for CPLXSXP.
//!
//! Ports the essential complex arithmetic from R's complex.c.
//! Supports vectorized binary operations (+, -, *, /) with recycling,
//! and complex constructors.

#[cfg(test)]
use crate::sexp::accessors::{COMPLEX, COMPLEX_ELT, REAL, REAL_ELT};
use crate::sexp::accessors::{TYPEOF, XLENGTH};
#[allow(unused_imports)]
use crate::sexp::constructors::{Rf_ScalarComplex, Rf_allocVector3};
use crate::sexp::ffi::{NA_INTEGER, R_xlen_t, Rcomplex, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::object::{Sexp, SexpMut, SexpResult};
use crate::sexp::owner::OwnerToken;

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

fn checked<T>(result: SexpResult<T>) -> T {
    result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Copy a numeric element after applying the existing materialization policy.
fn elt_complex_coerce(owner: OwnerToken<'_>, value: &Sexp<'_>, i: R_xlen_t) -> Rcomplex {
    let kind = value.typeof_();
    let len = value.len();
    let index = if len == 0 { 0 } else { i % len };
    if !matches!(
        kind,
        SEXPTYPE::CPLXSXP | SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
    ) {
        return Rcomplex {
            r: crate::sexp::ffi::NA_REAL,
            i: 0.0,
        };
    }
    checked(value.materialize_compact_payload_for_raw());
    checked(owner.require_active());
    if kind == SEXPTYPE::CPLXSXP {
        return checked(value.try_complex_elt(index));
    }
    if kind == SEXPTYPE::REALSXP {
        return Rcomplex {
            r: checked(value.try_real_elt(index)),
            i: 0.0,
        };
    }
    let integer = if kind == SEXPTYPE::INTSXP {
        checked(value.try_integer_elt(index))
    } else {
        checked(value.try_logical_elt(index))
    };
    let r = if integer == NA_INTEGER {
        crate::sexp::ffi::NA_REAL
    } else {
        f64::from(integer)
    };
    Rcomplex { r, i: 0.0 }
}

fn allocate_vector<'a>(
    owner: OwnerToken<'a>,
    kind: SEXPTYPE,
    len: R_xlen_t,
) -> Option<SexpMut<'a>> {
    checked(owner.require_active());
    // SAFETY: the owner is active, and no Rust payload loan crosses allocation.
    let raw = Rf_allocVector3(kind, len);
    checked(owner.require_active());
    (!raw.is_null()).then(|| checked(SexpMut::try_from_checked(checked(owner.sexp(raw)))))
}

/// Coerce a numeric SEXP to CPLXSXP.
pub fn coerce_to_complex(x: SEXP) -> SEXP {
    if TYPEOF(x) == SEXPTYPE::CPLXSXP {
        return x;
    }
    let n = XLENGTH(x);
    if n == 0 {
        // SAFETY: the translated caller retains the active runtime.
        let raw = Rf_allocVector3(SEXPTYPE::CPLXSXP, 0);
        return if raw.is_null() { R_NilValue() } else { raw };
    }
    checked(OwnerToken::with_current(|owner| {
        let source = checked(owner.sexp(x));
        let Some(mut result) = allocate_vector(owner, SEXPTYPE::CPLXSXP, n) else {
            return R_NilValue();
        };
        for i in 0..n {
            checked(result.try_set_complex_elt(i, elt_complex_coerce(owner, &source, i)));
        }
        result.freeze().as_raw()
    }))
}

// ---------------------------------------------------------------------------
// Binary operations
// ---------------------------------------------------------------------------

/// Apply a binary complex operation with recycling.
///
/// If both inputs are REALSXP/INTSXP/LGLSXP, the result is REALSXP if possible.
/// If either input is CPLXSXP, the result is CPLXSXP.
pub fn complex_binary(op: &str, sa: SEXP, sb: SEXP) -> SEXP {
    let na = XLENGTH(sa);
    let nb = XLENGTH(sb);
    if na == 0 || nb == 0 {
        // SAFETY: the translated caller retains the active runtime.
        return Rf_allocVector3(SEXPTYPE::CPLXSXP, 0);
    }
    if TYPEOF(sa) != SEXPTYPE::CPLXSXP && TYPEOF(sb) != SEXPTYPE::CPLXSXP {
        // SAFETY: the translated caller retains both original arguments.
        return crate::eval::arithmetic::real_binary(op, sa, sb);
    }

    let n = na.max(nb);
    checked(OwnerToken::with_current(|owner| {
        let a = checked(owner.sexp(sa));
        let b = checked(owner.sexp(sb));
        let Some(mut result) = allocate_vector(owner, SEXPTYPE::CPLXSXP, n) else {
            return R_NilValue();
        };
        super::arithmetic::warn_if_non_multiple_recycling(na, nb);
        checked(owner.require_active());

        for i in 0..n {
            let x = elt_complex_coerce(owner, &a, i);
            let y = elt_complex_coerce(owner, &b, i);
            if op != "+" && op != "-" && (is_na_complex(x) || is_na_complex(y)) {
                checked(result.try_set_complex_elt(i, NA_COMPLEX));
                continue;
            }
            let val = match op {
                "+" => {
                    let na = crate::sexp::ffi::NA_REAL;
                    let r_na = |v: f64| v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN;
                    Rcomplex {
                        r: if r_na(x.r) || r_na(y.r) {
                            na
                        } else {
                            x.r + y.r
                        },
                        i: if r_na(x.i) || r_na(y.i) {
                            na
                        } else {
                            x.i + y.i
                        },
                    }
                }
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

            checked(result.try_set_complex_elt(i, val));
        }
        let result = result.freeze();
        // SAFETY: the checked operands and result stay rooted during attribute propagation.
        {
            super::arithmetic::propagate_arithmetic_attributes(
                result.as_raw(),
                a.as_raw(),
                b.as_raw(),
                n,
            )
        };
        checked(owner.require_active());
        result.as_raw()
    }))
}

/// Complex power via GNU `mycpow` (integer exponents stay exact).
fn complex_pow(z: Rcomplex, w: Rcomplex) -> Rcomplex {
    if is_na_complex(z) || is_na_complex(w) {
        return NA_COMPLEX;
    }
    crate::mainutils::complex_cmath::mycpow_rcomplex(z, w)
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
pub fn complex_abs_vec(sa: SEXP) -> SEXP {
    if sa.is_null() || TYPEOF(sa) != SEXPTYPE::CPLXSXP {
        return R_NilValue();
    }
    checked(OwnerToken::with_current(|owner| {
        let source = checked(owner.sexp(sa));
        let n = source.len();
        let Some(mut result) = allocate_vector(owner, SEXPTYPE::REALSXP, n) else {
            return R_NilValue();
        };
        for i in 0..n {
            let z = elt_complex_coerce(owner, &source, i);
            let value = if is_na_complex(z) {
                crate::sexp::ffi::NA_REAL
            } else {
                complex_abs(z)
            };
            checked(result.try_set_real_elt(i, value));
        }
        result.freeze().as_raw()
    }))
}

/// Apply a unary complex function element-wise.
pub unsafe fn complex_unary_vec(sa: SEXP, f: fn(Rcomplex) -> Rcomplex) -> SEXP {
    if sa.is_null() || TYPEOF(sa) != SEXPTYPE::CPLXSXP {
        return R_NilValue();
    }
    checked(OwnerToken::with_current(|owner| {
        let source = checked(owner.sexp(sa));
        let n = source.len();
        let Some(mut result) = allocate_vector(owner, SEXPTYPE::CPLXSXP, n) else {
            return R_NilValue();
        };
        for i in 0..n {
            let z = elt_complex_coerce(owner, &source, i);
            let value = if is_na_complex(z) { NA_COMPLEX } else { f(z) };
            checked(owner.require_active());
            checked(result.try_set_complex_elt(i, value));
        }
        result.freeze().as_raw()
    }))
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
    apply_z_fn(z, crate::mainutils::complex_cmath::z_tan)
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

fn apply_z_fn(z: Rcomplex, f: fn(num::Complex<f64>) -> num::Complex<f64>) -> Rcomplex {
    let c = f(num::Complex::new(z.r, z.i));
    Rcomplex { r: c.re, i: c.im }
}

/// GNU `z_asin` — branch-cut aware complex arcsine.
pub fn complex_asin(z: Rcomplex) -> Rcomplex {
    apply_z_fn(z, crate::mainutils::complex_cmath::z_asin)
}

/// GNU `z_acos` — branch-cut aware complex arccosine.
pub fn complex_acos(z: Rcomplex) -> Rcomplex {
    apply_z_fn(z, crate::mainutils::complex_cmath::z_acos)
}

/// GNU `z_atan` — branch-cut aware complex arctangent.
pub fn complex_atan(z: Rcomplex) -> Rcomplex {
    apply_z_fn(z, crate::mainutils::complex_cmath::z_atan)
}

/// GNU `z_asinh` — branch-cut aware complex inverse hyperbolic sine.
pub fn complex_asinh(z: Rcomplex) -> Rcomplex {
    apply_z_fn(z, crate::mainutils::complex_cmath::z_asinh)
}

/// GNU `z_acosh` — branch-cut aware complex inverse hyperbolic cosine.
pub fn complex_acosh(z: Rcomplex) -> Rcomplex {
    apply_z_fn(z, crate::mainutils::complex_cmath::z_acosh)
}

/// GNU `z_atanh` — branch-cut aware complex inverse hyperbolic tangent.
pub fn complex_atanh(z: Rcomplex) -> Rcomplex {
    apply_z_fn(z, crate::mainutils::complex_cmath::z_atanh)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_complex_arithmetic_copies_after_altrep_collection() {
        use crate::sexp::altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement};
        use std::{cell::Cell, rc::Rc};

        struct CollectingReal(Rc<Cell<usize>>);
        impl AltrepClass for CollectingReal {
            fn vector_type(&self) -> SEXPTYPE {
                SEXPTYPE::REALSXP
            }
            fn length(&self, _: &AltrepContext<'_>) -> SexpResult<R_xlen_t> {
                Ok(1)
            }
            fn element<'a>(
                &self,
                _: &AltrepContext<'a>,
                _: R_xlen_t,
            ) -> SexpResult<AltrepElement<'a>> {
                self.0.set(self.0.get() + 1);
                crate::sexp::gengc::full_gc();
                Ok(AltrepElement::Real(5.0))
            }
        }

        let session = crate::sexp::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let callbacks = Rc::new(Cell::new(0));
        let class = session
            .register_altrep_class("complex.collection", CollectingReal(callbacks.clone()))
            .unwrap();
        let real = AltrepBuilder::new(class).build().unwrap();
        session.with_active(|| {
            // SAFETY: the owner remains active and all evaluated inputs are
            // rooted through materialization, computation, and publication.
            let complex = Rf_ScalarComplex(Rcomplex { r: 2.0, i: 3.0 });
            let complex = owner.sexp(complex).unwrap();
            let raw = complex_binary("*", complex.as_raw(), real.as_raw());
            let value = owner.sexp(raw).unwrap().try_complex_elt(0).unwrap();
            assert_eq!((value.r, value.i), (10.0, 15.0));
        });
        assert_eq!(callbacks.get(), 1);
    }

    #[test]
    fn test_complex_add() {
        let _session = crate::sexp::session::RSession::new();

        let a = Rf_ScalarComplex(Rcomplex { r: 1.0, i: 2.0 });
        let b = Rf_ScalarComplex(Rcomplex { r: 3.0, i: 4.0 });
        let result = complex_binary("+", a, b);
        let z = COMPLEX_ELT(result, 0);
        assert!((z.r - 4.0).abs() < 1e-10);
        assert!((z.i - 6.0).abs() < 1e-10);
    }

    #[test]
    fn test_complex_mul() {
        let _session = crate::sexp::session::RSession::new();

        // (1+2i) * (3+4i) = (3-8) + (4+6)i = -5 + 10i
        let a = Rf_ScalarComplex(Rcomplex { r: 1.0, i: 2.0 });
        let b = Rf_ScalarComplex(Rcomplex { r: 3.0, i: 4.0 });
        let result = complex_binary("*", a, b);
        let z = COMPLEX_ELT(result, 0);
        assert!((z.r - (-5.0)).abs() < 1e-10);
        assert!((z.i - 10.0).abs() < 1e-10);
    }

    #[test]
    fn test_complex_div() {
        let _session = crate::sexp::session::RSession::new();

        // (1+2i) / (1+i) = ((1+2i)(1-i)) / 2 = (3+i)/2 = 1.5 + 0.5i
        let a = Rf_ScalarComplex(Rcomplex { r: 1.0, i: 2.0 });
        let b = Rf_ScalarComplex(Rcomplex { r: 1.0, i: 1.0 });
        let result = complex_binary("/", a, b);
        let z = COMPLEX_ELT(result, 0);
        assert!((z.r - 1.5).abs() < 1e-10);
        assert!((z.i - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_complex_sub() {
        let _session = crate::sexp::session::RSession::new();

        let a = Rf_ScalarComplex(Rcomplex { r: 5.0, i: 3.0 });
        let b = Rf_ScalarComplex(Rcomplex { r: 2.0, i: 1.0 });
        let result = complex_binary("-", a, b);
        let z = COMPLEX_ELT(result, 0);
        assert!((z.r - 3.0).abs() < 1e-10);
        assert!((z.i - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_complex_abs() {
        let _session = crate::sexp::session::RSession::new();

        let a = Rf_ScalarComplex(Rcomplex { r: 3.0, i: 4.0 });
        let result = complex_abs_vec(a);
        assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
        let v = REAL_ELT(result, 0);
        assert!((v - 5.0).abs() < 1e-10);
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
