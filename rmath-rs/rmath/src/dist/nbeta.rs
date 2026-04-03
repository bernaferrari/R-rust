#![allow(unused_assignments)]
// Noncentral beta distribution: dnbeta, pnbeta, qnbeta
// Ported from dnbeta.c, pnbeta.c, qnbeta.c
//
// dnbeta.c: based on the algorithm that determines the largest term first,
//           then sums outward from the 'mid'.
// pnbeta.c: Algorithm AS 226 (Appl. Statist. 1987, Vol.36, No.2)
//           by Russell V. Lenth, with modifications AS R84, AS R95.
// qnbeta.c: Inversion of pnbeta via interval bisection.

use libm::*;

use crate::constants::*;
use crate::dpq::*;
use crate::error::*;
use crate::utils::*;

use super::beta::{dbeta_inner, pbeta_inner};
use crate::special::gamma::lgammafn;

// Constants
const DBL_EPSILON: f64 = 2.220446049250313e-16;
const DBL_MIN: f64 = 2.2250738585072014e-308;

// =====================================================================
// dnbeta
// =====================================================================

pub fn dnbeta_inner(x: f64, a: f64, b: f64, ncp: f64, log_p: bool) -> f64 {
    let eps = 1e-15;

    // IEEE_754
    if isnan(x) || isnan(a) || isnan(b) || isnan(ncp) {
        return x + a + b + ncp;
    }
    if ncp < 0.0 || a <= 0.0 || b <= 0.0 {
        return ml_warn_return_nan();
    }
    if !r_finite(a) || !r_finite(b) || !r_finite(ncp) {
        return ml_warn_return_nan();
    }

    if x < 0.0 || x > 1.0 {
        return r_d__0(log_p);
    }
    if ncp == 0.0 {
        return dbeta_inner(x, a, b, log_p);
    }

    // Non-central Beta: find the k that maximizes the k-th term, then sum outward
    let ncp2 = 0.5 * ncp; // ldexp(ncp, -1)
    let dx2 = ncp2 * x;
    let d = 0.5 * (dx2 - a - 1.0);
    let mut d_val = d * d + dx2 * (a + b) - a;

    let k_max = if d_val <= 0.0 {
        0_i32
    } else {
        d_val = ceil(d + sqrt(d_val));
        if d_val > 0.0 { d_val as i32 } else { 0 }
    };

    // Starting "middle term" -- first look at its log scale
    let term = dbeta_inner(x, a + k_max as f64, b, true);
    let mut p_k = dpois_raw(k_max as f64, ncp2, true);

    if x == 0.0 || !r_finite(term) || !r_finite(p_k) {
        // if term = +Inf
        return r_d_exp(p_k + term, log_p);
    }

    // Now sum from the inside out
    p_k += term; // = log(p_k) + log(t_k) == log(s_k)
    let mut sum = 1.0_f64; // = mid term (rescaled)
    let mut term = 1.0_f64;

    // middle to the left
    let mut k = k_max as f64;
    while k > 0.0 && term > sum * eps {
        k -= 1.0;
        let q = (k + 1.0) * (k + a) / (k + a + b) / dx2;
        term *= q;
        sum += term;
    }

    // middle to the right
    term = 1.0;
    k = k_max as f64;
    loop {
        let q = dx2 * (k + a + b) / (k + a) / (k + 1.0);
        k += 1.0;
        term *= q;
        sum += term;
        if !(term > sum * eps) {
            break;
        }
    }

    r_d_exp(p_k + log(sum), log_p)
}

/// dpois_raw: Poisson probability lb^x exp(-lb) / x!
fn dpois_raw(x: f64, lambda: f64, give_log: bool) -> f64 {
    crate::dist::gamma::dpois_raw(x, lambda, give_log)
}

// =====================================================================
// pnbeta (noncentral beta CDF)
// =====================================================================

/// pnbeta_raw: internal computation of the noncentral beta CDF
/// Ported from pnbeta.c -- pnbeta_raw
fn pnbeta_raw(x: f64, o_x: f64, a: f64, b: f64, ncp: f64) -> f64 {
    let errmax = 1.0e-9;
    let itrmax = 10000_i32;

    if ncp < 0.0 || a <= 0.0 || b <= 0.0 {
        return 0.0_f64 / 0.0_f64; // NaN
    }

    if x < 0.0 || o_x > 1.0 || (x == 0.0 && o_x == 1.0) {
        return 0.0;
    }
    if x > 1.0 || o_x < 0.0 || (x == 1.0 && o_x == 0.0) {
        return 1.0;
    }

    let c = ncp / 2.0;

    // initialize the series
    let x0 = floor(fmax2(c - 7.0 * sqrt(c), 0.0));
    let a0 = a + x0;
    let l_beta = lgammafn(a0) + lgammafn(b) - lgammafn(a0 + b);

    // pbeta_raw(x, a0, b, TRUE, FALSE) -- use our pbeta_inner
    let mut temp = pbeta_inner(x, a0, b, true, false);

    let mut gx =
        exp(a0 * log(x) + b * if x < 0.5 { log1p(-x) } else { log(o_x) } - l_beta - log(a0));

    let q = if a0 > a {
        // x0 >= 1
        exp(-c + x0 * log(c) - lgammafn(x0 + 1.0))
    } else {
        exp(-c)
    };

    let mut sumq = 1.0 - q;
    let mut ans = q * temp;
    let mut ax = ans;

    // recurse over subsequent terms until convergence
    let mut j = floor(x0);
    let mut errbd: f64;
    loop {
        j += 1.0;
        temp -= gx;
        gx *= x * (a + b + j - 1.0) / (a + j);
        let q_new = q * c / j;
        sumq -= q_new;
        ax = temp * q_new;
        ans += ax;
        errbd = (temp - gx) * sumq;

        if !(errbd > errmax && j < (itrmax as f64) + x0) {
            break;
        }
    }

    if errbd > errmax {
        ml_warning(ME_PRECISION, "pnbeta");
    }
    if j >= (itrmax as f64) + x0 {
        ml_warning(ME_NOCONV, "pnbeta");
    }

    ans
}

