#![allow(unsafe_op_in_unsafe_fn)] // legacy C-port unsafe boundary; see docs/unsafe-op-allowlist.tsv.
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2003-2023   The R Core Team.
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 *
 *  Ported from r-source/src/library/tools/src/init.c
 */

use std::os::raw::c_int;

use crate::mainutils::printutils::Rprintf;
use crate::sexp::ffi::*;

/* Test function used in tests/encodings.R */
pub unsafe fn Renctest(x: *mut *mut libc::c_char) {
    let s = std::ffi::CStr::from_ptr(*x);
    let len = s.to_bytes().len();
    let msg = format!("'{}', nbytes = {}\n", s.to_string_lossy(), len);
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    Rprintf(c_msg.as_ptr(), std::ptr::null_mut());
}

/* Stub: DllInfo registration */
pub unsafe fn R_init_tools(_dll: *mut std::ffi::c_void) {
    /* Registration handled by Rust's symbol exports */
}
