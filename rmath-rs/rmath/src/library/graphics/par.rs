/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1997--2022  The R Core Team
 *  Copyright (C) 1995, 1996  Robert Gentleman and Ross Ihaka
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
 *  Ported from r-source/src/library/graphics/src/par.c
 *
 *  GRZ-like state information.
 *  Provides the functionality of the "par" function in S.
 */

use std::ffi::c_void;
use std::os::raw::{c_char, c_double, c_int, c_uchar, c_ushort};

/// Local helper: get R_BlankString (empty string CHARSXP).
#[inline]
unsafe fn R_BlankString() -> SEXP {
    Rf_mkChar(b"\0".as_ptr() as *const c_char)
}

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

/* ---- ParTable: pure data, no graphics engine dependency ---- */

/// ParTab entry: maps a parameter name to a code.
/// code: 0 = normal, 1 = not inline, 2 = read-only,
///       -1 = unknown, -2 = obsolete, -3 = graphical args
#[derive(Clone, Copy)]
struct ParTab {
    name: *const c_char,
    code: c_int,
}

// Safety: PAR_TABLE only contains pointers to static string literals,
// which live for the entire program duration.
unsafe impl Sync for ParTab {}

/// The complete ParTable array from par.c.
/// This is pure data used by ParCode() to look up parameter codes.
static PAR_TABLE: &[ParTab] = &[
    ParTab {
        name: b"adj\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"ann\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"ask\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"bg\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"bty\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cex\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cex.axis\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cex.lab\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cex.main\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cex.sub\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cin\0".as_ptr() as *const c_char,
        code: 2,
    },
    ParTab {
        name: b"col\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"col.axis\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"col.lab\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"col.main\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"col.sub\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cra\0".as_ptr() as *const c_char,
        code: 2,
    },
    ParTab {
        name: b"crt\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"csi\0".as_ptr() as *const c_char,
        code: 2,
    },
    ParTab {
        name: b"csy\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cxy\0".as_ptr() as *const c_char,
        code: 2,
    },
    ParTab {
        name: b"din\0".as_ptr() as *const c_char,
        code: 2,
    },
    ParTab {
        name: b"err\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"family\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"fg\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"fig\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"fin\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"font\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"font.axis\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"font.lab\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"font.main\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"font.sub\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"lab\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"las\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"lend\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"lheight\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"ljoin\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"lmitre\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"lty\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"lwd\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"mai\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"mar\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"mex\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"mfcol\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"mfg\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"mfrow\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"mgp\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"mkh\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"new\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"oma\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"omd\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"omi\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"page\0".as_ptr() as *const c_char,
        code: 2,
    },
    ParTab {
        name: b"pch\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"pin\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"plt\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"ps\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"pty\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"smo\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"srt\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"tck\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"tcl\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"usr\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"xaxp\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"xaxs\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"xaxt\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"xlog\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"xpd\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"yaxp\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"yaxs\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"yaxt\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"ylbias\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"ylog\0".as_ptr() as *const c_char,
        code: 1,
    },
    /* Obsolete pars */
    ParTab {
        name: b"gamma\0".as_ptr() as *const c_char,
        code: -2,
    },
    ParTab {
        name: b"type\0".as_ptr() as *const c_char,
        code: -2,
    },
    ParTab {
        name: b"tmag\0".as_ptr() as *const c_char,
        code: -2,
    },
    /* Non-pars that might get passed to Specify2 */
    ParTab {
        name: b"asp\0".as_ptr() as *const c_char,
        code: -3,
    },
    ParTab {
        name: b"main\0".as_ptr() as *const c_char,
        code: -3,
    },
    ParTab {
        name: b"sub\0".as_ptr() as *const c_char,
        code: -3,
    },
    ParTab {
        name: b"xlab\0".as_ptr() as *const c_char,
        code: -3,
    },
    ParTab {
        name: b"ylab\0".as_ptr() as *const c_char,
        code: -3,
    },
    ParTab {
        name: b"xlim\0".as_ptr() as *const c_char,
        code: -3,
    },
    ParTab {
        name: b"ylim\0".as_ptr() as *const c_char,
        code: -3,
    },
    ParTab {
        name: std::ptr::null(),
        code: -1,
    },
];

