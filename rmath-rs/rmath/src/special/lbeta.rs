// Ported from R's nmath/lbeta.c
//
// This function returns the value of the log beta function:
//   log B(a,b) = log G(a) + log G(b) - log G(a+b)
//
// Original by W. Fullerton of Los Alamos Scientific Laboratory.

use crate::constants::*;
use crate::error::*;
use crate::special::gamma::{gammafn, lgammafn};
use crate::special::lgammacor::lgammacor;
use libm::{fabs, log, log1p};

const M_LN_SQRT_2PI: f64 = 0.918938533204672741780329736406; // log(sqrt(2*pi))

mod imp {
    use super::*;

    pub fn lbeta(a: f64, b: f64) -> f64 {
        // NaNs propagated correctly
        if isnan(a) || isnan(b) {
            return a + b;
        }

        let mut p = a;
        let mut q = a;
        if b < p {
            p = b;
        } // := min(a,b)
        if b > q {
            q = b;
        } // := max(a,b)

        // both arguments must be >= 0
        if p < 0.0 {
            return ml_warn_return_nan();
        } else if p == 0.0 {
            return ML_POSINF;
        } else if !r_finite(q) {
            // q == +Inf
            return ML_NEGINF;
        }

        if p >= 10.0 {
            // p and q are big.
            let corr = lgammacor(p) + lgammacor(q) - lgammacor(p + q);
            return log(q) * -0.5
                + M_LN_SQRT_2PI
                + corr
                + (p - 0.5) * log(p / (p + q))
                + q * log1p(-p / (p + q));
        } else if q >= 10.0 {
            // p is small, but q is big.
            let corr = lgammacor(q) - lgammacor(p + q);
            return lgammafn(p) + corr + p - p * log(p + q) + (q - 0.5) * log1p(-p / (p + q));
        } else {
            // p and q are small: p <= q < 10.
            // R change for very small args
            if p < 1e-306 {
                return log(fabs(p)) + (lgammafn(q) - lgammafn(p + q));
            } else {
                return log(gammafn(p) * (gammafn(q) / gammafn(p + q)));
            }
        }
    }
}

/// Compute the log of the beta function: log B(a, b).
pub fn lbeta(a: f64, b: f64) -> f64 {
    imp::lbeta(a, b)
}

// =====================================================================
// C FFI shims
// =====================================================================

#[unsafe(no_mangle)]
pub extern "C" fn Rf_lbeta(a: f64, b: f64) -> f64 {
    imp::lbeta(a, b)
}

#[unsafe(no_mangle)]
pub extern "C" fn lbeta_c(a: f64, b: f64) -> f64 {
    imp::lbeta(a, b)
}
