#![allow(clippy::neg_cmp_op_on_partial_ord)]
// Noncentral chi-squared distribution: dnchisq, pnchisq, qnchisq, rnchisq
// Ported from dnchisq.c, pnchisq.c, qnchisq.c, rnchisq.c
//
// dnchisq.c: based on formula (29.5b-c) in Johnson, Kotz, Balakrishnan (1995)
// pnchisq.c: Algorithm AS 275, Ding (1992), Appl.Statist., 41, 478-482
// qnchisq.c: Inverts pnchisq via bisection
// rnchisq.c: Kuensch's suggestion: sum of central chi-squared + Poisson mixture

use crate::constants::*;
use crate::dpq::*;
use crate::error::*;
use crate::special::gamma::lgammafn;
use crate::utils::*;
use libm::*;
use std::os::raw::{c_double, c_int};

use super::chisq::{dchisq_inner, pchisq_inner, qchisq_inner, rchisq_inner};
use super::gamma::{logspace_add, rgamma_inner};
use super::poisson::rpois_inner;

// Constants
const M_LN2: f64 = 0.693147180559945309417232121458; // log(2)
const DBL_EPSILON: f64 = 2.220446049250313e-16;
const DBL_MAX: f64 = 1.7976931348623157e+308;
const DBL_MIN: f64 = 2.2250738585072014e-308;
const DBL_MIN_EXP: i32 = -1022;

const _DBL_MIN_EXP: f64 = M_LN2 * (DBL_MIN_EXP as f64);

// ---- pnchisq_raw ----
// Internal function used by pnchisq and qnchisq.
// This is the core algorithm from pnchisq.c.
pub(crate) fn pnchisq_raw(
    x: f64,
    f: f64,
    theta: f64, // = ncp
    errmax: f64,
    reltol: f64,
    itrmax: i32,
    lower_tail: bool,
    log_p: bool,
) -> f64 {
    if x <= 0.0 {
        if x == 0.0 && f == 0.0 {
            let l_val = -0.5 * theta;
            return if lower_tail {
                r_d_exp(l_val, log_p)
            } else {
                if log_p {
                    r_log1_exp(l_val)
                } else {
                    -expm1(l_val)
                }
            };
        }
        return r_dt_0(lower_tail, log_p);
    }
    if !r_finite(x) {
        return r_dt_1(lower_tail, log_p);
    }

    if theta < 80.0 {
        if lower_tail
            && f > 0.0
            && log(x) < M_LN2 + 2.0 / f * (lgammafn(f / 2.0 + 1.0) + _DBL_MIN_EXP)
        {
            let lambda = 0.5 * theta;
            let mut pr = -lambda;
            let log_lam = log(lambda);
            let mut sum: f64 = ML_NEGINF;
            let mut sum2: f64 = ML_NEGINF;
            let mut i: i32 = 0;
            while i < 110 {
                i += 1;
                pr += log_lam - log(i as f64);
                sum2 = logspace_add(sum2, pr);
                sum = logspace_add(
                    sum,
                    pr + pchisq_inner(x, f + 2.0 * (i as f64), lower_tail, true),
                );
                if sum2 >= -1e-15 {
                    break;
                }
            }
            let ans = sum - sum2;
            return if log_p { ans } else { exp(ans) };
        } else {
            let lambda = 0.5 * theta;
            let mut sum: f64 = 0.0;
            let mut sum2: f64 = 0.0;
            let mut pr: f64 = exp(-lambda);
            let mut i: i32 = 0;
            while i < 110 {
                i += 1;
                pr *= lambda / (i as f64);
                sum2 += pr;
                sum += pr * pchisq_inner(x, f + 2.0 * (i as f64), lower_tail, false);
                if sum2 >= 1.0 - 1e-15 {
                    break;
                }
            }
            let ans = sum / sum2;
            return if log_p { log(ans) } else { ans };
        }
    }
    // else: theta == ncp >= 80 --------------------------------------------

    let lam = 0.5 * theta;
    let lam_sml = -lam < _DBL_MIN_EXP;
    let (mut u, lu, l_lam) = if lam_sml {
        (0.0, -lam, log(lam))
    } else {
        let u = exp(-lam);
        (u, 0.0, 0.0)
    };

    let mut v = u;
    let x2 = 0.5 * x;
    let f2 = 0.5 * f;
    let mut f_x_2n = f - x;

    let t_val = x2 - f2;
    let lt = if f2 * DBL_EPSILON > 0.125 && fabs(t_val) < sqrt(DBL_EPSILON) * f2 {
        (1.0 - t_val) * (2.0 - t_val / (f2 + 1.0))
            - 0.5 * log(2.0 * std::f64::consts::PI)
            - 0.5 * log(f2 + 1.0)
    } else {
        f2 * log(x2) - x2 - lgammafn(f2 + 1.0)
    };

    let t_sml = lt < _DBL_MIN_EXP;
    let (t, mut ans, mut term) = if t_sml {
        if x > f + theta + 5.0 * sqrt(2.0 * (f + 2.0 * theta)) {
            return r_dt_1(lower_tail, log_p);
        }
        let _l_x = log(x);
        (0.0, 0.0, 0.0)
    } else {
        let t = exp(lt);
        (t, v * t, v * t)
    };

    let mut n: i32 = 1;
    let mut f_2n = f + 2.0;
    f_x_2n += 2.0;
    let mut _lu = lu;
    let mut _l_lam = l_lam;
    let mut _t = t;
    let mut _t_sml = t_sml;
    let mut _lam_sml = lam_sml;
    let mut _l_x: f64 = 0.0;
    if t_sml {
        _l_x = log(x);
    }

    while n <= itrmax {
        if f_x_2n > 0.0 {
            let bound = _t * x / f_x_2n;
            let is_b = bound <= errmax;
            let is_r = term <= reltol * ans;
            if is_b && is_r {
                break;
            }
        }

        if _lam_sml {
            _lu += _l_lam - log(n as f64);
            if _lu >= _DBL_MIN_EXP {
                v = exp(_lu);
                u = v;
                _lam_sml = false;
            }
        } else {
            u *= lam / (n as f64);
            v += u;
        }
        if _t_sml {
            _l_x -= log(f_2n);
            if _l_x >= _DBL_MIN_EXP {
                _t = exp(_l_x);
                _t_sml = false;
            }
        } else {
            _t *= x / f_2n;
        }
        if !_lam_sml && !_t_sml {
            term = v * _t;
            ans += term;
        }

        n += 1;
        f_2n += 2.0;
        f_x_2n += 2.0;
    }

    r_dt_val(ans, lower_tail, log_p)
}

