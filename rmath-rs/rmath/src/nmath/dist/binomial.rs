// Binomial distribution: dbinom, pbinom, qbinom, rbinom
// Ported from dbinom.c, pbinom.c, qbinom.c, rbinom.c
// dbinom originally by Catherine Loader, catherine@research.bell-labs.com, October 23, 2000
// pbinom originally by Ross Ihaka, Copyright (C) 1998
// qbinom originally by Ross Ihaka, Copyright (C) 1998
// rbinom originally by Ross Ihaka, Copyright (C) 1998
//   Reference: Kachitvichyanukul, V. and Schmeiser, B. W. (1988).
//     Binomial random variate generation. Comm. ACM 31, 216-222. (Algorithm BTPEC).

use crate::nmath::constants::*;
use crate::nmath::dist::normal::qnorm5_inner;
use crate::nmath::dpq::*;
use crate::nmath::error::*;
use crate::nmath::rng::*;
use crate::nmath::special::bd0::bd0;
use crate::nmath::special::stirlerr::stirlerr;
use crate::nmath::utils::*;
use libm::*;

const DBL_EPSILON: f64 = 2.220446049250313e-16;
const _M_LN_2PI: f64 = 1.837877066409345483560659472811; // log(2*pi)

// ---- pow1p (also used by other distributions) ----

pub(crate) fn pow1p(x: f64, y: f64) -> f64 {
    if isnan(y) {
        return if x == 0.0 { 1.0 } else { y };
    }
    if 0.0 <= y && y == trunc(y) && y <= 4.0 {
        match y as i32 {
            0 => return 1.0,
            1 => return x + 1.0,
            2 => return x * (x + 2.0) + 1.0,
            3 => return x * (x * (x + 3.0) + 3.0) + 1.0,
            4 => return x * (x * (x * (x + 4.0) + 6.0) + 4.0) + 1.0,
            _ => {}
        }
    }
    let xp1 = x + 1.0;
    let x_ = xp1 - 1.0;
    if x_ == x || fabs(x) > 0.5 || isnan(x) {
        pow(xp1, y)
    } else {
        exp(y * log1p(x))
    }
}

// ---- dbinom_raw ----

pub(crate) fn dbinom_raw(x: f64, n: f64, p: f64, q: f64, give_log: bool) -> f64 {
    if p == 0.0 {
        return if x == 0.0 {
            r_d__1(give_log)
        } else {
            r_d__0(give_log)
        };
    }
    if q == 0.0 {
        return if x == n {
            r_d__1(give_log)
        } else {
            r_d__0(give_log)
        };
    }

    if x == 0.0 {
        if n == 0.0 {
            return r_d__1(give_log);
        }
        if p > q {
            return if give_log { n * log(q) } else { pow(q, n) };
        } else {
            return if give_log {
                n * log1p(-p)
            } else {
                pow1p(-p, n)
            };
        }
    }
    if x == n {
        if p > q {
            return if give_log {
                n * log1p(-q)
            } else {
                pow1p(-q, n)
            };
        } else {
            return if give_log { n * log(p) } else { pow(p, n) };
        }
    }
    if x < 0.0 || x > n {
        return r_d__0(give_log);
    }

    if !r_finite(n) {
        if r_finite(x) {
            return r_d__0(give_log);
        } else { /* n = DBL_MAX helps extreme dnbinom() cases */
        }
    }

    let lc = stirlerr(n) - stirlerr(x) - stirlerr(n - x) - bd0(x, n * p) - bd0(n - x, n * q);
    let lf = M_LN_2PI + log(x) + log1p(-x / n);

    r_d_exp(lc - 0.5 * lf, give_log)
}

// ---- dbinom ----

#[must_use]
pub fn dbinom_inner(x: f64, n: f64, p: f64, give_log: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(n) || isnan(p) {
        return x + n + p;
    }

    if p < 0.0 || p > 1.0 || (n < 0.0 || r_nonint(n)) {
        return ml_warn_return_nan();
    }
    // R_D_nonint_check(x):
    if r_nonint(x) {
        ml_warning(ME_DOMAIN, "");
        return r_d__0(give_log);
    }
    if x < 0.0 || !r_finite(x) {
        return r_d__0(give_log);
    }

    let n = r_forceint(n);
    let x = r_forceint(x);

    dbinom_raw(x, n, p, 1.0 - p, give_log)
}

