// Poisson distribution: dpois, ppois, qpois, rpois
// Ported from dpois.c, ppois.c, qpois.c, rpois.c
// dpois originally by Catherine Loader, catherine@research.bell-labs.com, October 23, 2000
// ppois originally by Ross Ihaka, Copyright (C) 1998
// qpois originally by Ross Ihaka, Copyright (C) 1998
// rpois originally by Ross Ihaka, Copyright (C) 1998
//   Reference: Ahrens, J.H. and Dieter, U. (1982).
//     Computer generation of Poisson deviates from modified normal distributions.
//     ACM Trans. Math. Software 8, 163-179.

use crate::constants::*;
use crate::dist::exponential::exp_rand;
use crate::dist::normal::norm_rand;
use crate::dist::normal::qnorm5_inner;
use crate::dpq::*;
use crate::error::*;
use crate::rng::*;
use crate::special::bd0::ebd0;
use crate::special::gamma::lgammafn;
use crate::special::stirlerr::stirlerr;
use crate::utils::*;
use libm::*;

const M_SQRT_2PI: f64 = 2.50662827463100050241576528481104525301; /* sqrt(2*pi) */
const X_LRG: f64 = 2.86111748575702815380240589208115399625e+307; /* = 2^1023 / pi */
const DBL_MIN: f64 = 2.2250738585072014e-308;
const DBL_EPSILON: f64 = 2.220446049250313e-16;
const M_2PI: f64 = 6.283185307179586476925286766559; // 2*pi
const M_1_SQRT_2PI: f64 = 0.398942280401432677939946059934; // 1/sqrt(2*pi)

// rpois polynomial coefficients
const A0: f64 = -0.5;
const A1: f64 = 0.3333333;
const A2: f64 = -0.2500068;
const A3: f64 = 0.2000118;
const A4: f64 = -0.1661269;
const A5: f64 = 0.1421878;
const A6: f64 = -0.1384794;
const A7: f64 = 0.1250060;

const ONE_7: f64 = 0.1428571428571428571;
const ONE_12: f64 = 0.0833333333333333333;
const ONE_24: f64 = 0.0416666666666666667;

// ---- dpois_raw (also called from dgamma, pgamma, dnbeta, dnbinom, dnchisq) ----

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
    let (yh, mut yl) = ebd0(x, lambda);
    yl += stirlerr(x);
    let lrg_x = x >= X_LRG;
    let r = if lrg_x {
        M_SQRT_2PI * sqrt(x)
    } else {
        M_2PI * x
    };
    if give_log {
        -yl - yh - (if lrg_x { log(r) } else { 0.5 * log(r) })
    } else {
        exp(-yl) * exp(-yh) / (if lrg_x { r } else { sqrt(r) })
    }
}

// ---- dpois ----

#[must_use]
pub fn dpois_inner(x: f64, lambda: f64, give_log: bool) -> f64 {
    if isnan(x) || isnan(lambda) {
        return x + lambda;
    }
    if lambda < 0.0 {
        return ml_warn_return_nan();
    }
    if r_nonint(x) {
        ml_warning(ME_DOMAIN, "");
        return r_d__0(give_log);
    }
    if x < 0.0 || !r_finite(x) {
        return r_d__0(give_log);
    }
    let x = r_forceint(x);
    dpois_raw(x, lambda, give_log)
}

// ---- ppois ----

#[must_use]
pub fn ppois_inner(x: f64, lambda: f64, lower_tail: bool, log_p: bool) -> f64 {
    if isnan(x) || isnan(lambda) {
        return x + lambda;
    }
    if lambda < 0.0 {
        return ml_warn_return_nan();
    }
    if x < 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    if lambda == 0.0 {
        return r_dt_1(lower_tail, log_p);
    }
    if !r_finite(x) {
        return r_dt_1(lower_tail, log_p);
    }
    let x = floor(x + 1e-7);
    crate::dist::gamma::pgamma_inner(lambda, x + 1.0, 1.0, !lower_tail, log_p)
}

// ---- qpois ----

