// Mathematical constants and error codes for R nmath
//
// This module centralizes all constants used across the nmath library,
// matching R's nmath.h definitions for compatibility.

// Infinity and NaN constants matching R's ML_* definitions
pub const ML_POSINF: f64 = f64::INFINITY;
pub const ML_NEGINF: f64 = f64::NEG_INFINITY;
pub const ML_NAN: f64 = f64::NAN;

// Error codes for ML_WARNING
pub const ML_VALID: u32 = 0;
pub const ME_DOMAIN: u32 = 1;
pub const ME_RANGE: u32 = 2;
pub const ME_NOCONV: u32 = 4;
pub const ME_PRECISION: u32 = 8;
pub const ME_UNDERFLOW: u32 = 16;

// IEEE 754 floating-point constants
pub const DBL_EPSILON: f64 = 2.220446049250313e-16;
pub const DBL_MIN: f64 = 2.2250738585072014e-308;
pub const DBL_MAX: f64 = 1.7976931348623157e+308;
pub const DBL_MIN_EXP: i32 = -1022;
pub const DBL_MAX_EXP: i32 = 1024;
pub const DBL_MANT_DIG: i32 = 53;

// Mathematical constants
pub const M_LN2: f64 = 0.693147180559945309417232121458;
pub const M_LN_2PI: f64 = 1.837877066409345483560659472811;
pub const M_SQRT_2PI: f64 = 2.50662827463100050241576528481104525301;
pub const M_1_SQRT_2PI: f64 = 0.398942280401432677939946059934;
pub const M_LN_SQRT_2PI: f64 = 0.918938533204672741780329736406;
pub const M_SQRT2: f64 = 1.414213562373095048801688724209;
pub const M_SQRT_32: f64 = 5.656854249492380195206754896838;
pub const M_2PI: f64 = 6.283185307179586476925286766559;
pub const M_LN_SQRT2: f64 = 0.346573590279972654708616060729;
pub const M_PI: f64 = 3.141592653589793238462643383280;

// Algorithm-specific constants
pub const SCALEFACTOR: f64 = 1.157920892373162e77;
pub const M_CUTOFF: f64 = M_LN2 * (DBL_MAX_EXP as f64) / DBL_EPSILON;
pub const M_LN4: f64 = 1.386294361119890618834464242916;
pub const BIG: f64 = 134217728.0;
pub const EXP_M1: f64 = 0.36787944117144232159;
pub const SQRT32: f64 = 5.656854249492380195206754896838;

/// Check if value is NaN
#[inline(always)]
pub fn r_isnan(x: f64) -> bool {
    x.is_nan()
}

/// Check if value is finite
#[inline(always)]
pub fn r_finite(x: f64) -> bool {
    x.is_finite()
}
