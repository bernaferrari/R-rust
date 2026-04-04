#![allow(clippy::neg_cmp_op_on_partial_ord)]
// Gamma distribution: dgamma, pgamma, qgamma, rgamma
// Ported from dgamma.c, pgamma.c, qgamma.c, rgamma.c
//
// pgamma.c authors: Morten Welinder, Martin Maechler
// qgamma.c authors: Best & Roberts (AS 91), R Core Team
// rgamma.c authors: Ahrens & Dieter (algorithms GD and GS)
// dgamma.c author: Catherine Loader

use crate::constants::*;
use crate::dist::exponential::exp_rand;
use crate::dist::normal::dnorm4_inner;
use crate::dist::normal::norm_rand;
use crate::dist::normal::pnorm5_inner;
use crate::dist::normal::qnorm5_inner;
use crate::dpq::*;
use crate::error::*;
use crate::rng::*;
use crate::special::bd0::ebd0;
use crate::special::gamma::{lgammafn, lgammafn1p, log1pmx};
use crate::special::stirlerr::stirlerr;
use crate::utils::*;
use libm::*;

// Constants
pub(crate) const M_LN2: f64 = 0.693147180559945309417232121458;
pub(crate) const DBL_EPSILON: f64 = 2.220446049250313e-16;
pub(crate) const DBL_MIN: f64 = 2.2250738585072014e-308;
pub(crate) const DBL_MAX_EXP: i32 = 1024;
const M_LN_2PI: f64 = 1.837877066409345483560659472811;
const M_SQRT_2PI: f64 = 2.50662827463100050241576528481104525301;
const X_LRG: f64 = 2.86111748575702815380240589208115399625e+307; // = 2^1023 / pi

// =====================================================================
// Internal helper functions from pgamma.c
// =====================================================================

/// Scalefactor:= (2^32)^8 = 2^256 = 1.157921e+77
const SCALEFACTOR: f64 = {
    let s1: f64 = 4294967296.0;
    let s2 = s1 * s1;
    let s3 = s2 * s2;
    s3 * s3
};

/// If |x| > |k| * M_cutoff, then log[ exp(-x) * k^x ] =~= -x
const M_CUTOFF: f64 = M_LN2 * (DBL_MAX_EXP as f64) / DBL_EPSILON; // =3.196577e18

// logcf is imported from crate::special::gamma

/// Compute the log of a sum from logs of terms, i.e.,
///   log (exp (logx) + exp (logy))
#[inline]
pub(crate) fn logspace_add(logx: f64, logy: f64) -> f64 {
    fmax2(logx, logy) + log1p(exp(-fabs(logx - logy)))
}

/// dpois_wrap(x_plus_1, lambda, give_log) := dpois(x_plus_1 - 1, lambda);
/// where dpois(k, L) := exp(-L) L^k / gamma(k+1) {the usual Poisson probabilities}
/// and dpois*(.., give_log = TRUE) := log(dpois*(..))
#[inline]
fn dpois_wrap(x_plus_1: f64, lambda: f64, give_log: bool) -> f64 {
    if !r_finite(lambda) {
        return r_d__0(give_log);
    }
    if x_plus_1 > 1.0 {
        return dpois_raw(x_plus_1 - 1.0, lambda, give_log);
    }
    if lambda > fabs(x_plus_1 - 1.0) * M_CUTOFF {
        return r_d_exp(-lambda - lgammafn(x_plus_1), give_log);
    } else {
        let d = dpois_raw(x_plus_1, lambda, give_log);
        return if give_log {
            d + log(x_plus_1 / lambda)
        } else {
            d * (x_plus_1 / lambda)
        };
    }
}

