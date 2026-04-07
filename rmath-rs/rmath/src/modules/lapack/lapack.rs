/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001--2025  The R Core Team.
 *  Copyright (C) 2003--2010  The R Foundation
 *
 *  Ported to Rust from R's src/modules/lapack/Lapack.c
 *
 *  This file provides:
 *  1. FFI declarations for LAPACK and BLAS Fortran routines
 *  2. Module-private helper functions for LAPACK wrapper logic
 *
 *  NOTE: The #[unsafe(no_mangle)] exported stubs live in lapack_impl.rs.
 *        This file provides the internal implementation helpers and FFI
 *        declarations that would be used to fill in those stubs with
 *        real LAPACK calls.
 */

use crate::main::errors::Rf_error1;
use core::ffi::c_char;
use std::ptr;

/// Rcomplex: a pair of doubles for complex numbers (matches R's Rcomplex)
#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct Rcomplex {
    pub r: f64,
    pub i: f64,
}

// ============================================================
// LAPACK Fortran FFI declarations
// ============================================================
//
// Fortran routines are called with pointers to all arguments.
// Character arguments are passed by pointer to a null-terminated string.
// Fortran uses column-major order.

unsafe extern "C" {
    // --- LAPACK routines ---

    /// DGESDD - computes the singular value decomposition (SVD) of a real M-by-N matrix
    pub fn dgesdd_(
        jobz: *const u8,
        m: *const libc::c_int,
        n: *const libc::c_int,
        a: *mut f64,
        lda: *const libc::c_int,
        s: *mut f64,
        u: *mut f64,
        ldu: *const libc::c_int,
        vt: *mut f64,
        ldvt: *const libc::c_int,
        work: *mut f64,
        lwork: *const libc::c_int,
        iwork: *mut libc::c_int,
        info: *mut libc::c_int,
    );

    /// DSYEVR - computes selected eigenvalues and, optionally, eigenvectors of a real symmetric matrix
    pub fn dsyevr_(
        jobz: *const u8,
        range: *const u8,
        uplo: *const u8,
        n: *const libc::c_int,
        a: *mut f64,
        lda: *const libc::c_int,
        vl: *const f64,
        vu: *const f64,
        il: *const libc::c_int,
        iu: *const libc::c_int,
        abstol: *const f64,
        m: *mut libc::c_int,
        w: *mut f64,
        z: *mut f64,
        ldz: *const libc::c_int,
        isuppz: *mut libc::c_int,
        work: *mut f64,
        lwork: *const libc::c_int,
        iwork: *mut libc::c_int,
        liwork: *const libc::c_int,
        info: *mut libc::c_int,
    );

    /// DGEEV - computes eigenvalues and, optionally, left and/or right eigenvectors of a real nonsymmetric matrix
    pub fn dgeev_(
        jobvl: *const u8,
        jobvr: *const u8,
        n: *const libc::c_int,
        a: *mut f64,
        lda: *const libc::c_int,
        wr: *mut f64,
        wi: *mut f64,
        vl: *mut f64,
        ldvl: *const libc::c_int,
        vr: *mut f64,
        ldvr: *const libc::c_int,
        work: *mut f64,
        lwork: *const libc::c_int,
        info: *mut libc::c_int,
    );

    /// DLANGE - returns the value of the one norm, or the Frobenius norm, or the infinity norm,
    /// or the element of largest absolute value of a real matrix
    pub fn dlange_(
        norm: *const u8,
        m: *const libc::c_int,
        n: *const libc::c_int,
        a: *const f64,
        lda: *const libc::c_int,
        work: *mut f64,
    ) -> f64;

    /// DGECON - estimates the reciprocal of the condition number of a general real matrix
    pub fn dgecon_(
        norm: *const u8,
        n: *const libc::c_int,
        a: *const f64,
        lda: *const libc::c_int,
        anorm: *const f64,
        rcond: *mut f64,
        work: *mut f64,
        iwork: *mut libc::c_int,
        info: *mut libc::c_int,
    );

    /// DGETRF - computes an LU factorization of a general M-by-N matrix
    pub fn dgetrf_(
        m: *const libc::c_int,
        n: *const libc::c_int,
        a: *mut f64,
        lda: *const libc::c_int,
        ipiv: *mut libc::c_int,
        info: *mut libc::c_int,
    );

    /// DTRCON - estimates the reciprocal of the condition number of a triangular matrix
    pub fn dtrcon_(
        norm: *const u8,
        uplo: *const u8,
        diag: *const u8,
        n: *const libc::c_int,
        a: *const f64,
        lda: *const libc::c_int,
        rcond: *mut f64,
        work: *mut f64,
        iwork: *mut libc::c_int,
        info: *mut libc::c_int,
    );

    /// ZGESDD - computes the singular value decomposition (SVD) of a complex M-by-N matrix
    pub fn zgesdd_(
        jobz: *const u8,
        m: *const libc::c_int,
        n: *const libc::c_int,
        a: *mut Rcomplex,
        lda: *const libc::c_int,
        s: *mut f64,
        u: *mut Rcomplex,
        ldu: *const libc::c_int,
        vt: *mut Rcomplex,
        ldvt: *const libc::c_int,
        work: *mut Rcomplex,
        lwork: *const libc::c_int,
        rwork: *mut f64,
        iwork: *mut libc::c_int,
        info: *mut libc::c_int,
    );

    /// ZLANGE - returns the value of the one norm, Frobenius norm, infinity norm, or
    /// element of largest absolute value of a complex matrix
    pub fn zlange_(
        norm: *const u8,
        m: *const libc::c_int,
        n: *const libc::c_int,
        a: *const Rcomplex,
        lda: *const libc::c_int,
        work: *mut f64,
    ) -> f64;

    /// ZGECON - estimates the reciprocal of the condition number of a general complex matrix
    pub fn zgecon_(
        norm: *const u8,
        n: *const libc::c_int,
        a: *const Rcomplex,
        lda: *const libc::c_int,
        anorm: *const f64,
        rcond: *mut f64,
        work: *mut Rcomplex,
        rwork: *mut f64,
        info: *mut libc::c_int,
    );

    /// ZGETRF - computes an LU factorization of a general M-by-N complex matrix
    pub fn zgetrf_(
        m: *const libc::c_int,
        n: *const libc::c_int,
        a: *mut Rcomplex,
        lda: *const libc::c_int,
        ipiv: *mut libc::c_int,
        info: *mut libc::c_int,
    );

    /// ZTRCON - estimates the reciprocal of the condition number of a triangular complex matrix
    pub fn ztrcon_(
        norm: *const u8,
        uplo: *const u8,
        diag: *const u8,
        n: *const libc::c_int,
        a: *const Rcomplex,
        lda: *const libc::c_int,
        rcond: *mut f64,
        work: *mut Rcomplex,
        rwork: *mut f64,
        info: *mut libc::c_int,
    );

    /// ZGESV - computes the solution to a complex system of linear equations A * X = B
    pub fn zgesv_(
        n: *const libc::c_int,
        nrhs: *const libc::c_int,
        a: *mut Rcomplex,
        lda: *const libc::c_int,
        ipiv: *mut libc::c_int,
        b: *mut Rcomplex,
        ldb: *const libc::c_int,
        info: *mut libc::c_int,
    );

    /// ZGEQP3 - computes a QR factorization with column pivoting of a complex matrix
    pub fn zgeqp3_(
        m: *const libc::c_int,
        n: *const libc::c_int,
        a: *mut Rcomplex,
        lda: *const libc::c_int,
        jpvt: *mut libc::c_int,
        tau: *mut Rcomplex,
        work: *mut Rcomplex,
        lwork: *const libc::c_int,
        rwork: *mut f64,
        info: *mut libc::c_int,
    );

    /// ZUNMQR - multiplies a complex general matrix by the unitary matrix Q from a QR factorization
    pub fn zunmqr_(
        side: *const u8,
        trans: *const u8,
        m: *const libc::c_int,
        n: *const libc::c_int,
        k: *const libc::c_int,
        a: *const Rcomplex,
        lda: *const libc::c_int,
        tau: *const Rcomplex,
        c__: *mut Rcomplex,
        ldc: *const libc::c_int,
        work: *mut Rcomplex,
        lwork: *const libc::c_int,
        info: *mut libc::c_int,
    );

    /// ZTRTRS - solves a triangular system of equations with a complex triangular matrix
    pub fn ztrtrs_(
        uplo: *const u8,
        trans: *const u8,
        diag: *const u8,
        n: *const libc::c_int,
        nrhs: *const libc::c_int,
        a: *const Rcomplex,
        lda: *const libc::c_int,
        b: *mut Rcomplex,
        ldb: *const libc::c_int,
        info: *mut libc::c_int,
    );

    /// ZHEEV - computes all eigenvalues and, optionally, eigenvectors of a complex Hermitian matrix
    pub fn zheev_(
        jobz: *const u8,
        uplo: *const u8,
        n: *const libc::c_int,
        a: *mut Rcomplex,
        lda: *const libc::c_int,
        w: *mut f64,
        work: *mut Rcomplex,
        lwork: *const libc::c_int,
        rwork: *mut f64,
        info: *mut libc::c_int,
    );

    /// ZGEEV - computes eigenvalues and, optionally, left and/or right eigenvectors of a complex matrix
    pub fn zgeev_(
        jobvl: *const u8,
        jobvr: *const u8,
        n: *const libc::c_int,
        a: *mut Rcomplex,
        lda: *const libc::c_int,
        w: *mut Rcomplex,
        vl: *mut Rcomplex,
        ldvl: *const libc::c_int,
        vr: *mut Rcomplex,
        ldvr: *const libc::c_int,
        work: *mut Rcomplex,
        lwork: *const libc::c_int,
        rwork: *mut f64,
        info: *mut libc::c_int,
    );

    /// DGESV - computes the solution to a real system of linear equations A * X = B
    pub fn dgesv_(
        n: *const libc::c_int,
        nrhs: *const libc::c_int,
        a: *mut f64,
        lda: *const libc::c_int,
        ipiv: *mut libc::c_int,
        b: *mut f64,
        ldb: *const libc::c_int,
        info: *mut libc::c_int,
    );

    /// DGEQP3 - computes a QR factorization with column pivoting of a real matrix
    pub fn dgeqp3_(
        m: *const libc::c_int,
        n: *const libc::c_int,
        a: *mut f64,
        lda: *const libc::c_int,
        jpvt: *mut libc::c_int,
        tau: *mut f64,
        work: *mut f64,
        lwork: *const libc::c_int,
        info: *mut libc::c_int,
    );

    /// DORMQR - overwrites the general real M-by-N matrix C with Q*C or Q^T*C or C*Q or C*Q^T
    pub fn dormqr_(
        side: *const u8,
        trans: *const u8,
        m: *const libc::c_int,
        n: *const libc::c_int,
        k: *const libc::c_int,
        a: *const f64,
        lda: *const libc::c_int,
        tau: *const f64,
        c__: *mut f64,
        ldc: *const libc::c_int,
        work: *mut f64,
        lwork: *const libc::c_int,
        info: *mut libc::c_int,
    );

    /// DTRTRS - solves a triangular system of equations with a real triangular matrix
    pub fn dtrtrs_(
        uplo: *const u8,
        trans: *const u8,
        diag: *const u8,
        n: *const libc::c_int,
        nrhs: *const libc::c_int,
        a: *const f64,
        lda: *const libc::c_int,
        b: *mut f64,
        ldb: *const libc::c_int,
        info: *mut libc::c_int,
    );

    /// DPOTRF - computes the Cholesky factorization of a real symmetric positive-definite matrix
    pub fn dpotrf_(
        uplo: *const u8,
        n: *const libc::c_int,
        a: *mut f64,
        lda: *const libc::c_int,
        info: *mut libc::c_int,
    );

    /// DPOTRI - computes the inverse of a real symmetric positive-definite matrix using the Cholesky factorization
    pub fn dpotri_(
        uplo: *const u8,
        n: *const libc::c_int,
        a: *mut f64,
        lda: *const libc::c_int,
        info: *mut libc::c_int,
    );

    /// DPSTRF - computes the Cholesky factorization with complete pivoting of a real symmetric positive-semidefinite matrix
    pub fn dpstrf_(
        uplo: *const u8,
        n: *const libc::c_int,
        a: *mut f64,
        lda: *const libc::c_int,
        piv: *mut libc::c_int,
        rank: *mut libc::c_int,
        tol: *const f64,
        work: *mut f64,
        info: *mut libc::c_int,
    );

    /// ILAVER - returns the LAPACK version
    pub fn ilaver_(major: *mut libc::c_int, minor: *mut libc::c_int, patch: *mut libc::c_int);

    // --- BLAS routines ---

    /// CDOTU_SUB - computes the unconjugated dot product of two complex vectors (CBLAS)
    #[cfg(target_os = "macos")]
    pub fn cblas_cdotu_sub(
        n: libc::c_int,
        x: *const libc::c_void,
        incx: libc::c_int,
        y: *const libc::c_void,
        incy: libc::c_int,
        dotu: *mut libc::c_void,
    );

    /// CDOTC_SUB - computes the conjugated dot product of two complex vectors (CBLAS)
    #[cfg(target_os = "macos")]
    pub fn cblas_cdotc_sub(
        n: libc::c_int,
        x: *const libc::c_void,
        incx: libc::c_int,
        y: *const libc::c_void,
        incy: libc::c_int,
        dotc: *mut libc::c_void,
    );

    /// ZDOTU_SUB - computes the unconjugated dot product of two double-complex vectors (CBLAS)
    #[cfg(target_os = "macos")]
    pub fn cblas_zdotu_sub(
        n: libc::c_int,
        x: *const libc::c_void,
        incx: libc::c_int,
        y: *const libc::c_void,
        incy: libc::c_int,
        dotu: *mut libc::c_void,
    );

    /// ZDOTC_SUB - computes the conjugated dot product of two double-complex vectors (CBLAS)
    #[cfg(target_os = "macos")]
    pub fn cblas_zdotc_sub(
        n: libc::c_int,
        x: *const libc::c_void,
        incx: libc::c_int,
        y: *const libc::c_void,
        incy: libc::c_int,
        dotc: *mut libc::c_void,
    );
}

