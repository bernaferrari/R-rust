use super::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's NA_REAL sentinel (NaN with specific bit pattern).
pub const NA_REAL: f64 = crate::sexp::ffi::NA_REAL;

/// IEEE double epsilon (machine epsilon).
pub(crate) const C_EPS: f64 = f64::EPSILON;

// ---------------------------------------------------------------------------
// NA/NaN helpers
// ---------------------------------------------------------------------------

/// Returns the IEEE double representation of R's NA value.
pub extern "C" fn R_ValueOfNA() -> f64 {
    NA_REAL
}

/// Check if a NaN value is specifically R's NA (not just any NaN).
pub extern "C" fn R_NaN_is_R_NA(x: f64) -> c_int {
    if crate::sexp::ffi::is_na_real(x) {
        1
    } else {
        0
    }
}

/// Check if a value is R's NA.
pub extern "C" fn R_IsNA(x: f64) -> c_int {
    if x.is_nan() && R_NaN_is_R_NA(x) != 0 {
        1
    } else {
        0
    }
}

/// Check if a value is NaN but not R's NA.
pub extern "C" fn R_IsNaN(x: f64) -> c_int {
    if x.is_nan() && R_NaN_is_R_NA(x) == 0 {
        1
    } else {
        0
    }
}

/// Finite check.
#[inline]
pub fn R_FINITE(x: f64) -> bool {
    x.is_finite()
}