/// dpois_raw: Poisson probability lb^x exp(-lb) / x!
/// Ported from dpois.c
pub(crate) fn dpois_raw(x: f64, lambda: f64, give_log: bool) -> f64 {
    if lambda == 0.0 {
        return if x == 0.0 {
            r_d__1(give_log)
        } else {
            r_d__0(give_log)
        };
    }
    if !r_finite(lambda) {
        return r_d__0(give_log);
    }
    if x < 0.0 {
        return r_d__0(give_log);
    }
    if x <= lambda * DBL_MIN {
        return r_d_exp(-lambda, give_log);
    }
    if lambda < x * DBL_MIN {
        if !r_finite(x) {
            return r_d__0(give_log);
        }
        return r_d_exp(-lambda + x * log(lambda) - lgammafn(x + 1.0), give_log);
    }
    let (yh, yl) = ebd0(x, lambda);
    let mut yl = yl;
    yl += stirlerr(x);
    let lrg_x = x >= X_LRG;
    let r = if lrg_x {
        M_SQRT_2PI * sqrt(x)
    } else {
        M_LN_2PI * x
    };
    if give_log {
        -yl - yh - (if lrg_x { log(r) } else { 0.5 * log(r) })
    } else {
        exp(-yl) * exp(-yh) / (if lrg_x { r } else { sqrt(r) })
    }
}

/// Abramowitz and Stegun 6.5.29 [right] -- for small x
fn pgamma_smallx(x: f64, alph: f64, lower_tail: bool, log_p: bool) -> f64 {
    let mut sum = 0.0;
    let mut c = alph;
    let mut n = 0.0_f64;
    let mut term: f64;

    // Relative to 6.5.29 all terms have been multiplied by alph
    // and the first, thus being 1, is omitted.
    loop {
        n += 1.0;
        c *= -x / n;
        term = c / (alph + n);
        sum += term;
        if !(fabs(term) > DBL_EPSILON * fabs(sum)) {
            break;
        }
    }

    if lower_tail {
        let f1 = if log_p { log1p(sum) } else { 1.0 + sum };
        let mut f2: f64;
        if alph > 1.0 {
            f2 = dpois_raw(alph, x, log_p);
            f2 = if log_p { f2 + x } else { f2 * exp(x) };
        } else if log_p {
            f2 = alph * log(x) - lgammafn1p(alph);
        } else {
            f2 = pow(x, alph) / exp(lgammafn1p(alph));
        }
        return if log_p { f1 + f2 } else { f1 * f2 };
    } else {
        let lf2 = alph * log(x) - lgammafn1p(alph);
        if log_p {
            return r_log1_exp(log1p(sum) + lf2);
        } else {
            let f1m1 = sum;
            let f2m1 = expm1(lf2);
            return -(f1m1 + f2m1 + f1m1 * f2m1);
        }
    }
}

fn pd_upper_series(x: f64, mut y: f64, log_p: bool) -> f64 {
    let mut term = x / y;
    let mut sum = term;

    loop {
        y += 1.0;
        term *= x / y;
        sum += term;
        if !(term > sum * DBL_EPSILON) {
            break;
        }
    }

    if log_p {
        log(sum)
    } else {
        sum
    }
}

/// Continued fraction for calculation of
/// scaled upper-tail F_gamma
/// ~= (y / d) * [1 + (1-y)/d + O(((1-y)/d)^2)]
fn pd_lower_cf(y: f64, d: f64) -> f64 {
    let max_it: i32 = 200000;

    if y == 0.0 {
        return 0.0;
    }

    let f0 = y / d;
    // Needed, e.g. for pgamma(10^c(100,295), shape=1.1, log=TRUE):
    if fabs(y - 1.0) < fabs(d) * DBL_EPSILON {
        return f0;
    }

    let f0 = if f0 > 1.0 { 1.0 } else { f0 };
    let mut c2 = y;
    let mut c4 = d; // original (y,d), *not* potentially scaled ones!
    let mut a1: f64 = 0.0;
    let mut b1: f64 = 1.0;
    let mut a2: f64 = y;
    let mut b2: f64 = d;

    // NEEDED_SCALE macro inlined
    while b2 > SCALEFACTOR {
        a1 /= SCALEFACTOR;
        b1 /= SCALEFACTOR;
        a2 /= SCALEFACTOR;
        b2 /= SCALEFACTOR;
    }

    let mut i: f64 = 0.0;
    let mut of = -1.0; // far away
    let mut f: f64 = 0.0;
    while (i as i32) < max_it {
        i += 1.0;
        c2 -= 1.0;
        let c3 = i * c2;
        c4 += 2.0;
        a1 = c4 * a2 + c3 * a1;
        b1 = c4 * b2 + c3 * b1;

        i += 1.0;
        c2 -= 1.0;
        let c3 = i * c2;
        c4 += 2.0;
        a2 = c4 * a1 + c3 * a2;
        b2 = c4 * b1 + c3 * b2;

        // NEEDED_SCALE
        if b2 > SCALEFACTOR {
            a1 /= SCALEFACTOR;
            b1 /= SCALEFACTOR;
            a2 /= SCALEFACTOR;
            b2 /= SCALEFACTOR;
        }

        if b2 != 0.0 {
            f = a2 / b2;
            // convergence check: relative; "absolute" for very small f:
            if fabs(f - of) <= DBL_EPSILON * fmax2(f0, fabs(f)) {
                return f;
            }
            of = f;
        }
    }

    // MATHLIB_WARNING -- should not happen
    f
}

