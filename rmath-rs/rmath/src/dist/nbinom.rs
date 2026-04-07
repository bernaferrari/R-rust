#![allow(unused_assignments)]
// Negative binomial distribution: dnbinom, pnbinom, qnbinom, rnbinom
// Ported from dnbinom.c, pnbinom.c, qnbinom.c, qnbinom_mu.c, rnbinom.c
// dnbinom originally by Catherine Loader, catherine@research.bell-labs.com, October 23, 2000
// dnbinom_mu originally by Martin Maechler, June 2008
// pnbinom originally by Ross Ihaka, Copyright (C) 1998
// qnbinom originally by Ross Ihaka, Copyright (C) 1998
// rnbinom originally by Ross Ihaka, Copyright (C) 1998
//   Reference: Devroye, L. (1986).
//     Non-Uniform Random Variate Generation. New York:Springer-Verlag. Pages 488 and 543.

use crate::constants::*;
use crate::dist::beta::pbeta_inner;
use crate::dist::binomial::dbinom_raw;
use crate::dist::gamma::rgamma_inner;
use crate::dist::normal::qnorm5_inner;
use crate::dist::poisson::{dpois_raw, ppois_inner, qpois_inner, rpois_inner};
use crate::dpq::*;
use crate::error::*;
use crate::special::gamma::lgammafn1p;
use crate::utils::*;
use libm::*;
use std::os::raw::{c_double, c_int};

const DBL_MAX: f64 = 1.7976931348623157e+308;
const DBL_EPSILON: f64 = 2.220446049250313e-16;

// ---- dnbinom ----

#[must_use]
pub fn dnbinom_inner(x: f64, size: f64, prob: f64, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(size) || isnan(prob) {
        return x + size + prob;
    }

    if prob <= 0.0 || prob > 1.0 || size < 0.0 {
        return ml_warn_return_nan();
    }
    // R_D_nonint_check(x)
    if r_nonint(x) {
        ml_warning(ME_DOMAIN, "");
        return r_d__0(log_p);
    }
    if x < 0.0 || !r_finite(x) {
        return r_d__0(log_p);
    }
    let x = r_forceint(x);

    if x == 0.0 {
        // limiting case as size approaches zero is point mass at zero
        if size == 0.0 {
            return r_d__1(log_p);
        }
        // size > 0: P(x, ..) = pr^n
        return if log_p {
            size * log(prob)
        } else {
            pow(prob, size)
        };
    }

    let size = if !r_finite(size) { DBL_MAX } else { size };

    if x < 1e-10 * size {
        // instead of dbinom_raw(), use 2 terms of Abramowitz & Stegun (6.1.47)
        let xx2s = /* x(x-1)/(2*size) robustly */
            if x < sqrt(DBL_MAX) { ldexp(x * (x - 1.0), -1) / size }
            else { x * (ldexp(x, -1) / size) };
        r_d_exp(
            size * log(prob) + x * (log(size) + log1p(-prob)) - lgammafn1p(x) + log1p(xx2s),
            log_p,
        )
    } else {
        // log( size/(size+x) ) is much less accurate than log1p(- x/(size+x))
        // for |x| << size (and actually when x < size):
        let p = if log_p {
            if x < size {
                log1p(-x / (size + x))
            } else {
                log(size / (size + x))
            }
        } else {
            size / (size + x)
        };
        let ans = dbinom_raw(size, x + size, prob, 1.0 - prob, log_p);
        if log_p { p + ans } else { p * ans }
    }
}

// ---- dnbinom_mu ----

