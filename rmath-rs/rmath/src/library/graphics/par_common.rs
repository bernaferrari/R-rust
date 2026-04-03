#![allow(unsafe_op_in_unsafe_fn)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1997-2012  The R Core Team
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
 *  Ported from r-source/src/library/graphics/src/par-common.c
 *
 *  Graphical parameters which are treated identically by
 *  par( <nam> = <value> )  and  highlevel  plotfun (..., <nam> = <value> ).
 *
 *  In the original C, this file is literally #included into par.c twice:
 *    once inside Specify()   (with #define FOR_PAR)
 *    once inside Specify2()  (without #define FOR_PAR)
 *
 *  For the Rust port, we make this a proper module with a single function
 *  that takes a `for_par` flag to distinguish the two contexts.
 *
 *  Currently a STUB since the graphics engine (GPar, dpptr, gpptr,
 *  R_DEV__, RGBpar3, etc.) is not yet ported.
 */

use std::ffi::c_void;

use crate::sexp::ffi::SEXP;

/// Opaque pointer to graphics device descriptor (pGEDevDesc).
/// The full type is defined in the Graphics Engine, which is not yet ported.
type pGEDevDesc = *mut c_void;

/// specify_common -- handle the common graphical parameters.
///
/// This is the Rust equivalent of the #include "par-common.c" fragment.
/// It handles parameters like adj, ann, bg, bty, cex, col, font, lab, las,
/// lty, lwd, mgp, pch, tck, tcl, xaxp, yaxp, xaxs, yaxs, xaxt, yaxt, xpd, etc.
///
/// # Arguments
/// * `what`   - the parameter name (C string)
/// * `value`  - the R SEXP value to set
/// * `for_par` - true if called from par() context (Specify), false for
///               high-level plot functions (Specify2)
/// * `dd`     - the graphics device descriptor (pGEDevDesc)
///
/// # Returns
/// * true  - the parameter was recognized and handled
/// * false - the parameter was not recognized (caller should try other handlers)
///
/// In the original C code, unhandled parameters simply fall through the
/// if-else chain and the caller continues to device-specific or
/// display-specific parameter handlers.
///
/// Stub: returns false (no parameters handled) since the GE internals
/// (R_DEV__, dpptr, gpptr, RGBpar3, lengthCheck, BoundsCheck, etc.)
/// are not yet ported.
pub(crate) unsafe fn specify_common(
    what: *const std::os::raw::c_char,
    _value: SEXP,
    _for_par: bool,
    _dd: pGEDevDesc,
) -> bool {
    use std::ffi::CStr;

    if what.is_null() {
        return false;
    }

    let what_str = match CStr::from_ptr(what).to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    /* Graphical parameters which are treated identically by
     * par( <nam> = <value> ) and highlevel plotfun (..., <nam> = <value> ).
     *
     * Stub implementation: we check if the parameter name is one of the
     * known common parameters and return true (recognized) but do not
     * actually set any values, since the graphics engine internals
     * (GPar, R_DEV__, dpptr, gpptr, etc.) are not yet ported.
     *
     * When the GE internals are available, each arm below should contain
     * the actual parameter validation and assignment logic from the
     * original par-common.c.
     */

    let _handled = match what_str {
        /* --- adj --- */
        "adj" => true,
        /* --- ann --- */
        "ann" => true,
        /* --- bg (par: plot region bg; inline: filled points bg) --- */
        "bg" => true,
        /* --- bty --- */
        "bty" => true,
        /* --- cex (par: sets cexbase; inline: sets cex directly) --- */
        "cex" => true,
        /* --- cex.main --- */
        "cex.main" => true,
        /* --- cex.lab --- */
        "cex.lab" => true,
        /* --- cex.sub --- */
        "cex.sub" => true,
        /* --- cex.axis --- */
        "cex.axis" => true,
        /* --- col --- */
        "col" => true,
        /* --- col.main --- */
        "col.main" => true,
        /* --- col.lab --- */
        "col.lab" => true,
        /* --- col.sub --- */
        "col.sub" => true,
        /* --- col.axis --- */
        "col.axis" => true,
        /* --- crt --- */
        "crt" => true,
        /* --- err --- */
        "err" => true,
        /* --- family --- */
        "family" => true,
        /* --- fg (par: sets both fg and col; inline: fg only) --- */
        "fg" => true,
        /* --- font --- */
        "font" => true,
        /* --- font.main --- */
        "font.main" => true,
        /* --- font.lab --- */
        "font.lab" => true,
        /* --- font.sub --- */
        "font.sub" => true,
        /* --- font.axis --- */
        "font.axis" => true,
        /* --- lab --- */
        "lab" => true,
        /* --- las --- */
        "las" => true,
        /* --- lend --- */
        "lend" => true,
        /* --- ljoin --- */
        "ljoin" => true,
        /* --- lmitre --- */
        "lmitre" => true,
        /* --- lty --- */
        "lty" => true,
        /* --- lwd --- */
        "lwd" => true,
        /* --- mgp --- */
        "mgp" => true,
        /* --- mkh --- */
        "mkh" => true,
        /* --- pch --- */
        "pch" => true,
        /* --- smo --- */
        "smo" => true,
        /* --- srt --- */
        "srt" => true,
        /* --- tck (must be treated in parallel with tcl) --- */
        "tck" => true,
        /* --- tcl (must be treated in parallel with tck) --- */
        "tcl" => true,
        /* --- xaxp --- */
        "xaxp" => true,
        /* --- xaxs --- */
        "xaxs" => true,
        /* --- xaxt --- */
        "xaxt" => true,
        /* --- xpd --- */
        "xpd" => true,
        /* --- yaxp --- */
        "yaxp" => true,
        /* --- yaxs --- */
        "yaxs" => true,
        /* --- yaxt --- */
        "yaxt" => true,
        /* Unknown parameter -- let the caller handle it */
        _ => return false,
    };

    true
}
