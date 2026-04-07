// Logistic distribution: dlogis, plogis, qlogis, rlogis
// Ported from dlogis.c, plogis.c, qlogis.c, rlogis.c

use crate::constants::*;
use crate::dpq::*;
use crate::error::*;
use crate::rng::*;
use libm::*;

// ---- log1pexp helper (from plogis.c) ----

/// Compute log(1 + exp(x)) without overflow (and fast for x > 18).
/// For the two cutoffs, consider in R
///   curve(log1p(exp(x)) - x,       33.1, 33.5, n=2^10)
///   curve(x+exp(-x) - log1p(exp(x)), 15, 25,   n=2^11)
#[inline]
fn log1pexp(x: f64) -> f64 {
    if x <= 18.0 {
        log1p(exp(x))
    } else if x > 33.3 {
        x
    } else {
        // 18.0 < x <= 33.3
        x + exp(-x)
    }
}

// ---- Inner implementations ----

#[must_use]
pub fn dlogis_inner(x: f64, location: f64, scale: f64, give_log: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(location) || isnan(scale) {
        return x + location + scale;
    }
    if scale <= 0.0 {
        return ml_warn_return_nan();
    }

    let x = fabs((x - location) / scale);
    let e = exp(-x);
    let f = 1.0 + e;
    if give_log {
        -(x + log(scale * f * f))
    } else {
        e / (scale * f * f)
    }
}

#[must_use]
pub fn plogis_inner(x: f64, location: f64, scale: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(location) || isnan(scale) {
        return x + location + scale;
    }
    if scale <= 0.0 {
        return ml_warn_return_nan();
    }

    let x = (x - location) / scale;
    if isnan(x) {
        return ml_warn_return_nan();
    }
    // R_P_bounds_Inf_01(x)
    if x == ML_NEGINF {
        return r_dt_0(lower_tail, log_p);
    }
    if x == ML_POSINF {
        return r_dt_1(lower_tail, log_p);
    }

    if log_p {
        // log(1 / (1 + exp( +- x ))) = -log(1 + exp( +- x))
        -log1pexp(if lower_tail { -x } else { x })
    } else {
        1.0 / (1.0 + exp(if lower_tail { -x } else { x }))
    }
}

#[must_use]
pub fn qlogis_inner(p: f64, location: f64, scale: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(p) || isnan(location) || isnan(scale) {
        return p + location + scale;
    }
    // R_Q_P01_boundaries(p, ML_NEGINF, ML_POSINF)
    if log_p {
        if p > 0.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { ML_POSINF } else { ML_NEGINF };
        }
        if p == ML_NEGINF {
            return if lower_tail { ML_NEGINF } else { ML_POSINF };
        }
    } else {
        if p < 0.0 || p > 1.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { ML_NEGINF } else { ML_POSINF };
        }
        if p == 1.0 {
            return if lower_tail { ML_POSINF } else { ML_NEGINF };
        }
    }

    if scale < 0.0 {
        return ml_warn_return_nan();
    }
    if scale == 0.0 {
        return location;
    }

    // p := logit(p) = log( p / (1-p) )
    let p = if log_p {
        if lower_tail {
            p - r_log1_exp(p)
        } else {
            r_log1_exp(p) - p
        }
    } else {
        log(if lower_tail {
            p / (1.0 - p)
        } else {
            (1.0 - p) / p
        })
    };

    location + scale * p
}

#[must_use]
pub fn rlogis_inner(location: f64, scale: f64) -> f64 {
    if isnan(location) || !r_finite(scale) {
        return ml_warn_return_nan();
    }

    if scale == 0.0 || !r_finite(location) {
        return location;
    } else {
        let u = unif_rand();
        location + scale * log(u / (1.0 - u))
    }
}

// ---- FFI shims ----

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn Rf_dlogis(x: f64, location: f64, scale: f64, give_log: i32) -> f64 {
    dlogis_inner(x, location, scale, give_log != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn dlogis(x: f64, location: f64, scale: f64, give_log: i32) -> f64 {
    dlogis_inner(x, location, scale, give_log != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn Rf_plogis(x: f64, location: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    plogis_inner(x, location, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn plogis(x: f64, location: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    plogis_inner(x, location, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn Rf_qlogis(p: f64, location: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    qlogis_inner(p, location, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn qlogis(p: f64, location: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    qlogis_inner(p, location, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn Rf_rlogis(location: f64, scale: f64) -> f64 {
    rlogis_inner(location, scale)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn rlogis(location: f64, scale: f64) -> f64 {
    rlogis_inner(location, scale)
}
