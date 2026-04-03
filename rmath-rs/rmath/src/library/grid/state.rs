#![allow(unsafe_op_in_unsafe_fn)]

//! Port of R's src/library/grid/src/state.c -- grid system state management.
//!
//! Manages per-device grid state including display lists, viewports,
//! graphics parameters, and engine callbacks.

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};

use crate::attrib_core;
use crate::sexp::accessors::{
    CADR, CAR, INTEGER, LENGTH, LOGICAL, REAL, SET_VECTOR_ELT, STRING_ELT, TYPEOF, VECTOR_ELT,
};
use crate::sexp::constructors::{Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector, Rf_mkString};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

use super::types::*;

/* ==================== GE function stubs ==================== */

unsafe extern "C" {
    /// Stub: get current graphics device
    fn GEcurrentDevice() -> pGEDevDesc;
    /// Stub: mark device as dirty
    fn GEdirtyDevice(dd: pGEDevDesc);
    /// Stub: start a new page on the device
    fn GENewPage(gc: pGEcontext, dd: pGEDevDesc);
    /// Stub: get device size in cm
    fn getDeviceSize(dd: pGEDevDesc, width: *mut c_double, height: *mut c_double);
    /// Stub: get the current device (grid wrapper for GEcurrentDevice)
    fn getDevice() -> pGEDevDesc;

    /// Stub: initialize viewport stack
    fn initVP(dd: pGEDevDesc);
    /// Stub: initialize graphics parameters
    fn initGPar(dd: pGEDevDesc);
    /// Stub: resolve graphics parameters
    fn resolveGPar(gp: SEXP, by_name: c_int) -> SEXP;
    /// Stub: create gcontext from gpar
    fn gcontextFromgpar(gp: SEXP, i: c_int, gc: pGEcontext, dd: pGEDevDesc);
}

/* ==================== GE event constants ==================== */

/// GE event: initialize state for a new device
pub const GE_InitState: c_int = 0;
/// GE event: finalize state when device is closed
pub const GE_FinaliseState: c_int = 1;
/// GE event: save state (before copy/resize)
pub const GE_SaveState: c_int = 2;
/// GE event: restore state (after copy/resize)
pub const GE_RestoreState: c_int = 3;
/// GE event: copy display list between devices
pub const GE_CopyState: c_int = 4;
/// GE event: check if device has a plot
pub const GE_CheckPlot: c_int = 5;
/// GE event: save snapshot state
pub const GE_SaveSnapshotState: c_int = 6;
/// GE event: restore snapshot state
pub const GE_RestoreSnapshotState: c_int = 7;
/// GE event: scale postscript
pub const GE_ScalePS: c_int = 8;

/// GE_INCHES constant for unit conversion
pub const GE_INCHES: c_int = 1;

/* ==================== Helper: Rf_error ==================== */

/// Panic with an R-style error message.
#[inline]
unsafe fn error(msg: &str) {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

/* ==================== Helper: R_BlankString ==================== */

/// Get R_BlankString.
#[inline]
unsafe fn R_BlankString() -> SEXP {
    crate::sexp::constructors::Rf_mkChar(b"\0".as_ptr() as *const c_char)
}

/* ==================== Helper: isNull ==================== */

#[inline]
unsafe fn isNull(x: SEXP) -> bool {
    x.is_null() || x == R_NilValue()
}

/* ==================== Helper: isVector ==================== */

#[inline]
unsafe fn isVector(x: SEXP) -> bool {
    let t = TYPEOF(x);
    t == SEXPTYPE::LGLSXP.0
        || t == SEXPTYPE::INTSXP.0
        || t == SEXPTYPE::REALSXP.0
        || t == SEXPTYPE::CPLXSXP.0
        || t == SEXPTYPE::STRSXP.0
        || t == SEXPTYPE::VECSXP.0
        || t == SEXPTYPE::EXPRSXP.0
}

/* ==================== Helper: isString ==================== */

#[inline]
unsafe fn isString(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::STRSXP.0
}

/* ==================== Helper: findVar ==================== */

#[inline]
unsafe fn findVar(sym: SEXP, rho: SEXP) -> SEXP {
    crate::sexp::envir::R_findVarInFrame(rho, sym)
}

/* ==================== Helper: lang1 ==================== */

#[inline]
unsafe fn lang1(_sym: SEXP) -> SEXP {
    // Stub: create a one-element call
    crate::sexp::memory_ext::allocLang(1)
}

/* ==================== Core state functions ==================== */

/// Create the grid system state (VECSXP of length 18).
/// One element per GSS_* constant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createGridSystemState() -> SEXP {
    Rf_allocVector(SEXPTYPE::VECSXP.0 as i32, 18)
}