fn do_search_pois(
    mut y: f64,
    z: &mut f64,
    p: f64,
    lambda: f64,
    incr: f64,
    lower_tail: bool,
    log_p: bool,
) -> f64 {
    let left = if lower_tail { *z >= p } else { *z < p };
    if left {
        loop {
            let mut newz = -1.0;
            if y > 0.0 {
                newz = ppois_inner(y - incr, lambda, lower_tail, log_p);
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
            let newz;
            y += incr;
            newz = ppois_inner(y, lambda, lower_tail, log_p);
            if isnan(newz) || (lower_tail && newz >= p) || (!lower_tail && newz < p) {
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
pub fn qpois_inner(p: f64, lambda: f64, lower_tail: bool, log_p: bool) -> f64 {
    if isnan(p) || isnan(lambda) {
        return p + lambda;
    }
    if !r_finite(lambda) {
        return ml_warn_return_nan();
    }
    if lambda < 0.0 {
        return ml_warn_return_nan();
    }

    // R_Q_P01_check(p)
    if log_p {
        if p > 0.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
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

    if lambda == 0.0 {
        return 0.0;
    }

    let p_is_0 = if lower_tail {
        if log_p { p == ML_NEGINF } else { p == 0.0 }
    } else {
        if log_p { p == 0.0 } else { p == 1.0 }
    };
    if p_is_0 {
        return 0.0;
    }

    let p_is_1 = if lower_tail {
        if log_p { p == 0.0 } else { p == 1.0 }
    } else {
        if log_p { p == ML_NEGINF } else { p == 0.0 }
    };
    if p_is_1 {
        return ML_POSINF;
    }

    let mu = lambda;
    let sigma = sqrt(lambda);
    let gamma = 1.0 / sigma;

    let z_val = qnorm5_inner(p, 0.0, 1.0, lower_tail, log_p);
    let mut y = r_forceint(mu + sigma * (z_val + gamma * (z_val * z_val - 1.0) / 6.0));
    if y < 0.0 {
        y = 0.0;
    }

    let mut z = ppois_inner(y, lambda, lower_tail, log_p);

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
        return do_search_pois(y, &mut z, p_adj, lambda, 1.0, lower_tail, log_p);
    }

    let mut oldincr;
    let mut incr = floor(y * inc_f);
    loop {
        oldincr = incr;
        y = do_search_pois(y, &mut z, p_adj, lambda, incr, lower_tail, log_p);
        incr = fmax2(1.0, floor(incr / i_shrink));
        if !(oldincr > 1.0 && incr > y * rel_tol) {
            break;
        }
    }
    return y;
}

// ---- rpois ----

// Persistent state for rpois (mirrors C's function-level static variables)
struct RpoisState {
    l: i32,
    m: i32,
    b1: f64,
    b2: f64,
    c: f64,
    c0: f64,
    c1: f64,
    c2: f64,
    c3: f64,
    pp: [f64; 36],
    p0: f64,
    p: f64,
    q: f64,
    s: f64,
    d: f64,
    omega: f64,
    big_l: f64,
    muprev: f64,
    muprev2: f64,
}

impl RpoisState {
    fn new() -> Self {
        RpoisState {
            l: 0,
            m: 0,
            b1: 0.0,
            b2: 0.0,
            c: 0.0,
            c0: 0.0,
            c1: 0.0,
            c2: 0.0,
            c3: 0.0,
            pp: [0.0; 36],
            p0: 0.0,
            p: 0.0,
            q: 0.0,
            s: 0.0,
            d: 0.0,
            omega: 0.0,
            big_l: 0.0,
            muprev: 0.0,
            muprev2: 0.0,
        }
    }
}

use std::cell::RefCell;
thread_local!(static RPOIS_STATE: RefCell<RpoisState> = RefCell::new(RpoisState::new()));

#[must_use]
pub fn rpois_inner(mu: f64) -> f64 {
    let fact: [f64; 10] = [1., 1., 2., 6., 24., 120., 720., 5040., 40320., 362880.];

    if !r_finite(mu) || mu < 0.0 {
        return ml_warn_return_nan();
    }
    if mu <= 0.0 {
        return 0.0;
    }

    let big_mu = mu >= 10.0;

    RPOIS_STATE.with(|state| {
        let mut st = state.borrow_mut();

        let mut new_big_mu = false;

        if !(big_mu && mu == st.muprev) {
            // maybe compute new persistent parameters
            if big_mu {
                new_big_mu = true;
                st.muprev = mu;
                st.s = sqrt(mu);
                st.d = 6.0 * mu * mu;
                st.big_l = floor(mu - 1.1484);
            } else {
                // Small mu (< 10)
                new_big_mu = false;
                if mu != st.muprev {
                    st.muprev = mu;
                    st.m = imax2(1, mu as i32);
                    st.l = 0;
                    st.q = exp(-mu);
                    st.p0 = st.q;
                    st.p = st.q;
                }
            }
        }

        if !big_mu {
            // Small mu path: inversion method
            loop {
                let u = unif_rand();
                if u <= st.p0 {
                    return 0.0;
                }

                if st.l != 0 {
                    let start_k = if u <= 0.458 { 1 } else { imin2(st.l, st.m) };
                    let mut k = start_k;
                    while k <= st.l {
                        if u <= st.pp[k as usize] {
                            return k as f64;
                        }
                        k += 1;
                    }
                    if st.l == 35 {
                        continue;
                    }
                }

                st.l += 1;
                let mut k = st.l;
                while k <= 35 {
                    st.p *= mu / (k as f64);
                    st.q += st.p;
                    st.pp[k as usize] = st.q;
                    if u <= st.q {
                        st.l = k;
                        return k as f64;
                    }
                    k += 1;
                }
                st.l = 35;
            }
        }

        // mu >= 10 path:

        // Step N. normal sample
        let g = mu + st.s * norm_rand();

        let (pois, fk, difmuk, u_sq, g_pos);

        if g >= 0.0 {
            pois = floor(g);
            if pois >= st.big_l {
                return pois;
            }
            fk = pois;
            difmuk = mu - fk;
            let u = unif_rand();
            if st.d * u >= difmuk * difmuk * difmuk {
                return pois;
            }
            u_sq = u;
            g_pos = true;
        } else {
            pois = -1.0;
            fk = 0.0;
            difmuk = 0.0;
            u_sq = 0.0;
            g_pos = false;
        }

        // Step P. preparations for steps Q and H.
        if new_big_mu || mu != st.muprev2 {
            st.muprev2 = mu;
            st.omega = M_1_SQRT_2PI / st.s;
            let b1 = ONE_24 / mu;
            let b2 = 0.3 * b1 * b1;
            st.c3 = ONE_7 * b1 * b2;
            st.c2 = b2 - 15.0 * st.c3;
            st.c1 = b1 - 6.0 * b2 + 45.0 * st.c3;
            st.c0 = 1.0 - b1 + 3.0 * b2 - 15.0 * st.c3;
            st.c = 0.1069 / mu;
            st.b1 = b1;
            st.b2 = b2;
        }

        // Step F (subroutine)
        let step_f = |pois_v: f64,
                      kflag_v: i32,
                      fk_v: f64,
                      difmuk_v: f64,
                      e_v: f64,
                      u_v: f64,
                      st: &RpoisState|
         -> bool {
            let (px, py): (f64, f64);
            if pois_v < 10.0 {
                px = -mu;
                py = pow(mu, pois_v) / fact[pois_v as usize];
            } else {
                let fk_l = fk_v;
                let mut del = ONE_12 / fk_l;
                del = del * (1.0 - 4.8 * del * del);
                let v = difmuk_v / fk_l;
                if fabs(v) <= 0.25 {
                    px = fk_l
                        * v
                        * v
                        * (((((((A7 * v + A6) * v + A5) * v + A4) * v + A3) * v + A2) * v + A1)
                            * v
                            + A0)
                        - del;
                } else {
                    px = fk_l * log(1.0 + v) - difmuk_v - del;
                }
                py = M_1_SQRT_2PI / sqrt(fk_l);
            }
            let mut x = (0.5 - difmuk_v) / st.s;
            x *= x;
            let fx = -0.5 * x;
            let fy = st.omega * (((st.c3 * x + st.c2) * x + st.c1) * x + st.c0);

            if kflag_v > 0 {
                // Step H. Hat acceptance
                st.c * fabs(u_v) <= py * exp(px + e_v) - fy * exp(fx + e_v)
            } else {
                // Step Q. Quotient acceptance
                fy - u_v * fy <= py * exp(px - fx)
            }
        };

        if g_pos && step_f(pois, 0, fk, difmuk, 0.0, u_sq, &st) {
            return pois;
        }

        loop {
            let e = exp_rand();
            let u = 2.0 * unif_rand() - 1.0;
            let t = 1.8 + fsign(e, u);
            if t > -0.6744 {
                let pois_v = floor(mu + st.s * t);
                let fk_v = pois_v;
                let difmuk_v = mu - fk_v;
                if step_f(pois_v, 1, fk_v, difmuk_v, e, u, &st) {
                    return pois_v;
                }
            }
        }
    })
}

// ---- FFI shims ----

#[must_use]
pub fn Rf_dpois(x: f64, lambda: f64, give_log: i32) -> f64 {
    dpois_inner(x, lambda, give_log != 0)
}

#[must_use]
pub fn dpois(x: f64, lambda: f64, give_log: i32) -> f64 {
    dpois_inner(x, lambda, give_log != 0)
}

#[must_use]
pub fn Rf_ppois(x: f64, lambda: f64, lower_tail: i32, log_p: i32) -> f64 {
    ppois_inner(x, lambda, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn ppois(x: f64, lambda: f64, lower_tail: i32, log_p: i32) -> f64 {
    ppois_inner(x, lambda, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_qpois(p: f64, lambda: f64, lower_tail: i32, log_p: i32) -> f64 {
    qpois_inner(p, lambda, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn qpois(p: f64, lambda: f64, lower_tail: i32, log_p: i32) -> f64 {
    qpois_inner(p, lambda, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_rpois(mu: f64) -> f64 {
    rpois_inner(mu)
}

#[must_use]
pub fn rpois(mu: f64) -> f64 {
    rpois_inner(mu)
}
