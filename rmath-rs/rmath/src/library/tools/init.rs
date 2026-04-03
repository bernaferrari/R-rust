#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

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

use crate::sexp::ffi::*;

unsafe extern "C" {
    fn Rprintf(format: *const i8, ...) -> c_int;
}

/* Test function used in tests/encodings.R */
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Renctest(x: *mut *mut i8) {
    let s = std::ffi::CStr::from_ptr(*x);
    let len = s.to_bytes().len();
    Rprintf(
        b"'%s', nbytes = %lld\n\0".as_ptr() as *const i8,
        *x,
        len as i64,
    );
}

/* Stub: DllInfo registration */
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_init_tools(_dll: *mut std::ffi::c_void) {
    /* Registration handled by Rust's symbol exports */
}