/// Initialize the display list for a device.
/// The top-level viewport goes at the start of the display list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn initDL(dd: pGEDevDesc) {
    // Stub: since pGEDevDesc is void*, we cannot access dd->gesd.
    let _ = dd;
}

/// Initialize some bits of the system state (called before engine redraw).
/// Does NOT init all state; display list, root viewport, and current gpar
/// are initialized separately (see initDL, initVP, initGPar).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn initOtherState(dd: pGEDevDesc) {
    // Stub: since pGEDevDesc is void*, we cannot access dd->gesd.
    let _ = dd;
}

/// Fill the grid system state with initial values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fillGridSystemState(state: SEXP, dd: pGEDevDesc) {
    use crate::sexp::ffi::NA_REAL;

    Rf_protect(state);

    // GSS_DEVSIZE: current size of device
    let devsize = Rf_allocVector(SEXPTYPE::REALSXP.0 as i32, 2);
    *REAL(devsize).add(0) = 0.0;
    *REAL(devsize).add(1) = 0.0;
    SET_VECTOR_ELT(state, GSS_DEVSIZE as R_xlen_t, devsize);

    // GSS_CURRLOC: current location of grid "pen"
    let currloc = Rf_allocVector(SEXPTYPE::REALSXP.0 as i32, 2);
    *REAL(currloc).add(0) = NA_REAL;
    *REAL(currloc).add(1) = NA_REAL;
    SET_VECTOR_ELT(state, GSS_CURRLOC as R_xlen_t, currloc);

    // GSS_PREVLOC: previous location of grid "pen"
    let prevloc = Rf_allocVector(SEXPTYPE::REALSXP.0 as i32, 2);
    *REAL(prevloc).add(0) = NA_REAL;
    *REAL(prevloc).add(1) = NA_REAL;
    SET_VECTOR_ELT(state, GSS_PREVLOC as R_xlen_t, prevloc);

    // GSS_DLON: is the display list on?
    SET_VECTOR_ELT(state, GSS_DLON as R_xlen_t, Rf_ScalarLogical(1));

    // GSS_ENGINEDLON: are we using the engine's display list?
    SET_VECTOR_ELT(state, GSS_ENGINEDLON as R_xlen_t, Rf_ScalarLogical(1));

    // GSS_CURRGROB: current grob being drawn
    SET_VECTOR_ELT(state, GSS_CURRGROB as R_xlen_t, R_NilValue());

    // GSS_ENGINERECORDING: are we inside a .Call.graphics call?
    SET_VECTOR_ELT(state, GSS_ENGINERECORDING as R_xlen_t, Rf_ScalarLogical(0));

    // GSS_GPAR: initialize graphics parameters
    initGPar(dd);

    // GSS_GPSAVED: previous gpar settings
    SET_VECTOR_ELT(state, GSS_GPSAVED as R_xlen_t, R_NilValue());

    // GSS_GLOBALINDEX: index in global state list
    SET_VECTOR_ELT(state, GSS_GLOBALINDEX as R_xlen_t, R_NilValue());

    // GSS_GRIDDEVICE: does this device contain grid output?
    SET_VECTOR_ELT(state, GSS_GRIDDEVICE as R_xlen_t, Rf_ScalarLogical(0));

    // GSS_SCALE: zoom factor
    SET_VECTOR_ELT(state, GSS_SCALE as R_xlen_t, Rf_ScalarReal(1.0));

    // GSS_RESOLVINGPATH: are we resolving a clipping path?
    SET_VECTOR_ELT(state, GSS_RESOLVINGPATH as R_xlen_t, Rf_ScalarLogical(0));

    // GSS_GROUPS: group name to device reference mapping
    SET_VECTOR_ELT(state, GSS_GROUPS as R_xlen_t, R_NilValue());

    Rf_unprotect(1);
}

/// Get a grid state element by index.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gridStateElement(dd: pGEDevDesc, elementIndex: c_int) -> SEXP {
    // Stub: since pGEDevDesc is void*, we cannot access gesd.
    let _ = dd;
    let _ = elementIndex;
    R_NilValue()
}

/// Set a grid state element by index.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setGridStateElement(dd: pGEDevDesc, elementIndex: c_int, value: SEXP) {
    // Stub: since pGEDevDesc is void*, we cannot access gesd.
    let _ = dd;
    let _ = elementIndex;
    let _ = value;
}

