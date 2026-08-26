// Ported from R's nmath/beta.c
//
// Beta function: B(a, b) = gamma(a)*gamma(b)/gamma(a+b)
//
// Original by W. Fullerton of Los Alamos Scientific Laboratory.
// Some modifications for IEEE 754 compliance.

use crate::constants::*;
use crate::error::ml_warn_return_nan;
use crate::special::gamma::gammafn;
use crate::special::lbeta::lbeta;
use libm::exp;

// For IEEE double precision DBL_EPSILON = 2^-52:
//   xmax from gammalims.c
const XMAX: f64 = 171.61447887182298;

mod imp {
    use super::*;

    pub fn beta(a: f64, b: f64) -> f64 {
        // NaNs propagated correctly
        if isnan(a) || isnan(b) {
            return a + b;
        }

        if a < 0.0 || b < 0.0 {
            return ml_warn_return_nan();
        } else if a == 0.0 || b == 0.0 {
            return ML_POSINF;
        } else if !r_finite(a) || !r_finite(b) {
            return 0.0;
        }

        if a + b < XMAX {
            // All the terms are positive, and all can be large for large
            // or small arguments. They are never much less than one.
            // gammafn(x) can still overflow for x ~ 1e-308,
            // but the result would too.
            let gab = gammafn(a + b);
            if gab == 0.0 {
                return 0.0;
            }
            (1.0 / gab) * gammafn(a) * gammafn(b)
        } else {
            let val = lbeta(a, b);
            exp(val)
        }
    }
}

/// Compute the beta function B(a, b) = gamma(a)*gamma(b)/gamma(a+b).
pub fn beta(a: f64, b: f64) -> f64 {
    imp::beta(a, b)
}

// =====================================================================
// C FFI shims
// =====================================================================

pub fn Rf_beta(a: f64, b: f64) -> f64 {
    imp::beta(a, b)
}

pub fn beta_c(a: f64, b: f64) -> f64 {
    imp::beta(a, b)
}
