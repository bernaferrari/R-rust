//! Linear algebra backend abstraction.
//!
//! LAPACK-shaped operations are implemented in pure Rust through faer-rs.
//! Keeping the LAPACK-shaped seam localizes the port's calling-convention
//! assumptions while the implementation remains portable across native,
//! Wasm, and mobile targets.

pub(crate) use super::lapack::{
    La_norm_type, La_rcond_type, La_valid_uplo, Rcomplex as LapRcomplex, fort_char, fort_str,
    unscramble_into,
};

pub mod rust_impl;

pub use rust_impl::{
    dgecon_, dgeev_, dgeqp3_, dgesdd_, dgesv_, dgetrf_, dlange_, dormqr_, dpotrf_, dpotri_,
    dpstrf_, dsyevr_, dtrcon_, dtrtrs_, zgecon_, zgeev_, zgeqp3_, zgesdd_, zgesv_, zgetrf_, zheev_,
    zlange_, ztrcon_, ztrtrs_, zunmqr_,
};

/// Identity of the active LAPACK backend.
pub fn backend_name() -> &'static str {
    "faer-pure-rust"
}
