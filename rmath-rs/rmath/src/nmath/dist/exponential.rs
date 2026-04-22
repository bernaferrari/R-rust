// Exponential distribution: dexp, pexp, qexp, rexp, exp_rand
// Ported from dexp.c, pexp.c, qexp.c, rexp.c, sexp.c

use crate::nmath::constants::*;
use crate::nmath::dpq::*;
use crate::nmath::error::*;
use crate::nmath::rng::*;
use libm::*;

// ---- exp_rand (standard exponential variate) ----

/// exp_rand: random variate from the standard exponential distribution.
#[must_use]
/// Ahrens-Dieter (1972) algorithm.
pub fn exp_rand() -> f64 {
    // q[k-1] = sum(log(2)^k / k!)  k=1,..,n,
    // The highest n (here 16) is determined by q[n-1] = 1.0
    // within standard precision
    static Q: [f64; 16] = [
        0.6931471805599453,
        0.9333736875190459,
        0.9888777961838675,
        0.9984959252914960,
        0.9998292811061389,
        0.9999833164100727,
        0.9999985691438767,
        0.9999998906925558,
        0.9999999924734159,
        0.9999999995283275,
        0.9999999999728814,
        0.9999999999985598,
        0.9999999999999289,
        0.9999999999999968,
        0.9999999999999999,
        1.0000000000000000,
    ];

    let mut a = 0.0_f64;
    let mut u = unif_rand();
    while u <= 0.0 || u >= 1.0 {
        u = unif_rand();
    }
    loop {
        u += u;
        if u > 1.0 {
            break;
        }
        a += Q[0];
    }
    u -= 1.0;

    if u <= Q[0] {
        return a + u;
    }

    let mut i: usize = 0;
    let mut ustar = unif_rand();
    let mut umin = ustar;
    loop {
        ustar = unif_rand();
        if umin > ustar {
            umin = ustar;
        }
        i += 1;
        if u <= Q[i] {
            break;
        }
    }
    a + umin * Q[0]
}

// ---- Inner implementations ----

#[must_use]
pub fn dexp_inner(x: f64, scale: f64, give_log: bool) -> f64 {
    if isnan(x) || isnan(scale) {
        return x + scale;
    }
    if scale <= 0.0 {
        return ml_warn_return_nan();
    }

    if x < 0.0 {
        return r_d__0(give_log);
    }

    if give_log {
        (-x / scale) - log(scale)
    } else {
        exp(-x / scale) / scale
    }
}

#[must_use]
pub fn pexp_inner(x: f64, scale: f64, lower_tail: bool, log_p: bool) -> f64 {
    if isnan(x) || isnan(scale) {
        return x + scale;
    }
    if scale < 0.0 {
        return ml_warn_return_nan();
    }

    if x <= 0.0 {
        return r_dt_0(lower_tail, log_p);
    }

    // same as weibull(shape = 1):
    let x = -(x / scale);
    if lower_tail {
        if log_p { r_log1_exp(x) } else { -expm1(x) }
    } else {
        r_d_exp(x, log_p)
    }
}

#[must_use]
pub fn qexp_inner(p: f64, scale: f64, lower_tail: bool, log_p: bool) -> f64 {
    if isnan(p) || isnan(scale) {
        return p + scale;
    }
    if scale < 0.0 {
        return ml_warn_return_nan();
    }

    // R_Q_P01_check(p)
    if (log_p && p > 0.0) || (!log_p && (p < 0.0 || p > 1.0)) {
        return ml_warn_return_nan();
    }
    if p == r_dt_0(lower_tail, log_p) {
        return 0.0;
    }

    -scale * r_dt_clog(p, lower_tail, log_p)
}

#[must_use]
pub fn rexp_inner(scale: f64) -> f64 {
    if !r_finite(scale) || scale <= 0.0 {
        if scale == 0.0 {
            return 0.0;
        }
        return ml_warn_return_nan();
    }
    scale * exp_rand()
}

// ---- FFI shims ----

#[must_use]
pub fn Rf_dexp(x: f64, scale: f64, give_log: i32) -> f64 {
    dexp_inner(x, scale, give_log != 0)
}

#[must_use]
pub fn dexp(x: f64, scale: f64, give_log: i32) -> f64 {
    dexp_inner(x, scale, give_log != 0)
}

#[must_use]
pub fn Rf_pexp(x: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    pexp_inner(x, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn pexp(x: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    pexp_inner(x, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_qexp(p: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    qexp_inner(p, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn qexp(p: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    qexp_inner(p, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_rexp(scale: f64) -> f64 {
    rexp_inner(scale)
}

#[must_use]
pub fn rexp(scale: f64) -> f64 {
    rexp_inner(scale)
}

#[must_use]
pub fn Rf_exp_rand() -> f64 {
    exp_rand()
}