fn pd_lower_series(lambda: f64, mut y: f64) -> f64 {
    let mut term = 1.0;
    let mut sum = 0.0;

    while y >= 1.0 && term > sum * DBL_EPSILON {
        term *= y / lambda;
        sum += term;
        y -= 1.0;
    }

    if y != floor(y) {
        // The series does not converge as the terms start getting bigger
        let f = pd_lower_cf(y, lambda + 1.0 - y);
        sum += term * f;
    }

    sum
}

/// Compute dnorm(x, 0, 1, FALSE) / pnorm(x, 0, 1, lower_tail, FALSE)
/// Abramowitz & Stegun 26.2.12
fn dpnorm(mut x: f64, mut lower_tail: bool, lp: f64) -> f64 {
    if x < 0.0 {
        x = -x;
        lower_tail = !lower_tail;
    }

    if x > 10.0 && !lower_tail {
        let mut term = 1.0 / x;
        let mut sum = term;
        let x2 = x * x;
        let mut i = 1.0_f64;

        loop {
            term *= -i / x2;
            sum += term;
            i += 2.0;
            if !(fabs(term) > DBL_EPSILON * sum) {
                break;
            }
        }

        1.0 / sum
    } else {
        let d = dnorm4_inner(x, 0.0, 1.0, false);
        d / exp(lp)
    }
}

/// Asymptotic expansion to calculate the probability that Poisson variate
/// has value <= x.
fn ppois_asymp(x: f64, lambda: f64, lower_tail: bool, log_p: bool) -> f64 {
    const COEFS_A: [f64; 8] = [
        -1e99, // placeholder used for 1-indexing
        2.0 / 3.0,
        -4.0 / 135.0,
        8.0 / 2835.0,
        16.0 / 8505.0,
        -8992.0 / 12629925.0,
        -334144.0 / 492567075.0,
        698752.0 / 1477701225.0,
    ];

    const COEFS_B: [f64; 8] = [
        -1e99, // placeholder
        1.0 / 12.0,
        1.0 / 288.0,
        -139.0 / 51840.0,
        -571.0 / 2488320.0,
        163879.0 / 209018880.0,
        5246819.0 / 75246796800.0,
        -534703531.0 / 902961561600.0,
    ];

    let dfm = lambda - x;
    let pt_ = -log1pmx(dfm / x);
    let mut s2pt = sqrt(2.0 * x * pt_);
    if dfm < 0.0 {
        s2pt = -s2pt;
    }

    let mut res12 = 0.0;
    let mut res1_ig = sqrt(x);
    let mut res1_term = res1_ig;
    let mut res2_ig = s2pt;
    let mut res2_term = res2_ig;
    for i in 1..8 {
        res12 += res1_ig * COEFS_A[i];
        res12 += res2_ig * COEFS_B[i];
        res1_term *= pt_ / (i as f64);
        res2_term *= 2.0 * pt_ / ((2 * i + 1) as f64);
        res1_ig = res1_ig / x + res1_term;
        res2_ig = res2_ig / x + res2_term;
    }

    let mut elfb = x;
    let mut elfb_term = 1.0;
    for i in 1..8 {
        elfb += elfb_term * COEFS_B[i];
        elfb_term /= x;
    }
    if !lower_tail {
        elfb = -elfb;
    }

    let f = res12 / elfb;

    let np = pnorm5_inner(s2pt, 0.0, 1.0, !lower_tail, log_p);

    if log_p {
        let n_d_over_p = dpnorm(s2pt, !lower_tail, np);
        return np + log1p(f * n_d_over_p);
    } else {
        let nd = dnorm4_inner(s2pt, 0.0, 1.0, log_p);
        return np + f * nd;
    }
}

