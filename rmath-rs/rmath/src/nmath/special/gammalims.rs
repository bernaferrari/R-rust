//! Port of R's nmath/gammalims.c -- gamma function limits.
//!
//! Original copyright:
//!   Mathlib : A C Library of Special Functions
//!   Copyright (C) 1998 Ross Ihaka
//!   Copyright (C) 1999-2025 The R Core Team
//!
//! This routine calculates the minimum and maximum legal bounds for x in
//! gammafn(x). These are not the only bounds, but they are the only
//! non-trivial ones to calculate.
//!
//! Translation into C of a Fortran subroutine by W. Fullerton of
//! Los Alamos Scientific Laboratory.

// IEEE 754 constants used directly -- no libm, constants, or d1mach needed

/// Calculate the minimum and maximum legal bounds for `x` in `gammafn(x)`.
///
/// Ported from R's `gammalims(double *xmin, double *xmax)` in nmath/gammalims.c.
///
/// For IEEE 754 systems, these are precomputed constants.
/// For non-IEEE systems, Newton iteration is used.
pub fn gammalims() -> (f64, f64) {
    (-170.5674972726612, 171.61447887182298)
}

pub unsafe fn Rf_gammalims(xmin: *mut f64, xmax: *mut f64) {
    if xmin.is_null() || xmax.is_null() {
        return;
    }

    let (xmin_value, xmax_value) = gammalims();

    unsafe {
        *xmin = xmin_value;
        *xmax = xmax_value;
    }
}
