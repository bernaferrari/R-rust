#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_assignments
)]

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
// Jenkins-Traub complex polynomial root-finder
// ---------------------------------------------------------------------------

/// Internal state for the Jenkins-Traub algorithm, replacing C's static globals.
struct CpolyRootState {
    nn: usize,
    pr: Vec<f64>,
    pi: Vec<f64>,
    hr: Vec<f64>,
    hi: Vec<f64>,
    qpr: Vec<f64>,
    qpi: Vec<f64>,
    qhr: Vec<f64>,
    qhi: Vec<f64>,
    shr: Vec<f64>,
    shi: Vec<f64>,
    sr: f64,
    si: f64,
    tr: f64,
    ti: f64,
    pvr: f64,
    pvi: f64,
}

impl CpolyRootState {
    const ETA: f64 = f64::EPSILON;
    const ARE: f64 = f64::EPSILON;
    const MRE: f64 = 2.0 * std::f64::consts::SQRT_2 * f64::EPSILON;
    const INFIN: f64 = f64::MAX;
    const SMALNO: f64 = f64::MIN_POSITIVE;
    const BASE: f64 = 2.0; // FLT_RADIX
    const COSR: f64 = -0.06975647374412529990; // cos(94°)
    const SINR: f64 = 0.99756405025982424767; // sin(94°)

    fn new(opr: &[f64], opi: &[f64], degree: usize) -> Self {
        let nn = degree;
        CpolyRootState {
            nn,
            pr: opr.to_vec(),
            pi: opi.to_vec(),
            hr: vec![0.0; nn],
            hi: vec![0.0; nn],
            qpr: vec![0.0; nn],
            qpi: vec![0.0; nn],
            qhr: vec![0.0; nn],
            qhi: vec![0.0; nn],
            shr: vec![0.0; nn],
            shi: vec![0.0; nn],
            sr: 0.0,
            si: 0.0,
            tr: 0.0,
            ti: 0.0,
            pvr: 0.0,
            pvi: 0.0,
        }
    }

    fn polyev_internal(
        n: usize,
        s_r: f64,
        s_i: f64,
        p_r: &[f64],
        p_i: &[f64],
        q_r: &mut [f64],
        q_i: &mut [f64],
    ) -> (f64, f64) {
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
        (v_r, v_i)
    }

    fn errev_internal(
        n: usize,
        qr: &[f64],
        qi: &[f64],
        ms: f64,
        mp: f64,
        a_re: f64,
        m_re: f64,
    ) -> f64 {
        let mut e = qr[0].hypot(qi[0]) * m_re / (a_re + m_re);
        for i in 0..n {
            e = e * ms + qr[i].hypot(qi[i]);
        }
        e * (a_re + m_re) - mp * m_re
    }

    /// No-shift stage: computes `l1` no-shift H polynomials from the derivative.
    fn noshft(&mut self, l1: usize) {
        let n = self.nn - 1;
        let nm1 = n - 1;
        let n_f = n as f64;

        for i in 0..n {
            let xni = (self.nn - i - 1) as f64;
            self.hr[i] = xni * self.pr[i] / n_f;
            self.hi[i] = xni * self.pi[i] / n_f;
        }

        for _ in 0..l1 {
            if self.hr[n - 1].hypot(self.hi[n - 1])
                <= Self::ETA * 10.0 * self.pr[n - 1].hypot(self.pi[n - 1])
            {
                for i in (1..=nm1).rev() {
                    self.hr[i] = self.hr[i - 1];
                    self.hi[i] = self.hi[i - 1];
                }
                self.hr[0] = 0.0;
                self.hi[0] = 0.0;
            } else {
                let (tr_val, ti_val) = cdivid(
                    -self.pr[self.nn - 1],
                    -self.pi[self.nn - 1],
                    self.hr[n - 1],
                    self.hi[n - 1],
                );
                for i in (1..=nm1).rev() {
                    let j = self.nn - i;
                    let t1 = self.hr[j - 2];
                    let t2 = self.hi[j - 2];
                    self.hr[j - 1] = tr_val * t1 - ti_val * t2 + self.pr[j - 1];
                    self.hi[j - 1] = tr_val * t2 + ti_val * t1 + self.pi[j - 1];
                }
                self.hr[0] = self.pr[0];
                self.hi[0] = self.pi[0];
            }
        }
    }

    /// Compute `t = -p(s)/h(s)`, setting `h_s_0` if h(s) is essentially zero.
    fn calct(&mut self) -> bool {
        let n = self.nn - 1;
        let (hvr, hvi) = Self::polyev_internal(
            n,
            self.sr,
            self.si,
            &self.hr,
            &self.hi,
            &mut self.qhr,
            &mut self.qhi,
        );

        let h_s_0 = hvr.hypot(hvi) <= Self::ARE * 10.0 * self.hr[n - 1].hypot(self.hi[n - 1]);
        if !h_s_0 {
            let (tr_val, ti_val) = cdivid(-self.pvr, -self.pvi, hvr, hvi);
            self.tr = tr_val;
            self.ti = ti_val;
        } else {
            self.tr = 0.0;
            self.ti = 0.0;
        }
        h_s_0
    }

