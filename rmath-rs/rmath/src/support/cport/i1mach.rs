//! Port of R's nmath/i1mach.c -- integer machine constants.
//!
//! Original copyright:
//!   Mathlib - A Mathematical Function Library
//!   Copyright (C) 1998  Ross Ihaka
//!   Copyright (C) 2000-2024 The R Core Team
//!
//! These functions return fundamental integer machine constants.
//! They are kept for compatibility with Fortran-era code that calls i1mach().

#![allow(non_snake_case)]

/// Return integer machine constants by index.
///
/// Ported from R's `Rf_i1mach(int i)` in nmath/i1mach.c.
///
/// # Indices (IEEE 754, 32-bit int)
/// - 1-4: standard input/output/error units (5,6,0,0)
/// - 5: bits per int (CHAR_BIT * sizeof(int))
/// - 6: sizeof(int)/sizeof(char)
/// - 7: radix (2)
/// - 8: digits of int (CHAR_BIT * sizeof(int) - 1)
/// - 9: largest int (INT_MAX)
/// - 10: float radix (FLT_RADIX)
/// - 11-13: float mantissa/min_exp/max_exp
/// - 14-16: double mantissa/min_exp/max_exp
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_i1mach(i: std::os::raw::c_int) -> std::os::raw::c_int {
    match i {
        1 => 5,
        2 => 6,
        3 => 0,
        4 => 0,
        5 => 8 * std::mem::size_of::<i32>() as i32, // CHAR_BIT * sizeof(int)
        6 => (std::mem::size_of::<i32>() / std::mem::size_of::<i8>()) as i32,
        7 => 2,                                         // FLT_RADIX
        8 => 8 * std::mem::size_of::<i32>() as i32 - 1, // CHAR_BIT * sizeof(int) - 1
        9 => i32::MAX,
        10 => 2,                           // FLT_RADIX
        11 => f32::MANTISSA_DIGITS as i32, // FLT_MANT_DIGITS = 24
        12 => f32::MIN_EXP,                // FLT_MIN_EXP = -125
        13 => f32::MAX_EXP,                // FLT_MAX_EXP = 128
        14 => f64::MANTISSA_DIGITS as i32, // DBL_MANT_DIGITS = 53
        15 => f64::MIN_EXP,                // DBL_MIN_EXP = -1021
        16 => f64::MAX_EXP,                // DBL_MAX_EXP = 1024
        _ => 0,
    }
}

/// Fortran-compatible entry point: `int F77_NAME(i1mach)(int *)`.
///
/// Takes a pointer to the index (Fortran pass-by-reference convention).
/// The name is mangled to match Fortran calling conventions (lowercase, trailing underscore).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn i1mach_(i: *const std::os::raw::c_int) -> std::os::raw::c_int {
    unsafe { Rf_i1mach(*i) }
}