/// pGEDevDesc is an opaque pointer to the graphics device descriptor.
/// The full type is defined in the Graphics Engine, which is not yet ported.
type pGEDevDesc = *mut c_void;

/// Look up a graphical parameter name in ParTable and return its code.
/// Returns -1 if not found.
///
/// This is the Rust equivalent of `static int ParCode(const char *what)`.
pub unsafe fn ParCode(what: *const c_char) -> c_int {
    if what.is_null() {
        return -1;
    }
    let what_str = std::ffi::CStr::from_ptr(what);
    let what_bytes = what_str.to_bytes();
    for entry in PAR_TABLE.iter() {
        if entry.name.is_null() {
            break;
        }
        let name_str = std::ffi::CStr::from_ptr(entry.name);
        if name_str.to_bytes() == what_bytes {
            return entry.code;
        }
    }
    -1
}

/* ---- Stub helper functions ---- */

/// Helper: compare two C strings for equality.
unsafe fn streql(a: *const c_char, b: *const c_char) -> bool {
    if a.is_null() || b.is_null() {
        return false;
    }
    libc::strcmp(a, b) == 0
}

/* ---- Stub: Specify (par(what = value)) ---- */

/// Specify -- set a graphical parameter via par().
/// Stub implementation: does nothing since the graphics engine is not ported.
unsafe fn Specify(_what: *const c_char, _value: SEXP, _dd: pGEDevDesc) {
    /* Stub: full implementation requires GPar, dpptr, gpptr, GReset, etc. */
}

/* ---- Stub: Specify2 (high-level plot args) ---- */

/// Specify2 -- set a graphical parameter from a high-level graphics function.
/// Stub implementation: does nothing.
unsafe fn Specify2(_what: *const c_char, _value: SEXP, _dd: pGEDevDesc) {
    /* Stub: full implementation requires GPar, dpptr, gpptr, etc. */
}

/* ---- Stub: Query (par(what) -- return current value) ---- */

/// Query -- return the current value of a graphical parameter.
/// Stub implementation: returns R_NilValue for all parameters.
unsafe fn Query(_what: *const c_char, _dd: pGEDevDesc) -> SEXP {
    R_NilValue()
}

/* ---- Stub: C_par (the main par() .Internal) ---- */

/// C_par -- implementation of R's par() function.
/// This is the .Internal(par(...)) entry point.
///
/// Original C signature:
///   SEXP C_par(SEXP call, SEXP op, SEXP args, SEXP rho)
///
/// Stub: returns an empty named list.
pub unsafe fn C_par(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::attrib_core::{R_NamesSymbol, getAttrib, setAttrib};

    /* Stub: the full implementation requires GEcurrentDevice(),
     * Query(), Specify(), GRecording(), GErecordGraphicOperation().
     * We parse args minimally and return an empty list. */
    let _ = call;
    let _ = op;
    let _ = rho;

    let args_cdr = CDR(args);
    if args_cdr == R_NilValue() {
        return R_NilValue();
    }

    let arg1 = CAR(args_cdr);
    let nargs = LENGTH(arg1);

    if nargs <= 0 {
        return R_NilValue();
    }

    /* Build a named list with all R_NilValue entries */
    let value = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, nargs));
    let newnames = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, nargs));

    /* Try to get names from the argument */
    let oldnames = getAttrib(arg1, R_NamesSymbol());

    for i in 0..(nargs as usize) {
        SET_VECTOR_ELT(value, i as R_xlen_t, R_NilValue());
        if oldnames != R_NilValue() {
            let tag = STRING_ELT(oldnames, i as R_xlen_t);
            SET_STRING_ELT(newnames, i as R_xlen_t, tag);
        } else {
            SET_STRING_ELT(newnames, i as R_xlen_t, R_BlankString());
        }
    }

    setAttrib(value, R_NamesSymbol(), newnames);
    Rf_unprotect(2);
    value
}

