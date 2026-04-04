//! FFI safety utilities for the R interpreter.
//!
//! Provides panic-catching wrappers for FFI boundaries.

#![deny(improper_ctypes_definitions)]
#![deny(improper_ctypes)]

#[macro_use]
pub mod panic_safety;

pub use panic_safety::*;
