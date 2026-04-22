// Lognormal distribution: dlnorm, plnorm, qlnorm, rlnorm
// Ported from dlnorm.c, plnorm.c, qlnorm.c, rlnorm.c

use crate::nmath::constants::*;
use crate::nmath::dist::normal::{pnorm5_inner, qnorm5_inner, rnorm_inner};
use crate::nmath::dpq::*;
use crate::nmath::error::*;
use libm::*;

// Constants
const M_LN_SQRT_2PI: f64 = 0.918938533204672741780329736406; // log(sqrt(2*pi))
const M_1_SQRT_2PI: f64 = 0.398942280401432677939946059934; // 1/sqrt(2*pi)

// ---- Inner implementations ----

#[must_use]
pub fn dlnorm_inner(x: f64, meanlog: f64, sdlog: f64, give_log: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(meanlog) || isnan(sdlog) {
        return x + meanlog + sdlog;
    }
    if sdlog < 0.0 {
        return ml_warn_return_nan();
    }
    if !r_finite(x) && log(x) == meanlog {
        return ML_NAN; /* log(x) - meanlog is NaN */
    }
    if sdlog == 0.0 {
        return if log(x) == meanlog {
            ML_POSINF
        } else {
            r_d__0(give_log)
        };
    }
    if x <= 0.0 {
        return r_d__0(give_log);
    }

    let y = (log(x) - meanlog) / sdlog;
    if give_log {
        -(M_LN_SQRT_2PI + 0.5 * y * y + log(x * sdlog))
    } else {
        M_1_SQRT_2PI * exp(-0.5 * y * y) / (x * sdlog)
    }
}

#[must_use]
pub fn plnorm_inner(x: f64, meanlog: f64, sdlog: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(meanlog) || isnan(sdlog) {
        return x + meanlog + sdlog;
    }
    if sdlog < 0.0 {
        return ml_warn_return_nan();
    }

    if x > 0.0 {
        pnorm5_inner(log(x), meanlog, sdlog, lower_tail, log_p)
    } else {
        r_dt_0(lower_tail, log_p)
    }
}

#[must_use]
pub fn qlnorm_inner(p: f64, meanlog: f64, sdlog: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(p) || isnan(meanlog) || isnan(sdlog) {
        return p + meanlog + sdlog;
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

    exp(qnorm5_inner(p, meanlog, sdlog, lower_tail, log_p))
}

#[must_use]
pub fn rlnorm_inner(meanlog: f64, sdlog: f64) -> f64 {
    if isnan(meanlog) || !r_finite(sdlog) || sdlog < 0.0 {
        return ml_warn_return_nan();
    }

    exp(rnorm_inner(meanlog, sdlog))
}

// ---- FFI shims ----

#[must_use]
pub fn Rf_dlnorm(x: f64, meanlog: f64, sdlog: f64, give_log: i32) -> f64 {
    dlnorm_inner(x, meanlog, sdlog, give_log != 0)
}

#[must_use]
pub fn dlnorm(x: f64, meanlog: f64, sdlog: f64, give_log: i32) -> f64 {
    dlnorm_inner(x, meanlog, sdlog, give_log != 0)
}

#[must_use]
pub fn Rf_plnorm(x: f64, meanlog: f64, sdlog: f64, lower_tail: i32, log_p: i32) -> f64 {
    plnorm_inner(x, meanlog, sdlog, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn plnorm(x: f64, meanlog: f64, sdlog: f64, lower_tail: i32, log_p: i32) -> f64 {
    plnorm_inner(x, meanlog, sdlog, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_qlnorm(p: f64, meanlog: f64, sdlog: f64, lower_tail: i32, log_p: i32) -> f64 {
    qlnorm_inner(p, meanlog, sdlog, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn qlnorm(p: f64, meanlog: f64, sdlog: f64, lower_tail: i32, log_p: i32) -> f64 {
    qlnorm_inner(p, meanlog, sdlog, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_rlnorm(meanlog: f64, sdlog: f64) -> f64 {
    rlnorm_inner(meanlog, sdlog)
}

#[must_use]
pub fn rlnorm(meanlog: f64, sdlog: f64) -> f64 {
    rlnorm_inner(meanlog, sdlog)
}