// ---- pbinom ----

#[must_use]
pub fn pbinom_inner(x: f64, n: f64, p: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(n) || isnan(p) {
        return x + n + p;
    }
    if !r_finite(n) || !r_finite(p) {
        return ml_warn_return_nan();
    }

    if r_nonint(n) {
        ml_warning(ME_DOMAIN, "");
        return ml_warn_return_nan();
    }
    let n = r_forceint(n);
    if n < 0.0 || p < 0.0 || p > 1.0 {
        return ml_warn_return_nan();
    }

    if x < 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    let x = floor(x + 1e-7);
    if n <= x {
        return r_dt_1(lower_tail, log_p);
    }

    crate::nmath::dist::beta::pbeta_inner(p, x + 1.0, n - x, !lower_tail, log_p)
}

// ---- qbinom ----

fn do_search_binom(
    mut y: f64,
    z: &mut f64,
    p: f64,
    n: f64,
    pr: f64,
    incr: f64,
    lower_tail: bool,
    log_p: bool,
) -> f64 {
    let max_y = n;
    let left = if lower_tail { *z >= p } else { *z < p };

    if left {
        loop {
            let mut newz = -1.0;
            if y > 0.0 {
                newz = pbinom_inner(y - incr, n, pr, lower_tail, log_p);
            } else if y < 0.0 {
                y = 0.0;
            }
            if y == 0.0 || isnan(newz) || (lower_tail && newz < p) || (!lower_tail && newz >= p) {
                return y;
            }
            y = fmax2(0.0, y - incr);
            *z = newz;
        }
    } else {
        loop {
            let prevy = y;
            let mut newz = -1.0;
            y += incr;
            if y < max_y {
                newz = pbinom_inner(y, n, pr, lower_tail, log_p);
            } else if y > max_y {
                y = max_y;
            }
            if y == max_y || isnan(newz) || (lower_tail && newz >= p) || (!lower_tail && newz < p) {
                if incr <= 1.0 {
                    *z = newz;
                    return y;
                }
                return prevy;
            }
            *z = newz;
        }
    }
}

#[must_use]
pub fn qbinom_inner(p: f64, n: f64, pr: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(p) || isnan(n) || isnan(pr) {
        return p + n + pr;
    }
    if !r_finite(n) || !r_finite(pr) {
        return ml_warn_return_nan();
    }
    if !r_finite(p) && !log_p {
        return ml_warn_return_nan();
    }

    let n = r_forceint(n);

    if pr < 0.0 || pr > 1.0 || n < 0.0 {
        return ml_warn_return_nan();
    }

    // R_Q_P01_boundaries(p, 0, n)
    if log_p {
        if p > 0.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { n } else { 0.0 };
        }
        if p == ML_NEGINF {
            return if lower_tail { 0.0 } else { n };
        }
    } else {
        if p < 0.0 || p > 1.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { 0.0 } else { n };
        }
        if p == 1.0 {
            return if lower_tail { n } else { 0.0 };
        }
    }

    if pr == 0.0 || n == 0.0 {
        return 0.0;
    }
    if pr == 1.0 {
        return n;
    }

    let q = 1.0 - pr;
    let mu = n * pr;
    let sigma = sqrt(n * pr * q);
    let gamma = (q - pr) / sigma;

    let z_val = qnorm5_inner(p, 0.0, 1.0, lower_tail, log_p);
    let mut y = r_forceint(mu + sigma * (z_val + gamma * (z_val * z_val - 1.0) / 6.0));

    // q_DISCR_CHECK_BOUNDARY (with _dist_MAX_y = n)
    if y > n {
        y = n;
    } else if y < 0.0 {
        y = 0.0;
    }

    let mut z = pbinom_inner(y, n, pr, lower_tail, log_p);

    let pf_n = 8.0;
    let pf_l = 2.0;
    let y_large = 4096.0;
    let inc_f = 1.0 / 64.0;
    let i_shrink = 8.0;
    let rel_tol = 1e-15;
    let xf = 4.0;

    let mut p_adj = p;
    if log_p {
        let e = pf_l * DBL_EPSILON;
        if lower_tail && p > -ML_POSINF {
            p_adj = p * (1.0 + e);
        } else {
            p_adj = p * (1.0 - e);
        }
    } else {
        let e = pf_n * DBL_EPSILON;
        if lower_tail {
            p_adj = p * (1.0 - e);
        } else if 1.0 - p > xf * e {
            p_adj = p * (1.0 + e);
        }
    }

    if y < y_large {
        return do_search_binom(y, &mut z, p_adj, n, pr, 1.0, lower_tail, log_p);
    }

    let mut oldincr;
    let mut incr = floor(y * inc_f);
    loop {
        oldincr = incr;
        y = do_search_binom(y, &mut z, p_adj, n, pr, incr, lower_tail, log_p);
        incr = fmax2(1.0, floor(incr / i_shrink));
        if !(oldincr > 1.0 && incr > y * rel_tol) {
            break;
        }
    }
    return y;
}