/// pgamma_raw: internal pgamma assuming (x, alph) are not NA & alph > 0
pub(crate) fn pgamma_raw(x: f64, alph: f64, lower_tail: bool, log_p: bool) -> f64 {
    let res: f64;

    // R_P_bounds_01(x, 0., ML_POSINF)
    if x <= 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    if x >= ML_POSINF {
        return r_dt_1(lower_tail, log_p);
    }

    if x < 1.0 {
        res = pgamma_smallx(x, alph, lower_tail, log_p);
    } else if x <= alph - 1.0 && x < 0.8 * (alph + 50.0) {
        // incl. large alph compared to x
        let sum = pd_upper_series(x, alph, log_p);
        let d = dpois_wrap(alph, x, log_p);
        if !lower_tail {
            res = if log_p {
                r_log1_exp(d + sum)
            } else {
                1.0 - d * sum
            };
        } else {
            res = if log_p { sum + d } else { sum * d };
        }
    } else if alph - 1.0 < x && alph < 0.8 * (x + 50.0) {
        // incl. large x compared to alph
        let d = dpois_wrap(alph, x, log_p);
        let mut sum: f64;
        if alph < 1.0 {
            if x * DBL_EPSILON > 1.0 - alph {
                sum = r_d__1(log_p);
            } else {
                let f = pd_lower_cf(alph, x - (alph - 1.0)) * x / alph;
                sum = if log_p { log(f) } else { f };
            }
        } else {
            sum = pd_lower_series(x, alph - 1.0);
            sum = if log_p { log1p(sum) } else { 1.0 + sum };
        }
        if !lower_tail {
            res = if log_p { sum + d } else { sum * d };
        } else {
            res = if log_p {
                r_log1_exp(d + sum)
            } else {
                1.0 - d * sum
            };
        }
    } else {
        // x >= 1 and x fairly near alph.
        res = ppois_asymp(alph - 1.0, x, !lower_tail, log_p);
    }

    // We lose a fair amount of accuracy to underflow in the cases
    // where the final result is very close to DBL_MIN.
    // In those cases, simply redo via log space.
    if !log_p && res < DBL_MIN / DBL_EPSILON {
        return exp(pgamma_raw(x, alph, lower_tail, true));
    } else {
        res
    }
}

// =====================================================================
// dgamma
// =====================================================================

pub fn dgamma_inner(x: f64, shape: f64, scale: f64, give_log: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(shape) || isnan(scale) {
        return x + shape + scale;
    }
    if shape < 0.0 || scale <= 0.0 {
        return ml_warn_return_nan();
    }
    if x < 0.0 {
        return r_d__0(give_log);
    }
    if shape == 0.0 {
        // point mass at 0
        return if x == 0.0 {
            ML_POSINF
        } else {
            r_d__0(give_log)
        };
    }
    if x == 0.0 {
        if shape < 1.0 {
            return ML_POSINF;
        }
        if shape > 1.0 {
            return r_d__0(give_log);
        }
        // else
        return if give_log { -log(scale) } else { 1.0 / scale };
    }

    let pr: f64;
    if shape < 1.0 {
        pr = dpois_raw(shape, x / scale, give_log);
        return if give_log {
            // NB: currently *always* shape/x > 0 if shape < 1:
            // -- overflow to Inf happens, but underflow to 0 does NOT
            if r_finite(shape / x) {
                pr + log(shape / x)
            } else {
                // shape/x overflows to +Inf
                log(shape) - log(x)
            }
        } else {
            pr * shape / x
        };
    }
    // else shape >= 1
    pr = dpois_raw(shape - 1.0, x / scale, give_log);
    return if give_log {
        pr - log(scale)
    } else {
        pr / scale
    };
}

// =====================================================================
// pgamma
// =====================================================================

pub fn pgamma_inner(x: f64, alph: f64, scale: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(alph) || isnan(scale) {
        return x + alph + scale;
    }
    if alph < 0.0 || scale <= 0.0 {
        return ml_warn_return_nan();
    }
    let x = x / scale;
    // IEEE_754
    if isnan(x) {
        // eg. original x = scale = +Inf
        return x;
    }
    if alph == 0.0 {
        // limit case; useful e.g. in pnchisq()
        return if x <= 0.0 {
            r_dt_0(lower_tail, log_p)
        } else {
            r_dt_1(lower_tail, log_p)
        };
    }
    pgamma_raw(x, alph, lower_tail, log_p)
}

