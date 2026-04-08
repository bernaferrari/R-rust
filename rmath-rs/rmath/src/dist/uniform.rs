// Uniform distribution: dunif, punif, qunif, runif
// Ported from dunif.c, punif.c, qunif.c, runif.c

use crate::constants::*;
use crate::dpq::*;
use crate::error::*;
use crate::rng::*;
use libm::*;

// ---- Inner implementations (Rust bool params) ----

#[must_use]
pub fn dunif_inner(x: f64, a: f64, b: f64, give_log: bool) -> f64 {
    if isnan(x) || isnan(a) || isnan(b) {
        return x + a + b;
    }
    if b <= a {
        return ml_warn_return_nan();
    }

    if a <= x && x <= b {
        return if give_log { -log(b - a) } else { 1.0 / (b - a) };
    }
    r_d__0(give_log)
}

#[must_use]
pub fn punif_inner(x: f64, a: f64, b: f64, lower_tail: bool, log_p: bool) -> f64 {
    if isnan(x) || isnan(a) || isnan(b) {
        return x + a + b;
    }
    if b < a {
        return ml_warn_return_nan();
    }
    if !r_finite(a) || !r_finite(b) {
        return ml_warn_return_nan();
    }

    if x >= b {
        return r_dt_1(lower_tail, log_p);
    }
    if x <= a {
        return r_dt_0(lower_tail, log_p);
    }

    if lower_tail {
        r_d_val((x - a) / (b - a), log_p)
    } else {
        r_d_val((b - x) / (b - a), log_p)
    }
}

#[must_use]
pub fn qunif_inner(p: f64, a: f64, b: f64, lower_tail: bool, log_p: bool) -> f64 {
    if isnan(p) || isnan(a) || isnan(b) {
        return p + a + b;
    }
    // R_Q_P01_check(p)
    if (log_p && p > 0.0) || (!log_p && (p < 0.0 || p > 1.0)) {
        return ml_warn_return_nan();
    }
    if !r_finite(a) || !r_finite(b) {
        return ml_warn_return_nan();
    }
    if b < a {
        return ml_warn_return_nan();
    }
    if b == a {
        return a;
    }

    a + r_dt_qiv(p, lower_tail, log_p) * (b - a)
}

#[must_use]
pub fn runif_inner(a: f64, b: f64) -> f64 {
    if !r_finite(a) || !r_finite(b) || b < a {
        return ml_warn_return_nan();
    }
    if a == b {
        return a;
    }

    let mut u;
    loop {
        u = unif_rand();
        if u > 0.0 && u < 1.0 {
            break;
        }
    }
    a + (b - a) * u
}

// ---- FFI shims (c_int -> bool) ----

#[must_use]
pub extern "C" fn Rf_dunif(x: f64, a: f64, b: f64, give_log: i32) -> f64 {
    dunif_inner(x, a, b, give_log != 0)
}

#[must_use]
pub extern "C" fn dunif(x: f64, a: f64, b: f64, give_log: i32) -> f64 {
    dunif_inner(x, a, b, give_log != 0)
}

#[must_use]
pub extern "C" fn Rf_punif(x: f64, a: f64, b: f64, lower_tail: i32, log_p: i32) -> f64 {
    punif_inner(x, a, b, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn punif(x: f64, a: f64, b: f64, lower_tail: i32, log_p: i32) -> f64 {
    punif_inner(x, a, b, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn Rf_qunif(p: f64, a: f64, b: f64, lower_tail: i32, log_p: i32) -> f64 {
    qunif_inner(p, a, b, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn qunif(p: f64, a: f64, b: f64, lower_tail: i32, log_p: i32) -> f64 {
    qunif_inner(p, a, b, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn Rf_runif(a: f64, b: f64) -> f64 {
    runif_inner(a, b)
}

#[must_use]
pub extern "C" fn runif(a: f64, b: f64) -> f64 {
    runif_inner(a, b)
}