/* ---- Stub: C_layout (the layout() .Internal) ---- */

/// C_layout -- implementation of R's layout() function.
/// This is the .Internal(layout(...)) entry point.
///
/// Original C signature:
///   SEXP C_layout(SEXP args)
///
/// Stub: returns R_NilValue.
pub unsafe fn C_layout(args: SEXP) -> SEXP {
    let _ = args;
    /* Stub: the full implementation requires GEcurrentDevice(),
     * dpptr, gpptr, GReset, and all the layout parameter processing. */
    R_NilValue()
}

/* ---- Stub: ProcessInlinePars ---- */

/// ProcessInlinePars -- handles inline par specifications in graphics functions.
/// Stub implementation: does nothing.
#[unsafe(no_mangle)]
pub unsafe fn ProcessInlinePars(_s: SEXP, _dd: pGEDevDesc) {
    /* Stub: full implementation walks a list and calls Specify2 for each tagged pair */
}

/* ---- Stub: baseCallback (GE event handler) ---- */

/// baseCallback -- event handler for the base graphics system, registered
/// with the Graphics Engine via GEregisterSystem.
/// Stub: returns R_NilValue for all events.
pub unsafe fn baseCallback(_task: c_int, _dd: pGEDevDesc, _data: SEXP) -> SEXP {
    R_NilValue()
}

/* ---- Stub: registerBase / unregisterBase / RunregisterBase ---- */

/// registerBase -- register the base graphics system with the Graphics Engine.
/// Stub: does nothing.
pub fn registerBase() {
    /* Stub: calls GEregisterSystem(baseCallback, &baseRegisterIndex) */
}

/// unregisterBase -- unregister the base graphics system.
/// Stub: does nothing.
pub fn unregisterBase() {
    /* Stub: calls GEunregisterSystem(baseRegisterIndex) */
}

/// RunregisterBase -- R-callable wrapper for unregisterBase.
/// Returns R_NilValue.
pub unsafe fn RunregisterBase() -> SEXP {
    unregisterBase();
    R_NilValue()
}

/* ---- Stub: gpptr / dpptr / dpSavedptr / Rf_setBaseDevice ---- */

/// gpptr -- get the current GPar pointer (graphics parameters).
/// Stub: returns null.
#[unsafe(no_mangle)]
pub unsafe fn gpptr(_dd: pGEDevDesc) -> *mut c_void {
    std::ptr::null_mut()
}

/// dpptr -- get the display GPar pointer (display parameters).
/// Stub: returns null.
#[unsafe(no_mangle)]
pub unsafe fn dpptr(_dd: pGEDevDesc) -> *mut c_void {
    std::ptr::null_mut()
}

/// dpSavedptr -- get the saved display GPar pointer.
/// Stub: returns null.
pub unsafe fn dpSavedptr(_dd: pGEDevDesc) -> *mut c_void {
    std::ptr::null_mut()
}

/// Rf_setBaseDevice -- mark the device as "dirty" (has received base output).
/// Stub: does nothing.
pub unsafe fn Rf_setBaseDevice(_val: c_int, _dd: pGEDevDesc) {
    /* Stub: sets bss->baseDevice = val */
}

/* ---- Stub: currentFigureLocation ---- */

/// currentFigureLocation -- get the current figure's row and column.
/// Stub: sets both to 0.
pub unsafe fn currentFigureLocation(row: *mut c_int, col: *mut c_int, _dd: pGEDevDesc) {
    if !row.is_null() {
        *row = 0;
    }
    if !col.is_null() {
        *col = 0;
    }
}

/* ---- Stub: restoredpSaved ---- */

/// restoredpSaved -- restore display parameters from saved state.
/// Stub: does nothing.
pub unsafe fn restoredpSaved(_dd: pGEDevDesc) {
    /* Stub: full implementation copies all fields from dpSaved to dp */
}
