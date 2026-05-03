//! Port of R's src/library/grid/src/state.c -- grid system state management.
//!
//! Manages per-device grid state including display lists, viewports,
//! graphics parameters, and engine callbacks.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

use crate::attrib_core;
use crate::library::grid::gpar::initGPar;
use crate::library::grid::grid::getDevice;
use crate::sexp::accessors::{
    CADR, CAR, INTEGER, LENGTH, LOGICAL, REAL, SET_VECTOR_ELT, STRING_ELT, TYPEOF, VECTOR_ELT,
};
use crate::sexp::constructors::{Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector, Rf_mkString};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::types::*;

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
unsafe fn error(msg: &str) {
    unsafe {
        std::panic::panic_any(crate::sexp::context::RError {
            message: msg.to_string(),
        });
    }
}

/* ==================== Helper: R_BlankString ==================== */

/// Get R_BlankString.
#[inline]
unsafe fn R_BlankString() -> SEXP {
    unsafe { crate::sexp::constructors::Rf_mkChar(b"\0".as_ptr() as *const c_char) }
}

/* ==================== Helper: isNull ==================== */

#[inline]
unsafe fn isNull(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() }
}

/* ==================== Helper: isVector ==================== */

#[inline]
unsafe fn isVector(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::VECSXP
            || t == SEXPTYPE::EXPRSXP
    }
}

/* ==================== Helper: isString ==================== */

#[inline]
unsafe fn isString(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::STRSXP }
}

/* ==================== Helper: findVar ==================== */

#[inline]
unsafe fn findVar(sym: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::sexp::envir::R_findVarInFrame(rho, sym) }
}

/* ==================== Helper: lang1 ==================== */

#[inline]
unsafe fn lang1(_sym: SEXP) -> SEXP {
    unsafe {
        // Stub: create a one-element call
        crate::sexp::memory_ext::allocLang(1)
    }
}

/* ==================== Core state functions ==================== */

/// Create the grid system state (VECSXP of length 18).
/// One element per GSS_* constant.
pub unsafe fn createGridSystemState() -> SEXP {
    unsafe { Rf_allocVector(SEXPTYPE::VECSXP, 18) }
}

/// Initialize the display list for a device.
///
/// Creates an empty list to hold grob references, sets the display list
/// index to 0, and marks the display list as active.
pub unsafe fn initDL(dd: pGEDevDesc) {
    unsafe {
        let dl = Rf_allocVector(SEXPTYPE::VECSXP, 100);
        let _dl_guard = protect(dl);
        setGridStateElement(dd, GSS_DL, dl);
        setGridStateElement(dd, GSS_DLINDEX, Rf_allocVector(SEXPTYPE::INTSXP, 1));
        setGridStateElement(dd, GSS_DLON, crate::sexp::constructors::Rf_ScalarLogical(1));
    }
}

/// Initialize some bits of the system state (called before engine redraw).
/// Does NOT init all state; display list, root viewport, and current gpar
/// are initialized separately (see initDL, initVP, initGPar).
pub unsafe fn initOtherState(dd: pGEDevDesc) {
    unsafe {
        // Stub: since pGEDevDesc is void*, we cannot access dd->gesd.
        let _ = dd;
    }
}

/// Fill the grid system state with initial values.
pub unsafe fn fillGridSystemState(state: SEXP, dd: pGEDevDesc) {
    unsafe {
        use crate::sexp::ffi::NA_REAL;

        let _state_guard = protect(state);

        // GSS_DEVSIZE: current size of device
        let devsize = Rf_allocVector(SEXPTYPE::REALSXP, 2);
        *REAL(devsize).add(0) = 0.0;
        *REAL(devsize).add(1) = 0.0;
        SET_VECTOR_ELT(state, GSS_DEVSIZE as R_xlen_t, devsize);

        // GSS_CURRLOC: current location of grid "pen"
        let currloc = Rf_allocVector(SEXPTYPE::REALSXP, 2);
        *REAL(currloc).add(0) = NA_REAL;
        *REAL(currloc).add(1) = NA_REAL;
        SET_VECTOR_ELT(state, GSS_CURRLOC as R_xlen_t, currloc);

        // GSS_PREVLOC: previous location of grid "pen"
        let prevloc = Rf_allocVector(SEXPTYPE::REALSXP, 2);
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

        set_current_grid_state(state);
    }
}

