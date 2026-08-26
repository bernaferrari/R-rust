// Error handling from R's nmath.h
// ML_WARNING, ML_WARN_return_NAN macros translated to Rust

use crate::constants::*;

/// Print a mathlib warning to stderr.
/// In standalone mode, this prints to stderr.
/// In integrated mode, this would call Rf_warning via FFI.
#[inline]
pub fn ml_warning(err_code: u32, func_name: &str) {
    let msg = match err_code {
        ME_DOMAIN => "argument out of domain in '%s'\n",
        ME_RANGE => "value out of range in '%s'\n",
        ME_NOCONV => "convergence failed in '%s'\n",
        ME_PRECISION => "full precision may not have been achieved in '%s'\n",
        ME_UNDERFLOW => "underflow occurred in '%s'\n",
        _ => "unknown error in '%s'\n",
    };
    let formatted = msg.replace("%s", func_name);
    eprint!("{formatted}");
}

/// Return NaN after issuing a domain warning.
/// This is a macro in C: { ML_WARNING(ME_DOMAIN, ""); return ML_NAN; }
#[inline]
pub fn ml_warn_return_nan() -> f64 {
    ml_warning(ME_DOMAIN, "");
    ML_NAN
}
