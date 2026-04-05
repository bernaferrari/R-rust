#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! R's S-expression type system.
//!
//! This module provides Rust-native implementations of R's SEXPREC/SEXP types,
//! used throughout the R interpreter. The design follows a two-layer approach:
//! - `ffi` submodule: raw `#[repr(C)]` types for FFI compatibility
//! - `globals` submodule: global singleton values (R_NilValue, etc.)
//! - `accessors` submodule: C-compatible accessor functions (TYPEOF, CAR, CDR, etc.)
//! - `memory` submodule: arena allocator for R objects
//! - `constructors` submodule: FFI constructor functions (allocVector, cons, etc.)
//! - `symbol` submodule: symbol table and interning

pub mod accessors;
pub mod altrep;
pub mod constructors;
pub mod context;
pub mod envir;
pub mod ffi;
pub mod gengc;
pub mod globals;
pub mod memory;
pub mod memory_ext;
pub mod protect;
pub mod safe;
pub mod symbol;

// Re-export commonly used types at the module level
pub use ffi::{
    Closxp, Envsxp, Listsxp, Primsxp, Promsxp, R_IsNA, R_IsNaN, R_len_t, R_size_t, R_xlen_t,
    Rboolean, Rbyte, Rcomplex, SexprecCore, SexprecData, SxpInfo, Symsxp, Vecsxp, DOTSXP, FALSE,
    ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_FINITE, R_NA_BIT_PATTERN, SEXP, SEXPTYPE, TRUE,
};

pub use altrep::{
    altrep_as_integer_slice, altrep_as_real_slice, altrep_class, altrep_dataptr, altrep_elt,
    altrep_length, force_materialization, is_altrep, is_materialized, AltrepBuilder, AltrepClass,
    AltrepData, REPEAT_CLASS, SEQUENCE_CLASS,
};
