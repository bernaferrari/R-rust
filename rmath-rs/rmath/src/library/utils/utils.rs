#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_assignments,
    non_camel_case_types
)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2012   The R Core Team.
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
 *  Ported from r-source/src/library/utils/src/utils.c
 */

use std::os::raw::{c_double, c_int};

use crate::sexp::accessors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;

unsafe extern "C" {
    fn Rdownload(args: SEXP) -> SEXP;
}

pub unsafe fn download(args: SEXP) -> SEXP {
    Rdownload(CDR(args))
}