/// Callable from R code: set a grid state element.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn L_setGridState(elementIndex: SEXP, value: SEXP) -> SEXP {
    let dd = getDevice();
    setGridStateElement(dd, *INTEGER(elementIndex).add(0), value);
    R_NilValue()
}

/* ==================== State slot management ==================== */

/// Remove state from global variable (for GC).
unsafe fn deglobaliseState(state: SEXP) {
    let index = *INTEGER(VECTOR_ELT(state, GSS_GLOBALINDEX as R_xlen_t)).add(0);
    let sym = Rf_install(b".GRID.STATE\0".as_ptr() as *const c_char);
    let globalstate = findVar(sym, R_gridEvalEnv);
    SET_VECTOR_ELT(globalstate, index as R_xlen_t, R_NilValue());
}

/// Find an empty slot in the global state list.
unsafe fn findStateSlot() -> c_int {
    let sym = Rf_install(b".GRID.STATE\0".as_ptr() as *const c_char);
    let globalstate = findVar(sym, R_gridEvalEnv);
    let len = LENGTH(globalstate) as i32;
    for i in 0..len {
        if isNull(VECTOR_ELT(globalstate, i as R_xlen_t)) {
            return i;
        }
    }
    error("unable to store 'grid' state.  Too many devices open?");
    -1
}

/// Store state in a global variable (to prevent GC).
unsafe fn globaliseState(state: SEXP) {
    let index = findStateSlot();
    let sym = Rf_install(b".GRID.STATE\0".as_ptr() as *const c_char);
    let globalstate = findVar(sym, R_gridEvalEnv);
    Rf_protect(globalstate);
    let indexsxp = Rf_allocVector(SEXPTYPE::INTSXP.0 as i32, 1);
    Rf_protect(indexsxp);
    *INTEGER(indexsxp).add(0) = index;
    SET_VECTOR_ELT(state, GSS_GLOBALINDEX as R_xlen_t, indexsxp);
    SET_VECTOR_ELT(globalstate, index as R_xlen_t, state);
    Rf_unprotect(2);
}

/* ==================== GE event callback ==================== */

/// Grid callback for graphics engine events.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gridCallback(task: GEevent, dd: pGEDevDesc, data: SEXP) -> SEXP {
    let mut result: SEXP = R_NilValue();

    match task {
        GE_InitState => {
            // Create the initial grid state for a device
            let gridState = createGridSystemState();
            Rf_protect(gridState);
            // Store that state with the device for easy retrieval
            // (stub: cannot access dd->gesd)
            // Initialize the grid state for a device
            fillGridSystemState(gridState, dd);
            // Store the state beneath a top-level variable so it does not get GC'd
            globaliseState(gridState);
            // Indicate success
            result = R_BlankString();
            Rf_unprotect(1);
        }
        GE_FinaliseState => {
            // Stub: cannot access dd->gesd with void* pGEDevDesc.
        }
        GE_SaveState => {
            // Nothing to do
        }
        GE_RestoreState => {
            // Stub: complex state restoration logic deferred until GE is ported.
        }
        GE_CopyState => {
            // Stub: copy display list between devices
        }
        GE_CheckPlot => {
            let valid = Rf_allocVector(SEXPTYPE::LGLSXP.0 as i32, 1);
            *LOGICAL(valid).add(0) = 1;
            result = valid;
        }
        GE_SaveSnapshotState => {
            result = Rf_allocVector(SEXPTYPE::VECSXP.0 as i32, 2);
            Rf_protect(result);
            SET_VECTOR_ELT(result, 0, R_NilValue());
            SET_VECTOR_ELT(result, 1, R_NilValue());
            let pkgName = Rf_mkString(b"grid\0".as_ptr() as *const c_char);
            Rf_protect(pkgName);
            attrib_core::setAttrib(
                result,
                Rf_install(b"pkgName\0".as_ptr() as *const c_char),
                pkgName,
            );
            Rf_unprotect(2);
        }
        GE_RestoreSnapshotState => {
            // Stub: complex snapshot restoration logic deferred until GE is ported.
            result = R_NilValue();
        }
        GE_ScalePS => {
            // data is a numeric scale factor
            let scale = Rf_allocVector(SEXPTYPE::REALSXP.0 as i32, 1);
            *REAL(scale).add(0) = 1.0; // stub
            let _ = dd;
            let _ = data;
            result = scale;
        }
        _ => {
            // Unknown event - ignore
        }
    }

    result
}
