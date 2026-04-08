/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001-2025 The R Core Team
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
 */

//! Port of R's src/library/grid/src/clippath.c
//!
//! Grid clip path type checking and resolution.

use std::os::raw::c_int;

use crate::sexp::envir::findFun;
use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;
use std::cell::Cell;

use super::types::*;

// ---------------------------------------------------------------------------
// External stubs for functions not yet ported
// ---------------------------------------------------------------------------

/// lang2(a, b) — build a call of two arguments
#[unsafe(no_mangle)]
unsafe fn lang2(a: SEXP, b: SEXP) -> SEXP {
    crate::sexp::constructors::Rf_cons(a, crate::sexp::constructors::Rf_cons(b, R_NilValue()))
}

/// R_gridEvalEnv — the grid package evaluation environment
thread_local! { static R_gridEvalEnv: Cell<SEXP> = Cell::new(std::ptr::null_mut()); }

/// Rf_inherits — check if object inherits from a given class
#[unsafe(no_mangle)]
unsafe fn Rf_inherits(x: SEXP, what: *const std::os::raw::c_char) -> c_int {
    if x.is_null() || what.is_null() {
        return 0;
    }
    let klass = crate::attrib_core::getAttrib(x, crate::attrib_core::R_ClassSymbol());
    if klass.is_null() || klass == R_NilValue() {
        return 0;
    }
    use crate::sexp::accessors::{CHAR, LENGTH, STRING_ELT, TYPEOF};
    use std::ffi::CStr;
    if TYPEOF(klass) != crate::sexp::ffi::SEXPTYPE::STRSXP.0 {
        return 0;
    }
    let cn = match CStr::from_ptr(what).to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let n = LENGTH(klass);
    for i in 0..n {
        let elt = STRING_ELT(klass, i as crate::sexp::ffi::R_xlen_t);
        if !elt.is_null() {
            let cs = CHAR(elt);
            if !cs.is_null() {
                if let Ok(s2) = CStr::from_ptr(cs).to_str() {
                    if s2 == cn {
                        return 1;
                    }
                }
            }
        }
    }
    0
}

/// ScalarLogical — create a single-element logical vector
#[unsafe(no_mangle)]
unsafe fn ScalarLogical(x: c_int) -> SEXP {
    let s = crate::sexp::constructors::Rf_allocVector(crate::sexp::ffi::SEXPTYPE::LGLSXP.0, 1);
    *crate::sexp::accessors::LOGICAL(s) = x;
    s
}

/// setGridStateElement — set a grid state element on a device
#[unsafe(no_mangle)]
unsafe fn setGridStateElement(
    _dd: *const u8, /* pGEDevDesc */
    _elementIndex: c_int,
    _value: SEXP,
) {
    // STUB: requires state.c
}

/// Rf_eval_with_gd — evaluate expression with device context
#[unsafe(no_mangle)]
unsafe fn Rf_eval_with_gd(_call: SEXP, _env: SEXP, _dd: *const u8 /* pGEDevDesc */) -> SEXP {
    R_NilValue()
}

// ---------------------------------------------------------------------------
// isClipPath — check if object is a GridClipPath
// ---------------------------------------------------------------------------

pub unsafe fn isClipPath(clip: SEXP) -> bool {
    Rf_inherits(
        clip,
        b"GridClipPath\0".as_ptr() as *const std::os::raw::c_char,
    ) != 0
}

// ---------------------------------------------------------------------------
// resolveClipPath — resolve a clip path via R callback
// ---------------------------------------------------------------------------

pub unsafe fn resolveClipPath(path: SEXP, dd: *const u8 /* pGEDevDesc */) -> SEXP {
    setGridStateElement(dd, GSS_RESOLVINGPATH, ScalarLogical(1));
    let resolve_fn = Rf_protect(findFun(
        Rf_install(b"resolveClipPath\0".as_ptr() as *const std::os::raw::c_char),
        R_gridEvalEnv.with(|v| v.get()),
    ));
    let r_fcall = Rf_protect(lang2(resolve_fn, path));
    let result = Rf_eval_with_gd(r_fcall, R_gridEvalEnv.with(|v| v.get()), dd);
    setGridStateElement(dd, GSS_RESOLVINGPATH, ScalarLogical(0));
    Rf_unprotect(2);
    result
}