    /// Calculate the next shifted H polynomial.
    fn nexth(&mut self, h_s_0: bool) {
        let n = self.nn - 1;
        if !h_s_0 {
            for j in 1..n {
                let t1 = self.qhr[j - 1];
                let t2 = self.qhi[j - 1];
                self.hr[j] = self.tr * t1 - self.ti * t2 + self.qpr[j];
                self.hi[j] = self.tr * t2 + self.ti * t1 + self.qpi[j];
            }
            self.hr[0] = self.qpr[0];
            self.hi[0] = self.qpi[0];
        } else {
            for j in 1..n {
                self.hr[j] = self.qhr[j - 1];
                self.hi[j] = self.qhi[j - 1];
            }
            self.hr[0] = 0.0;
            self.hi[0] = 0.0;
        }
    }

    /// Variable-shift iteration (stage 3).
    fn vrshft(&mut self, l3: usize, zr: &mut f64, zi: &mut f64) -> bool {
        let mut b = false;
        self.sr = *zr;
        self.si = *zi;

        let mut omp = f64::MAX;
        let mut relstp = 0.0f64;

        for iter in 1..=l3 {
            let (pvr, pvi) = Self::polyev_internal(
                self.nn,
                self.sr,
                self.si,
                &self.pr,
                &self.pi,
                &mut self.qpr,
                &mut self.qpi,
            );
            self.pvr = pvr;
            self.pvi = pvi;

            let mp = self.pvr.hypot(self.pvi);
            let ms = self.sr.hypot(self.si);

            if mp
                <= 20.0
                    * Self::errev_internal(
                        self.nn,
                        &self.qpr,
                        &self.qpi,
                        ms,
                        mp,
                        Self::ARE,
                        Self::MRE,
                    )
            {
                *zr = self.sr;
                *zi = self.si;
                return true;
            }

            let mut do_l10 = false;
            if iter > 1 {
                if !b && mp >= omp && relstp < 0.05 {
                    let tp = relstp.max(Self::ETA);
                    let r1 = tp.sqrt();
                    let r2 = self.sr * (r1 + 1.0) - self.si * r1;
                    self.si = self.sr * r1 + self.si * (r1 + 1.0);
                    self.sr = r2;
                    let (pvr, pvi) = Self::polyev_internal(
                        self.nn,
                        self.sr,
                        self.si,
                        &self.pr,
                        &self.pi,
                        &mut self.qpr,
                        &mut self.qpi,
                    );
                    self.pvr = pvr;
                    self.pvi = pvi;
                    for _ in 0..5 {
                        let h_s_0 = self.calct();
                        self.nexth(h_s_0);
                    }
                    omp = Self::INFIN;
                    b = true;
                    do_l10 = true;
                } else if mp * 0.1 > omp {
                    return false;
                }
            }
            omp = mp;

            if !do_l10 {
                let h_s_0 = self.calct();
                self.nexth(h_s_0);
            }
            let h_s_0 = self.calct();
            if !h_s_0 {
                relstp = self.tr.hypot(self.ti) / self.sr.hypot(self.si);
                self.sr += self.tr;
                self.si += self.ti;
            }
        }
        false
    }

    /// Fixed-shift iteration (stage 2).
    fn fxshft(&mut self, l2: usize, zr: &mut f64, zi: &mut f64) -> bool {
        let n = self.nn - 1;

        let (pvr, pvi) = Self::polyev_internal(
            self.nn,
            self.sr,
            self.si,
            &self.pr,
            &self.pi,
            &mut self.qpr,
            &mut self.qpi,
        );
        self.pvr = pvr;
        self.pvi = pvi;

        let mut test = true;
        let mut pasd = false;
        let mut otr = 0.0f64;
        let mut oti = 0.0f64;

        let _ = self.calct();

        for j in 1..=l2 {
            otr = self.tr;
            oti = self.ti;

            let h_s_0_first = self.calct();
            self.nexth(h_s_0_first);
            *zr = self.sr + self.tr;
            *zi = self.si + self.ti;

            let h_s_0 = self.calct();
            if !h_s_0 && test && j != l2 {
                if (self.tr - otr).hypot(self.ti - oti) >= zr.hypot(*zi) * 0.5 {
                    pasd = false;
                } else if !pasd {
                    pasd = true;
                } else {
                    for i in 0..n {
                        self.shr[i] = self.hr[i];
                        self.shi[i] = self.hi[i];
                    }
                    let svsr = self.sr;
                    let svsi = self.si;
                    if self.vrshft(10, zr, zi) {
                        return true;
                    }
                    test = false;
                    for i in 0..n {
                        self.hr[i] = self.shr[i];
                        self.hi[i] = self.shi[i];
                    }
                    self.sr = svsr;
                    self.si = svsi;
                    let (pvr, pvi) = Self::polyev_internal(
                        self.nn,
                        self.sr,
                        self.si,
                        &self.pr,
                        &self.pi,
                        &mut self.qpr,
                        &mut self.qpi,
                    );
                    self.pvr = pvr;
                    self.pvi = pvi;
                    let _ = self.calct();
                }
            }
        }

        self.vrshft(10, zr, zi)
    }

