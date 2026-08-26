// Chi-squared distribution: dchisq, pchisq, qchisq, rchisq
// Ported from dchisq.c, pchisq.c, qchisq.c, rchisq.c

use crate::constants::*;
use crate::error::*;

// ---- dchisq ----

#[must_use]
pub fn dchisq_inner(x: f64, df: f64, give_log: bool) -> f64 {
    crate::dist::gamma::dgamma_inner(x, df / 2.0, 2.0, give_log)
}

// ---- pchisq ----

#[must_use]
pub fn pchisq_inner(x: f64, df: f64, lower_tail: bool, log_p: bool) -> f64 {
    crate::dist::gamma::pgamma_inner(x, df / 2.0, 2.0, lower_tail, log_p)
}

// ---- qchisq ----

#[must_use]
pub fn qchisq_inner(p: f64, df: f64, lower_tail: bool, log_p: bool) -> f64 {
    crate::dist::gamma::qgamma_inner(p, 0.5 * df, 2.0, lower_tail, log_p)
}

// ---- rchisq ----

#[must_use]
pub fn rchisq_inner(df: f64) -> f64 {
    if !r_finite(df) || df < 0.0 {
        return ml_warn_return_nan();
    }
    crate::dist::gamma::rgamma_inner(df / 2.0, 2.0)
}

// ---- FFI shims ----

#[must_use]
pub fn Rf_dchisq(x: f64, df: f64, give_log: i32) -> f64 {
    dchisq_inner(x, df, give_log != 0)
}

#[must_use]
pub fn dchisq(x: f64, df: f64, give_log: i32) -> f64 {
    dchisq_inner(x, df, give_log != 0)
}

#[must_use]
pub fn Rf_pchisq(x: f64, df: f64, lower_tail: i32, log_p: i32) -> f64 {
    pchisq_inner(x, df, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn pchisq(x: f64, df: f64, lower_tail: i32, log_p: i32) -> f64 {
    pchisq_inner(x, df, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_qchisq(p: f64, df: f64, lower_tail: i32, log_p: i32) -> f64 {
    qchisq_inner(p, df, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn qchisq(p: f64, df: f64, lower_tail: i32, log_p: i32) -> f64 {
    qchisq_inner(p, df, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_rchisq(df: f64) -> f64 {
    rchisq_inner(df)
}

#[must_use]
pub fn rchisq(df: f64) -> f64 {
    rchisq_inner(df)
}