// =====================================================================
// qgamma
// =====================================================================

/// qchisq_appr: Starting approximation for chi-squared quantile.
/// Used internally by qgamma.
fn qchisq_appr(p: f64, nu: f64, g: f64, lower_tail: bool, log_p: bool, tol: f64) -> f64 {
    const C7: f64 = 4.67;
    const C8: f64 = 6.66;
    const C9: f64 = 6.73;
    const C10: f64 = 13.32;

    // test arguments and initialise
    // IEEE_754
    if isnan(p) || isnan(nu) {
        return p + nu;
    }

    // R_Q_P01_check(p)
    if (log_p && p > 0.0) || (!log_p && (p < 0.0 || p > 1.0)) {
        return ml_warn_return_nan();
    }
    if nu <= 0.0 {
        return ml_warn_return_nan();
    }

    let alpha = 0.5 * nu; // = [pq]gamma() shape
    let c = alpha - 1.0;

    if nu < (-1.24) * (r_dt_log(p, lower_tail, log_p)) {
        // for small chi-squared
        let lgam1pa = if alpha < 0.5 {
            lgammafn1p(alpha)
        } else {
            log(alpha) + g
        };
        let ch = exp((lgam1pa + r_dt_log(p, lower_tail, log_p)) / alpha + M_LN2);
        ch
    } else if nu > 0.32 {
        // using Wilson and Hilferty estimate
        let x = qnorm5_inner(p, 0.0, 1.0, lower_tail, log_p);
        let p1 = 2.0 / (9.0 * nu);
        let mut ch = nu * pow(x * sqrt(p1) + 1.0 - p1, 3.0);
        // approximation for p tending to 1:
        if ch > 2.2 * nu + 6.0 {
            ch = -2.0 * (r_dt_clog(p, lower_tail, log_p) - c * log(0.5 * ch) + g);
        }
        ch
    } else {
        // "small nu": 1.24*(-log(p)) <= nu <= 0.32
        let mut ch = 0.4;
        let a = r_dt_clog(p, lower_tail, log_p) + g + c * M_LN2;
        loop {
            let q = ch;
            let p1 = 1.0 / (1.0 + ch * (C7 + ch));
            let p2 = ch * (C9 + ch * (C8 + ch));
            let t = -0.5 + (C7 + 2.0 * ch) * p1 - (C9 + ch * (C10 + 3.0 * ch)) / p2;
            ch -= (1.0 - exp(a + 0.5 * ch) * p2 * p1) / t;
            if !(fabs(q - ch) > tol * fabs(ch)) {
                break;
            }
        }
        ch
    }
}

