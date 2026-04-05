#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_assignments,
    non_camel_case_types
)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2011-2023   The R Core Team.
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
 *  Ported from r-source/src/library/parallel/src/ncpus.c
 */

use std::os::raw::c_int;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

#[cfg(unix)]
use libc::{_SC_NPROCESSORS_CONF, _SC_NPROCESSORS_ONLN, c_long, sysconf};

/// Detect the number of physical and logical processors.
///
/// Returns a length-2 integer vector:
///   [0] = number of physical cores
///   [1] = number of logical processors (including hyperthreading)

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ncpus(_virtual: SEXP) -> SEXP {
    let res = Rf_allocVector(SEXPTYPE::INTSXP.0, 2);
    Rf_protect(res);
    let ians = INTEGER(res);

    *ians.add(1) = NA_INTEGER as c_int;

    #[cfg(unix)]
    {
        // Try sysconf for logical processor count
        let logical = unsafe { sysconf(_SC_NPROCESSORS_ONLN) };
        if logical > 0 {
            *ians.add(1) = logical as c_int;
        }

        // Try sysconf for physical core count
        // Note: _SC_NPROCESSORS_CONF is the closest approximation;
        // on most systems this equals _SC_NPROCESSORS_ONLN when HT is not present,
        // and on systems with HT it still reports logical cores.
        // The C version uses Windows-specific APIs for the physical count,
        // which are not available on Unix. We use available_parallelism as fallback.
        let conf = unsafe { sysconf(_SC_NPROCESSORS_CONF) };
        if conf > 0 {
            *ians.add(0) = conf as c_int;
        } else if let Ok(threads) = std::thread::available_parallelism() {
            *ians.add(0) = threads.get() as c_int;
        }
    }

    #[cfg(not(unix))]
    {
        // Fallback for non-Unix: use std::thread::available_parallelism
        if let Ok(threads) = std::thread::available_parallelism() {
            let n = threads.get() as c_int;
            *ians.add(0) = n;
            *ians.add(1) = n;
        }
    }

    Rf_unprotect(1);
    res
}