// ---- dnchisq ----

#[must_use]
pub fn dnchisq_inner(x: f64, df: f64, ncp: f64, log_p: bool) -> f64 {
    let eps: f64 = 5e-15;

    // IEEE_754
    if isnan(x) || isnan(df) || isnan(ncp) {
        return x + df + ncp;
    }

    if !r_finite(df) || !r_finite(ncp) || ncp < 0.0 || df < 0.0 {
        return ml_warn_return_nan();
    }

    if x < 0.0 {
        return r_d__0(log_p);
    }
    if x == 0.0 && df < 2.0 {
        return ML_POSINF;
    }
    if ncp == 0.0 {
        return if df > 0.0 {
            dchisq_inner(x, df, log_p)
        } else {
            r_d__0(log_p)
        };
    }
    if x == ML_POSINF {
        return r_d__0(log_p);
    }

    let ncp2 = 0.5 * ncp;

    let mut imax = ceil((-(2.0 + df) + sqrt((2.0 - df) * (2.0 - df) + 4.0 * ncp * x)) / 4.0);
    if imax < 0.0 {
        imax = 0.0;
    }
    let (mid, dfmid) = if r_finite(imax) {
        let dfmid = df + 2.0 * imax;
        let mid = super::poisson::dpois_raw(imax, ncp2, false) * dchisq_inner(x, dfmid, false);
        (mid, dfmid)
    } else {
        (0.0, 0.0)
    };

    if mid == 0.0 {
        if log_p || ncp > 1000.0 {
            let nl = df + ncp;
            let ic = nl / (nl + ncp);
            return dchisq_inner(x * ic, nl * ic, log_p);
        } else {
            return r_d__0(log_p);
        }
    }

    let mut sum = mid;

    // upper tail
    let mut term = mid;
    let mut df = dfmid;
    let mut i = imax;
    let x2 = x * ncp2;
    loop {
        i += 1.0;
        let q = x2 / i / df;
        df += 2.0;
        term *= q;
        sum += term;
        if !(q >= 1.0 || term * q > (1.0 - q) * eps || term > 1e-10 * sum) {
            break;
        }
    }
    // lower tail
    term = mid;
    df = dfmid;
    i = imax;
    while i != 0.0 {
        df -= 2.0;
        let q = i * df / x2;
        i -= 1.0;
        term *= q;
        sum += term;
        if q < 1.0 && term * q <= (1.0 - q) * eps {
            break;
        }
    }
    r_d_val(sum, log_p)
}

// ---- pnchisq ----

