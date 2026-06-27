// Weibull distribution: dweibull, pweibull, qweibull, rweibull
// Ported from dweibull.c, pweibull.c, qweibull.c, rweibull.c

use crate::nmath::constants::*;
use crate::nmath::dist::exponential::exp_rand;
use crate::nmath::dpq::*;
use crate::nmath::error::*;
use libm::*;

// ---- Inner implementations ----

#[must_use]
pub fn dweibull_inner(x: f64, shape: f64, scale: f64, give_log: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(shape) || isnan(scale) {
        return x + shape + scale;
    }
    if shape < 0.0 || scale <= 0.0 {
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
    if shape < 0.0 || scale <= 0.0 {
        return ml_warn_return_nan();
    }

    // R_P_bounds_01(x, 0., ML_POSINF)
    if x <= 0.0 {
        return r_d__0(log_p);
    }
    if x >= ML_POSINF {
        return r_d__1(log_p);
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
    if shape < 0.0 || scale <= 0.0 {
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
    if !r_finite(shape) || !r_finite(scale) || shape < 0.0 || scale <= 0.0 {
        if scale == 0.0 {
            return 0.0;
        }
        /* else */
        return ml_warn_return_nan();
    }
    if shape == 0.0 {
        return if exp_rand() <= 1.0 { 0.0 } else { ML_POSINF };
    }

    scale * pow(-log(exp_rand()), 1.0 / shape)
}

// ---- FFI shims ----

#[must_use]
pub fn Rf_dweibull(x: f64, shape: f64, scale: f64, give_log: i32) -> f64 {
    dweibull_inner(x, shape, scale, give_log != 0)
}

#[must_use]
pub fn dweibull(x: f64, shape: f64, scale: f64, give_log: i32) -> f64 {
    dweibull_inner(x, shape, scale, give_log != 0)
}

#[must_use]
pub fn Rf_pweibull(x: f64, shape: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    pweibull_inner(x, shape, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn pweibull(x: f64, shape: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    pweibull_inner(x, shape, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_qweibull(p: f64, shape: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    qweibull_inner(p, shape, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn qweibull(p: f64, shape: f64, scale: f64, lower_tail: i32, log_p: i32) -> f64 {
    qweibull_inner(p, shape, scale, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_rweibull(shape: f64, scale: f64) -> f64 {
    rweibull_inner(shape, scale)
}

#[must_use]
pub fn rweibull(shape: f64, scale: f64) -> f64 {
    rweibull_inner(shape, scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-10;

    #[test]
    fn weibull_pq_roundtrip() {
        let probs = [0.01, 0.1, 0.25, 0.5, 0.75, 0.9, 0.99];
        for &p in &probs {
            let x = qweibull_inner(p, 2.0, 1.0, true, false);
            let p2 = pweibull_inner(x, 2.0, 1.0, true, false);
            assert!((p2 - p).abs() < TOL, "roundtrip failed at p={p}: got {p2}");
        }
    }

    #[test]
    fn weibull_density_non_negative() {
        for &x in &[-1.0, 0.0, 0.5, 1.0, 5.0] {
            let d = dweibull_inner(x, 2.0, 1.0, false);
            assert!(
                d >= 0.0 || d.is_infinite(),
                "density negative at x={x}: {d}"
            );
        }
    }

    #[test]
    fn weibull_cdf_boundary() {
        assert_eq!(pweibull_inner(0.0, 2.0, 1.0, true, false), 0.0);
        assert_eq!(pweibull_inner(f64::INFINITY, 2.0, 1.0, true, false), 1.0);
    }

    /// PR#19055: shape = 0 is now supported (previously returned NaN).
    #[test]
    fn weibull_shape_zero_is_supported_not_nan() {
        // Density degenerates: spike at 0, zero elsewhere.
        assert!(dweibull_inner(0.0, 0.0, 1.0, false).is_infinite());
        assert_eq!(dweibull_inner(1.5, 0.0, 1.0, false), 0.0);
        // CDF: 0 at x<=0, jumps to 1-e^{-1} for any x>0, reaches 1 at +Inf.
        assert_eq!(pweibull_inner(0.0, 0.0, 1.0, true, false), 0.0);
        assert!(
            (pweibull_inner(1.5, 0.0, 1.0, true, false) - (1.0 - 1.0 / std::f64::consts::E)).abs()
                < TOL
        );
        assert_eq!(pweibull_inner(f64::INFINITY, 0.0, 1.0, true, false), 1.0);
        // Quantile: median is 0; mass above 1-1/e maps to +Inf.
        assert_eq!(qweibull_inner(0.5, 0.0, 1.0, true, false), 0.0);
        assert!(qweibull_inner(0.9, 0.0, 1.0, true, false).is_infinite());
    }

    /// rweibull with shape = 0 returns only 0 or +Inf (never NaN).
    #[test]
    fn weibull_shape_zero_random_is_zero_or_inf() {
        let _session = crate::sexp::session::RSession::new();
        for _ in 0..100 {
            let r = rweibull_inner(0.0, 1.0);
            assert!(
                r == 0.0 || r.is_infinite(),
                "rweibull(shape=0) unexpected: {r}"
            );
        }
    }

    /// Negative shape still warns and returns NaN (guard retained).
    #[test]
    fn weibull_negative_shape_is_nan() {
        assert!(dweibull_inner(1.0, -1.0, 1.0, false).is_nan());
        assert!(pweibull_inner(1.0, -1.0, 1.0, true, false).is_nan());
    }
}
