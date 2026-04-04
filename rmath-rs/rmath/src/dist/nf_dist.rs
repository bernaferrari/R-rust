// Noncentral F distribution: dnf, pnf, qnf, rnf
// Ported from dnf.c, pnf.c, qnf.c, rnf.c
//
// dnf.c author: Peter Ruckdeschel
// pnf.c, qnf.c: R Core Team
// rnf: based on rnchisq (Kuensch suggestion)

use libm::*;

use crate::constants::*;
use crate::dpq::*;
use crate::error::*;
use crate::utils::*;

use super::chisq::{pchisq_inner, qchisq_inner, rchisq_inner};
use super::gamma::{dgamma_inner, rgamma_inner};
use super::poisson::rpois_inner;
use crate::dist::gamma::logspace_add;
use crate::special::gamma::lgammafn;

// Constants
const DBL_EPSILON: f64 = 2.220446049250313e-16;
const DBL_MAX: f64 = 1.7976931348623157e+308;
const DBL_MIN: f64 = 2.2250738585072014e-308;

// =====================================================================
// Noncentral chi-squared helpers (needed by nf_dist)
// Ported from dnchisq.c, pnchisq.c, qnchisq.c, rnchisq.c
// =====================================================================

/// dpois_raw: Poisson probability lb^x exp(-lb) / x!
/// Inline version needed by dnchisq
fn dpois_raw(x: f64, lambda: f64, give_log: bool) -> f64 {
    crate::dist::gamma::dpois_raw(x, lambda, give_log)
}

/// dchisq: density of chi-squared distribution
fn dchisq(x: f64, df: f64, give_log: bool) -> f64 {
    dgamma_inner(x, df / 2.0, 2.0, give_log)
}

/// Noncentral chi-squared density
/// Ported from dnchisq.c
fn dnchisq(x: f64, df: f64, ncp: f64, give_log: bool) -> f64 {
    let eps = 5e-15;

    // IEEE_754
    if isnan(x) || isnan(df) || isnan(ncp) {
        return x + df + ncp;
    }

    if !r_finite(df) || !r_finite(ncp) || ncp < 0.0 || df < 0.0 {
        return ml_warn_return_nan();
    }

    if x < 0.0 {
        return r_d__0(give_log);
    }
    if x == 0.0 && df < 2.0 {
        return ML_POSINF;
    }
    if ncp == 0.0 {
        return if df > 0.0 {
            dchisq(x, df, give_log)
        } else {
            r_d__0(give_log)
        };
    }
    if x == ML_POSINF {
        return r_d__0(give_log);
    }

    let ncp2 = 0.5 * ncp;

    // find max element of sum
    let mut imax = ceil((-(2.0 + df) + sqrt((2.0 - df) * (2.0 - df) + 4.0 * ncp * x)) / 4.0);
    if imax < 0.0 {
        imax = 0.0;
    }

    let (mid, dfmid) = if r_finite(imax) {
        let dfmid = df + 2.0 * imax;
        let mid = dpois_raw(imax, ncp2, false) * dchisq(x, dfmid, false);
        (mid, dfmid)
    } else {
        (0.0, 0.0)
    };

    if mid == 0.0 {
        if give_log || ncp > 1000.0 {
            let nl = df + ncp;
            let ic = nl / (nl + ncp);
            return dchisq(x * ic, nl * ic, give_log);
        } else {
            return r_d__0(give_log);
        }
    }

    let mut sum = mid;
    let mut term = mid;
    let mut df_v = dfmid;
    let mut i = imax;
    let x2 = x * ncp2;

    // upper tail
    loop {
        i += 1.0;
        let q = x2 / i / df_v;
        df_v += 2.0;
        term *= q;
        sum += term;
        if !(q >= 1.0 || term * q > (1.0 - q) * eps || term > 1e-10 * sum) {
            break;
        }
    }

    // lower tail
    term = mid;
    df_v = dfmid;
    i = imax;
    while i != 0.0 {
        df_v -= 2.0;
        let q = i * df_v / x2;
        i -= 1.0;
        term *= q;
        sum += term;
        if q < 1.0 && term * q <= (1.0 - q) * eps {
            break;
        }
    }

    r_d_val(sum, give_log)
}