#[must_use]
pub fn dnbinom_mu_inner(x: f64, size: f64, mu: f64, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(size) || isnan(mu) {
        return x + size + mu;
    }

    if mu < 0.0 || size < 0.0 {
        return ml_warn_return_nan();
    }
    // R_D_nonint_check(x)
    if r_nonint(x) {
        ml_warning(ME_DOMAIN, "");
        return r_d__0(log_p);
    }
    if x < 0.0 || !r_finite(x) {
        return r_d__0(log_p);
    }

    // limiting case as size approaches zero is point mass at zero,
    // even if mu is kept constant. limit distribution does not
    // have mean mu, though.
    if x == 0.0 && size == 0.0 {
        return r_d__1(log_p);
    }
    let x = r_forceint(x);

    if !r_finite(size) {
        // limit case: Poisson
        return dpois_raw(x, mu, log_p);
    }

    if x == 0.0 {
        // be accurate, both for n << mu, and n >> mu
        return r_d_exp(
            size * if size < mu {
                log(size / (size + mu))
            } else {
                log1p(-mu / (size + mu))
            },
            log_p,
        );
    }

    if x < 1e-10 * size {
        // don't use dbinom_raw() but MM's formula
        let p = if size < mu {
            log(size / (1.0 + size / mu))
        } else {
            log(mu / (1.0 + mu / size))
        };
        let xx2s = /* x(x-1)/(2*size) robustly */
            if x < sqrt(DBL_MAX) { ldexp(x * (x - 1.0), -1) / size }
            else { x * (ldexp(x, -1) / size) };
        r_d_exp(x * p - mu - lgammafn1p(x) + log1p(xx2s), log_p)
    } else {
        // log( size/(size+x) ) is much less accurate than log1p(- x/(size+x))
        // for |x| << size (and actually when x < size):
        let p = if log_p {
            if x < size {
                log1p(-x / (size + x))
            } else {
                log(size / (size + x))
            }
        } else {
            size / (size + x)
        };
        let ans = dbinom_raw(size, x + size, size / (size + mu), mu / (size + mu), log_p);
        if log_p { p + ans } else { p * ans }
    }
}

// ---- pnbinom ----

#[must_use]
pub fn pnbinom_inner(x: f64, size: f64, prob: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(size) || isnan(prob) {
        return x + size + prob;
    }
    if !r_finite(size) || !r_finite(prob) {
        return ml_warn_return_nan();
    }
    if size < 0.0 || prob <= 0.0 || prob > 1.0 {
        return ml_warn_return_nan();
    }

    // limiting case: point mass at zero
    if size == 0.0 {
        return if x >= 0.0 {
            r_dt_1(lower_tail, log_p)
        } else {
            r_dt_0(lower_tail, log_p)
        };
    }

    if x < 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    if !r_finite(x) {
        return r_dt_1(lower_tail, log_p);
    }
    let x = floor(x + 1e-7);
    pbeta_inner(prob, size, x + 1.0, lower_tail, log_p)
}

// ---- pnbinom_mu ----

#[must_use]
pub fn pnbinom_mu_inner(x: f64, size: f64, mu: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(size) || isnan(mu) {
        return x + size + mu;
    }
    if !r_finite(mu) {
        return ml_warn_return_nan();
    }
    if size < 0.0 || mu < 0.0 {
        return ml_warn_return_nan();
    }

    // limiting case: point mass at zero
    if size == 0.0 {
        return if x >= 0.0 {
            r_dt_1(lower_tail, log_p)
        } else {
            r_dt_0(lower_tail, log_p)
        };
    }

    if x < 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    if !r_finite(x) {
        return r_dt_1(lower_tail, log_p);
    }
    if !r_finite(size) {
        // limit case: Poisson
        return ppois_inner(x, mu, lower_tail, log_p);
    }

    let x = floor(x + 1e-7);
    // pbeta(pr, size, x + 1, lower_tail, log_p) where pr = size/(size+mu)
    let pr = size / (size + mu);
    pbeta_inner(pr, size, x + 1.0, lower_tail, log_p)
}

// ---- qnbinom (size, prob parametrization) ----

