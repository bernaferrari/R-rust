/*!
 * Port of R's trio library - Portable printf/scanf implementation.
 *
 * Original copyright (C) 1998, 2009 Bjorn Reese and Daniel Stenberg.
 * BSD-style license.
 *
 * This module provides C-compatible FFI functions for printf/scanf formatting
 * and NaN/Infinity handling, matching the trio API used by R.
 */

pub mod trio;
pub mod trionan;
pub mod triostr;