/// Noncentral chi-squared probability (raw, used internally)
/// Ported from pnchisq.c -- pnchisq_raw
fn pnchisq_raw(
    x: f64,
    f: f64,
    theta: f64,
    errmax: f64,
    reltol: f64,
    itrmax: i32,
    lower_tail: bool,
    log_p: bool,
) -> f64 {
    let _dbl_min_exp = 0.693147180559945309417232121458 * (-1022.0_f64); // M_LN2 * DBL_MIN_EXP

    if x <= 0.0 {
        if x == 0.0 && f == 0.0 {
            let l_val = -0.5 * theta;
            return if lower_tail {
                r_d_exp(l_val, log_p)
            } else {
                r_d_lexp(l_val, log_p)
            };
        }
        return r_dt_0(lower_tail, log_p);
    }
    if !r_finite(x) {
        return r_dt_1(lower_tail, log_p);
    }

    if theta < 80.0 {
        // theta < 80: use Poisson mixture of central chi-squared
        let lambda = 0.5 * theta;

        if lower_tail
            && f > 0.0
            && log(x)
                < 0.693147180559945309417232121458
                    + 2.0 / f * (lgammafn(f / 2.0 + 1.0) + _dbl_min_exp)
        {
            // log-space computation
            let mut sum = ML_NEGINF;
            let mut sum2 = ML_NEGINF;
            let mut pr = -lambda;
            let log_lam = log(lambda);

            for i in 1..=110_i32 {
                let i_f = i as f64;
                pr += log_lam - log(i_f);
                sum2 = logspace_add(sum2, pr);
                sum = logspace_add(sum, pr + pchisq_inner(x, f + 2.0 * i_f, true, true));
                if sum2 >= -1e-15 {
                    break;
                }
            }
            let ans = sum - sum2;
            return if log_p { ans } else { exp(ans) };
        } else {
            let mut sum: f64 = 0.0;
            let mut sum2: f64 = 0.0;
            let mut pr = exp(-lambda);

            for i in 1..=110_i32 {
                let i_f = i as f64;
                pr *= lambda / i_f;
                sum2 += pr;
                sum += pr * pchisq_inner(x, f + 2.0 * i_f, lower_tail, false);
                if sum2 >= 1.0 - 1e-15 {
                    break;
                }
            }
            let ans = sum / sum2;
            return if log_p { log(ans) } else { ans };
        }
    }

    // else: theta >= 80 -- series expansion
    let lam = 0.5 * theta;
    let lam_sml = -lam < _dbl_min_exp;

    let (mut u, mut lu, l_lam);
    if lam_sml {
        u = 0.0;
        lu = -lam;
        l_lam = log(lam);
    } else {
        u = exp(-lam);
        lu = -lam; // not used
        l_lam = log(lam);
    }

    let x2 = 0.5 * x;
    let f2 = 0.5 * f;

    let mut lt = {
        let t_val = x2 - f2;
        if f2 * DBL_EPSILON > 0.125 && fabs(t_val) < sqrt(DBL_EPSILON) * f2 {
            (1.0 - t_val) * (2.0 - t_val / (f2 + 1.0))
                - 0.918938533204672741780329736406 // M_LN_SQRT_2PI
                - 0.5 * log(f2 + 1.0)
        } else {
            f2 * log(x2) - x2 - lgammafn(f2 + 1.0)
        }
    };

    let t_sml = lt < _dbl_min_exp;

    let (mut ans, mut term, mut t);
    let l_x;
    if t_sml {
        l_x = log(x);
        ans = 0.0;
        term = 0.0;
        t = 0.0;
    } else {
        l_x = 0.0; // not used when !t_sml
        t = exp(lt);
        ans = if lam_sml { 0.0 } else { u * t };
        term = ans;
    }

    let mut f_2n = f + 2.0;
    let mut f_x_2n = f - x + 2.0;

    for n in 1..=itrmax {
        let n_f = n as f64;

        // convergence check
        if f_x_2n > 0.0 {
            let bound = t * x / f_x_2n;
            let is_b = bound <= errmax;
            let is_r = term <= reltol * ans;
            if is_b && is_r {
                break;
            }
        }

        // update u (Poisson)
        let mut v: f64 = 0.0;
        if lam_sml {
            lu += l_lam - log(n_f);
            if lu >= _dbl_min_exp {
                v = exp(lu);
                u = v;
                // Note: lam_sml should be set to false here but since we can't
                // mutate it in a simple way, we handle both paths
            }
        } else {
            u *= lam / n_f;
            v = u;
        }

        // update t (chi-squared)
        if t_sml {
            lt += l_x - log(f_2n);
            if lt >= _dbl_min_exp {
                t = exp(lt);
                // t_sml should be set to false here
            }
        } else {
            t *= x / f_2n;
        }

        if (!lam_sml || lu >= _dbl_min_exp) && (!t_sml || lt >= _dbl_min_exp) {
            term = v * t;
            ans += term;
        }

        f_2n += 2.0;
        f_x_2n += 2.0;
    }

    r_dt_val(ans, lower_tail, log_p)
}

