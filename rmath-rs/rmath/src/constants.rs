// Constants from R's nmath.h (standalone section)

pub const ML_POSINF: f64 = 1.0 / 0.0;
pub const ML_NEGINF: f64 = (-1.0) / 0.0;
pub const ML_NAN: f64 = 0.0 / 0.0;

pub const ML_VALID: u32 = 0;
pub const ME_DOMAIN: u32 = 1;
pub const ME_RANGE: u32 = 2;
pub const ME_NOCONV: u32 = 4;
pub const ME_PRECISION: u32 = 8;
pub const ME_UNDERFLOW: u32 = 16;

/// IEEE 754 NaN check
#[inline(always)]
pub fn isnan(x: f64) -> bool {
    x != x
}

/// Finite check (matches R's R_FINITE for standalone)
#[inline(always)]
pub fn r_finite(x: f64) -> bool {
    !isnan(x) && x != ML_POSINF && x != ML_NEGINF
}