pub fn qgamma_inner(p: f64, alpha: f64, scale: f64, lower_tail: bool, log_p: bool) -> f64 {
    const EPS1: f64 = 1e-2;
    const EPS2: f64 = 5e-7; // final precision of AS 91
    const EPS_N: f64 = 1e-15; // precision of Newton step / iterations
    const MAXIT: i32 = 1000;
    const PMIN: f64 = 1e-100;
    const PMAX: f64 = 1.0 - 1e-14;

    const I420: f64 = 1.0 / 420.0;
    const I2520: f64 = 1.0 / 2520.0;
    const I5040: f64 = 1.0 / 5040.0;

    // test arguments and initialise
    // IEEE_754
    if isnan(p) || isnan(alpha) || isnan(scale) {
        return p + alpha + scale;
    }

    // R_Q_P01_boundaries(p, 0., ML_POSINF)
    if log_p {
        if p > 0.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { ML_POSINF } else { 0.0 };
        }
        if p == ML_NEGINF {
            return if lower_tail { 0.0 } else { ML_POSINF };
        }
    } else {
        if p < 0.0 || p > 1.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { 0.0 } else { ML_POSINF };
        }
        if p == 1.0 {
            return if lower_tail { ML_POSINF } else { 0.0 };
        }
    }

    if alpha < 0.0 || scale <= 0.0 {
        return ml_warn_return_nan();
    }
    if alpha == 0.0 {
        return 0.0;
    } // all mass at 0

    let mut max_it_newton: i32 = 1;
    if alpha < 1e-10 {
        max_it_newton = 7;
    }

    let p_ = r_dt_qiv(p, lower_tail, log_p); // lower_tail prob (in any case)
    let g = lgammafn(alpha); // log Gamma(v/2)

    //----- Phase I : Starting Approximation
    let mut ch = qchisq_appr(p, 2.0 * alpha, g, lower_tail, log_p, EPS1);
    if !r_finite(ch) {
        max_it_newton = 0;
    }
    if r_finite(ch) && ch < EPS2 {
        max_it_newton = 20;
    }

    if r_finite(ch) && ch >= EPS2 {
        if p_ > PMAX || p_ < PMIN {
            max_it_newton = 20;
        } else {
            //----- Phase II: Iteration
            // Call pgamma() [AS 239] and calculate seven term taylor series
            let c = alpha - 1.0;
            let s6 = (120.0 + c * (346.0 + 127.0 * c)) * I5040; // used below, is "const"

            let ch0 = ch; // save initial approx.
            let mut converged = false;
            for _i in 1..=MAXIT {
                let q = ch;
                let p1 = 0.5 * ch;
                let p2 = p_ - pgamma_raw(p1, alpha, true, false);

                if !r_finite(p2) || ch <= 0.0 {
                    ch = ch0;
                    max_it_newton = 27;
                    converged = true;
                    break;
                }

                let t = p2 * exp(alpha * M_LN2 + g + p1 - c * log(ch));
                let b = t / ch;
                let a = 0.5 * t - b * c;
                let s1 =
                    (210.0 + a * (140.0 + a * (105.0 + a * (84.0 + a * (70.0 + 60.0 * a))))) * I420;
                let s2 = (420.0 + a * (735.0 + a * (966.0 + a * (1141.0 + 1278.0 * a)))) * I2520;
                let s3 = (210.0 + a * (462.0 + a * (707.0 + 932.0 * a))) * I2520;
                let s4 =
                    (252.0 + a * (672.0 + 1182.0 * a) + c * (294.0 + a * (889.0 + 1740.0 * a)))
                        * I5040;
                let s5 = (84.0 + 2264.0 * a + c * (1175.0 + 606.0 * a)) * I2520;

                ch += t
                    * (1.0 + 0.5 * t * s1
                        - b * c * (s1 - b * (s2 - b * (s3 - b * (s4 - b * (s5 - b * s6))))));
                if fabs(q - ch) < EPS2 * ch {
                    converged = true;
                    break;
                }
                if fabs(q - ch) > 0.1 * ch {
                    // diverging? -- also forces ch > 0
                    if ch < q {
                        ch = 0.9 * q;
                    } else {
                        ch = 1.1 * q;
                    }
                }
            }
            let _ = converged; // suppress unused warning
        }
    }

    // END: Newton refinement
    let mut x = 0.5 * scale * ch;
    if max_it_newton > 0 {
        // always use log scale
        let mut log_p = log_p;
        let mut p = p;
        if !log_p {
            p = log(p);
            log_p = true;
        }
        if x == 0.0 {
            let _1_p = 1.0 + 1e-7;
            let _1_m = 1.0 - 1e-7;
            x = DBL_MIN;
            let p_ = pgamma_inner(x, alpha, scale, lower_tail, log_p);
            if (lower_tail && p_ > p * _1_p) || (!lower_tail && p_ < p * _1_m) {
                return 0.0;
            }
            // else: continue, using x = DBL_MIN instead of 0
            let mut p_ = pgamma_inner(x, alpha, scale, lower_tail, log_p);
            if p_ == ML_NEGINF {
                return 0.0;
            }
            for _i in 1..=max_it_newton {
                let p1 = p_ - p;
                if fabs(p1) < fabs(EPS_N * p) {
                    break;
                }
                let dg = dgamma_inner(x, alpha, scale, log_p);
                if dg == r_d__0(log_p) {
                    break;
                }
                // delta x = f(x)/f'(x);
                let t = if log_p { p1 * exp(p_ - dg) } else { p1 / dg };
                let t = if lower_tail { x - t } else { x + t };
                p_ = pgamma_inner(t, alpha, scale, lower_tail, log_p);
                if fabs(p_ - p) > fabs(p1) || (false/* i > 1 && fabs(p_ - p) == fabs(p1) */) {
                    // no improvement
                    break;
                }
                x = t;
            }
        } else {
            let mut p_ = pgamma_inner(x, alpha, scale, lower_tail, log_p);
            if p_ == ML_NEGINF {
                return 0.0;
            }
            for _i in 1..=max_it_newton {
                let p1 = p_ - p;
                if fabs(p1) < fabs(EPS_N * p) {
                    break;
                }
                let dg = dgamma_inner(x, alpha, scale, log_p);
                if dg == r_d__0(log_p) {
                    break;
                }
                let t = if log_p { p1 * exp(p_ - dg) } else { p1 / dg };
                let t = if lower_tail { x - t } else { x + t };
                p_ = pgamma_inner(t, alpha, scale, lower_tail, log_p);
                if fabs(p_ - p) > fabs(p1) || (false/* i > 1 && fabs(p_ - p) == fabs(p1) */) {
                    break;
                }
                x = t;
            }
        }
    }

    x
}

