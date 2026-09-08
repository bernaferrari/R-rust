/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2000--2013  The R Core Team
 *
 *  Ported from r-source/src/library/tcltk/src/tcltk_win.c
 *
 *  Windows-specific Tcl/Tk startup/shutdown stubs.
 */

use std::os::raw::{c_int, c_void};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

unsafe extern "C" {
    fn tcltk_init(TkUp: *mut c_int);
}

// ---------------------------------------------------------------------------
// tcltk_start -- Windows-only entry point called from .C("tcltk_start")
// ---------------------------------------------------------------------------

/// Start the Tcl/Tk subsystem on Windows.
///
/// In the real implementation this:
///   1. Saves the current foreground window (ActiveTCL steals focus)
///   2. Calls tcltk_init if not already done
///   3. Installs a Tcl polling function via set_R_Tcldo
///   4. Restores the foreground window
#[cfg(target_os = "windows")]
pub unsafe fn tcltk_start() {
    let mut tk_up: c_int = 0;
    tcltk_init(&mut tk_up);
    // Stub: no set_R_Tcldo / SetForegroundWindow available
}

#[cfg(not(target_os = "windows"))]
pub unsafe fn tcltk_start() {
    // No-op on non-Windows platforms
}

// ---------------------------------------------------------------------------
// tcltk_end -- Windows-only teardown called from .C("tcltk_end")
// ---------------------------------------------------------------------------

/// Stop the Tcl/Tk subsystem on Windows.
///
/// In the real implementation this calls unset_R_Tcldo to remove
/// the Tcl event polling callback.
#[cfg(target_os = "windows")]
pub fn tcltk_end() {
    // Stub: no unset_R_Tcldo available
}

#[cfg(not(target_os = "windows"))]
pub fn tcltk_end() {
    // No-op on non-Windows platforms
}

// ---------------------------------------------------------------------------
// Windows-specific internal stubs (module-private, no #[no_mangle])
// ---------------------------------------------------------------------------

/// Tcl spin loop -- processes pending Tcl events via Tcl_ServiceAll.
#[cfg(target_os = "windows")]
unsafe fn TclSpinLoop(_data: *mut c_void) {
    // Stub: in the real implementation calls Tcl_ServiceAll()
}

/// R_ToplevelExec wrapper that invokes TclSpinLoop.
#[cfg(target_os = "windows")]
unsafe fn _R_tcldo() {
    // Stub: in the real implementation calls R_ToplevelExec(TclSpinLoop, NULL)
}
