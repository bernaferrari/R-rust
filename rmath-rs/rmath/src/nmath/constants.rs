// Constants from R's nmath.h (standalone section)
//
// This module centralizes all mathematical constants used across the nmath library.
// Previously, constants like M_LN2, DBL_EPSILON, etc. were duplicated in 5+ files.

// ─── Infinity and NaN ─────────────────────────────────────────────────────

pub const ML_POSINF: f64 = f64::INFINITY;
pub const ML_NEGINF: f64 = f64::NEG_INFINITY;
pub const ML_NAN: f64 = f64::NAN;

// ─── Error codes ──────────────────────────────────────────────────────────

pub const ML_VALID: u32 = 0;
pub const ME_DOMAIN: u32 = 1;
pub const ME_RANGE: u32 = 2;
pub const ME_NOCONV: u32 = 4;
pub const ME_PRECISION: u32 = 8;
pub const ME_UNDERFLOW: u32 = 16;

// ─── IEEE 754 floating-point constants ────────────────────────────────────

pub const DBL_EPSILON: f64 = 2.220446049250313e-16;
pub const DBL_MIN: f64 = 2.2250738585072014e-308;
pub const DBL_MAX: f64 = 1.7976931348623157e+308;
pub const DBL_MIN_EXP: i32 = -1022;
pub const DBL_MAX_EXP: i32 = 1024;
pub const DBL_MANT_DIG: i32 = 53;

// ─── Mathematical constants ───────────────────────────────────────────────

pub const M_LN2: f64 = 0.693147180559945309417232121458;
pub const M_LN_2PI: f64 = 1.837877066409345483560659472811;
pub const M_SQRT_2PI: f64 = 2.50662827463100050241576528481104525301;
pub const M_1_SQRT_2PI: f64 = 0.398942280401432677939946059934;
pub const M_LN_SQRT_2PI: f64 = 0.918938533204672741780329736406;
pub const M_SQRT2: f64 = 1.414213562373095048801688724209;
pub const M_SQRT_32: f64 = 5.656854249492380195206754896838;
pub const M_2PI: f64 = 6.283185307179586476925286766559;
pub const M_LN_SQRT2: f64 = 0.346573590279972654708616060729;

// ─── Algorithm-specific constants ─────────────────────────────────────────

/// Scalefactor:= (2^32)^8 = 2^256 = 1.157921e+77
pub const SCALEFACTOR: f64 = {
    let s1: f64 = 4294967296.0;
    let s2 = s1 * s1;
    let s3 = s2 * s2;
    s3 * s3
};

/// If |x| > |k| * M_cutoff, then log[ exp(-x) * k^x ] =~= -x
pub const M_CUTOFF: f64 = M_LN2 * (DBL_MAX_EXP as f64) / DBL_EPSILON;

/// = log(4), used in beta distribution algorithms
pub const M_LN4: f64 = 1.386294361119890618834464242916;

/// = 2^27, used for high-precision uniform generation
pub const BIG: f64 = 134217728.0;

/// exp(-1) = 1/e
pub const EXP_M1: f64 = 0.36787944117144232159;

/// sqrt(32) — full double precision
pub const SQRT32: f64 = 5.656854249492380195206754896838;

// ─── NaN/Finite checks ────────────────────────────────────────────────────

/// IEEE 754 NaN check (preferred name: r_isnan)
#[inline(always)]
pub fn r_isnan(x: f64) -> bool {
    x.is_nan()
}

/// Alias for r_isnan — kept for backward compatibility with existing callers.
/// Note: when `use libm::*` is also present, libm::isnan takes precedence.
/// Both produce identical results.
#[inline(always)]
pub fn isnan(x: f64) -> bool {
    r_isnan(x)
}

/// Finite check (matches R's R_FINITE for standalone)
#[inline(always)]
pub fn r_finite(x: f64) -> bool {
    x.is_finite()
}