// ============================================================
// Helper functions (module-private)
// ============================================================

/// Validate and normalize a LAPACK norm type string.
///
/// Converts single-character norm specification to uppercase LAPACK convention:
/// - '1' -> 'O' (one-norm alias)
/// - 'E' -> 'F' (Euclidean/Frobenius alias)
/// - 'M', 'O', 'I', 'F' pass through
///
/// Returns the normalized character or calls Rf_error on invalid input.
pub(crate) fn La_norm_type(typstr: &str) -> u8 {
    if typstr.len() != 1 {
        let msg = format!(
            "argument type[1]='{}' must be a character string of string length 1",
            typstr
        );
        let cmsg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");
        unsafe {
            Rf_error1(
                b"invalid argument\0".as_ptr() as *const c_char,
                cmsg.as_ptr(),
            )
        };
        unreachable!()
    }
    let typup = typstr.as_bytes()[0].to_ascii_uppercase();
    match typup {
        b'1' => b'O', // alias
        b'E' => b'F', // alias
        b'M' | b'O' | b'I' | b'F' => typup,
        _ => {
            let msg = format!(
                "argument type[1]='{}' must be one of 'M','1','O','I','F' or 'E'",
                typstr
            );
            let cmsg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");
            unsafe {
                Rf_error1(
                    b"invalid argument\0".as_ptr() as *const c_char,
                    cmsg.as_ptr(),
                )
            };
            unreachable!()
        }
    }
}

