//! rmath: Rust port of R's nmath statistical library
//!
//! This crate provides a drop-in replacement for R's libRmath.a,
//! implementing statistical math functions with C-compatible FFI.

#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]

pub mod constants;
pub mod dpq;
pub mod error;
pub mod rng;
pub mod utils;
pub mod special;
pub mod dist;
pub mod fprec;
pub mod appl;
pub mod xdr;
pub mod tzone;
#[allow(unused_variables, unused_assignments, unused_mut)]
pub mod tzone_strftime;

pub mod trio;
pub mod intl;
#[allow(dead_code, unused_imports, unused_variables, unused_mut, unused_assignments, non_camel_case_types, clippy::all)]
pub mod tre;
#[allow(dead_code, unused_imports, unused_variables, unused_mut, unused_assignments, non_camel_case_types, clippy::all)]
pub mod graphapp;
#[allow(dead_code, unused_imports, unused_variables, unused_mut, unused_assignments, non_camel_case_types, clippy::all)]
pub mod sexp;
#[allow(dead_code, unused_imports, unused_variables, unused_mut, unused_assignments, non_camel_case_types, clippy::all)]
pub mod mainutils;
#[allow(dead_code, unused_imports, unused_variables, unused_mut, unused_assignments, non_camel_case_types, clippy::all)]
pub mod unix;
#[allow(dead_code, unused_imports, unused_variables, unused_mut, unused_assignments, non_camel_case_types, clippy::all)]
pub mod eval;
