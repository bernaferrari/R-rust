// Geometric distribution: dgeom, pgeom, qgeom, rgeom
// Ported from dgeom.c, pgeom.c, qgeom.c, rgeom.c
// dgeom originally by Catherine Loader, catherine@research.bell-labs.com, October 23, 2000
// pgeom originally by Ross Ihaka, Copyright (C) 1998
// qgeom originally by Ross Ihaka, Copyright (C) 1998
// rgeom originally by Ross Ihaka, Copyright (C) 1998
//   Reference: Devroye, L. (1986). Non-Uniform Random Variate Generation.
//     New York: Springer-Verlag. Pages 488f.

use crate::constants::*;
use crate::dist::binomial::dbinom_raw;
use crate::dist::exponential::exp_rand;
use crate::dist::poisson::rpois_inner;
use crate::dpq::*;
use crate::error::*;
use crate::utils::*;
use libm::*;

// ---- dgeom ----

pub fn dgeom_inner(x: f64, p: f64, give_log: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(p) {
        return x + p;
    }

    if p <= 0.0 || p > 1.0 {
        return ml_warn_return_nan();
    }

    // R_D_nonint_check(x):
    if r_nonint(x) {
        ml_warning(ME_DOMAIN, "");
        return r_d__0(give_log);
    }
    if x < 0.0 || !r_finite(x) || p == 0.0 {
        return r_d__0(give_log);
    }
    let x = r_forceint(x);

    /* prob = (1-p)^x, stable for small p */
    let prob = dbinom_raw(0.0, x, p, 1.0 - p, give_log);

    if give_log { log(p) + prob } else { p * prob }
}

// ---- pgeom ----

pub fn pgeom_inner(x: f64, p: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(p) {
        return x + p;
    }
    if p <= 0.0 || p > 1.0 {
        return ml_warn_return_nan();
    }

    if x < 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    if !r_finite(x) {
        return r_dt_1(lower_tail, log_p);
    }
    let x = floor(x + 1e-7);

    if p == 1.0 {
        let val = if lower_tail { 1.0 } else { 0.0 };
        return if log_p { log(val) } else { val };
    }

    let x = log1p(-p) * (x + 1.0);
    if log_p {
        r_dt_clog(x, lower_tail, log_p)
    } else {
        if lower_tail { -expm1(x) } else { exp(x) }
    }
}

// ---- qgeom ----

pub fn qgeom_inner(p: f64, prob: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(p) || isnan(prob) {
        return p + prob;
    }
    if prob <= 0.0 || prob > 1.0 {
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

    if prob == 1.0 {
        return 0.0;
    }

    // R_Q_P01_boundaries(p, 0, ML_POSINF) -- already handled above

    /* add a fuzz to ensure left continuity, but value must be >= 0 */
    fmax2(
        0.0,
        ceil(r_dt_clog(p, lower_tail, log_p) / log1p(-prob) - 1.0 - 1e-12),
    )
}

// ---- rgeom ----

pub fn rgeom_inner(p: f64) -> f64 {
    if !r_finite(p) || p <= 0.0 || p > 1.0 {
        return ml_warn_return_nan();
    }

    rpois_inner(exp_rand() * ((1.0 - p) / p))
}

// ---- FFI shims ----

#[unsafe(no_mangle)]
pub extern "C" fn Rf_dgeom(x: f64, p: f64, give_log: i32) -> f64 {
    dgeom_inner(x, p, give_log != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn dgeom(x: f64, p: f64, give_log: i32) -> f64 {
    dgeom_inner(x, p, give_log != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_pgeom(x: f64, p: f64, lower_tail: i32, log_p: i32) -> f64 {
    pgeom_inner(x, p, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn pgeom(x: f64, p: f64, lower_tail: i32, log_p: i32) -> f64 {
    pgeom_inner(x, p, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_qgeom(p: f64, prob: f64, lower_tail: i32, log_p: i32) -> f64 {
    qgeom_inner(p, prob, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn qgeom(p: f64, prob: f64, lower_tail: i32, log_p: i32) -> f64 {
    qgeom_inner(p, prob, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_rgeom(p: f64) -> f64 {
    rgeom_inner(p)
}

#[unsafe(no_mangle)]
pub extern "C" fn rgeom(p: f64) -> f64 {
    rgeom_inner(p)
}
