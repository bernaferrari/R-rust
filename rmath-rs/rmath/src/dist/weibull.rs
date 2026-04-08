// Weibull distribution: dweibull, pweibull, qweibull, rweibull
// Ported from dweibull.c, pweibull.c, qweibull.c, rweibull.c

use crate::constants::*;
use crate::dist::exponential::exp_rand;
use crate::dpq::*;
use crate::error::*;
use libm::*;

// ---- Inner implementations ----

#[must_use]
pub fn dweibull_inner(x: f64, shape: f64, scale: f64, give_log: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(shape) || isnan(scale) {
        return x + shape + scale;
    }
    if shape <= 0.0 || scale <= 0.0 {
        return ml_warn_return_nan();
    }

    if x < 0.0 {
        return r_d__0(give_log);
    }
    if !r_finite(x) {
        return r_d__0(give_log);
    }
    // need to handle x == 0 separately
    if x == 0.0 && shape < 1.0 {
        return ML_POSINF;
    }
    let tmp1 = pow(x / scale, shape - 1.0);
    let tmp2 = tmp1 * (x / scale);
    // These are incorrect if tmp1 == 0
    if give_log {
        -tmp2 + log(shape * tmp1 / scale)
    } else {
        shape * tmp1 * exp(-tmp2) / scale
    }
}

#[must_use]
pub fn pweibull_inner(x: f64, shape: f64, scale: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(shape) || isnan(scale) {
        return x + shape + scale;
    }
    if shape <= 0.0 || scale <= 0.0 {
        return ml_warn_return_nan();
    }

    if x <= 0.0 {
        return r_dt_0(lower_tail, log_p);
    }
    let x = -pow(x / scale, shape);
    if lower_tail {
        if log_p { r_log1_exp(x) } else { -expm1(x) }
    } else {
        r_d_exp(x, log_p)
    }
}

#[must_use]
pub fn qweibull_inner(p: f64, shape: f64, scale: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(p) || isnan(shape) || isnan(scale) {
        return p + shape + scale;
    }
    if shape <= 0.0 || scale <= 0.0 {
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

    scale * pow(-r_dt_clog(p, lower_tail, log_p), 1.0 / shape)
}

#[must_use]
pub fn rweibull_inner(shape: f64, scale: f64) -> f64 {
    if !r_finite(shape) || !r_finite(scale) || shape <= 0.0 || scale <= 0.0 {
        if scale == 0.0 {
            return 0.0;
        }
        /* else */
        return ml_warn_return_nan();
    }

    scale * pow(-log(exp_rand()), 1.0 / shape)
}

// ---- FFI shims ----

#[must_use]
pub extern "C" fn Rf_dweibull(x: f64, shape: f64, scale: f64, give_log: i32) -> f64 {
    dweibull_inner(x, shape, scale, give_log != 0)
}

#[must_use]
pub extern "C" fn dweibull(x: f64, shape: f64, scale: f64, give_log: i32) -> f64 {
    dweibull_inner(x, shape, scale, give_log != 0)
}

#[must_use]
pub extern "C" fn Rf_pweibull(x: f64, shape: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    pweibull_inner(x, shape, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn pweibull(x: f64, shape: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    pweibull_inner(x, shape, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn Rf_qweibull(p: f64, shape: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    qweibull_inner(p, shape, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn qweibull(p: f64, shape: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    qweibull_inner(p, shape, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn Rf_rweibull(shape: f64, scale: f64) -> f64 {
    rweibull_inner(shape, scale)
}

#[must_use]
pub extern "C" fn rweibull(shape: f64, scale: f64) -> f64 {
    rweibull_inner(shape, scale)
}
