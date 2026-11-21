use super::helpers::initVP;
use super::*;
use crate::library::grid::{
    integer_elt, logical_elt, protect, scalar_logical, set_vector_elt, vector_elt,
};

pub fn L_getDisplayList() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_DL)
}

pub fn L_setDisplayList(dl: SEXP) -> SEXP {
    let dd = getDevice();
    setGridStateElement(dd, GSS_DL, dl);
    R_NilValue()
}

pub fn L_getDLelt(index: SEXP) -> SEXP {
    let dd = getDevice();
    let dl = gridStateElement(dd, GSS_DL);
    let _dl_guard = protect(dl);
    vector_elt(dl, integer_elt(index, 0) as R_xlen_t)
}

pub fn L_setDLelt(value: SEXP) -> SEXP {
    let dd = getDevice();
    let dl = gridStateElement(dd, GSS_DL);
    let _dl_guard = protect(dl);
    let dlindex = gridStateElement(dd, GSS_DLINDEX);
    set_vector_elt(dl, integer_elt(dlindex, 0) as R_xlen_t, value);
    R_NilValue()
}

pub fn L_getDLindex() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_DLINDEX)
}

pub fn L_setDLindex(index: SEXP) -> SEXP {
    let dd = getDevice();
    setGridStateElement(dd, GSS_DLINDEX, index);
    R_NilValue()
}

pub fn L_getDLon() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_DLON)
}

pub fn L_setDLon(value: SEXP) -> SEXP {
    let dd = getDevice();
    let prev = gridStateElement(dd, GSS_DLON);
    setGridStateElement(dd, GSS_DLON, value);
    prev
}

pub fn L_getEngineDLon() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_ENGINEDLON)
}

pub fn L_setEngineDLon(value: SEXP) -> SEXP {
    let dd = getDevice();
    setGridStateElement(dd, GSS_ENGINEDLON, value);
    R_NilValue()
}

/* ==============================
 * Grid state accessors
 * ============================== */

pub fn L_getCurrentGrob() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_CURRGROB)
}

pub fn L_setCurrentGrob(value: SEXP) -> SEXP {
    let dd = getDevice();
    setGridStateElement(dd, GSS_CURRGROB, value);
    R_NilValue()
}

pub fn L_getEngineRecording() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_ENGINERECORDING)
}

pub fn L_setEngineRecording(value: SEXP) -> SEXP {
    let dd = getDevice();
    setGridStateElement(dd, GSS_ENGINERECORDING, value);
    R_NilValue()
}

/* ==============================
 * GPar accessors
 * ============================== */

pub fn L_currentGPar() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_GPAR)
}

/* ==============================
 * Page and initialization
 * ============================== */

pub fn L_newpagerecording() -> SEXP {
    let dd = getDevice();
    if !dd.is_null() {
        NewFrameConfirm(dd);
    }
    R_NilValue()
}

pub fn L_newpage() -> SEXP {
    let dd = getDevice();
    if !dd.is_null() {
        let currentgp = gridStateElement(dd, GSS_GPAR);
        let gc = R_GE_gcontext::zeroed();
        gcontextFromgpar(currentgp, 0, Some(&gc), dd);
        GENewPage(Some(&gc), dd);
    }
    R_NilValue()
}

pub fn L_clearDefinitions(clearGroups: SEXP) -> SEXP {
    let dd = getDevice();
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
        rmath_grid_release_definitions(dd, clear_groups);
    }
    R_NilValue()
}

pub fn L_initGPar() -> SEXP {
    let dd = getDevice();
    initGPar(dd);
    R_NilValue()
}

pub fn L_initViewportStack() -> SEXP {
    let dd = getDevice();
    initVP(dd);
    R_NilValue()
}

pub fn L_initDisplayList() -> SEXP {
    let dd = getDevice();
    initDL(dd);
    R_NilValue()
}

/* ==============================
 * getViewportTransform
 * ============================== */
