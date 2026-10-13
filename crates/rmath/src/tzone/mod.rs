#![allow(
    unreachable_code,
    clippy::comparison_to_empty,
    clippy::manual_memcpy,
    non_camel_case_types
)]
/*
 * Rust port of R's timezone library (src/extra/tzone/localtime.c).
 *
 * Original C code:
 *   Modifications copyright (C) 2007-2023 The R Core Team (GPL)
 *   Base tzcode: public domain by Arthur David Olson
 *
 * This port faithfully reproduces the C implementation in idiomatic Rust,
 * providing FFI-compatible entry points for R's datetime functionality.
 */

use std::env;
use std::fs::File;
use std::io::Read;

use crate::sexp::instance::with_required_current_instance;

// ---------------------------------------------------------------------------
// Constants from tzfile.h
// ---------------------------------------------------------------------------

// Split from the original single-file module; pure code movement.
mod consts;
mod load;
mod localtime;
mod parse;
mod types;
mod tzset;
mod utils;

#[cfg(test)]
mod tests;

pub(crate) use consts::*;
pub(crate) use load::*;
pub(crate) use localtime::*;
pub(crate) use parse::*;
pub(crate) use types::*;
pub(crate) use tzset::*;
pub(crate) use utils::*;