// =====================================================================
// rgamma
// =====================================================================

use std::cell::Cell;

thread_local! {
    static RG_AA: Cell<f64> = Cell::new(0.0);
    static RG_AAA: Cell<f64> = Cell::new(0.0);
    static RG_S: Cell<f64> = Cell::new(0.0);
    static RG_S2: Cell<f64> = Cell::new(0.0);
    static RG_D: Cell<f64> = Cell::new(0.0);
    static RG_Q0: Cell<f64> = Cell::new(0.0);
    static RG_B: Cell<f64> = Cell::new(0.0);
    static RG_SI: Cell<f64> = Cell::new(0.0);
    static RG_C: Cell<f64> = Cell::new(0.0);
}

pub fn rgamma_inner(a: f64, scale: f64) -> f64 {
    // Constants
    const SQRT32: f64 = 5.656854;
    const EXP_M1: f64 = 0.36787944117144232159; // exp(-1) = 1/e

    // Coefficients q[k]
    const Q1: f64 = 0.04166669;
    const Q2: f64 = 0.02083148;
    const Q3: f64 = 0.00801191;
    const Q4: f64 = 0.00144121;
    const Q5: f64 = -7.388e-5;
    const Q6: f64 = 2.4511e-4;
    const Q7: f64 = 2.424e-4;

    // Coefficients a[k]
    const A1: f64 = 0.3333333;
    const A2: f64 = -0.250003;
    const A3: f64 = 0.2000062;
    const A4: f64 = -0.1662921;
    const A5: f64 = 0.1423657;
    const A6: f64 = -0.1367177;
    const A7: f64 = 0.1233795;

    if isnan(a) || isnan(scale) {
        return ml_warn_return_nan();
    }
    if a <= 0.0 || scale <= 0.0 {
        if scale == 0.0 || a == 0.0 {
            return 0.0;
        }
        return ml_warn_return_nan();
    }
    if !r_finite(a) || !r_finite(scale) {
        return ML_POSINF;
    }

    if a < 1.0 {
        // GS algorithm for parameters a < 1
        let e = 1.0 + EXP_M1 * a;
        loop {
            let p = e * unif_rand();
            if p >= 1.0 {
                let x = -log((e - p) / a);
                if exp_rand() >= (1.0 - a) * log(x) {
                    return scale * x;
                }
            } else {
                // p < 1 <==> log(p) < 0
                let x = exp(log(p) / a);
                if exp_rand() >= x {
                    return scale * x;
                }
            }
        }
    }

    // --- a >= 1 : GD algorithm ---

    // Step 1: Recalculations of s2, s, d if a has changed
    RG_AA.with(|aa| {
        if a != aa.get() {
            aa.set(a);
            let s2 = a - 0.5;
            let s = sqrt(s2);
            let d = SQRT32 - s * 12.0;
            RG_S2.with(|v| v.set(s2));
            RG_S.with(|v| v.set(s));
            RG_D.with(|v| v.set(d));
        }
    });

    // Step 2: t = standard normal deviate,
    //         x = (s,1/2) -normal deviate.
    // immediate acceptance (i)
    let t = norm_rand();
    let mut x = RG_S.with(|v| v.get()) + 0.5 * t;
    let ret_val = x * x;
    if t >= 0.0 {
        return scale * ret_val;
    }

    // Step 3: u = 0,1 - uniform sample. squeeze acceptance (s)
    let u = unif_rand();
    if RG_D.with(|v| v.get()) * u <= t * t * t {
        return scale * ret_val;
    }

    // Step 4: recalculations of q0, b, si, c if necessary
    RG_AAA.with(|aaa| {
        if a != aaa.get() {
            aaa.set(a);
            let r = 1.0 / a;
            let q0 = ((((((Q7 * r + Q6) * r + Q5) * r + Q4) * r + Q3) * r + Q2) * r + Q1) * r;

            let s2 = RG_S2.with(|v| v.get());
            let s = RG_S.with(|v| v.get());

            let (b, si, c) = if a <= 3.686 {
                (0.463 + s + 0.178 * s2, 1.235, 0.195 / s - 0.079 + 0.16 * s)
            } else if a <= 13.022 {
                (1.654 + 0.0076 * s2, 1.68 / s + 0.275, 0.062 / s + 0.024)
            } else {
                (1.77, 0.75, 0.1515 / s)
            };

            RG_Q0.with(|v| v.set(q0));
            RG_B.with(|v| v.set(b));
            RG_SI.with(|v| v.set(si));
            RG_C.with(|v| v.set(c));
        }
    });

    let q0 = RG_Q0.with(|v| v.get());
    let s = RG_S.with(|v| v.get());
    let s2 = RG_S2.with(|v| v.get());
    let b_val = RG_B.with(|v| v.get());
    let si = RG_SI.with(|v| v.get());
    let c_val = RG_C.with(|v| v.get());

    // Step 5: no quotient test if x not positive
    if x > 0.0 {
        // Step 6: calculation of v and quotient q
        let v = t / (s + s);
        let q = if fabs(v) <= 0.25 {
            q0 + 0.5
                * t
                * t
                * ((((((A7 * v + A6) * v + A5) * v + A4) * v + A3) * v + A2) * v + A1)
                * v
        } else {
            q0 - s * t + 0.25 * t * t + (s2 + s2) * log(1.0 + v)
        };

        // Step 7: quotient acceptance (q)
        if log(1.0 - u) <= q {
            return scale * ret_val;
        }
    }

    loop {
        // Step 8: e = standard exponential deviate
        //         u = 0,1 - uniform deviate
        //         t = (b,si)-double exponential (laplace) sample
        let e = exp_rand();
        let mut u = unif_rand();
        u = u + u - 1.0;
        let t = if u < 0.0 {
            b_val - si * e
        } else {
            b_val + si * e
        };
        // Step 9: rejection if t < tau(1) = -0.71874483771719
        if t >= -0.71874483771719 {
            // Step 10: calculation of v and quotient q
            let v = t / (s + s);
            let q = if fabs(v) <= 0.25 {
                q0 + 0.5
                    * t
                    * t
                    * ((((((A7 * v + A6) * v + A5) * v + A4) * v + A3) * v + A2) * v + A1)
                    * v
            } else {
                q0 - s * t + 0.25 * t * t + (s2 + s2) * log(1.0 + v)
            };
            // Step 11: hat acceptance (h)
            if q > 0.0 {
                let w = expm1(q);
                if c_val * fabs(u) <= w * exp(e - 0.5 * t * t) {
                    x = s + 0.5 * t;
                    return scale * x * x;
                }
            }
        }
    }
}

// =====================================================================
// FFI shims
// =====================================================================

#[unsafe(no_mangle)]
pub extern "C" fn Rf_dgamma(x: f64, shape: f64, scale: f64, give_log: i32) -> f64 {
    dgamma_inner(x, shape, scale, give_log != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn dgamma(x: f64, shape: f64, scale: f64, give_log: i32) -> f64 {
    dgamma_inner(x, shape, scale, give_log != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_pgamma(x: f64, shape: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    pgamma_inner(x, shape, scale, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn pgamma(x: f64, shape: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    pgamma_inner(x, shape, scale, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_qgamma(p: f64, shape: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    qgamma_inner(p, shape, scale, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn qgamma(p: f64, shape: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    qgamma_inner(p, shape, scale, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_rgamma(shape: f64, scale: f64) -> f64 {
    rgamma_inner(shape, scale)
}

#[unsafe(no_mangle)]
pub extern "C" fn rgamma(shape: f64, scale: f64) -> f64 {
    rgamma_inner(shape, scale)
}
