//! R output capture for embedding.
//!
//! Captures Rprintf, REprintf, and other R output functions
//! so they can be returned to the caller instead of printing
//! to stdout/stderr.

use super::accessors::{CAR, CDR, CHAR, PRINTNAME, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH};
use super::ffi::{NA_INTEGER, R_IsNA, R_IsNaN, R_xlen_t, SEXP, SEXPTYPE};
use super::globals::R_NilValue;
use super::instance::RInstance;
use super::object::Sexp;

mod arrays;
mod body;
mod capture;
mod datetimes;
mod dispatch;
mod formatting;
mod lists;
mod matrix;
mod native_bytes;
mod summary;
mod vectors;

pub(crate) use arrays::*;
pub(crate) use body::*;
pub use capture::*;
pub(crate) use datetimes::*;
pub(crate) use dispatch::*;
pub(crate) use formatting::*;
pub(crate) use lists::*;
pub(crate) use matrix::*;
pub(crate) use native_bytes::NativeOutputBytes;
pub(crate) use summary::*;
pub(crate) use vectors::*;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(deprecated)] // translated tests exercise the Sexp compat setters
mod tests;