/// Noncentral chi-squared probability (public wrapper)
fn pnchisq(x: f64, df: f64, ncp: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(df) || isnan(ncp) {
        return x + df + ncp;
    }
    if !r_finite(df) || !r_finite(ncp) {
        return ml_warn_return_nan();
    }
    if df < 0.0 || ncp < 0.0 {
        return ml_warn_return_nan();
    }

    let ans = pnchisq_raw(
        x,
        df,
        ncp,
        1e-12,
        8.0 * DBL_EPSILON,
        1000000,
        lower_tail,
        log_p,
    );

    if x <= 0.0 || x == ML_POSINF {
        return ans;
    }

    if ncp >= 80.0 {
        if lower_tail {
            return fmin2(ans, r_d__1(log_p));
        } else {
            if ans
                < (if log_p {
                    -10.0 * 2.302585092994045684017991454684
                } else {
                    1e-10
                })
            {
                ml_warning(ME_PRECISION, "pnchisq");
            }
            if !log_p && ans < 0.0 {
                return 0.0;
            }
        }
    }

    if !log_p || ans < -1e-8 {
        ans
    } else {
        // log_p && ans close to 0, use other tail for accuracy
        let ans2 = pnchisq_raw(
            x,
            df,
            ncp,
            1e-12,
            8.0 * DBL_EPSILON,
            1000000,
            !lower_tail,
            false,
        );
        log1p(-ans2)
    }
}

