#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of useful standalone utilities from R's src/main/platform.c
//!
//! The original file is very large (~3700 lines) and the vast majority of it
//! consists of SEXP-based `do_*` functions (file operations, locale, etc.).
//! Those are too tightly coupled to R's type system to port.
//!
//! This module ports the handful of standalone C-utility functions that have
//! no SEXP dependency, and provides stubs for the SEXP-based public API.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

use crate::sexp::ffi::SEXP;
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// Standalone utility: R_Date
// ---------------------------------------------------------------------------

mod datetime;
mod files;
mod sysinfo;
#[cfg(test)]
mod tests;

pub use datetime::*;
pub use files::*;
pub use sysinfo::*;