/// Validate and normalize a LAPACK condition number norm type string.
///
/// Currently only supports '1' (one-norm) or 'I' (infinity-norm):
/// - '1' -> 'O'
/// - 'O' or 'I' pass through
///
/// Returns 'O' or 'I', or calls Rf_error on invalid input.
pub(crate) fn La_rcond_type(typstr: &str) -> u8 {
    if typstr.len() != 1 {
        let msg = format!(
            "argument type[1]='{}' must be a character string of string length 1",
            typstr
        );
        let cmsg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");
        unsafe {
            Rf_error1(
                b"invalid argument\0".as_ptr() as *const c_char,
                cmsg.as_ptr(),
            )
        };
        unreachable!()
    }
    let typup = typstr.as_bytes()[0].to_ascii_uppercase();
    match typup {
        b'1' => b'O', // alias
        b'O' | b'I' => typup,
        _ => {
            let msg = format!(
                "argument type[1]='{}' must be one of '1','O', or 'I'",
                typstr
            );
            let cmsg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");
            unsafe {
                Rf_error1(
                    b"invalid argument\0".as_ptr() as *const c_char,
                    cmsg.as_ptr(),
                )
            };
            unreachable!()
        }
    }
}

/// Validate and normalize an uplo (upper/lower triangular) string.
///
/// Converts single-character uplo specification to uppercase:
/// - 'U' or 'L' pass through
///
/// Returns 'U' or 'L', or calls Rf_error on invalid input.
pub(crate) fn La_valid_uplo(uplostr: &str) -> u8 {
    if uplostr.len() != 1 {
        let msg = format!(
            "argument type[1]='{}' must be a character string of string length 1",
            uplostr
        );
        let cmsg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");
        unsafe {
            Rf_error1(
                b"invalid argument\0".as_ptr() as *const c_char,
                cmsg.as_ptr(),
            )
        };
        unreachable!()
    }
    let uplo = uplostr.as_bytes()[0].to_ascii_uppercase();
    match uplo {
        b'U' | b'L' => uplo,
        _ => {
            let msg = format!("argument type[1]='{}' must be 'U' or 'L'", uplostr);
            let cmsg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");
            unsafe {
                Rf_error1(
                    b"invalid argument\0".as_ptr() as *const c_char,
                    cmsg.as_ptr(),
                )
            };
            unreachable!()
        }
    }
}

