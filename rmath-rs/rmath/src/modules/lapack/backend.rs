//! Linear algebra backend abstraction.
//!
//! This module provides a unified interface for LAPACK/BLAS operations,
//! dispatching either to Fortran FFI (fortran-backend feature) or to
//! pure Rust implementations via faer-rs (rust-backend feature, default).
//!
//! The goal is to allow the rest of the codebase — especially lapack_impl.rs —
//! to remain almost unchanged regardless of which backend is active. Only the
//! import path switches from `super::lapack::` to `super::backend::`.

pub(crate) use super::lapack::{
    fort_char, fort_str, La_norm_type, La_rcond_type, La_valid_uplo, Rcomplex as LapRcomplex,
    unscramble,
};

// Fortran backend takes precedence if both features are enabled.
#[cfg(feature = "fortran-backend")]
pub use super::lapack::{
    dgecon_, dgeev_, dgeqp3_, dgesdd_, dgesv_, dgetrf_, dlange_, dormqr_, dpotrf_, dpotri_,
    dpstrf_, dtrcon_, dtrtrs_, dsyevr_, zgecon_, zgeev_, zgeqp3_, zgesdd_, zgesv_, zgetrf_,
    zheev_, zlange_, ztrcon_, ztrtrs_, zunmqr_,
};

#[cfg(all(feature = "rust-backend", not(feature = "fortran-backend")))]
pub mod rust_impl;

#[cfg(all(feature = "rust-backend", not(feature = "fortran-backend")))]
pub use rust_impl::{
    dgecon_, dgeev_, dgeqp3_, dgesdd_, dgesv_, dgetrf_, dlange_, dormqr_, dpotrf_, dpotri_,
    dpstrf_, dtrcon_, dtrtrs_, dsyevr_, zgecon_, zgeev_, zgeqp3_, zgesdd_, zgesv_, zgetrf_,
    zheev_, zlange_, ztrcon_, ztrtrs_, zunmqr_,
};
