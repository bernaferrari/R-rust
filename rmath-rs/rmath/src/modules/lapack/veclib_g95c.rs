
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001-2018 The R Core Team
 *
 *  Ported to Rust from R's src/modules/lapack/vecLibg95c.c
 *
 *  These functions provide Fortran-callable wrappers around the CBLAS
 *  dot product functions on macOS. Fortran expects the result to be
 *  returned via an argument pointer, whereas the CBLAS cblas_{c,z}dot{u,c}_sub
 *  functions already work this way. These wrappers convert the Fortran calling
 *  convention (all arguments by reference) to the CBLAS calling convention.
 *
 *  These calls were deprecated in 'new' Accelerate and are warned about, so
 *  suppress the warnings.
 *
 *  FIXME: define ACCELERATE_NEW_LAPACK where appropriate
 *  *and* use new entry points.
 */

#[cfg(target_os = "macos")]
use libc::c_int;

// ============================================================
// Fortran-callable CBLAS wrappers for macOS vecLib compatibility
// ============================================================
//
// These symbols have names matching the Fortran name-mangling convention
// used by g95 (rcblas_ prefix, all lowercase, trailing underscore).
//
// The FC_FUNC_ macro in R expands to the Fortran-callable name.

#[cfg(target_os = "macos")]
unsafe extern "C" {
    /// cblas_cdotu_sub - CBLAS unconjugated complex float dot product
    fn cblas_cdotu_sub(
        n: c_int,
        x: *const libc::c_void,
        incx: c_int,
        y: *const libc::c_void,
        incy: c_int,
        dotu: *mut libc::c_void,
    );

    /// cblas_cdotc_sub - CBLAS conjugated complex float dot product
    fn cblas_cdotc_sub(
        n: c_int,
        x: *const libc::c_void,
        incx: c_int,
        y: *const libc::c_void,
        incy: c_int,
        dotc: *mut libc::c_void,
    );

    /// cblas_zdotu_sub - CBLAS unconjugated double complex dot product
    fn cblas_zdotu_sub(
        n: c_int,
        x: *const libc::c_void,
        incx: c_int,
        y: *const libc::c_void,
        incy: c_int,
        dotu: *mut libc::c_void,
    );

    /// cblas_zdotc_sub - CBLAS conjugated double complex dot product
    fn cblas_zdotc_sub(
        n: c_int,
        x: *const libc::c_void,
        incx: c_int,
        y: *const libc::c_void,
        incy: c_int,
        dotc: *mut libc::c_void,
    );
}

/// Fortran-callable wrapper for cblas_cdotu_sub (unconjugated complex float dot product).
///
/// Fortran interface:
///   CALL RCBLAS_CDOTU_SUB(N, X, INCX, Y, INCY, DOTU)
///
/// All arguments are passed by reference (Fortran convention).
#[cfg(target_os = "macos")]
pub unsafe fn rcblas_cdotu_sub_(
    n: *const c_int,
    x: *const libc::c_void,
    incx: *const c_int,
    y: *const libc::c_void,
    incy: *const c_int,
    dotu: *mut libc::c_void,
) {
    unsafe {
        cblas_cdotu_sub(*n, x, *incx, y, *incy, dotu);
    }
}

/// Fortran-callable wrapper for cblas_cdotc_sub (conjugated complex float dot product).
///
/// Fortran interface:
///   CALL RCBLAS_CDOTC_SUB(N, X, INCX, Y, INCY, DOTC)
///
/// All arguments are passed by reference (Fortran convention).
#[cfg(target_os = "macos")]
pub unsafe fn rcblas_cdotc_sub_(
    n: *const c_int,
    x: *const libc::c_void,
    incx: *const c_int,
    y: *const libc::c_void,
    incy: *const c_int,
    dotc: *mut libc::c_void,
) {
    unsafe {
        cblas_cdotc_sub(*n, x, *incx, y, *incy, dotc);
    }
}

/// Fortran-callable wrapper for cblas_zdotu_sub (unconjugated double complex dot product).
///
/// Fortran interface:
///   CALL RCBLAS_ZDOTU_SUB(N, X, INCX, Y, INCY, DOTU)
///
/// All arguments are passed by reference (Fortran convention).
#[cfg(target_os = "macos")]
pub unsafe fn rcblas_zdotu_sub_(
    n: *const c_int,
    x: *const libc::c_void,
    incx: *const c_int,
    y: *const libc::c_void,
    incy: *const c_int,
    dotu: *mut libc::c_void,
) {
    unsafe {
        cblas_zdotu_sub(*n, x, *incx, y, *incy, dotu);
    }
}

/// Fortran-callable wrapper for cblas_zdotc_sub (conjugated double complex dot product).
///
/// Fortran interface:
///   CALL RCBLAS_ZDOTC_SUB(N, X, INCX, Y, INCY, DOTC)
///
/// All arguments are passed by reference (Fortran convention).
#[cfg(target_os = "macos")]
pub unsafe fn rcblas_zdotc_sub_(
    n: *const c_int,
    x: *const libc::c_void,
    incx: *const c_int,
    y: *const libc::c_void,
    incy: *const c_int,
    dotc: *mut libc::c_void,
) {
    unsafe {
        cblas_zdotc_sub(*n, x, *incx, y, *incy, dotc);
    }
}

// ============================================================
// Suppress deprecation warnings for Accelerate on macOS
// ============================================================
//
// The vecLib BLAS calls used above were deprecated in newer versions
// of macOS Accelerate. In C, this was handled with:
//   #pragma clang diagnostic ignored "-Wdeprecated-declarations"
//
// In Rust, we use the allow attribute at the module level for any
// deprecated API usage. Since the actual CBLAS calls are through
// extern "C" declarations (not Rust APIs), no additional suppression
// is needed at the Rust level. The linker may still produce warnings
// at link time, which can be suppressed with:
//   -Wl,-w (for clang linker)
//
// The long-term fix (as noted in the original C source) is to
// define ACCELERATE_NEW_LAPACK and use the new entry points.