/// Noncentral chi-squared quantile
/// Ported from qnchisq.c
fn qnchisq(p: f64, df: f64, ncp: f64, lower_tail: bool, log_p: bool) -> f64 {
    let accu = 1e-13;
    let _racc = 4.0 * DBL_EPSILON;
    let eps = 1e-11;

    // IEEE_754
    if isnan(p) || isnan(df) || isnan(ncp) {
        return p + df + ncp;
    }
    if !r_finite(df) {
        return ml_warn_return_nan();
    }
    if df < 0.0 || ncp < 0.0 {
        return ml_warn_return_nan();
    }

    // R_Q_P01_boundaries(p, 0, ML_POSINF);
    if log_p {
        if p > 0.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { 0.0 } else { ML_POSINF };
        }
        if p == ML_NEGINF {
            return if lower_tail { ML_POSINF } else { 0.0 };
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

    let pp = r_d_qiv(p, log_p);
    if pp > 1.0 - DBL_EPSILON {
        return if lower_tail { ML_POSINF } else { 0.0 };
    }

    // Pearson's (1959) approximation for initial value
    let b = (ncp * ncp) / (df + 3.0 * ncp);
    let c = (df + 3.0 * ncp) / (df + 2.0 * ncp);
    let ff = (df + 2.0 * ncp) / (c * c);
    let mut ux = b + c * qchisq_inner(p, ff, lower_tail, log_p);
    if ux <= 0.0 {
        ux = 1.0;
    }
    let ux0 = ux;

    let mut lower_tail = lower_tail;
    let mut p = p;

    if !lower_tail && ncp >= 80.0 {
        if pp < 1e-10 {
            ml_warning(ME_PRECISION, "qnchisq");
        }
        p = if log_p { -expm1(p) } else { 0.5 - p + 0.5 };
        lower_tail = true;
    } else {
        p = pp;
    }

    let mut pp = fmin2(1.0 - DBL_EPSILON, p * (1.0 + eps));
    let mut lx;

    if lower_tail {
        while ux < DBL_MAX && pnchisq_raw(ux, df, ncp, eps, 1e-10, 10000, true, false) < pp {
            ux *= 2.0;
        }
        pp = p * (1.0 - eps);
        lx = fmin2(ux0, DBL_MAX);
        while lx > DBL_MIN && pnchisq_raw(lx, df, ncp, eps, 1e-10, 10000, true, false) > pp {
            lx *= 0.5;
        }
    } else {
        while ux < DBL_MAX && pnchisq_raw(ux, df, ncp, eps, 1e-10, 10000, false, false) > pp {
            ux *= 2.0;
        }
        pp = p * (1.0 - eps);
        lx = fmin2(ux0, DBL_MAX);
        while lx > DBL_MIN && pnchisq_raw(lx, df, ncp, eps, 1e-10, 10000, false, false) < pp {
            lx *= 0.5;
        }
    }

    // interval bisection
    loop {
        let nx = 0.5 * (lx + ux);
        let cmp = pnchisq_raw(
            nx,
            df,
            ncp,
            accu,
            4.0 * DBL_EPSILON,
            100000,
            lower_tail,
            false,
        );
        if lower_tail {
            if cmp > p {
                ux = nx;
            } else {
                lx = nx;
            }
        } else {
            if cmp < p {
                ux = nx;
            } else {
                lx = nx;
            }
        }
        if (ux - lx) / nx <= accu {
            break;
        }
    }

    0.5 * (ux + lx)
}

/// Random variates from noncentral chi-squared distribution
/// Ported from rnchisq.c
fn rnchisq(df: f64, lambda: f64) -> f64 {
    if isnan(df) || !r_finite(lambda) || df < 0.0 || lambda < 0.0 {
        return ml_warn_return_nan();
    }

    if lambda == 0.0 {
        return if df == 0.0 {
            0.0
        } else {
            rgamma_inner(df / 2.0, 2.0)
        };
    } else {
        let mut r = rpois_inner(lambda / 2.0);
        if r > 0.0 {
            r = rchisq_inner(2.0 * r);
        }
        if df > 0.0 {
            r += rgamma_inner(df / 2.0, 2.0);
        }
        r
    }
}

// =====================================================================
// dnf
// =====================================================================

pub fn dnf_inner(x: f64, df1: f64, df2: f64, ncp: f64, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(df1) || isnan(df2) || isnan(ncp) {
        return x + df2 + df1 + ncp;
    }

    if df1 <= 0.0 || df2 <= 0.0 || ncp < 0.0 {
        return ml_warn_return_nan();
    }
    if x < 0.0 {
        return r_d__0(log_p);
    }
    if !r_finite(ncp) {
        return ml_warn_return_nan();
    }

    if !r_finite(df1) && !r_finite(df2) {
        // both +Inf
        if x == 1.0 {
            return ML_POSINF;
        } else {
            return r_d__0(log_p);
        }
    }
    if !r_finite(df2) {
        // df2 = +Inf
        return df1 * dnchisq(x * df1, df1, ncp, log_p);
    }
    if df1 > 1e14 && ncp < 1e7 {
        let f = 1.0 + ncp / df1;
        let z = dgamma_inner(1.0 / x / f, df2 / 2.0, 2.0 / df2, log_p);
        return if log_p {
            z - 2.0 * log(x) - log(f)
        } else {
            z / (x * x) / f
        };
    }

    let y = (df1 / df2) * x;
    let z = super::nbeta::dnbeta_inner(y / (1.0 + y), df1 / 2.0, df2 / 2.0, ncp, log_p);
    if log_p {
        z + log(df1) - log(df2) - 2.0 * log1p(y)
    } else {
        z * (df1 / df2) / (1.0 + y) / (1.0 + y)
    }
}

// =====================================================================
// pnf
// =====================================================================

pub fn pnf_inner(x: f64, df1: f64, df2: f64, ncp: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(df1) || isnan(df2) || isnan(ncp) {
        return x + df2 + df1 + ncp;
    }
    if df1 <= 0.0 || df2 <= 0.0 || ncp < 0.0 {
        return ml_warn_return_nan();
    }
    if !r_finite(ncp) {
        return ml_warn_return_nan();
    }
    if !r_finite(df1) && !r_finite(df2) {
        return ml_warn_return_nan();
    }

    // R_P_bounds_01(x, 0., ML_POSINF);
    if x < 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    if x == 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    if !r_finite(x) {
        return r_dt_1(lower_tail, log_p);
    }

    if df2 > 1e8 {
        return pnchisq(x * df1, df1, ncp, lower_tail, log_p);
    }

    let y = (df1 / df2) * x;
    super::nbeta::pnbeta_inner(y / (1.0 + y), df1 / 2.0, df2 / 2.0, ncp, lower_tail, log_p)
}

// =====================================================================
// qnf
// =====================================================================

pub fn qnf_inner(p: f64, df1: f64, df2: f64, ncp: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(p) || isnan(df1) || isnan(df2) || isnan(ncp) {
        return p + df1 + df2 + ncp;
    }
    if df1 <= 0.0 || df2 <= 0.0 || ncp < 0.0 {
        return ml_warn_return_nan();
    }
    if !r_finite(ncp) {
        return ml_warn_return_nan();
    }
    if !r_finite(df1) && !r_finite(df2) {
        return ml_warn_return_nan();
    }

    // R_Q_P01_boundaries(p, 0, ML_POSINF);
    if log_p {
        if p > 0.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { 0.0 } else { ML_POSINF };
        }
        if p == ML_NEGINF {
            return if lower_tail { ML_POSINF } else { 0.0 };
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

    if df2 > 1e8 {
        return qnchisq(p, df1, ncp, lower_tail, log_p) / df1;
    }

    let y = super::nbeta::qnbeta_inner(p, df1 / 2.0, df2 / 2.0, ncp, lower_tail, log_p);
    y / (1.0 - y) * (df2 / df1)
}

// =====================================================================
// rnf
// =====================================================================

pub fn rnf_inner(df1: f64, df2: f64, ncp: f64) -> f64 {
    if isnan(df1) || isnan(df2) || isnan(ncp) {
        return ml_warn_return_nan();
    }
    if df1 <= 0.0 || df2 <= 0.0 || ncp < 0.0 {
        return ml_warn_return_nan();
    }

    let v1;
    if !r_finite(df1) {
        v1 = 1.0;
    } else {
        v1 = rnchisq(df1, ncp) / df1;
    }

    let v2 = if !r_finite(df2) {
        1.0
    } else {
        rchisq_inner(df2) / df2
    };

    v1 / v2
}

// =====================================================================
// FFI shims
// =====================================================================

#[unsafe(no_mangle)]
pub extern "C" fn Rf_dnf(x: f64, df1: f64, df2: f64, ncp: f64, give_log: i32) -> f64 {
    dnf_inner(x, df1, df2, ncp, give_log != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn dnf(x: f64, df1: f64, df2: f64, ncp: f64, give_log: i32) -> f64 {
    dnf_inner(x, df1, df2, ncp, give_log != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_pnf(x: f64, df1: f64, df2: f64, ncp: f64, lower_tail: i32, log_p: i32) -> f64 {
    pnf_inner(x, df1, df2, ncp, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn pnf(x: f64, df1: f64, df2: f64, ncp: f64, lower_tail: i32, log_p: i32) -> f64 {
    pnf_inner(x, df1, df2, ncp, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_qnf(p: f64, df1: f64, df2: f64, ncp: f64, lower_tail: i32, log_p: i32) -> f64 {
    qnf_inner(p, df1, df2, ncp, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn qnf(p: f64, df1: f64, df2: f64, ncp: f64, lower_tail: i32, log_p: i32) -> f64 {
    qnf_inner(p, df1, df2, ncp, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_rnf(df1: f64, df2: f64, ncp: f64) -> f64 {
    rnf_inner(df1, df2, ncp)
}

#[unsafe(no_mangle)]
pub extern "C" fn rnf(df1: f64, df2: f64, ncp: f64) -> f64 {
    rnf_inner(df1, df2, ncp)
}
