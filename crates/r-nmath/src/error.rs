// Error handling for R nmath library
//
// Provides ML_WARNING and related error reporting functions,
// matching R's error handling semantics for numerical computations.

use crate::constants::{ME_DOMAIN, ME_NOCONV, ME_PRECISION, ME_RANGE, ME_UNDERFLOW, ML_NAN};

/// Print a warning message for numerical computation issues.
/// Matches R's ML_WARNING macro behavior.
pub fn ml_warning(err_code: u32, s: &str) {
    if err_code == 0 {
        return;
    }

    let msg = match err_code {
        ME_DOMAIN => "argument out of domain",
        ME_RANGE => "value out of range",
        ME_NOCONV => "convergence failed",
        ME_PRECISION => "full precision may not have been achieved",
        ME_UNDERFLOW => "underflow occurred in function",
        _ => "unknown error",
    };

    if s.is_empty() {
        eprintln!("Warning: {}", msg);
    } else {
        eprintln!("Warning: {} in '{}'", msg, s);
    }
}

/// Return NaN after issuing a domain warning.
/// This is the standard R pattern for invalid arguments.
pub fn ml_warn_return_nan(s: &str) -> f64 {
    ml_warning(ME_DOMAIN, s);
    ML_NAN
}
