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

use crate::sexp::context::r_error;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};

use basics::NA_REAL;
pub use basics::*;
pub use coerce::*;
pub use convert::*;
pub use format::*;
pub use misc::*;
pub use strptime::*;

// ---------------------------------------------------------------------------
// Checked-access boundary helpers
//
// Every unsafe op below is a small audited boundary: the call requires only
// that its SEXP arguments are live under the active runtime, which is the same
// ambient contract the checked `Sexp` API assumes. Keeping them in one place
// lets the datetime translators stay on checked element access.
// ---------------------------------------------------------------------------

/// Checked node factory bound to the active runtime session. Datetime helpers
/// only run while the evaluator holds a session.
fn node_factory() -> crate::sexp::object::SessionNodeFactory<'static> {
    crate::sexp::object::SessionNodeFactory::new(
        // SAFETY: datetime helpers only run inside evaluated R calls.
        unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap_or_else(|error| r_error(error.to_string())),
    )
}

fn wrapped(x: SEXP) -> crate::sexp::object::Sexp<'static> {
    node_factory()
        .wrap(x)
        .unwrap_or_else(|error| r_error(error.to_string()))
}

fn mutable_vector(x: SEXP) -> crate::sexp::object::SexpMut<'static> {
    node_factory()
        .wrap(x)
        .and_then(crate::sexp::object::SexpMut::try_from_checked)
        .unwrap_or_else(|error| r_error(error.to_string()))
}

fn vector_element(x: SEXP, i: R_xlen_t) -> SEXP {
    wrapped(x)
        .try_vector_elt(i)
        .map(|element| element.as_raw())
        .unwrap_or_else(|error| r_error(error.to_string()))
}

/// Audited boundary: read-only named-attribute traversal of a live object.
fn attribute(x: SEXP, symbol: SEXP) -> SEXP {
    // SAFETY: getAttrib walks the attribute pairlist without mutation.
    crate::sexp::attrib_core::getAttrib(x, symbol)
}

/// Audited boundary: named-attribute write on a live object.
fn set_attribute(x: SEXP, symbol: SEXP, value: SEXP) {
    // SAFETY: setAttrib mutates one named slot of a live object.
    crate::sexp::attrib_core::setAttrib(x, symbol, value)
}

/// Audited boundary: the names symbol is a process-global singleton.
fn names_symbol() -> SEXP {
    // SAFETY: returns the shared R_NamesSymbol singleton.
    crate::sexp::attrib_core::R_NamesSymbol()
}

/// Audited boundary: the class symbol is a process-global singleton.
fn class_symbol() -> SEXP {
    // SAFETY: returns the shared R_ClassSymbol singleton.
    crate::sexp::attrib_core::R_ClassSymbol()
}

/// Audited boundary: symbol interning under the active runtime.
fn install(name: &std::ffi::CStr) -> SEXP {
    // SAFETY: interns a NUL-terminated name under the active runtime.
    unsafe { crate::sexp::symbol::Rf_install(name.as_ptr()) }
}

/// Checked STRSXP(1) scalar; matches `Rf_mkString` on the session heap.
fn mk_string(text: &str) -> SEXP {
    node_factory()
        .strings(&[text])
        .map(|vector| vector.as_raw())
        .unwrap_or_else(|error| r_error(error.to_string()))
}

/// Checked CHARSXP; matches `Rf_mkCharLen` on the session heap.
fn mk_chars(bytes: &[u8]) -> SEXP {
    node_factory()
        .allocate(|arena| Some(arena.alloc_charsxp(bytes)))
        .map(|chars| chars.as_raw())
        .unwrap_or_else(|error| r_error(error.to_string()))
}

/// Checked vector allocation; matches `Rf_allocVector3` on the session heap.
fn alloc_vector(kind: SEXPTYPE, length: R_xlen_t) -> SEXP {
    node_factory()
        .allocate(|arena| Some(arena.alloc_vector(kind, length)))
        .map(|vector| vector.as_raw())
        .unwrap_or_else(|error| r_error(error.to_string()))
}

/// Checked logical scalar; matches `Rf_ScalarLogical` on the session heap.
fn scalar_logical(value: c_int) -> SEXP {
    let vector = node_factory()
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::LGLSXP, 1)))
        .unwrap_or_else(|error| r_error(error.to_string()));
    let mut vector = crate::sexp::object::SexpMut::try_from_checked(vector)
        .unwrap_or_else(|error| r_error(error.to_string()));
    vector
        .try_set_logical_elt(0, value)
        .unwrap_or_else(|error| r_error(error.to_string()));
    vector.freeze().as_raw()
}

/// Checked integer scalar; matches `Rf_ScalarInteger` on the session heap.
fn scalar_integer(value: c_int) -> SEXP {
    let vector = node_factory()
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .unwrap_or_else(|error| r_error(error.to_string()));
    let mut vector = crate::sexp::object::SexpMut::try_from_checked(vector)
        .unwrap_or_else(|error| r_error(error.to_string()));
    vector
        .try_set_integer_elt(0, value)
        .unwrap_or_else(|error| r_error(error.to_string()));
    vector.freeze().as_raw()
}

/// Checked real scalar; matches `Rf_ScalarReal` on the session heap.
fn scalar_real(value: c_double) -> SEXP {
    let vector = node_factory()
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::REALSXP, 1)))
        .unwrap_or_else(|error| r_error(error.to_string()));
    let mut vector = crate::sexp::object::SexpMut::try_from_checked(vector)
        .unwrap_or_else(|error| r_error(error.to_string()));
    vector
        .try_set_real_elt(0, value)
        .unwrap_or_else(|error| r_error(error.to_string()));
    vector.freeze().as_raw()
}
