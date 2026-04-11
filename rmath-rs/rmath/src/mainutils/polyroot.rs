#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/complex.c — polynomial root-finding (Jenkins-Traub).
//!
//! This module ports the standalone complex arithmetic helper functions
//! used by R's polyroot() implementation (Jenkins-Traub algorithm).
//!
//! Ported standalone functions:
//!   cdivid (complex division avoiding overflow),
//!   polyev (polynomial evaluation via Horner),
//!   errev (error estimation for Horner evaluation),
//!   cpoly_cauchy (Cauchy lower bound for root moduli),
//!   cpoly_scale (coefficient scaling factor)
//!
//! SEXP-dependent stubs:
//!   R_cpolyroot (main entry point, requires SEXP)

use std::os::raw::{c_double, c_int};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's positive infinity.
pub const R_PosInf: c_double = f64::INFINITY;

// ---------------------------------------------------------------------------
// cdivid — complex division avoiding overflow
// ---------------------------------------------------------------------------

/// Complex division `c = a / b`, avoiding overflow.
///
/// Computes `(cr + i*ci) = (ar + i*ai) / (br + i*bi)` using the
/// Smith method to avoid intermediate overflow.
///
/// If `br` and `bi` are both zero, returns `(+Inf, +Inf)`.
pub fn cdivid(ar: c_double, ai: c_double, br: c_double, bi: c_double) -> (c_double, c_double) {
    if br == 0.0 && bi == 0.0 {
        return (R_PosInf, R_PosInf);
    }

    if br.abs() >= bi.abs() {
        let r = bi / br;
        let d = br + r * bi;
        ((ar + ai * r) / d, (ai - ar * r) / d)
    } else {
        let r = br / bi;
        let d = bi + r * br;
        ((ar * r + ai) / d, (ai * r - ar) / d)
    }
}

// ---------------------------------------------------------------------------
// polyev — polynomial evaluation (Horner)
// ---------------------------------------------------------------------------

/// Evaluate a polynomial at a complex point using Horner's method.
///
/// Given polynomial `p[0] + p[1]*s + ... + p[n-1]*s^(n-1)`,
/// computes the value `v` and the partial sums `q`.
///
/// # Parameters
/// - `n`: degree + 1 (number of coefficients)
/// - `s_r`, `s_i`: evaluation point (real and imaginary parts)
/// - `p_r`, `p_i`: real and imaginary parts of coefficients (length `n`)
/// - Returns: `(v_r, v_i, q_r, q_i)` — the polynomial value and partial sums
pub fn polyev(
    n: usize,
    s_r: c_double,
    s_i: c_double,
    p_r: &[c_double],
    p_i: &[c_double],
) -> (c_double, c_double, Vec<c_double>, Vec<c_double>) {
    let mut q_r = vec![0.0; n];
    let mut q_i = vec![0.0; n];

    q_r[0] = p_r[0];
    q_i[0] = p_i[0];
    let mut v_r = q_r[0];
    let mut v_i = q_i[0];

    for i in 1..n {
        let t = v_r * s_r - v_i * s_i + p_r[i];
        v_i = v_r * s_i + v_i * s_r + p_i[i];
        q_i[i] = v_i;
        v_r = t;
        q_r[i] = v_r;
    }

    (v_r, v_i, q_r, q_i)
}

// ---------------------------------------------------------------------------
// errev — error estimation for Horner polynomial evaluation
// ---------------------------------------------------------------------------

/// Estimate the error in evaluating a polynomial by Horner's recurrence.
///
/// # Parameters
/// - `qr`, `qi`: real and imaginary parts of partial sum vectors
/// - `ms`: modulus of the evaluation point
/// - `mp`: modulus of the polynomial value
/// - `a_re`: error bound on complex addition
/// - `m_re`: error bound on complex multiplication
///
/// Returns the estimated error bound.
pub fn errev(
    qr: &[c_double],
    qi: &[c_double],
    ms: c_double,
    mp: c_double,
    a_re: c_double,
    m_re: c_double,
) -> c_double {
    let n = qr.len();
    if n == 0 {
        return 0.0;
    }

    let mut e = (qr[0].hypot(qi[0])) * m_re / (a_re + m_re);
    for i in 0..n {
        e = e * ms + qr[i].hypot(qi[i]);
    }
    e * (a_re + m_re) - mp * m_re
}

// ---------------------------------------------------------------------------
// cpoly_cauchy — Cauchy lower bound on root moduli
// ---------------------------------------------------------------------------

/// Compute a lower bound on the moduli of the zeros of a polynomial.
///
/// `pot` contains the moduli of the coefficients (length `n`).
/// Returns the Cauchy lower bound.
pub fn cpoly_cauchy(n: usize, pot: &mut [c_double]) -> c_double {
    if n <= 1 {
        return 0.0;
    }

    let n1 = n - 1;
    pot[n1] = -pot[n1];

    // compute upper estimate of bound
    let mut x = ((-pot[n1]).ln() - pot[0].ln() / (n1 as c_double)).exp();

    // if newton step at the origin is better, use it
    if pot[n1 - 1] != 0.0 {
        let xm = -pot[n1] / pot[n1 - 1];
        if xm < x {
            x = xm;
        }
    }

    // chop the interval (0,x) until f <= 0
    loop {
        let xm = x * 0.1;
        let mut f = pot[0];
        for i in 1..n {
            f = f * xm + pot[i];
        }
        if f <= 0.0 {
            break;
        }
        x = xm;
    }

    let mut dx = x;

    // do Newton iteration until x converges to two decimal places
    while (dx / x).abs() > 0.005 {
        let mut q = vec![0.0; n];
        q[0] = pot[0];
        for i in 1..n {
            q[i] = q[i - 1] * x + pot[i];
        }
        let f = q[n1];
        let mut delf = q[0];
        for i in 1..n1 {
            delf = delf * x + q[i];
        }
        dx = f / delf;
        x -= dx;
    }

    x
}

