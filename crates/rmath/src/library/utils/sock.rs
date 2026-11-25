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

#![cfg(not(target_arch = "wasm32"))]

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::attrib_core::setAttrib;
use crate::main::coerce::asInteger;
use crate::mainutils::errors::{Rf_error, Rf_error_cstr};
#[cfg(not(target_arch = "wasm32"))]
use crate::modules::internet::rsock::{
    in_Rsockclose, in_Rsockconnect, in_Rsocklisten, in_Rsockopen, in_Rsockread, in_Rsockwrite,
};
use crate::sexp::accessors::{LENGTH, SET_STRING_ELT, STRING_ELT, translateChar};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_allocVector, Rf_mkChar, Rf_mkCharLen,
};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

fn require_scalar(x: SEXP, message: &[u8]) {
    if LENGTH(x) != 1 {
        Rf_error_cstr(crate::mainutils::errors::cstr_until_nul(message));
    }
}