// ---- rbinom ----

use std::cell::RefCell;

struct RbinomState {
    c: f64,
    fm: f64,
    npq: f64,
    p1: f64,
    p2: f64,
    p3: f64,
    p4: f64,
    qn: f64,
    xl: f64,
    xll: f64,
    xlr: f64,
    xm: f64,
    xr: f64,
    psave: f64,
    nsave: i32,
    m: i32,
}

impl RbinomState {
    fn new() -> Self {
        RbinomState {
            c: 0.0,
            fm: 0.0,
            npq: 0.0,
            p1: 0.0,
            p2: 0.0,
            p3: 0.0,
            p4: 0.0,
            qn: 0.0,
            xl: 0.0,
            xll: 0.0,
            xlr: 0.0,
            xm: 0.0,
            xr: 0.0,
            psave: -1.0,
            nsave: -1,
            m: 0,
        }
    }
}

thread_local!(static RBINOM_STATE: RefCell<RbinomState> = RefCell::new(RbinomState::new()));

#[must_use]
pub fn rbinom_inner(nin: f64, pp: f64) -> f64 {
    let int_max = i32::MAX as f64;

    if !r_finite(nin) {
        return ml_warn_return_nan();
    }
    let r = r_forceint(nin);
    if r != nin {
        return ml_warn_return_nan();
    }
    if !r_finite(pp) || r < 0.0 || pp < 0.0 || pp > 1.0 {
        return ml_warn_return_nan();
    }

    if r == 0.0 || pp == 0.0 {
        return 0.0;
    }
    if pp == 1.0 {
        return r;
    }

    if r >= int_max {
        return qbinom_inner(unif_rand(), r, pp, false, false);
    }

    let n = r as i32;
    let p = fmin2(pp, 1.0 - pp);
    let q = 1.0 - p;
    let np = n as f64 * p;
    let r_val = p / q;
    let g = r_val * ((n as f64) + 1.0);

    RBINOM_STATE.with(|state| {
        let mut st = state.borrow_mut();

        // Setup, perform only when parameters change
        if pp != st.psave || n != st.nsave {
            st.psave = pp;
            st.nsave = n;
            if np < 30.0 {
                st.qn = pow(q, n as f64);
            } else {
                let ffm = np + p;
                st.m = ffm as i32;
                st.fm = st.m as f64;
                st.npq = np * q;
                st.p1 = ((2.195 * sqrt(st.npq) - 4.6 * q) as i32 as f64) + 0.5;
                st.xm = st.fm + 0.5;
                st.xl = st.xm - st.p1;
                st.xr = st.xm + st.p1;
                st.c = 0.134 + 20.5 / (15.3 + st.fm);
                let al = (ffm - st.xl) / (ffm - st.xl * p);
                st.xll = al * (1.0 + 0.5 * al);
                let al = (st.xr - ffm) / (st.xr * q);
                st.xlr = al * (1.0 + 0.5 * al);
                st.p2 = st.p1 * (1.0 + st.c + st.c);
                st.p3 = st.p2 + st.c / st.xll;
                st.p4 = st.p3 + st.c / st.xlr;
            }
        } else if n == st.nsave {
            if np < 30.0 {
                // go to L_np_small
            } else {
                // fall through to BTPE
            }
        }

        let use_small = np < 30.0;

        if !use_small {
            // BTPE algorithm
            loop {
                let u = unif_rand() * st.p4;
                let v = unif_rand();
                let ix: i32;

                if u <= st.p1 {
                    ix = (st.xm - st.p1 * v + u) as i32;
                } else if u <= st.p2 {
                    let x = st.xl + (u - st.p1) / st.c;
                    let v = v * st.c + 1.0 - fabs(st.xm - x) / st.p1;
                    if v > 1.0 || v <= 0.0 {
                        continue;
                    }
                    ix = x as i32;
                } else if u > st.p3 {
                    ix = (st.xr - log(v) / st.xlr) as i32;
                    if ix > n {
                        continue;
                    }
                    let v = v * (u - st.p3) * st.xlr;
                    // Fall through to accept/reject test
                    let k = (ix - st.m).abs();
                    if k <= 20 || k >= (st.npq / 2.0 - 1.0) as i32 {
                        let mut f = 1.0;
                        if st.m < ix {
                            for i in (st.m + 1)..=ix {
                                f *= g / (i as f64) - r_val;
                            }
                        } else if st.m != ix {
                            for i in (ix + 1)..=st.m {
                                f /= g / (i as f64) - r_val;
                            }
                        }
                        if v <= f {
                            return ix as f64;
                        }
                    } else {
                        let amaxp = ((k as f64) / st.npq)
                            * (((k as f64) * ((k as f64) / 3.0 + 0.625) + 0.1666666666666)
                                / st.npq
                                + 0.5);
                        let ynorm = -((k as f64) * (k as f64)) / (2.0 * st.npq);
                        let alv = log(v);
                        if alv < ynorm - amaxp {
                            return ix as f64;
                        }
                        if alv <= ynorm + amaxp {
                            let x1 = (ix + 1) as f64;
                            let f1 = st.fm + 1.0;
                            let z = (n + 1 - st.m) as f64;
                            let w = (n - ix + 1) as f64;
                            let z2 = z * z;
                            let x2 = x1 * x1;
                            let f2 = f1 * f1;
                            let w2 = w * w;
                            let stirling_term1 = (13860.0
                                - (462.0 - (132.0 - (99.0 - 140.0 / f2) / f2) / f2) / f2)
                                / f1
                                / 166320.0;
                            let stirling_term2 = (13860.0
                                - (462.0 - (132.0 - (99.0 - 140.0 / z2) / z2) / z2) / z2)
                                / z
                                / 166320.0;
                            let stirling_term3 = (13860.0
                                - (462.0 - (132.0 - (99.0 - 140.0 / x2) / x2) / x2) / x2)
                                / x1
                                / 166320.0;
                            let stirling_term4 = (13860.0
                                - (462.0 - (132.0 - (99.0 - 140.0 / w2) / w2) / w2) / w2)
                                / w
                                / 166320.0;
                            if alv
                                <= st.xm * log(f1 / x1)
                                    + (n - st.m) as f64 * log(z / w)
                                    + (ix - st.m) as f64 * log(w * p / (x1 * q))
                                    + stirling_term1
                                    + stirling_term2
                                    + stirling_term3
                                    + stirling_term4
                            {
                                return ix as f64;
                            }
                        }
                    }
                    continue;
                } else {
                    // left tail
                    ix = (st.xl + log(v) / st.xll) as i32;
                    if ix < 0 {
                        continue;
                    }
                    let v = v * (u - st.p2) * st.xll;
                    let k = (ix - st.m).abs();
                    if k <= 20 || k >= (st.npq / 2.0 - 1.0) as i32 {
                        let mut f = 1.0;
                        if st.m < ix {
                            for i in (st.m + 1)..=ix {
                                f *= g / (i as f64) - r_val;
                            }
                        } else if st.m != ix {
                            for i in (ix + 1)..=st.m {
                                f /= g / (i as f64) - r_val;
                            }
                        }
                        if v <= f {
                            return ix as f64;
                        }
                    } else {
                        let amaxp = ((k as f64) / st.npq)
                            * (((k as f64) * ((k as f64) / 3.0 + 0.625) + 0.1666666666666)
                                / st.npq
                                + 0.5);
                        let ynorm = -((k as f64) * (k as f64)) / (2.0 * st.npq);
                        let alv = log(v);
                        if alv < ynorm - amaxp {
                            return ix as f64;
                        }
                        if alv <= ynorm + amaxp {
                            let x1 = (ix + 1) as f64;
                            let f1 = st.fm + 1.0;
                            let z = (n + 1 - st.m) as f64;
                            let w = (n - ix + 1) as f64;
                            let z2 = z * z;
                            let x2 = x1 * x1;
                            let f2 = f1 * f1;
                            let w2 = w * w;
                            let stirling_term1 = (13860.0
                                - (462.0 - (132.0 - (99.0 - 140.0 / f2) / f2) / f2) / f2)
                                / f1
                                / 166320.0;
                            let stirling_term2 = (13860.0
                                - (462.0 - (132.0 - (99.0 - 140.0 / z2) / z2) / z2) / z2)
                                / z
                                / 166320.0;
                            let stirling_term3 = (13860.0
                                - (462.0 - (132.0 - (99.0 - 140.0 / x2) / x2) / x2) / x2)
                                / x1
                                / 166320.0;
                            let stirling_term4 = (13860.0
                                - (462.0 - (132.0 - (99.0 - 140.0 / w2) / w2) / w2) / w2)
                                / w
                                / 166320.0;
                            if alv
                                <= st.xm * log(f1 / x1)
                                    + (n - st.m) as f64 * log(z / w)
                                    + (ix - st.m) as f64 * log(w * p / (x1 * q))
                                    + stirling_term1
                                    + stirling_term2
                                    + stirling_term3
                                    + stirling_term4
                            {
                                return ix as f64;
                            }
                        }
                    }
                    continue;
                }

                // triangular region or parallelogram reached ix
                // For triangular and parallelogram, simple acceptance
                return ix as f64;
            }
        }

        // L_np_small: np = n*p < 30 : inverse CDF
        loop {
            let mut ix: i32 = 0;
            let mut f = st.qn;
            let mut u = unif_rand();
            loop {
                if u < f {
                    break;
                }
                if ix > 110 {
                    break;
                }
                u -= f;
                ix += 1;
                f *= g / (ix as f64) - r_val;
            }
            let result = ix;
            if st.psave > 0.5 {
                return (n - result) as f64;
            }
            return result as f64;
        }
    })
}

