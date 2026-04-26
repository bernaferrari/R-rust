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
use std::slice;

use crate::sexp::accessors::INTEGER;
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{NA_INTEGER, SEXP, SEXPTYPE};
use crate::sexp::protect::protect;

#[cfg(unix)]
use libc::{_SC_NPROCESSORS_CONF, _SC_NPROCESSORS_ONLN, sysconf};

fn detect_cpu_counts() -> [c_int; 2] {
    let mut counts = [NA_INTEGER as c_int, NA_INTEGER as c_int];

    #[cfg(unix)]
    {
        let logical = unsafe { sysconf(_SC_NPROCESSORS_ONLN) };
        if logical > 0 {
            counts[1] = logical as c_int;
        }

        let configured = unsafe { sysconf(_SC_NPROCESSORS_CONF) };
        if configured > 0 {
            counts[0] = configured as c_int;
        } else if let Ok(threads) = std::thread::available_parallelism() {
            counts[0] = threads.get() as c_int;
        }
    }

    #[cfg(not(unix))]
    {
        if let Ok(threads) = std::thread::available_parallelism() {
            let n = threads.get() as c_int;
            counts = [n, n];
        }
    }

    counts
}

/// Detect the number of physical and logical processors.
///
/// Returns a length-2 integer vector:
///   [0] = number of physical cores
///   [1] = number of logical processors (including hyperthreading)
pub unsafe fn ncpus(_virtual: SEXP) -> SEXP {
    let res = unsafe { Rf_allocVector(SEXPTYPE::INTSXP, 2) };
    let _res_guard = protect(res);
    let output = unsafe { slice::from_raw_parts_mut(INTEGER(res), 2) };
    output.copy_from_slice(&detect_cpu_counts());
    res
}