fn do_search_nbinom(
    mut y: f64,
    z: &mut f64,
    p: f64,
    size: f64,
    prob: f64,
    incr: f64,
    lower_tail: bool,
    log_p: bool,
) -> f64 {
    let left = if lower_tail { *z >= p } else { *z < p };
    if left {
        loop {
            let mut newz = -1.0;
            if y > 0.0 {
                newz = pnbinom_inner(y - incr, size, prob, lower_tail, log_p);
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
            newz = pnbinom_inner(y, size, prob, lower_tail, log_p);
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
pub fn qnbinom_inner(p: f64, size: f64, prob: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(p) || isnan(size) || isnan(prob) {
        return p + size + prob;
    }

    // this happens if specified via mu, size, since prob == size/(size+mu)
    if prob == 0.0 && size == 0.0 {
        return 0.0;
    }
    if prob <= 0.0 || prob > 1.0 || size < 0.0 {
        return ml_warn_return_nan();
    }
    if prob == 1.0 || size == 0.0 {
        return 0.0;
    }

    // R_Q_P01_boundaries(p, 0, ML_POSINF)
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

    let q_val = 1.0 / prob;
    let p_val = (1.0 - prob) * q_val; // = (1 - prob) / prob = Q - 1
    let mu = size * p_val;
    let sigma = sqrt(size * p_val * q_val);
    let gamma = (q_val + p_val) / sigma;

    let z_val = qnorm5_inner(p, 0.0, 1.0, lower_tail, log_p);
    let mut y = r_forceint(mu + sigma * (z_val + gamma * (z_val * z_val - 1.0) / 6.0));

    // q_DISCR_CHECK_BOUNDARY (no _dist_MAX_y for nbinom, just clamp to >= 0)
    if y < 0.0 {
        y = 0.0;
    }

    let mut z = pnbinom_inner(y, size, prob, lower_tail, log_p);

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
        return do_search_nbinom(y, &mut z, p_adj, size, prob, 1.0, lower_tail, log_p);
    }

    let mut oldincr;
    let mut incr = floor(y * inc_f);
    loop {
        oldincr = incr;
        y = do_search_nbinom(y, &mut z, p_adj, size, prob, incr, lower_tail, log_p);
        incr = fmax2(1.0, floor(incr / i_shrink));
        if !(oldincr > 1.0 && incr > y * rel_tol) {
            break;
        }
    }
    return y;
}

// ---- qnbinom_mu (size, mu parametrization) ----

fn do_search_nbinom_mu(
    mut y: f64,
    z: &mut f64,
    p: f64,
    size: f64,
    mu: f64,
    incr: f64,
    lower_tail: bool,
    log_p: bool,
) -> f64 {
    let left = if lower_tail { *z >= p } else { *z < p };
    if left {
        loop {
            let mut newz = -1.0;
            if y > 0.0 {
                newz = pnbinom_mu_inner(y - incr, size, mu, lower_tail, log_p);
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
            newz = pnbinom_mu_inner(y, size, mu, lower_tail, log_p);
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
pub fn qnbinom_mu_inner(p: f64, size: f64, mu: f64, lower_tail: bool, log_p: bool) -> f64 {
    if size == ML_POSINF {
        // limit case: Poisson
        return qpois_inner(p, mu, lower_tail, log_p);
    }

    // IEEE_754
    if isnan(p) || isnan(size) || isnan(mu) {
        return p + size + mu;
    }

    if mu == 0.0 || size == 0.0 {
        return 0.0;
    }
    if mu < 0.0 || size < 0.0 {
        return ml_warn_return_nan();
    }

    // R_Q_P01_boundaries(p, 0, ML_POSINF)
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

    let q_val = 1.0 + mu / size; // (size+mu)/size = 1 / prob
    let p_val = mu / size; // = (1 - prob) * Q = (1 - prob) / prob = Q - 1
    let sigma = sqrt(size * p_val * q_val);
    let gamma = (q_val + p_val) / sigma;

    let z_val = qnorm5_inner(p, 0.0, 1.0, lower_tail, log_p);
    let mut y = r_forceint(mu + sigma * (z_val + gamma * (z_val * z_val - 1.0) / 6.0));

    // q_DISCR_CHECK_BOUNDARY (no _dist_MAX_y for nbinom, just clamp to >= 0)
    if y < 0.0 {
        y = 0.0;
    }

    let mut z = pnbinom_mu_inner(y, size, mu, lower_tail, log_p);

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
        return do_search_nbinom_mu(y, &mut z, p_adj, size, mu, 1.0, lower_tail, log_p);
    }

    let mut oldincr;
    let mut incr = floor(y * inc_f);
    loop {
        oldincr = incr;
        y = do_search_nbinom_mu(y, &mut z, p_adj, size, mu, incr, lower_tail, log_p);
        incr = fmax2(1.0, floor(incr / i_shrink));
        if !(oldincr > 1.0 && incr > y * rel_tol) {
            break;
        }
    }
    return y;
}

// ---- rnbinom ----

#[must_use]
pub fn rnbinom_inner(size: f64, prob: f64) -> f64 {
    if !r_finite(prob) || isnan(size) || size <= 0.0 || prob <= 0.0 || prob > 1.0 {
        // prob = 1 is ok, PR#1218
        return ml_warn_return_nan();
    }
    let size = if !r_finite(size) { DBL_MAX / 2.0 } else { size };
    // '/2' to prevent rgamma() returning Inf
    if prob == 1.0 {
        return 0.0;
    }
    rpois_inner(rgamma_inner(size, (1.0 - prob) / prob))
}

// ---- rnbinom_mu ----

#[must_use]
pub fn rnbinom_mu_inner(size: f64, mu: f64) -> f64 {
    if !r_finite(mu) || isnan(size) || size <= 0.0 || mu < 0.0 {
        return ml_warn_return_nan();
    }
    let size = if !r_finite(size) { DBL_MAX / 2.0 } else { size };
    if mu == 0.0 {
        return 0.0;
    }
    rpois_inner(rgamma_inner(size, mu / size))
}

// ---- FFI shims ----

#[unsafe(no_mangle)]
pub extern "C" fn Rf_dnbinom(
    x: c_double,
    size: c_double,
    prob: c_double,
    log_p: c_int,
) -> c_double {
    dnbinom_inner(x, size, prob, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn dnbinom(x: c_double, size: c_double, prob: c_double, log_p: c_int) -> c_double {
    dnbinom_inner(x, size, prob, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_dnbinom_mu(
    x: c_double,
    size: c_double,
    mu: c_double,
    log_p: c_int,
) -> c_double {
    dnbinom_mu_inner(x, size, mu, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn dnbinom_mu(x: c_double, size: c_double, mu: c_double, log_p: c_int) -> c_double {
    dnbinom_mu_inner(x, size, mu, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_pnbinom(
    x: c_double,
    size: c_double,
    prob: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    pnbinom_inner(x, size, prob, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn pnbinom(
    x: c_double,
    size: c_double,
    prob: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    pnbinom_inner(x, size, prob, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_pnbinom_mu(
    x: c_double,
    size: c_double,
    mu: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    pnbinom_mu_inner(x, size, mu, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn pnbinom_mu(
    x: c_double,
    size: c_double,
    mu: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    pnbinom_mu_inner(x, size, mu, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_qnbinom(
    p: c_double,
    size: c_double,
    prob: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    qnbinom_inner(p, size, prob, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn qnbinom(
    p: c_double,
    size: c_double,
    prob: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    qnbinom_inner(p, size, prob, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_qnbinom_mu(
    p: c_double,
    size: c_double,
    mu: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    qnbinom_mu_inner(p, size, mu, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn qnbinom_mu(
    p: c_double,
    size: c_double,
    mu: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    qnbinom_mu_inner(p, size, mu, lower_tail != 0, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn Rf_rnbinom(size: c_double, prob: c_double) -> c_double {
    rnbinom_inner(size, prob)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn rnbinom(size: c_double, prob: c_double) -> c_double {
    rnbinom_inner(size, prob)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn Rf_rnbinom_mu(size: c_double, mu: c_double) -> c_double {
    rnbinom_mu_inner(size, mu)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn rnbinom_mu(size: c_double, mu: c_double) -> c_double {
    rnbinom_mu_inner(size, mu)
}