#[must_use]
pub fn pnchisq_inner(x: f64, df: f64, ncp: f64, lower_tail: bool, log_p: bool) -> f64 {
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

    let mut ans = pnchisq_raw(
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
            ans = fmin2(ans, r_d__1(log_p));
        } else {
            if ans < (if log_p { -23.025850929940457 } else { 1e-10 }) {
                crate::error::ml_warning(crate::constants::ME_PRECISION, "pnchisq");
            }
            if !log_p && ans < 0.0 {
                ans = 0.0;
            }
        }
    }
    if !log_p || ans < -1e-8 {
        ans
    } else {
        ans = pnchisq_raw(
            x,
            df,
            ncp,
            1e-12,
            8.0 * DBL_EPSILON,
            1000000,
            !lower_tail,
            false,
        );
        log1p(-ans)
    }
}

// ---- qnchisq ----

#[must_use]
pub fn qnchisq_inner(p: f64, df: f64, ncp: f64, lower_tail: bool, log_p: bool) -> f64 {
    let accu: f64 = 1e-13;
    let racc: f64 = 4.0 * DBL_EPSILON;
    let eps: f64 = 1e-11;
    let r_eps: f64 = 1e-10;

    let mut p = p;
    let mut lower_tail = lower_tail;

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

    let b = (ncp * ncp) / (df + 3.0 * ncp);
    let c = (df + 3.0 * ncp) / (df + 2.0 * ncp);
    let ff = (df + 2.0 * ncp) / (c * c);
    let mut ux = b + c * qchisq_inner(p, ff, lower_tail, log_p);
    if ux <= 0.0 {
        ux = 1.0;
    }
    let ux0 = ux;

    if !lower_tail && ncp >= 80.0 {
        if pp < 1e-10 {
            crate::error::ml_warning(crate::constants::ME_PRECISION, "qnchisq");
        }
        p = if log_p { -expm1(p) } else { 0.5 - p + 0.5 };
        lower_tail = true;
    } else {
        p = pp;
    }

    let mut pp = fmin2(1.0 - DBL_EPSILON, p * (1.0 + eps));
    let mut lx: f64;
    if lower_tail {
        while ux < DBL_MAX && pnchisq_raw(ux, df, ncp, eps, r_eps, 10000, true, false) < pp {
            ux *= 2.0;
        }
        pp = p * (1.0 - eps);
        lx = fmin2(ux0, DBL_MAX);
        while lx > DBL_MIN && pnchisq_raw(lx, df, ncp, eps, r_eps, 10000, true, false) > pp {
            lx *= 0.5;
        }
    } else {
        while ux < DBL_MAX && pnchisq_raw(ux, df, ncp, eps, r_eps, 10000, false, false) > pp {
            ux *= 2.0;
        }
        pp = p * (1.0 - eps);
        lx = fmin2(ux0, DBL_MAX);
        while lx > DBL_MIN && pnchisq_raw(lx, df, ncp, eps, r_eps, 10000, false, false) < pp {
            lx *= 0.5;
        }
    }

    if lower_tail {
        loop {
            let nx = 0.5 * (lx + ux);
            if pnchisq_raw(nx, df, ncp, accu, racc, 100000, true, false) > p {
                ux = nx;
            } else {
                lx = nx;
            }
            if !((ux - lx) / nx > accu) {
                break;
            }
        }
    } else {
        loop {
            let nx = 0.5 * (lx + ux);
            if pnchisq_raw(nx, df, ncp, accu, racc, 100000, false, false) < p {
                ux = nx;
            } else {
                lx = nx;
            }
            if !((ux - lx) / nx > accu) {
                break;
            }
        }
    }
    0.5 * (ux + lx)
}

// ---- rnchisq ----

#[must_use]
pub fn rnchisq_inner(df: f64, lambda: f64) -> f64 {
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

// ---- FFI shims ----

#[unsafe(no_mangle)]
pub extern "C" fn Rf_dnchisq(
    x: c_double,
    df: c_double,
    ncp: c_double,
    give_log: c_int,
) -> c_double {
    dnchisq_inner(x, df, ncp, give_log != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn dnchisq(x: c_double, df: c_double, ncp: c_double, give_log: c_int) -> c_double {
    dnchisq_inner(x, df, ncp, give_log != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_pnchisq(
    x: c_double,
    df: c_double,
    ncp: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    pnchisq_inner(x, df, ncp, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn pnchisq(
    x: c_double,
    df: c_double,
    ncp: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    pnchisq_inner(x, df, ncp, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_qnchisq(
    p: c_double,
    df: c_double,
    ncp: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    qnchisq_inner(p, df, ncp, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn qnchisq(
    p: c_double,
    df: c_double,
    ncp: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    qnchisq_inner(p, df, ncp, lower_tail != 0, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn Rf_rnchisq(df: c_double, ncp: c_double) -> c_double {
    rnchisq_inner(df, ncp)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn rnchisq(df: c_double, ncp: c_double) -> c_double {
    rnchisq_inner(df, ncp)
}