    /// Main entry point — find all roots of a complex polynomial.
    ///
    /// Returns `Ok((zeror, zeroi))` on success, `Err(())` if the algorithm fails.
    fn solve(mut self, zeror: &mut [f64], zeroi: &mut [f64]) -> Result<(), ()> {
        let degree = self.nn - 1;
        let d1 = degree;

        if self.pr[0] == 0.0 && self.pi[0] == 0.0 {
            return Err(());
        }

        let mut nn = self.nn;
        while self.pr[nn - 1] == 0.0 && self.pi[nn - 1] == 0.0 {
            let d_n = d1 + 1 - nn;
            zeror[d_n] = 0.0;
            zeroi[d_n] = 0.0;
            nn -= 1;
        }
        nn += 1;
        self.nn = nn;

        if nn == 1 {
            return Ok(());
        }

        for i in 0..nn {
            self.shr[i] = self.pr[i].hypot(self.pi[i]);
        }

        let bnd = cpoly_scale(
            &self.shr[..nn],
            Self::ETA,
            Self::INFIN,
            Self::SMALNO,
            Self::BASE,
        );
        if bnd != 1.0 {
            for i in 0..nn {
                self.pr[i] *= bnd;
                self.pi[i] *= bnd;
            }
        }

        let mut xx = std::f64::consts::FRAC_1_SQRT_2;
        let mut yy = -xx;

        while nn > 2 {
            for i in 0..nn {
                self.shr[i] = self.pr[i].hypot(self.pi[i]);
            }
            let bnd = cpoly_cauchy(nn, &mut self.shr);

            let mut found = false;
            for _i1 in 0..2 {
                self.noshft(5);

                for i2 in 1..=9 {
                    let xxx = Self::COSR * xx - Self::SINR * yy;
                    yy = Self::SINR * xx + Self::COSR * yy;
                    xx = xxx;
                    self.sr = bnd * xx;
                    self.si = bnd * yy;

                    let mut zr = 0.0f64;
                    let mut zi = 0.0f64;
                    if self.fxshft(i2 * 10, &mut zr, &mut zi) {
                        let d_n = d1 + 2 - nn;
                        zeror[d_n] = zr;
                        zeroi[d_n] = zi;
                        nn -= 1;
                        for i in 0..nn {
                            self.pr[i] = self.qpr[i];
                            self.pi[i] = self.qpi[i];
                        }
                        self.nn = nn;
                        found = true;
                        break;
                    }
                }
                if found {
                    break;
                }
            }

            if !found {
                return Err(());
            }
        }

        let (cr, ci) = cdivid(-self.pr[1], -self.pi[1], self.pr[0], self.pi[0]);
        zeror[d1] = cr;
        zeroi[d1] = ci;

        Ok(())
    }
}

/// Find all roots of a complex polynomial using the Jenkins-Traub algorithm.
///
/// Port of `R_cpolyroot` from `complex.c` (lines 940-1070).
///
/// Given `coef` as interleaved complex coefficients `[re0, im0, re1, im1, ...]`
/// and `degree` as the polynomial degree, returns a pointer to a heap-allocated
/// array of `2 * degree` doubles containing `[re0, im0, re1, im1, ...]` root values,
/// or null on failure.
///
/// The caller is responsible for freeing the returned pointer.
pub unsafe fn R_cpolyroot(coef: *mut c_double, degree: c_int) -> *mut std::ffi::c_void {
    unsafe {
        if coef.is_null() || degree <= 0 {
            return std::ptr::null_mut();
        }

        let deg = degree as usize;
        let n = deg + 1;

        let opr = std::slice::from_raw_parts(coef, n);
        let opi = std::slice::from_raw_parts(coef.add(n), n);

        let mut zeror = vec![0.0f64; n];
        let mut zeroi = vec![0.0f64; n];

        let state = CpolyRootState::new(opr, opi, n);

        if state.solve(&mut zeror, &mut zeroi).is_err() {
            return std::ptr::null_mut();
        }

        let mut result = Vec::with_capacity(2 * deg);
        for i in 0..deg {
            result.push(zeror[i]);
            result.push(zeroi[i]);
        }

        // SAFETY: leak the Vec to give the caller a raw pointer. The caller
        // (SEXP wrapper) takes ownership of this memory.
        result.leak().as_mut_ptr() as *mut std::ffi::c_void
    }
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