// ---- FFI shims ----

#[must_use]
pub extern "C" fn Rf_dbinom(x: f64, n: f64, p: f64, give_log: i32) -> f64 {
    dbinom_inner(x, n, p, give_log != 0)
}

#[must_use]
pub extern "C" fn dbinom(x: f64, n: f64, p: f64, give_log: i32) -> f64 {
    dbinom_inner(x, n, p, give_log != 0)
}

#[must_use]
pub extern "C" fn Rf_pbinom(x: f64, n: f64, p: f64, lower_tail: i32, log_p: i32) -> f64 {
    pbinom_inner(x, n, p, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn pbinom(x: f64, n: f64, p: f64, lower_tail: i32, log_p: i32) -> f64 {
    pbinom_inner(x, n, p, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn Rf_qbinom(p: f64, n: f64, pr: f64, lower_tail: i32, log_p: i32) -> f64 {
    qbinom_inner(p, n, pr, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn qbinom(p: f64, n: f64, pr: f64, lower_tail: i32, log_p: i32) -> f64 {
    qbinom_inner(p, n, pr, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn Rf_rbinom(n: f64, p: f64) -> f64 {
    rbinom_inner(n, p)
}

#[must_use]
pub extern "C" fn rbinom(n: f64, p: f64) -> f64 {
    rbinom_inner(n, p)
}
