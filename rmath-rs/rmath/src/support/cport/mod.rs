//! Standalone C utility function ports from R's src/main/ and nmath/
//!
//! These are small, self-contained utility functions that don't depend
//! on R's core type system (SEXP, etc.).

pub mod cutil;
pub mod d1mach;
pub mod i1mach;
pub mod localecharset;
pub mod machar;
pub mod qsort;
