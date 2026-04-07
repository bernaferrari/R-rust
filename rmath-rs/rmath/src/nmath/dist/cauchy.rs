// Cauchy distribution: dcauchy, pcauchy, qcauchy, rcauchy
// Ported from dcauchy.c, pcauchy.c, qcauchy.c, rcauchy.c

use crate::nmath::constants::*;
use crate::nmath::dpq::*;
use crate::nmath::error::*;
use crate::nmath::rng::*;
use crate::nmath::special::cospi::tanpi;
use libm::*;

const PI: f64 = 3.14159265358979323846264338327950288;

// ---- Inner implementations ----

#[must_use]
pub fn dcauchy_inner(x: f64, location: f64, scale: f64, give_log: bool) -> f64 {
    if isnan(x) || isnan(location) || isnan(scale) {
        return x + location + scale;
    }
    if scale <= 0.0 {
        return ml_warn_return_nan();
    }

    let y = (x - location) / scale;
    if give_log {
        -log(PI * scale * (1.0 + y * y))
    } else {
        1.0 / (PI * scale * (1.0 + y * y))
    }
}

#[must_use]
pub fn pcauchy_inner(x: f64, location: f64, scale: f64, lower_tail: bool, log_p: bool) -> f64 {
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
    if !r_finite(x) {
        if x < 0.0 {
            return r_dt_0(lower_tail, log_p);
        } else {
            return r_dt_1(lower_tail, log_p);
        }
    }

    let x = if !lower_tail { -x } else { x };

    // For large x, use atan(1/x) to avoid cancellation
    if fabs(x) > 1.0 {
        let y = atan(1.0 / x) / PI;
        if x > 0.0 {
            r_d_clog(y, log_p)
        } else {
            r_d_val(-y, log_p)
        }
    } else {
        r_d_val(0.5 + atan(x) / PI, log_p)
    }
}

#[must_use]
pub fn qcauchy_inner(p: f64, location: f64, scale: f64, lower_tail: bool, log_p: bool) -> f64 {
    if isnan(p) || isnan(location) || isnan(scale) {
        return p + location + scale;
    }

    // R_Q_P01_check(p)
    if (log_p && p > 0.0) || (!log_p && (p < 0.0 || p > 1.0)) {
        return ml_warn_return_nan();
    }
    if scale <= 0.0 || !r_finite(scale) {
        if scale == 0.0 {
            return location;
        }
        return ml_warn_return_nan();
    }

    let my_inf = location + (if lower_tail { scale } else { -scale }) * ML_POSINF;

    if log_p {
        if p > -1.0 {
            if p == 0.0 {
                return my_inf;
            }
            let lt = !lower_tail;
            let p = -expm1(p);
            // Continue with !lower_tail, transformed p
            if p == 0.5 {
                return location;
            }
            if p == 0.0 {
                return location + (if lt { scale } else { -scale }) * ML_NEGINF;
            }
            return location + (if lt { -scale } else { scale }) / tanpi(p);
        } else {
            let p = exp(p);
            if p == 0.5 {
                return location;
            }
            if p == 0.0 {
                return location + (if lower_tail { scale } else { -scale }) * ML_NEGINF;
            }
            return location + (if lower_tail { -scale } else { scale }) / tanpi(p);
        }
    } else {
        let mut p = p;
        let mut lt = lower_tail;
        if p > 0.5 {
            if p == 1.0 {
                return my_inf;
            }
            p = 1.0 - p;
            lt = !lt;
        }
        if p == 0.5 {
            return location;
        }
        if p == 0.0 {
            return location + (if lt { scale } else { -scale }) * ML_NEGINF;
        }
        location + (if lt { -scale } else { scale }) / tanpi(p)
    }
}

#[must_use]
pub fn rcauchy_inner(location: f64, scale: f64) -> f64 {
    if isnan(location) || !r_finite(scale) || scale < 0.0 {
        return ml_warn_return_nan();
    }
    if scale == 0.0 || !r_finite(location) {
        return location;
    }
    location + scale * tan(PI * unif_rand())
}

// ---- FFI shims ----

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn Rf_dcauchy(x: f64, location: f64, scale: f64, give_log: i32) -> f64 {
    dcauchy_inner(x, location, scale, give_log != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn dcauchy(x: f64, location: f64, scale: f64, give_log: i32) -> f64 {
    dcauchy_inner(x, location, scale, give_log != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_pcauchy(
    x: f64,
    location: f64,
    scale: f64,
    lower_tail: i32,
    log_p: i32,
) -> f64 {
    pcauchy_inner(x, location, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn pcauchy(x: f64, location: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    pcauchy_inner(x, location, scale, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_qcauchy(
    p: f64,
    location: f64,
    scale: f64,
    lower_tail: i32,
    log_p: i32,
) -> f64 {
    qcauchy_inner(p, location, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn qcauchy(p: f64, location: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    qcauchy_inner(p, location, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn Rf_rcauchy(location: f64, scale: f64) -> f64 {
    rcauchy_inner(location, scale)
}

#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn rcauchy(location: f64, scale: f64) -> f64 {
    rcauchy_inner(location, scale)
}
