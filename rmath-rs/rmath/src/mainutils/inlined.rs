#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/inlined.c
//!
//! The original C file forces compilation of the inline functions declared
//! in `Rinlinedfuns.h`. Those functions are now implemented in `crate::sexp::accessors`.
//!
//! This module re-exports the accessor functions for backward compatibility
//! with other mainutils modules.

// Re-export all accessor functions from the sexp module.
// These are #[unsafe(no_mangle)] extern "C" functions, so they are already
// available at link time. The re-exports here are for documentation
// and for use within the mainutils module.
pub use crate::sexp::accessors::*;
