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
 *  Copyright (C) 2012-2024   The R Core Team.
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
 *  Ported from r-source/src/library/utils/src/sock.c
 */

use crate::sexp::ffi::*;

unsafe extern "C" {
    fn Rsockconnect(sport: SEXP, shost: SEXP) -> SEXP;
    fn Rsockread(sport: SEXP, smaxlen: SEXP) -> SEXP;
    fn Rsockclose(sport: SEXP) -> SEXP;
    fn Rsockopen(sport: SEXP) -> SEXP;
    fn Rsocklisten(sport: SEXP) -> SEXP;
    fn Rsockwrite(sport: SEXP, sstring: SEXP) -> SEXP;
}

pub unsafe fn sockconnect(sport: SEXP, shost: SEXP) -> SEXP {
    Rsockconnect(sport, shost)
}

pub unsafe fn sockread(sport: SEXP, smaxlen: SEXP) -> SEXP {
    Rsockread(sport, smaxlen)
}

pub unsafe fn sockclose(sport: SEXP) -> SEXP {
    Rsockclose(sport)
}

pub unsafe fn sockopen(sport: SEXP) -> SEXP {
    Rsockopen(sport)
}

pub unsafe fn socklisten(sport: SEXP) -> SEXP {
    Rsocklisten(sport)
}

pub unsafe fn sockwrite(sport: SEXP, sstring: SEXP) -> SEXP {
    Rsockwrite(sport, sstring)
}
