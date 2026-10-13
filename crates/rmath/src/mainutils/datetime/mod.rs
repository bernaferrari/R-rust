#![allow(unused_variables)]
#![allow(unused_assignments)]
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2000-2026  The R Core Team.
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
 *
 *      Interfaces to POSIX date-time conversion functions.
 *
 *  Ported from R source: src/main/datetime.c
 */
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_double, c_int, c_long};

// Timezone/datetime go through the ported tzone layer (R's own
// extra/tzone) instead of libc: one implementation on every host.
use crate::tzone::stm as tz_tm;
use crate::tzone::{R_gmtime_r, R_localtime_r, R_mktime, R_tzname};
use crate::tzone_strftime::R_strftime;
use crate::tzone_strftime::stm as sf_tm;

use crate::mainutils::rstrptime::R_strptime;
use crate::sexp::accessors::*;
use crate::sexp::attrib_core::{R_ClassSymbol, R_NamesSymbol, R_classgets, getAttrib, setAttrib};

use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::*;
use crate::sexp::globals::{R_MissingArg, R_NaString, R_NilValue};
use crate::sexp::protect::*;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// Split from the original single-file module; pure code movement.
mod basics;
mod coerce;
mod convert;
mod format;
mod misc;
mod strptime;

#[cfg(test)]
mod tests;

use basics::NA_REAL;
pub use basics::*;
pub use coerce::*;
pub use convert::*;
pub use format::*;
pub use misc::*;
pub use strptime::*;