/// pnbeta2: pnbeta with o_x parameter (1 - x, for accuracy)
/// Ported from pnbeta.c -- pnbeta2
fn pnbeta2(x: f64, o_x: f64, a: f64, b: f64, ncp: f64, lower_tail: bool, log_p: bool) -> f64 {
    let ans = pnbeta_raw(x, o_x, a, b, ncp);

    if lower_tail {
        if log_p {
            if ans > 0.0 { log(ans) } else { ML_NEGINF }
        } else {
            ans
        }
    } else {
        if ans > 1.0 - 1e-10 {
            ml_warning(ME_PRECISION, "pnbeta");
        }
        if ans > 1.0 {
            return if log_p { r_d_lexp(0.0, true) } else { 0.0 };
        }
        if log_p { log1p(-ans) } else { 1.0 - ans }
    }
}

pub fn pnbeta_inner(x: f64, a: f64, b: f64, ncp: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(a) || isnan(b) || isnan(ncp) {
        return x + a + b + ncp;
    }

    // R_P_bounds_01(x, 0., 1.);
    if x < 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    if x == 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    if x > 1.0 {
        return r_dt_1(lower_tail, log_p);
    }
    if x == 1.0 {
        return r_dt_1(lower_tail, log_p);
    }

    pnbeta2(x, 1.0 - x, a, b, ncp, lower_tail, log_p)
}

// =====================================================================
// qnbeta
// =====================================================================

pub fn qnbeta_inner(p: f64, a: f64, b: f64, ncp: f64, lower_tail: bool, log_p: bool) -> f64 {
    let accu = 1e-15;
    let eps = 1e-14;

    // IEEE_754
    if isnan(p) || isnan(a) || isnan(b) || isnan(ncp) {
        return p + a + b + ncp;
    }
    if !r_finite(a) {
        return ml_warn_return_nan();
    }
    if ncp < 0.0 || a <= 0.0 || b <= 0.0 {
        return ml_warn_return_nan();
    }

    // R_Q_P01_boundaries(p, 0, 1);
    if log_p {
        if p > 0.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { 0.0 } else { 1.0 };
        }
        if p == ML_NEGINF {
            return if lower_tail { 1.0 } else { 0.0 };
        }
    } else {
        if p < 0.0 || p > 1.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { 0.0 } else { 1.0 };
        }
        if p == 1.0 {
            return if lower_tail { 1.0 } else { 0.0 };
        }
    }

    // R_DT_qIv(p)
    let p = r_dt_qiv(p, lower_tail, log_p);

    // Invert pnbeta(.):
    // 1. finding an upper and lower bound
    if p > 1.0 - DBL_EPSILON {
        return 1.0;
    }

    let mut pp = fmin2(1.0 - DBL_EPSILON, p * (1.0 + eps));
    let mut ux = 0.5;
    while ux < 1.0 - DBL_EPSILON && pnbeta_inner(ux, a, b, ncp, true, false) < pp {
        ux = 0.5 * (1.0 + ux);
    }

    pp = p * (1.0 - eps);
    let mut lx = 0.5;
    while lx > DBL_MIN && pnbeta_inner(lx, a, b, ncp, true, false) > pp {
        lx *= 0.5;
    }

    // 2. interval (lx, ux) halving
    loop {
        let nx = 0.5 * (lx + ux);
        if pnbeta_inner(nx, a, b, ncp, true, false) > p {
            ux = nx;
        } else {
            lx = nx;
        }
        if (ux - lx) / nx <= accu {
            break;
        }
    }

    0.5 * (ux + lx)
}

// =====================================================================
// FFI shims
// =====================================================================

#[unsafe(no_mangle)]
pub extern "C" fn Rf_dnbeta(x: f64, a: f64, b: f64, ncp: f64, give_log: i32) -> f64 {
    dnbeta_inner(x, a, b, ncp, give_log != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn dnbeta(x: f64, a: f64, b: f64, ncp: f64, give_log: i32) -> f64 {
    dnbeta_inner(x, a, b, ncp, give_log != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_pnbeta(x: f64, a: f64, b: f64, ncp: f64, lower_tail: i32, log_p: i32) -> f64 {
    pnbeta_inner(x, a, b, ncp, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn pnbeta(x: f64, a: f64, b: f64, ncp: f64, lower_tail: i32, log_p: i32) -> f64 {
    pnbeta_inner(x, a, b, ncp, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_qnbeta(p: f64, a: f64, b: f64, ncp: f64, lower_tail: i32, log_p: i32) -> f64 {
    qnbeta_inner(p, a, b, ncp, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn qnbeta(p: f64, a: f64, b: f64, ncp: f64, lower_tail: i32, log_p: i32) -> f64 {
    qnbeta_inner(p, a, b, ncp, lower_tail != 0, log_p != 0)
}
