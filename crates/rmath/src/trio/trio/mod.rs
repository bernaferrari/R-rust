/*!
 * Port of R's trio.c - Portable printf/scanf implementation.
 *
 * Original copyright (C) 1998, 2009 Bjorn Reese and Daniel Stenberg.
 * BSD-style license.
 *
 * Modified for R to support round-to-even and buffered output for
 * multi-byte characters.
 *
 * This Rust port uses closures/traits for output instead of raw function
 * pointers, and uses `std::io::Write` where appropriate.
 */

#![allow(non_camel_case_types, unused_assignments, unused_variables)]

use std::io::Write;
use std::os::raw::{c_char, c_int, c_long, c_void};
use std::ptr;

// Re-export from sub-modules
pub use crate::trio::trionan::*;
pub use crate::trio::triostr;

// ============================================================================
// Type definitions (ported from triodef.h)
// ============================================================================

mod core;
mod format;
#[cfg(test)]
mod tests;

pub use core::*;
pub use format::*;
