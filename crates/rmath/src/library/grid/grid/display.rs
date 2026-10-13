use super::helpers::initVP;
use super::*;

pub unsafe fn L_getDisplayList() -> SEXP {
    unsafe {
        let dd = getDevice();
        gridStateElement(dd, GSS_DL)
    }
}

pub unsafe fn L_setDisplayList(dl: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        setGridStateElement(dd, GSS_DL, dl);
        R_NilValue()
    }
}

pub unsafe fn L_getDLelt(index: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let dl = gridStateElement(dd, GSS_DL);
        let _dl_guard = protect(dl);
        let result = VECTOR_ELT(dl, *INTEGER(index) as R_xlen_t);
        result
    }
}

pub unsafe fn L_setDLelt(value: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let dl = gridStateElement(dd, GSS_DL);
        let _dl_guard = protect(dl);
        let dlindex = gridStateElement(dd, GSS_DLINDEX);
        SET_VECTOR_ELT(dl, *INTEGER(dlindex) as R_xlen_t, value);
        R_NilValue()
    }
}

pub unsafe fn L_getDLindex() -> SEXP {
    unsafe {
        let dd = getDevice();
        gridStateElement(dd, GSS_DLINDEX)
    }
}

pub unsafe fn L_setDLindex(index: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        setGridStateElement(dd, GSS_DLINDEX, index);
        R_NilValue()
    }
}

pub unsafe fn L_getDLon() -> SEXP {
    unsafe {
        let dd = getDevice();
        gridStateElement(dd, GSS_DLON)
    }
}

pub unsafe fn L_setDLon(value: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let prev = gridStateElement(dd, GSS_DLON);
        setGridStateElement(dd, GSS_DLON, value);
        prev
    }
}

pub unsafe fn L_getEngineDLon() -> SEXP {
    unsafe {
        let dd = getDevice();
        gridStateElement(dd, GSS_ENGINEDLON)
    }
}

pub unsafe fn L_setEngineDLon(value: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        setGridStateElement(dd, GSS_ENGINEDLON, value);
        R_NilValue()
    }
}

/* ==============================
 * Grid state accessors
 * ============================== */

pub unsafe fn L_getCurrentGrob() -> SEXP {
    unsafe {
        let dd = getDevice();
        gridStateElement(dd, GSS_CURRGROB)
    }
}

pub unsafe fn L_setCurrentGrob(value: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        setGridStateElement(dd, GSS_CURRGROB, value);
        R_NilValue()
    }
}

pub unsafe fn L_getEngineRecording() -> SEXP {
    unsafe {
        let dd = getDevice();
        gridStateElement(dd, GSS_ENGINERECORDING)
    }
}

pub unsafe fn L_setEngineRecording(value: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        setGridStateElement(dd, GSS_ENGINERECORDING, value);
        R_NilValue()
    }
}

/* ==============================
 * GPar accessors
 * ============================== */

pub unsafe fn L_currentGPar() -> SEXP {
    unsafe {
        let dd = getDevice();
        gridStateElement(dd, GSS_GPAR)
    }
}

/* ==============================
 * Page and initialization
 * ============================== */

pub unsafe fn L_newpagerecording() -> SEXP {
    unsafe {
        let dd = getDevice();
        if !dd.is_null() {
            NewFrameConfirm(dd);
        }
        R_NilValue()
    }
}

pub unsafe fn L_newpage() -> SEXP {
    unsafe {
        let dd = getDevice();
        if !dd.is_null() {
            let currentgp = gridStateElement(dd, GSS_GPAR);
            let mut gc: [u8; 256] = [0; 256];
            gcontextFromgpar(currentgp, 0, gc.as_mut_ptr() as pGEcontext, dd);
            GENewPage(gc.as_ptr() as pGEcontext, dd);
        }
        R_NilValue()
    }
}

pub unsafe fn L_clearDefinitions(clearGroups: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        if !dd.is_null() {
            setGridStateElement(dd, GSS_RESOLVINGPATH, Rf_ScalarLogical(0));
            let clear_groups = if TYPEOF(clearGroups) == SEXPTYPE::LGLSXP && LENGTH(clearGroups) > 0
            {
                if *LOGICAL(clearGroups) != 0 { 1 } else { 0 }
            } else {
                0
            };
            rmath_grid_release_definitions(dd, clear_groups);
        }
        R_NilValue()
    }
}

pub unsafe fn L_initGPar() -> SEXP {
    unsafe {
        let dd = getDevice();
        initGPar(dd);
        R_NilValue()
    }
}

pub unsafe fn L_initViewportStack() -> SEXP {
    unsafe {
        let dd = getDevice();
        initVP(dd);
        R_NilValue()
    }
}

pub unsafe fn L_initDisplayList() -> SEXP {
    unsafe {
        let dd = getDevice();
        initDL(dd);
        R_NilValue()
    }
}

/* ==============================
 * getViewportTransform
 * ============================== */
