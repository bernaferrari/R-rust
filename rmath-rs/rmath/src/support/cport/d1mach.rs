//! Port of R's nmath/d1mach.c -- double-precision machine constants.
//!
//! Original copyright:
//!   Mathlib - A Mathematical Function Library
//!   Copyright (C) 1998  Ross Ihaka
//!   Copyright (C) 2000-2024 The R Core Team
//!
//! These functions return fundamental double-precision machine constants.
//! They are kept for compatibility with Fortran-era code that calls d1mach().
//! New code should use the DBL_* macros directly.

#![allow(non_snake_case)]

/// Return double-precision machine constants by index.
///
/// Ported from R's `Rf_d1mach(int i)` in nmath/d1mach.c.
///
/// # Indices
/// - 1: DBL_MIN (smallest positive normalized number)
/// - 2: DBL_MAX (largest finite number)
/// - 3: FLT_RADIX^(-DBL_MANT_DIG) = 0.5*DBL_EPSILON
/// - 4: FLT_RADIX^(1-DBL_MANT_DIG) = DBL_EPSILON
/// - 5: log10(2)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_d1mach(i: std::os::raw::c_int) -> f64 {
    match i {
        1 => f64::MIN_POSITIVE,  // DBL_MIN
        2 => f64::MAX,           // DBL_MAX
        3 => 0.5 * f64::EPSILON, // FLT_RADIX ^ -DBL_MANT_DIG
        4 => f64::EPSILON,       // FLT_RADIX ^ (1-DBL_MANT_DIG)
        5 => std::f64::consts::LOG10_2,
        _ => 0.0,
    }
}

/// Fortran-compatible entry point: `double F77_NAME(d1mach)(int *)`.
///
/// Takes a pointer to the index (Fortran pass-by-reference convention).
/// The name is mangled to match Fortran calling conventions (lowercase, trailing underscore).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn d1mach_(i: *const std::os::raw::c_int) -> f64 {
    unsafe { Rf_d1mach(*i) }
}