// ---------------------------------------------------------------------------
// cpoly_scale — compute scaling factor for polynomial coefficients
// ---------------------------------------------------------------------------

/// Compute a scaling factor for polynomial coefficients.
///
/// Returns a power of `base` that keeps coefficients in a good range
/// for numerical stability.
///
/// # Parameters
/// - `pot`: moduli of coefficients (length `n`)
/// - `eps`, `BIG`, `small`, `base`: floating-point arithmetic constants
pub fn cpoly_scale(
    pot: &[c_double],
    eps: c_double,
    big: c_double,
    small: c_double,
    base: c_double,
) -> c_double {
    let n = pot.len();
    if n == 0 {
        return 1.0;
    }

    let high = big.sqrt();
    let lo = small / eps;
    let mut max_ = 0.0;
    let mut min_ = big;

    for i in 0..n {
        let x = pot[i];
        if x > max_ {
            max_ = x;
        }
        if x != 0.0 && x < min_ {
            min_ = x;
        }
    }

    if min_ < lo || max_ > high {
        let x = lo / min_;
        let sc = if x <= 1.0 {
            1.0 / (max_.sqrt() * min_.sqrt())
        } else {
            let mut s = x;
            if big / s > max_ {
                s = 1.0;
            }
            s
        };
        let ell = (sc.ln() / base.ln() + 0.5) as i32;
        crate::special::mlutils::R_pow_di(base, ell)
    } else {
        1.0
    }
}

// ---------------------------------------------------------------------------
// SEXP-dependent stub
// ---------------------------------------------------------------------------

/// Placeholder: `R_cpolyroot` — requires SEXP for input/output.
pub unsafe fn R_cpolyroot(_coef: *mut c_double, _degree: c_int) -> *mut std::ffi::c_void {
    std::ptr::null_mut()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cdivid_basic() {
        // (3+4i) / (1+2i) = (11-2i)/5 = 2.2-0.4i
        let (cr, ci) = cdivid(3.0, 4.0, 1.0, 2.0);
        assert!((cr - 2.2).abs() < 1e-10);
        assert!((ci - (-0.4)).abs() < 1e-10);
    }

    #[test]
    fn test_cdivid_real() {
        // 6 / 2 = 3
        let (cr, ci) = cdivid(6.0, 0.0, 2.0, 0.0);
        assert!((cr - 3.0).abs() < 1e-10);
        assert!(ci.abs() < 1e-10);
    }

    #[test]
    fn test_cdivid_zero() {
        let (cr, ci) = cdivid(1.0, 0.0, 0.0, 0.0);
        assert!(cr.is_infinite());
        assert!(ci.is_infinite());
    }

    #[test]
    fn test_cdivid_by_imaginary() {
        // 2i / i = 2
        let (cr, ci) = cdivid(0.0, 2.0, 0.0, 1.0);
        assert!((cr - 2.0).abs() < 1e-10);
        assert!(ci.abs() < 1e-10);
    }

    #[test]
    fn test_polyev_linear() {
        // p(x) = 2x + 1, coefficients: [2, 1] (highest degree first)
        // at x = 3 => 2*3 + 1 = 7
        let p_r = &[2.0, 1.0];
        let p_i = &[0.0, 0.0];
        let (v_r, v_i, _, _) = polyev(2, 3.0, 0.0, p_r, p_i);
        assert!((v_r - 7.0).abs() < 1e-10);
        assert!(v_i.abs() < 1e-10);
    }

    #[test]
    fn test_polyev_quadratic() {
        // p(x) = -x^2 + 0x + 1, coefficients: [-1, 0, 1]
        // at x = 2 => -4 + 0 + 1 = -3
        let p_r = &[-1.0, 0.0, 1.0];
        let p_i = &[0.0, 0.0, 0.0];
        let (v_r, v_i, _, _) = polyev(3, 2.0, 0.0, p_r, p_i);
        assert!((v_r - (-3.0)).abs() < 1e-10);
    }

    #[test]
    fn test_polyev_complex() {
        // p(x) = i*x + 1, coefficients real: [0, 1], imag: [0, 1]
        // at x = i => i*i + 1 = -1 + 1 = 0
        let p_r = &[0.0, 1.0];
        let p_i = &[1.0, 0.0];
        let (v_r, v_i, _, _) = polyev(2, 0.0, 1.0, p_r, p_i);
        assert!(v_r.abs() < 1e-10);
        assert!(v_i.abs() < 1e-10);
    }

    #[test]
    fn test_cpoly_cauchy_simple() {
        // p(x) = x^2 - 2 => roots are +-sqrt(2) ≈ 1.414
        // pot[0]=1 (leading coeff), pot[1]=0 (x coeff), pot[2]=2 (constant)
        let mut pot = [1.0, 0.0, 2.0];
        let bound = cpoly_cauchy(3, &mut pot);
        assert!(bound > 1.0);
        assert!(bound < 3.0);
    }

    #[test]
    fn test_cpoly_scale_no_scale() {
        // Coefficients in reasonable range -> scale = 1
        let pot = [1.0, 2.0, 3.0];
        let scale = cpoly_scale(&pot, 1e-10, 1e20, 1e-20, 2.0);
        assert!((scale - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_cpoly_scale_wide_range() {
        // Very small min and very large max -> should scale
        let pot = [1e-30, 1.0, 1e30];
        let scale = cpoly_scale(&pot, 1e-10, 1e20, 1e-20, 2.0);
        assert!(scale != 1.0);
    }
}
