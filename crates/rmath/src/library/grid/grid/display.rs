use super::helpers::initVP;
use super::*;
use crate::library::grid::{
    integer_elt, logical_elt, protect, scalar_logical, set_vector_elt, vector_elt,
};

pub fn L_getDisplayList() -> SEXP {
    // SAFETY: device registry bridge; the display list slot is device state.
    let dd = unsafe { getDevice() };
    gridStateElement(dd, GSS_DL)
}

pub fn L_setDisplayList(dl: SEXP) -> SEXP {
    // SAFETY: device registry bridge; the display list slot is device state.
    let dd = unsafe { getDevice() };
    setGridStateElement(dd, GSS_DL, dl);
    R_NilValue()
}

pub fn L_getDLelt(index: SEXP) -> SEXP {
    // SAFETY: device registry bridge; the display list slot is device state.
    let dd = unsafe { getDevice() };
    let dl = gridStateElement(dd, GSS_DL);
    let _dl_guard = protect(dl);
    vector_elt(dl, integer_elt(index, 0) as R_xlen_t)
}

pub fn L_setDLelt(value: SEXP) -> SEXP {
    // SAFETY: device registry bridge; the display list slot is device state.
    let dd = unsafe { getDevice() };
    let dl = gridStateElement(dd, GSS_DL);
    let _dl_guard = protect(dl);
    let dlindex = gridStateElement(dd, GSS_DLINDEX);
    set_vector_elt(dl, integer_elt(dlindex, 0) as R_xlen_t, value);
    R_NilValue()
}

pub fn L_getDLindex() -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    gridStateElement(dd, GSS_DLINDEX)
}

pub fn L_setDLindex(index: SEXP) -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    setGridStateElement(dd, GSS_DLINDEX, index);
    R_NilValue()
}

pub fn L_getDLon() -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    gridStateElement(dd, GSS_DLON)
}

pub fn L_setDLon(value: SEXP) -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    let prev = gridStateElement(dd, GSS_DLON);
    setGridStateElement(dd, GSS_DLON, value);
    prev
}

pub fn L_getEngineDLon() -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    gridStateElement(dd, GSS_ENGINEDLON)
}

pub fn L_setEngineDLon(value: SEXP) -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    setGridStateElement(dd, GSS_ENGINEDLON, value);
    R_NilValue()
}

/* ==============================
 * Grid state accessors
 * ============================== */

pub fn L_getCurrentGrob() -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    gridStateElement(dd, GSS_CURRGROB)
}

pub fn L_setCurrentGrob(value: SEXP) -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    setGridStateElement(dd, GSS_CURRGROB, value);
    R_NilValue()
}

pub fn L_getEngineRecording() -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    gridStateElement(dd, GSS_ENGINERECORDING)
}

pub fn L_setEngineRecording(value: SEXP) -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    setGridStateElement(dd, GSS_ENGINERECORDING, value);
    R_NilValue()
}

/* ==============================
 * GPar accessors
 * ============================== */

pub fn L_currentGPar() -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    gridStateElement(dd, GSS_GPAR)
}

/* ==============================
 * Page and initialization
 * ============================== */

pub fn L_newpagerecording() -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    if !dd.is_null() {
        NewFrameConfirm(dd);
    }
    R_NilValue()
}

pub fn L_newpage() -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    if !dd.is_null() {
        let currentgp = gridStateElement(dd, GSS_GPAR);
        let mut gc: [u8; 256] = [0; 256];
        gcontextFromgpar(currentgp, 0, gc.as_mut_ptr() as pGEcontext, dd);
        // SAFETY: `gc` is the opaque engine context buffer just filled by
        // `gcontextFromgpar`; `dd` is the live device returned above.
        GENewPage(gc.as_ptr() as pGEcontext, dd);
    }
    R_NilValue()
}

pub fn L_clearDefinitions(clearGroups: SEXP) -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    if !dd.is_null() {
        setGridStateElement(dd, GSS_RESOLVINGPATH, scalar_logical(0));
        let clear_groups = if TYPEOF(clearGroups) == SEXPTYPE::LGLSXP && LENGTH(clearGroups) > 0 {
            if logical_elt(clearGroups, 0) != 0 {
                1
            } else {
                0
            }
        } else {
            0
        };
        // SAFETY: `dd` is the live device returned by `getDevice` above.
        unsafe { rmath_grid_release_definitions(dd, clear_groups) };
    }
    R_NilValue()
}

pub fn L_initGPar() -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    initGPar(dd);
    R_NilValue()
}

pub fn L_initViewportStack() -> SEXP {
    // SAFETY: device registry bridge; `initVP` is an unsafe viewport/device
    // bridge, so the call stays wrapped.
    let dd = unsafe { getDevice() };
    unsafe { initVP(dd) };
    R_NilValue()
}

pub fn L_initDisplayList() -> SEXP {
    // SAFETY: device registry bridge.
    let dd = unsafe { getDevice() };
    initDL(dd);
    R_NilValue()
}

/* ==============================
 * getViewportTransform
 * ============================== */