/// Unscramble eigenvectors from DGEEV output.
///
/// DGEEV returns real and imaginary parts of eigenvalues, and the real
/// eigenvector matrix needs to be unscrambled when complex eigenvalues occur
/// in conjugate pairs. This function constructs a complex matrix from the
/// real eigenvector output, handling the conjugate pair structure.
///
/// # Arguments
/// * `imaginary` - Array of imaginary parts of eigenvalues (length n)
/// * `n` - Matrix dimension
/// * `vecs` - Real eigenvector matrix from DGEEV (n x n, column-major)
///
/// # Returns
/// A new complex matrix (n x n) with properly unscrambled eigenvectors.
pub(crate) fn unscramble(imaginary: &[f64], n: i32, vecs: &[f64]) -> Vec<Rcomplex> {
    let n_usize = n as usize;
    let mut s = vec![Rcomplex::default(); n_usize * n_usize];

    let mut j: usize = 0;
    while j < n_usize {
        if imaginary[j] != 0.0 {
            let j1 = j + 1;
            for i in 0..n_usize {
                s[i + n_usize * j].r = vecs[i + j * n_usize];
                s[i + n_usize * j].i = vecs[i + j1 * n_usize];
                s[i + n_usize * j1].r = vecs[i + j * n_usize];
                s[i + n_usize * j1].i = -vecs[i + j1 * n_usize];
            }
            j = j1; // skip the conjugate partner
        } else {
            for i in 0..n_usize {
                s[i + n_usize * j].r = vecs[i + j * n_usize];
                s[i + n_usize * j].i = 0.0;
            }
        }
        j += 1;
    }

    s
}

// ============================================================
// Fortran character argument helpers
// ============================================================

/// Convert a Rust char to a null-terminated byte array suitable for passing
/// to Fortran LAPACK routines.
#[inline]
pub(crate) fn fort_char(c: u8) -> [u8; 2] {
    [c, 0]
}

/// Convert a Rust &str to a null-terminated byte array suitable for passing
/// to Fortran LAPACK routines.
#[inline]
pub(crate) fn fort_str(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = s.bytes().collect();
    v.push(0);
    v
}