/// Get a grid state element by index.
pub unsafe fn gridStateElement(dd: pGEDevDesc, elementIndex: c_int) -> SEXP {
    unsafe {
        let _ = dd;
        let state = current_grid_state();
        if state.is_null() || state == R_NilValue() {
            return R_NilValue();
        }
        VECTOR_ELT(state, elementIndex as R_xlen_t)
    }
}

/// Set a grid state element by index.
pub unsafe fn setGridStateElement(dd: pGEDevDesc, elementIndex: c_int, value: SEXP) {
    unsafe {
        let _ = dd;
        let state = current_grid_state();
        if state.is_null() || state == R_NilValue() {
            return;
        }
        SET_VECTOR_ELT(state, elementIndex as R_xlen_t, value);
    }
}

/// Callable from R code: set a grid state element.
pub unsafe fn L_setGridState(elementIndex: SEXP, value: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        setGridStateElement(dd, *INTEGER(elementIndex).add(0), value);
        R_NilValue()
    }
}

/* ==================== State slot management ==================== */

/// Remove state from global variable (for GC).
unsafe fn deglobaliseState(state: SEXP) {
    unsafe {
        let index = *INTEGER(VECTOR_ELT(state, GSS_GLOBALINDEX as R_xlen_t)).add(0);
        let sym = Rf_install(b".GRID.STATE\0".as_ptr() as *const c_char);
        let globalstate = findVar(sym, grid_eval_env());
        SET_VECTOR_ELT(globalstate, index as R_xlen_t, R_NilValue());
    }
}

/// Find an empty slot in the global state list.
unsafe fn findStateSlot() -> c_int {
    unsafe {
        let sym = Rf_install(b".GRID.STATE\0".as_ptr() as *const c_char);
        let globalstate = findVar(sym, grid_eval_env());
        let len = LENGTH(globalstate) as i32;
        for i in 0..len {
            if isNull(VECTOR_ELT(globalstate, i as R_xlen_t)) {
                return i;
            }
        }
        error("unable to store 'grid' state.  Too many devices open?");
        -1
    }
}

/// Store state in a global variable (to prevent GC).
unsafe fn globaliseState(state: SEXP) {
    unsafe {
        let index = findStateSlot();
        let sym = Rf_install(b".GRID.STATE\0".as_ptr() as *const c_char);
        let globalstate = findVar(sym, grid_eval_env());
        let _globalstate_guard = protect(globalstate);
        let indexsxp = Rf_allocVector(SEXPTYPE::INTSXP, 1);
        let _index_guard = protect(indexsxp);
        *INTEGER(indexsxp).add(0) = index;
        SET_VECTOR_ELT(state, GSS_GLOBALINDEX as R_xlen_t, indexsxp);
        SET_VECTOR_ELT(globalstate, index as R_xlen_t, state);
    }
}

/* ==================== GE event callback ==================== */

/// Grid callback for graphics engine events.
pub unsafe fn gridCallback(task: GEevent, dd: pGEDevDesc, data: SEXP) -> SEXP {
    unsafe {
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
                let valid = Rf_allocVector(SEXPTYPE::LGLSXP, 1);
                *LOGICAL(valid).add(0) = 1;
                result = valid;
            }
            GE_SaveSnapshotState => {
                result = Rf_allocVector(SEXPTYPE::VECSXP, 2);
                let _result_guard = protect(result);
                SET_VECTOR_ELT(result, 0, R_NilValue());
                SET_VECTOR_ELT(result, 1, R_NilValue());
                let pkgName = Rf_mkString(b"grid\0".as_ptr() as *const c_char);
                let _pkg_name_guard = protect(pkgName);
                attrib_core::setAttrib(
                    result,
                    Rf_install(b"pkgName\0".as_ptr() as *const c_char),
                    pkgName,
                );
            }
            GE_RestoreSnapshotState => {
                // Stub: complex snapshot restoration logic deferred until GE is ported.
                result = R_NilValue();
            }
            GE_ScalePS => {
                // data is a numeric scale factor
                let scale = Rf_allocVector(SEXPTYPE::REALSXP, 1);
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
}
