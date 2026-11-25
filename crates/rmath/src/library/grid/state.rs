//! Port of R's src/library/grid/src/state.c -- grid system state management.
//!
//! Manages per-device grid state including display lists, viewports,
//! graphics parameters, and engine callbacks.

use std::os::raw::c_int;

use crate::attrib_core;
use crate::library::grid::gpar::initGPar;
use crate::library::grid::grid::getDevice;
use crate::sexp::accessors::{LENGTH, TYPEOF};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

use super::types::*;
use super::{
    alloc_vector, install, integer_elt, mk_char, mk_string, protect, scalar_logical, scalar_real,
    set_integer_elt, set_logical_elt, set_real_elt, set_vector_elt, vector_elt,
};

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
pub const GE_INCHES: c_int = crate::mainutils::engine::GE_INCHES;

/* ==================== Helper: Rf_error ==================== */

/// Panic with an R-style error message.
#[inline]
fn error(msg: &str) {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

/* ==================== Helper: R_BlankString ==================== */

/// Get R_BlankString.
#[inline]
fn R_BlankString() -> SEXP {
    mk_char("")
}

/* ==================== Helper: isNull ==================== */

#[inline]
fn isNull(x: SEXP) -> bool {
    x.is_null() || x == R_NilValue()
}

/* ==================== Helper: isVector ==================== */

#[inline]
fn isVector(x: SEXP) -> bool {
    let t = TYPEOF(x);
    t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::VECSXP
        || t == SEXPTYPE::EXPRSXP
}

/* ==================== Helper: isString ==================== */

#[inline]
fn isString(x: SEXP) -> bool {
    TYPEOF(x) == SEXPTYPE::STRSXP
}

/* ==================== Helper: findVar ==================== */

/// Raw environment-frame lookup bridge.
#[inline]
fn findVar(sym: SEXP, rho: SEXP) -> SEXP {
    crate::sexp::envir::R_findVarInFrame(rho, sym)
}

/* ==================== Helper: lang1 ==================== */

#[inline]
fn lang1(_sym: SEXP) -> SEXP {
    // Stub: create a one-element call
    crate::sexp::memory_ext::allocLang(1)
}

/* ==================== Core state functions ==================== */

/// Create the grid system state (VECSXP of length 18).
/// One element per GSS_* constant.
pub fn createGridSystemState() -> SEXP {
    alloc_vector(SEXPTYPE::VECSXP, 18)
}

/// Initialize the display list for a device.
///
/// Creates an empty list to hold grob references, sets the display list
/// index to 0, and marks the display list as active.
pub fn initDL(dd: pGEDevDesc) {
    let dl = alloc_vector(SEXPTYPE::VECSXP, 100);
    let _dl_guard = protect(dl);
    setGridStateElement(dd, GSS_DL, dl);
    setGridStateElement(dd, GSS_DLINDEX, alloc_vector(SEXPTYPE::INTSXP, 1));
    setGridStateElement(dd, GSS_DLON, scalar_logical(1));
}


/// Fill the grid system state with initial values.
pub fn fillGridSystemState(state: SEXP, dd: pGEDevDesc) {
    use crate::sexp::ffi::NA_REAL;

    let _state_guard = protect(state);

    // GSS_DEVSIZE: current size of device
    let devsize = alloc_vector(SEXPTYPE::REALSXP, 2);
    set_real_elt(devsize, 0, 0.0);
    set_real_elt(devsize, 1, 0.0);
    set_vector_elt(state, GSS_DEVSIZE as R_xlen_t, devsize);

    // GSS_CURRLOC: current location of grid "pen"
    let currloc = alloc_vector(SEXPTYPE::REALSXP, 2);
    set_real_elt(currloc, 0, NA_REAL);
    set_real_elt(currloc, 1, NA_REAL);
    set_vector_elt(state, GSS_CURRLOC as R_xlen_t, currloc);

    // GSS_PREVLOC: previous location of grid "pen"
    let prevloc = alloc_vector(SEXPTYPE::REALSXP, 2);
    set_real_elt(prevloc, 0, NA_REAL);
    set_real_elt(prevloc, 1, NA_REAL);
    set_vector_elt(state, GSS_PREVLOC as R_xlen_t, prevloc);

    // GSS_DLON: is the display list on?
    set_vector_elt(state, GSS_DLON as R_xlen_t, scalar_logical(1));

    // GSS_ENGINEDLON: are we using the engine's display list?
    set_vector_elt(state, GSS_ENGINEDLON as R_xlen_t, scalar_logical(1));

    // GSS_CURRGROB: current grob being drawn
    set_vector_elt(state, GSS_CURRGROB as R_xlen_t, R_NilValue());

    // GSS_ENGINERECORDING: are we inside a .Call.graphics call?
    set_vector_elt(state, GSS_ENGINERECORDING as R_xlen_t, scalar_logical(0));

    // GSS_GPAR: initialize graphics parameters
    initGPar(dd);

    // GSS_GPSAVED: previous gpar settings
    set_vector_elt(state, GSS_GPSAVED as R_xlen_t, R_NilValue());

    // GSS_GLOBALINDEX: index in global state list
    set_vector_elt(state, GSS_GLOBALINDEX as R_xlen_t, R_NilValue());

    // GSS_GRIDDEVICE: does this device contain grid output?
    set_vector_elt(state, GSS_GRIDDEVICE as R_xlen_t, scalar_logical(0));

    // GSS_SCALE: zoom factor
    set_vector_elt(state, GSS_SCALE as R_xlen_t, scalar_real(1.0));

    // GSS_RESOLVINGPATH: are we resolving a clipping path?
    set_vector_elt(state, GSS_RESOLVINGPATH as R_xlen_t, scalar_logical(0));

    // GSS_GROUPS: group name to device reference mapping
    set_vector_elt(state, GSS_GROUPS as R_xlen_t, R_NilValue());

    set_current_grid_state(state);
}

/// Get a grid state element by index.
pub fn gridStateElement(dd: pGEDevDesc, elementIndex: c_int) -> SEXP {
    let _ = dd;
    let state = current_grid_state();
    if state.is_null() || state == R_NilValue() {
        return R_NilValue();
    }
    vector_elt(state, elementIndex as R_xlen_t)
}

/// Set a grid state element by index.
pub fn setGridStateElement(dd: pGEDevDesc, elementIndex: c_int, value: SEXP) {
    let _ = dd;
    let state = current_grid_state();
    if state.is_null() || state == R_NilValue() {
        return;
    }
    set_vector_elt(state, elementIndex as R_xlen_t, value);
}

/// Callable from R code: set a grid state element.
pub fn L_setGridState(elementIndex: SEXP, value: SEXP) -> SEXP {
    let dd = getDevice();
    setGridStateElement(dd, integer_elt(elementIndex, 0), value);
    R_NilValue()
}

/* ==================== State slot management ==================== */

/// Remove state from global variable (for GC).
fn deglobaliseState(state: SEXP) {
    let index = integer_elt(vector_elt(state, GSS_GLOBALINDEX as R_xlen_t), 0);
    let sym = install(".GRID.STATE");
    let globalstate = findVar(sym, grid_eval_env());
    set_vector_elt(globalstate, index as R_xlen_t, R_NilValue());
}

/// Find an empty slot in the global state list.
fn findStateSlot() -> c_int {
    let sym = install(".GRID.STATE");
    let globalstate = findVar(sym, grid_eval_env());
    let len = LENGTH(globalstate) as i32;
    for i in 0..len {
        if isNull(vector_elt(globalstate, i as R_xlen_t)) {
            return i;
        }
    }
    error("unable to store 'grid' state.  Too many devices open?");
    -1
}

/// Store state in a global variable (to prevent GC).
fn globaliseState(state: SEXP) {
    let index = findStateSlot();
    let sym = install(".GRID.STATE");
    let globalstate = findVar(sym, grid_eval_env());
    let _globalstate_guard = protect(globalstate);
    let indexsxp = alloc_vector(SEXPTYPE::INTSXP, 1);
    let _index_guard = protect(indexsxp);
    set_integer_elt(indexsxp, 0, index);
    set_vector_elt(state, GSS_GLOBALINDEX as R_xlen_t, indexsxp);
    set_vector_elt(globalstate, index as R_xlen_t, state);
}

/* ==================== GE event callback ==================== */

/// Grid callback for graphics engine events.
pub fn gridCallback(task: GEevent, dd: pGEDevDesc, data: SEXP) -> SEXP {
    let mut result: SEXP = R_NilValue();

    match task {
        GE_InitState => {
            // Create the initial grid state for a device
            let gridState = createGridSystemState();
            let _grid_state_guard = protect(gridState);
            // Store that state with the device for easy retrieval
            // (stub: cannot access dd->gesd)
            // Initialize the grid state for a device
            fillGridSystemState(gridState, dd);
            // Store the state beneath a top-level variable so it does not get GC'd
            globaliseState(gridState);
            // Indicate success
            result = R_BlankString();
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
            let valid = alloc_vector(SEXPTYPE::LGLSXP, 1);
            set_logical_elt(valid, 0, 1);
            result = valid;
        }
        GE_SaveSnapshotState => {
            result = alloc_vector(SEXPTYPE::VECSXP, 2);
            let _result_guard = protect(result);
            set_vector_elt(result, 0, R_NilValue());
            set_vector_elt(result, 1, R_NilValue());
            let pkgName = mk_string("grid");
            let _pkg_name_guard = protect(pkgName);

            attrib_core::setAttrib(result, install("pkgName"), pkgName);
        }
        GE_RestoreSnapshotState => {
            // Stub: complex snapshot restoration logic deferred until GE is ported.
            result = R_NilValue();
        }
        GE_ScalePS => {
            // data is a numeric scale factor
            let scale = alloc_vector(SEXPTYPE::REALSXP, 1);
            set_real_elt(scale, 0, 1.0); // stub
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
