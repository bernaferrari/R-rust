//! Linear algebra backend abstraction.
//!
//! This module provides a unified interface for LAPACK/BLAS operations,
//! dispatching either to Fortran FFI (fortran-backend feature) or to
//! pure Rust implementations via faer-rs (rust-backend feature, default).
//!
//! The goal is to allow the rest of the codebase — especially lapack_impl.rs —
//! to remain almost unchanged regardless of which backend is active. Only the
//! import path switches from `super::lapack::` to `super::backend::`.

#[cfg(all(feature = "fortran-backend", feature = "rust-backend"))]
compile_error!(
    "features `fortran-backend` and `rust-backend` are mutually exclusive; enable exactly one"
);

#[cfg(not(any(feature = "fortran-backend", feature = "rust-backend")))]
compile_error!(
    "no linear-algebra backend selected; enable exactly one of `rust-backend` or `fortran-backend`"
);

pub(crate) use super::lapack::{
    La_norm_type, La_rcond_type, La_valid_uplo, Rcomplex as LapRcomplex, fort_char, fort_str,
    unscramble,
};

#[cfg(feature = "fortran-backend")]
pub use super::lapack::{
    dgecon_, dgeev_, dgeqp3_, dgesdd_, dgesv_, dgetrf_, dlange_, dormqr_, dpotrf_, dpotri_,
    dpstrf_, dsyevr_, dtrcon_, dtrtrs_, zgecon_, zgeev_, zgeqp3_, zgesdd_, zgesv_, zgetrf_, zheev_,
    zlange_, ztrcon_, ztrtrs_, zunmqr_,
};

#[cfg(all(feature = "rust-backend", not(feature = "fortran-backend")))]
pub mod rust_impl;

#[cfg(all(feature = "rust-backend", not(feature = "fortran-backend")))]
pub use rust_impl::{
    dgecon_, dgeev_, dgeqp3_, dgesdd_, dgesv_, dgetrf_, dlange_, dormqr_, dpotrf_, dpotri_,
    dpstrf_, dsyevr_, dtrcon_, dtrtrs_, zgecon_, zgeev_, zgeqp3_, zgesdd_, zgesv_, zgetrf_, zheev_,
    zlange_, ztrcon_, ztrtrs_, zunmqr_,
};

/// Identity of the active LAPACK backend.
pub fn backend_name() -> &'static str {
    if cfg!(feature = "fortran-backend") {
        "system-fortran"
    } else {
        "faer-pure-rust"
    }
}
