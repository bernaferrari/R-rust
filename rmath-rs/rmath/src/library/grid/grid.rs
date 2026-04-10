/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Port of R's src/library/grid/src/grid.c (5470 lines)
 *
 *  grid -- main grid drawing primitives and state management.
 */

use std::cell::Cell;
use std::ffi::c_void;
use std::os::raw::{c_char, c_double, c_int, c_uint};
use std::ptr;

use crate::attrib_core::{R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

use super::clippath::{isClipPath, resolveClipPath};
use super::gpar::{
    GP_ALPHA, GP_CEX, GP_COL, GP_FILL, GP_FONT, GP_FONTFAMILY, GP_FONTSIZE, GP_GAMMA, GP_LEX,
    GP_LINEEND, GP_LINEHEIGHT, GP_LINEJOIN, GP_LINEMITRE, GP_LTY, GP_LWD, pGEDevDesc, pGEcontext,
};
use super::just::{justification, justifyX, justifyY};
use super::layout::calcViewportLocationFromLayout;
use super::mask::{isMask, resolveMask};
use super::types::*;
use super::unit::{
    L_INCHES, L_NATIVE, L_NPC, transformDimn, transformHeighttoINCHES, transformLocn,
    transformWHfromNPC, transformWHtoNPC, transformWidthHeightFromINCHES, transformWidthtoINCHES,
    transformXYFromINCHES, transformXYfromNPC, transformXYtoNPC, transformXtoINCHES,
    transformYtoINCHES, unit, unitLength, unitUnit, unitValue,
};
use super::util::{copyRect, getListElement, intersect, rect, setListElement, textRect};
use super::viewport::*;

unsafe extern "C" {
    fn Rf_duplicate(x: SEXP) -> SEXP;
    fn Rf_inherits(x: SEXP, klass: *const c_char) -> c_int;
    fn Rf_eval_with_gd(call: SEXP, env: SEXP, dd: pGEDevDesc) -> SEXP;
    fn lang2(symbol: SEXP, arg: SEXP) -> SEXP;
    fn lang3(symbol: SEXP, arg1: SEXP, arg2: SEXP) -> SEXP;
    fn lang4(symbol: SEXP, arg1: SEXP, arg2: SEXP, arg3: SEXP) -> SEXP;
    fn findVar(symbol: SEXP, env: SEXP) -> SEXP;
    fn findFun(symbol: SEXP, env: SEXP) -> SEXP;
    fn defineVar(symbol: SEXP, value: SEXP, env: SEXP);
    fn NewFrameConfirm(dev: *const c_void);
    fn NoDevices() -> c_int;
    fn asBool(x: SEXP) -> c_int;
    fn installTrChar(name: SEXP) -> SEXP;
    fn Rf_CreateAtVector(axp: *mut c_double, usr: *mut c_double, n: c_int, log: c_int) -> SEXP;
    fn col2name(color: c_int) -> *const c_char;
    fn RGBpar3(col: *mut c_void, i: c_int, bg: c_uint) -> c_uint;
    fn GESymbol(x: f64, y: f64, pch: c_int, size: f64, gc: pGEcontext, dd: pGEDevDesc);
    fn GEstring_to_pch(s: SEXP) -> c_int;
    fn GEExpressionMetric(
        x: SEXP,
        gc: pGEcontext,
        ascent: *mut f64,
        descent: *mut f64,
        width: *mut f64,
        dd: pGEDevDesc,
    );
    fn GEStrMetric(
        str: *const c_char,
        ce: c_int,
        gc: pGEcontext,
        ascent: *mut f64,
        descent: *mut f64,
        width: *mut f64,
        dd: pGEDevDesc,
    );
    fn getCharCE(s: SEXP) -> c_int;
    fn mbcslocale() -> c_int;
    fn Rf_ucstoutf8(buf: *mut c_char, c: c_int) -> usize;
    fn GEinitDisplayList(dd: pGEDevDesc);
    fn GEdeviceDirty(dd: pGEDevDesc) -> c_int;
    fn GEregisterSystem(callback: *const c_void, index: *mut c_int);
    fn GEunregisterSystem(index: c_int);
    fn vmaxget() -> *mut c_void;
    fn vmaxset(vmax: *mut c_void);
    fn R_alloc(n: usize, size: usize) -> *mut c_void;
}

unsafe extern "C" {
    fn gridStateElement(dd: pGEDevDesc, elementIndex: c_int) -> SEXP;
    fn setGridStateElement(dd: pGEDevDesc, elementIndex: c_int, value: SEXP);
    fn initVP(dd: pGEDevDesc);
    fn initDL(dd: pGEDevDesc);
    fn initGPar(dd: pGEDevDesc);
    fn resolveGPar(gp: SEXP, by_name: c_int) -> SEXP;
    fn gcontextFromgpar(gp: SEXP, i: c_int, gc: pGEcontext, dd: pGEDevDesc);
    fn initGContext(
        gp: SEXP,
        gc: pGEcontext,
        dd: pGEDevDesc,
        gpIsScalar: *mut c_int,
        gcCache: pGEcontext,
    );
    fn updateGContext(
        gp: SEXP,
        i: c_int,
        gc: pGEcontext,
        dd: pGEDevDesc,
        gpIsScalar: *mut c_int,
        gcCache: pGEcontext,
    );
    fn gridCallback(dd: pGEDevDesc, code: c_int, data: *mut c_void);
}

// GE drawing functions
unsafe extern "C" {
    fn GEcurrentDevice() -> pGEDevDesc;
    fn GEMode(mode: c_int, dd: pGEDevDesc);
    fn GELine(x1: f64, y1: f64, x2: f64, y2: f64, gc: pGEcontext, dd: pGEDevDesc);
    fn GEPolyline(n: c_int, x: *const f64, y: *const f64, gc: pGEcontext, dd: pGEDevDesc);
    fn GEPolygon(n: c_int, x: *const f64, y: *const f64, gc: pGEcontext, dd: pGEDevDesc);
    fn GECircle(x: f64, y: f64, r: f64, gc: pGEcontext, dd: pGEDevDesc);
    fn GERect(x0: f64, y0: f64, x1: f64, y1: f64, gc: pGEcontext, dd: pGEDevDesc);
    fn GESetClip(x0: f64, y0: f64, x1: f64, y1: f64, dd: pGEDevDesc);
    fn GENewPage(gc: pGEcontext, dd: pGEDevDesc);
    fn GEText(
        x: f64,
        y: f64,
        str: *const c_char,
        ce: c_int,
        hjust: f64,
        vjust: f64,
        rot: f64,
        gc: pGEcontext,
        dd: pGEDevDesc,
    );
    fn GEMathText(
        x: f64,
        y: f64,
        expr: SEXP,
        hjust: f64,
        vjust: f64,
        rot: f64,
        gc: pGEcontext,
        dd: pGEDevDesc,
    );
    fn GEPretty(min: *mut f64, max: *mut f64, n: *mut c_int);
    fn GEXspline(
        n: c_int,
        x: *mut f64,
        y: *mut f64,
        s: *mut f64,
        open: c_int,
        rep: c_int,
        draw: c_int,
        gc: pGEcontext,
        dd: pGEDevDesc,
    ) -> SEXP;
    fn GEPath(
        x: *mut f64,
        y: *mut f64,
        npoly: c_int,
        nper: *mut c_int,
        winding: c_int,
        gc: pGEcontext,
        dd: pGEDevDesc,
    );
    fn GERaster(
        image: *mut c_uint,
        w: c_int,
        h: c_int,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        rot: f64,
        interpolate: c_int,
        gc: pGEcontext,
        dd: pGEDevDesc,
    );
    fn GECap(dd: pGEDevDesc) -> SEXP;
    fn toDeviceX(x: f64, from: c_int, dd: pGEDevDesc) -> f64;
    fn toDeviceY(y: f64, from: c_int, dd: pGEDevDesc) -> f64;
    fn toDeviceWidth(x: f64, from: c_int, dd: pGEDevDesc) -> f64;
    fn toDeviceHeight(y: f64, from: c_int, dd: pGEDevDesc) -> f64;
    fn fromDeviceX(x: f64, from: c_int, dd: pGEDevDesc) -> f64;
    fn fromDeviceY(y: f64, from: c_int, dd: pGEDevDesc) -> f64;
    fn fromDeviceWidth(x: f64, from: c_int, dd: pGEDevDesc) -> f64;
    fn fromDeviceHeight(y: f64, from: c_int, dd: pGEDevDesc) -> f64;
}

/* ==============================
 * Constants
 * ============================== */

const GE_INCHES: c_int = 1;
const R_TRANWHITE: c_int = 0x7FFFFFFF;
const NA_LOGICAL: c_int = -1;
const DEG2RAD: f64 = std::f64::consts::PI / 180.0;
const NA_REAL: f64 = crate::sexp::ffi::NA_REAL;
const CE_UTF8: c_int = 1;
const CE_SYMBOL: c_int = 2;
const CE_NATIVE: c_int = 0;

// Symbol drawing constants
const SMALL: f64 = 0.25;
const RADIUS: f64 = 0.375;
const SQRC: f64 = 0.88622692545275801364;
const DMDC: f64 = 1.25331413731550025119;
const TRC0: f64 = 1.55512030155621416073;
const TRC1: f64 = 1.34677368708859836060;
const TRC2: f64 = 0.77756015077810708036;

// R_GE_group version constant
const R_GE_group: c_int = 5;

/* ==============================
 * Local helper: R_gridEvalEnv.with(|v| v.get())
 * ============================== */

thread_local! { static R_gridEvalEnv.with(|v| v.get()): Cell<SEXP> = Cell::new(ptr::null_mut()); }

/* ==============================
 * Local helper: numeric(x, index)
 * ============================== */

#[inline]
unsafe fn numeric(x: SEXP, index: c_int) -> f64 {
    *REAL(x).add(index as usize)
}

/* ==============================
 * Local helper: isNull
 * ============================== */

#[inline]
unsafe fn isNull(x: SEXP) -> bool {
    x.is_null() || x == R_NilValue()
}

/* ==============================
 * Local helper: fmin2, fmax2
 * ============================== */

#[inline]
fn fmin2(a: f64, b: f64) -> f64 {
    a.min(b)
}

#[inline]
fn fmax2(a: f64, b: f64) -> f64 {
    a.max(b)
}

/* ==============================
 * getDevice
 * ============================== */

#[unsafe(no_mangle)]
pub unsafe fn getDevice() -> pGEDevDesc {
    GEcurrentDevice()
}

/* ==============================
 * getDeviceSize
 * ============================== */

#[unsafe(no_mangle)]
pub unsafe fn getDeviceSize(dd: pGEDevDesc, devWidthCM: *mut c_double, devHeightCM: *mut c_double) {
    // STUB: requires device access
    // dd->dev->size(&left, &right, &bottom, &top, dd->dev);
    // *devWidthCM = fabs(right - left) * dd->dev->ipr[0] * 2.54;
    // *devHeightCM = fabs(top - bottom) * dd->dev->ipr[1] * 2.54;
    *devWidthCM = 0.0;
    *devHeightCM = 0.0;
}

/* ==============================
 * deviceChanged
 * ============================== */

unsafe fn deviceChanged(devWidthCM: c_double, devHeightCM: c_double, currentvp: SEXP) -> bool {
    let mut result = false;
    let pvpDevWidthCM = Rf_protect(VECTOR_ELT(currentvp, PVP_DEVWIDTHCM as R_xlen_t));
    let pvpDevHeightCM = Rf_protect(VECTOR_ELT(currentvp, PVP_DEVHEIGHTCM as R_xlen_t));
    if (*REAL(pvpDevWidthCM) - devWidthCM).abs() > 1e-6 {
        result = true;
        *REAL(pvpDevWidthCM) = devWidthCM;
        SET_VECTOR_ELT(currentvp, PVP_DEVWIDTHCM as R_xlen_t, pvpDevWidthCM);
    }
    if (*REAL(pvpDevHeightCM) - devHeightCM).abs() > 1e-6 {
        result = true;
        *REAL(pvpDevHeightCM) = devHeightCM;
        SET_VECTOR_ELT(currentvp, PVP_DEVHEIGHTCM as R_xlen_t, pvpDevHeightCM);
    }
    Rf_unprotect(2);
    result
}

/* ==============================
 * L_initGrid
 * ============================== */

pub unsafe fn L_initGrid(GridEvalEnv: SEXP) -> SEXP {
    R_gridEvalEnv.with(|v| v.get()).with(|v| v.set(GridEvalEnv));
    // GEregisterSystem(gridCallback, &mut gridRegisterIndex);
    R_NilValue()
}

/* ==============================
 * L_killGrid
 * ============================== */

pub unsafe fn L_killGrid() -> SEXP {
    // GEunregisterSystem(gridRegisterIndex);
    R_NilValue()
}

/* ==============================
 * dirtyGridDevice
 * ============================== */

pub unsafe fn dirtyGridDevice(dd: pGEDevDesc) {
    // STUB: full implementation requires GE device state
}

/* ==============================
 * L_gridDirty
 * ============================== */

pub unsafe fn L_gridDirty() -> SEXP {
    let dd = getDevice();
    dirtyGridDevice(dd);
    R_NilValue()
}

/* ==============================
 * getViewportContext
 * ============================== */

unsafe fn getViewportContext(vp: SEXP, vpc: *mut LViewportContext) {
    fillViewportContextFromViewport(vp, vpc);
}

/* ==============================
 * L_currentViewport
 * ============================== */

pub unsafe fn L_currentViewport() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_VP)
}

/* ==============================
 * doSetViewport
 * ============================== */

pub unsafe fn doSetViewport(vp: SEXP, topLevelVP: c_int, pushing: c_int, dd: pGEDevDesc) -> SEXP {
    let mut devWidthCM: c_double = 0.0;
    let mut devHeightCM: c_double = 0.0;
    let mut xx1: c_double = 0.0;
    let mut yy1: c_double = 0.0;
    let mut xx2: c_double = 0.0;
    let mut yy2: c_double = 0.0;

    getDeviceSize(dd, &mut devWidthCM, &mut devHeightCM);

    if topLevelVP == 0 && pushing != 0 {
        let parent = gridStateElement(dd, GSS_VP);
        SET_VECTOR_ELT(vp, PVP_PARENT as R_xlen_t, parent);
        // defineVar(installTrChar(STRING_ELT(VECTOR_ELT(vp, VP_NAME), 0)),
        //           vp, VECTOR_ELT(parent, PVP_CHILDREN));
    }

    // calcViewportTransform(vp, viewportParent(vp),
    //                       topLevelVP == 0 && !deviceChanged(devWidthCM, devHeightCM, viewportParent(vp)), dd);

    // Clipping region establishment - STUB
    // (full implementation requires unit conversion and GE calls)

    // Save device size
    let widthCM = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, 1));
    *REAL(widthCM) = devWidthCM;
    SET_VECTOR_ELT(vp, PVP_DEVWIDTHCM as R_xlen_t, widthCM);

    let heightCM = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, 1));
    *REAL(heightCM) = devHeightCM;
    SET_VECTOR_ELT(vp, PVP_DEVHEIGHTCM as R_xlen_t, heightCM);

    Rf_unprotect(2);
    vp
}

/* ==============================
 * L_setviewport
 * ============================== */

pub unsafe fn L_setviewport(invp: SEXP, hasParent: SEXP) -> SEXP {
    let dd = getDevice();
    let vp = Rf_protect(Rf_duplicate(invp));

    // STUB: call R function pushedvp()
    // PROTECT(fcall = lang2(install("pushedvp"), vp));
    // PROTECT(pushedvp = Rf_eval_with_gd(fcall, R_gridEvalEnv.with(|v| v.get()), ptr::null_mut()));

    let pushedvp = doSetViewport(vp, if *LOGICAL(hasParent) != 0 { 0 } else { 1 }, 1, dd);

    setGridStateElement(dd, GSS_VP, pushedvp);

    Rf_unprotect(1);
    R_NilValue()
}

/* ==============================
 * Viewport search helpers
 * ============================== */

unsafe fn noChildren(children: SEXP) -> bool {
    let fcall = Rf_protect(lang2(
        Rf_install(b"no.children\0".as_ptr() as *const c_char),
        children,
    ));
    let result = Rf_protect(Rf_eval_with_gd(
        fcall,
        R_gridEvalEnv.with(|v| v.get()),
        ptr::null_mut(),
    ));
    let r = asBool(result) != 0;
    Rf_unprotect(2);
    r
}

unsafe fn childExists(name: SEXP, children: SEXP) -> bool {
    let fcall = Rf_protect(lang3(
        Rf_install(b"child.exists\0".as_ptr() as *const c_char),
        name,
        children,
    ));
    let result = Rf_protect(Rf_eval_with_gd(
        fcall,
        R_gridEvalEnv.with(|v| v.get()),
        ptr::null_mut(),
    ));
    let r = asBool(result) != 0;
    Rf_unprotect(2);
    r
}

unsafe fn childList(children: SEXP) -> SEXP {
    let fcall = Rf_protect(lang2(
        Rf_install(b"child.list\0".as_ptr() as *const c_char),
        children,
    ));
    let result = Rf_protect(Rf_eval_with_gd(
        fcall,
        R_gridEvalEnv.with(|v| v.get()),
        ptr::null_mut(),
    ));
    Rf_unprotect(2);
    result
}

unsafe fn findViewport(name: SEXP, strict: SEXP, vp: SEXP, depth: c_int) -> SEXP {
    let result = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, 2));
    let zeroDepth = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    *INTEGER(zeroDepth) = 0;
    let curDepth = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
    *INTEGER(curDepth) = depth;

    if noChildren(viewportChildren(vp)) {
        SET_VECTOR_ELT(result, 0 as R_xlen_t, zeroDepth);
        SET_VECTOR_ELT(result, 1 as R_xlen_t, R_NilValue());
    } else if childExists(name, viewportChildren(vp)) {
        SET_VECTOR_ELT(result, 0 as R_xlen_t, curDepth);
        // SET_VECTOR_ELT(result, 1, findVar(installTrChar(STRING_ELT(name, 0)), viewportChildren(vp)));
        SET_VECTOR_ELT(result, 1 as R_xlen_t, R_NilValue());
    } else {
        if *LOGICAL(strict) != 0 {
            SET_VECTOR_ELT(result, 0 as R_xlen_t, zeroDepth);
            SET_VECTOR_ELT(result, 1 as R_xlen_t, R_NilValue());
        } else {
            // STUB: findInChildren(name, strict, viewportChildren(vp), depth + 1);
            SET_VECTOR_ELT(result, 0 as R_xlen_t, zeroDepth);
            SET_VECTOR_ELT(result, 1 as R_xlen_t, R_NilValue());
        }
    }
    Rf_unprotect(3);
    result
}

unsafe fn findInChildren(name: SEXP, strict: SEXP, children: SEXP, depth: c_int) -> SEXP {
    let childnames = Rf_protect(childList(children));
    let n = LENGTH(childnames);
    let mut count: c_int = 0;
    let mut found = false;
    let mut result = R_NilValue();
    Rf_protect(result);
    while count < n && !found {
        // result = findViewport(name, strict, PROTECT(findVar(...)), depth);
        count += 1;
    }
    if !found {
        let temp = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, 2));
        let zeroDepth = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP.0, 1));
        *INTEGER(zeroDepth) = 0;
        SET_VECTOR_ELT(temp, 0 as R_xlen_t, zeroDepth);
        SET_VECTOR_ELT(temp, 1 as R_xlen_t, R_NilValue());
        Rf_unprotect(2);
        result = temp;
    }
    Rf_unprotect(2);
    result
}

/* ==============================
 * L_downviewport
 * ============================== */

pub unsafe fn L_downviewport(name: SEXP, strict: SEXP) -> SEXP {
    let dd = getDevice();
    let gvp = gridStateElement(dd, GSS_VP);
    let found = Rf_protect(findViewport(name, strict, gvp, 1));
    if *INTEGER(VECTOR_ELT(found, 0 as R_xlen_t)) > 0 {
        let vp = doSetViewport(VECTOR_ELT(found, 1 as R_xlen_t), 0, 0, dd);
        setGridStateElement(dd, GSS_VP, vp);
        Rf_unprotect(1);
        VECTOR_ELT(found, 0 as R_xlen_t)
    } else {
        Rf_unprotect(1);
        Rf_ScalarInteger(0)
    }
}

/* ==============================
 * L_downvppath
 * ============================== */

pub unsafe fn L_downvppath(path: SEXP, name: SEXP, strict: SEXP) -> SEXP {
    let dd = getDevice();
    let gvp = gridStateElement(dd, GSS_VP);
    let found = Rf_protect(findViewport(name, strict, gvp, 1));
    if *INTEGER(VECTOR_ELT(found, 0 as R_xlen_t)) > 0 {
        let vp = doSetViewport(VECTOR_ELT(found, 1 as R_xlen_t), 0, 0, dd);
        setGridStateElement(dd, GSS_VP, vp);
        Rf_unprotect(1);
        VECTOR_ELT(found, 0 as R_xlen_t)
    } else {
        Rf_unprotect(1);
        Rf_ScalarInteger(0)
    }
}

/* ==============================
 * L_unsetviewport
 * ============================== */

pub unsafe fn L_unsetviewport(n: SEXP) -> SEXP {
    let dd = getDevice();
    let gvp = Rf_protect(gridStateElement(dd, GSS_VP));
    let mut newvp = VECTOR_ELT(gvp, PVP_PARENT as R_xlen_t);
    if isNull(newvp) {
        Rf_unprotect(1);
        return R_NilValue();
    }
    for _i in 1..*INTEGER(n) {
        newvp = VECTOR_ELT(gvp, PVP_PARENT as R_xlen_t);
        if isNull(newvp) {
            break;
        }
    }
    // Full implementation would:
    // 1. Remove child from parent's children
    // 2. Check device size change
    // 3. Restore parent gpar
    // 4. Set clipping region
    // 5. Set mask
    // STUB: core actions
    setGridStateElement(dd, GSS_VP, newvp);
    Rf_unprotect(1);
    R_NilValue()
}

/* ==============================
 * L_upviewport
 * ============================== */

pub unsafe fn L_upviewport(n: SEXP) -> SEXP {
    let dd = getDevice();
    let gvp = Rf_protect(gridStateElement(dd, GSS_VP));
    let mut newvp = VECTOR_ELT(gvp, PVP_PARENT as R_xlen_t);
    if isNull(newvp) {
        Rf_unprotect(1);
        return R_NilValue();
    }
    for _i in 1..*INTEGER(n) {
        newvp = VECTOR_ELT(gvp, PVP_PARENT as R_xlen_t);
        if isNull(newvp) {
            break;
        }
    }
    // Full implementation similar to L_unsetviewport but without modifying parent-child
    setGridStateElement(dd, GSS_VP, newvp);
    Rf_unprotect(1);
    R_NilValue()
}

/* ==============================
 * Display list accessors
 * ============================== */

pub unsafe fn L_getDisplayList() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_DL)
}

pub unsafe fn L_setDisplayList(dl: SEXP) -> SEXP {
    let dd = getDevice();
    setGridStateElement(dd, GSS_DL, dl);
    R_NilValue()
}

pub unsafe fn L_getDLelt(index: SEXP) -> SEXP {
    let dd = getDevice();
    let dl = Rf_protect(gridStateElement(dd, GSS_DL));
    let result = VECTOR_ELT(dl, *INTEGER(index) as R_xlen_t);
    Rf_unprotect(1);
    result
}

pub unsafe fn L_setDLelt(value: SEXP) -> SEXP {
    let dd = getDevice();
    let dl = Rf_protect(gridStateElement(dd, GSS_DL));
    let dlindex = gridStateElement(dd, GSS_DLINDEX);
    SET_VECTOR_ELT(dl, *INTEGER(dlindex) as R_xlen_t, value);
    Rf_unprotect(1);
    R_NilValue()
}

pub unsafe fn L_getDLindex() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_DLINDEX)
}

pub unsafe fn L_setDLindex(index: SEXP) -> SEXP {
    let dd = getDevice();
    setGridStateElement(dd, GSS_DLINDEX, index);
    R_NilValue()
}

pub unsafe fn L_getDLon() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_DLON)
}

pub unsafe fn L_setDLon(value: SEXP) -> SEXP {
    let dd = getDevice();
    let prev = gridStateElement(dd, GSS_DLON);
    setGridStateElement(dd, GSS_DLON, value);
    prev
}

pub unsafe fn L_getEngineDLon() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_ENGINEDLON)
}

pub unsafe fn L_setEngineDLon(value: SEXP) -> SEXP {
    let dd = getDevice();
    setGridStateElement(dd, GSS_ENGINEDLON, value);
    R_NilValue()
}

/* ==============================
 * Grid state accessors
 * ============================== */

pub unsafe fn L_getCurrentGrob() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_CURRGROB)
}

pub unsafe fn L_setCurrentGrob(value: SEXP) -> SEXP {
    let dd = getDevice();
    setGridStateElement(dd, GSS_CURRGROB, value);
    R_NilValue()
}

pub unsafe fn L_getEngineRecording() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_ENGINERECORDING)
}

pub unsafe fn L_setEngineRecording(value: SEXP) -> SEXP {
    let dd = getDevice();
    setGridStateElement(dd, GSS_ENGINERECORDING, value);
    R_NilValue()
}

/* ==============================
 * GPar accessors
 * ============================== */

pub unsafe fn L_currentGPar() -> SEXP {
    let dd = getDevice();
    gridStateElement(dd, GSS_GPAR)
}

/* ==============================
 * Page and initialization
 * ============================== */

pub unsafe fn L_newpagerecording() -> SEXP {
    let dd = getDevice();
    // STUB: NewFrameConfirm, GEinitDisplayList
    R_NilValue()
}

pub unsafe fn L_newpage() -> SEXP {
    let dd = getDevice();
    // STUB: GENewPage
    R_NilValue()
}

pub unsafe fn L_clearDefinitions(_clearGroups: SEXP) -> SEXP {
    // STUB: releasePattern, releaseClipPath, releaseMask, releaseGroup
    R_NilValue()
}

pub unsafe fn L_initGPar() -> SEXP {
    let dd = getDevice();
    initGPar(dd);
    R_NilValue()
}

pub unsafe fn L_initViewportStack() -> SEXP {
    let dd = getDevice();
    initVP(dd);
    R_NilValue()
}

pub unsafe fn L_initDisplayList() -> SEXP {
    let dd = getDevice();
    initDL(dd);
    R_NilValue()
}

/* ==============================
 * getViewportTransform
 * ============================== */

pub unsafe fn getViewportTransform(
    currentvp: SEXP,
    dd: pGEDevDesc,
    vpWidthCM: *mut c_double,
    vpHeightCM: *mut c_double,
    transform: *mut LTransform,
    rotationAngle: *mut c_double,
) {
    let mut devWidthCM: c_double = 0.0;
    let mut devHeightCM: c_double = 0.0;
    getDeviceSize(dd, &mut devWidthCM, &mut devHeightCM);
    // if deviceChanged(devWidthCM, devHeightCM, currentvp) {
    //     calcViewportTransform(currentvp, viewportParent(currentvp), 1, dd);
    // }
    *vpWidthCM = 0.0;
    *vpHeightCM = 0.0;
    *rotationAngle = 0.0;
}

/* ==============================
 * L_convert
 * ============================== */

pub unsafe fn L_convert(x: SEXP, whatfrom: SEXP, whatto: SEXP, unitto: SEXP) -> SEXP {
    // Full implementation requires unit conversion functions
    // STUB: return empty numeric
    let nx = unitLength(x);
    Rf_allocVector(SEXPTYPE::REALSXP.0, nx)
}

/* ==============================
 * L_devLoc
 * ============================== */

pub unsafe fn L_devLoc(x: SEXP, y: SEXP, device: SEXP) -> SEXP {
    // Full implementation requires unit conversion
    let maxn = unitLength(x);
    let result = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, 2));
    SET_VECTOR_ELT(
        result,
        0 as R_xlen_t,
        Rf_allocVector(SEXPTYPE::REALSXP.0, maxn),
    );
    SET_VECTOR_ELT(
        result,
        1 as R_xlen_t,
        Rf_allocVector(SEXPTYPE::REALSXP.0, maxn),
    );
    Rf_unprotect(1);
    result
}

/* ==============================
 * L_devDim
 * ============================== */

pub unsafe fn L_devDim(x: SEXP, y: SEXP, device: SEXP) -> SEXP {
    let maxn = unitLength(x);
    let result = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, 2));
    SET_VECTOR_ELT(
        result,
        0 as R_xlen_t,
        Rf_allocVector(SEXPTYPE::REALSXP.0, maxn),
    );
    SET_VECTOR_ELT(
        result,
        1 as R_xlen_t,
        Rf_allocVector(SEXPTYPE::REALSXP.0, maxn),
    );
    Rf_unprotect(1);
    result
}

/* ==============================
 * L_layoutRegion
 * ============================== */

pub unsafe fn L_layoutRegion(layoutPosRow: SEXP, layoutPosCol: SEXP) -> SEXP {
    let dd = getDevice();
    let currentvp = gridStateElement(dd, GSS_VP);
    if isNull(viewportLayout(currentvp)) {
        return R_NilValue();
    }
    let answer = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, 4));
    // calcViewportLocationFromLayout(layoutPosRow, layoutPosCol, currentvp, &vpl);
    Rf_unprotect(1);
    answer
}

/* ==============================
 * Edge detection functions
 * ============================== */

unsafe fn rectEdge(
    xmin: c_double,
    ymin: c_double,
    xmax: c_double,
    ymax: c_double,
    theta: c_double,
    edgex: *mut c_double,
    edgey: *mut c_double,
) {
    let xm = (xmin + xmax) / 2.0;
    let ym = (ymin + ymax) / 2.0;
    let dx = (xmax - xmin) / 2.0;
    let dy = (ymax - ymin) / 2.0;

    if theta == 0.0 {
        *edgex = xmax;
        *edgey = ym;
    } else if theta == 270.0 {
        *edgex = xm;
        *edgey = ymin;
    } else if theta == 180.0 {
        *edgex = xmin;
        *edgey = ym;
    } else if theta == 90.0 {
        *edgex = xm;
        *edgey = ymax;
    } else {
        let cutoff = dy / dx;
        let angle = theta / 180.0 * std::f64::consts::PI;
        let tan_theta = angle.tan();
        let cos_theta = angle.cos();
        let sin_theta = angle.sin();
        if tan_theta.abs() < cutoff {
            if cos_theta > 0.0 {
                *edgex = xmax;
                *edgey = ym + tan_theta * dx;
            } else {
                *edgex = xmin;
                *edgey = ym - tan_theta * dx;
            }
        } else {
            if sin_theta > 0.0 {
                *edgey = ymax;
                *edgex = xm + dy / tan_theta;
            } else {
                *edgey = ymin;
                *edgex = xm - dy / tan_theta;
            }
        }
    }
}

unsafe fn circleEdge(
    x: c_double,
    y: c_double,
    r: c_double,
    theta: c_double,
    edgex: *mut c_double,
    edgey: *mut c_double,
) {
    let angle = theta / 180.0 * std::f64::consts::PI;
    *edgex = x + r * angle.cos();
    *edgey = y + r * angle.sin();
}

unsafe fn polygonEdge(
    x: *const f64,
    y: *const f64,
    n: c_int,
    theta: c_double,
    edgex: *mut c_double,
    edgey: *mut c_double,
) {
    let mut xmin_val = f64::MAX;
    let mut xmax_val = f64::MIN;
    let mut ymin_val = f64::MAX;
    let mut ymax_val = f64::MIN;
    let angle = theta / 180.0 * std::f64::consts::PI;
    let xm: f64;
    let ym: f64;
    let mut found = 0;

    for i in 0..n as usize {
        if *x.add(i) < xmin_val {
            xmin_val = *x.add(i);
        }
        if *x.add(i) > xmax_val {
            xmax_val = *x.add(i);
        }
        if *y.add(i) < ymin_val {
            ymin_val = *y.add(i);
        }
        if *y.add(i) > ymax_val {
            ymax_val = *y.add(i);
        }
    }
    xm = (xmin_val + xmax_val) / 2.0;
    ym = (ymin_val + ymax_val) / 2.0;

    if (xmin_val - xmax_val).abs() < 1e-6
        || (ymin_val - ymax_val).abs() / (xmin_val - xmax_val).abs() > 1000.0
    {
        *edgex = xmin_val;
        if theta == 90.0 {
            *edgey = ymax_val;
        } else if theta == 270.0 {
            *edgey = ymin_val;
        } else {
            *edgey = ym;
        }
        return;
    }
    if (ymin_val - ymax_val).abs() < 1e-6
        || (xmin_val - xmax_val).abs() / (ymin_val - ymax_val).abs() > 1000.0
    {
        *edgey = ymin_val;
        if theta == 0.0 {
            *edgex = xmax_val;
        } else if theta == 180.0 {
            *edgex = xmin_val;
        } else {
            *edgex = xm;
        }
        return;
    }

    let mut found_i: usize = 0;
    for i in 0..n as usize {
        let v1 = i;
        let v2 = if i + 1 == n as usize { 0 } else { i + 1 };
        let mut vangle1 = (*y.add(v1) - ym).atan2(*x.add(v1) - xm);
        if vangle1 < 0.0 {
            vangle1 += 2.0 * std::f64::consts::PI;
        }
        let mut vangle2 = (*y.add(v2) - ym).atan2(*x.add(v2) - xm);
        if vangle2 < 0.0 {
            vangle2 += 2.0 * std::f64::consts::PI;
        }

        if (vangle1 >= vangle2 && vangle1 >= angle && vangle2 <= angle)
            || (vangle1 < vangle2
                && ((vangle1 >= angle && 0.0 <= angle)
                    || (vangle2 <= angle && 2.0 * std::f64::consts::PI >= angle)))
        {
            found = 1;
            found_i = i;
            break;
        }
    }

    if found != 0 {
        let mut x2: f64 = 0.0;
        let mut y2: f64 = 0.0;
        rectEdge(
            xmin_val, ymin_val, xmax_val, ymax_val, theta, &mut x2, &mut y2,
        );
        let x3 = *x.add(found_i);
        let y3 = *y.add(found_i);
        let x4 = *x.add(if found_i + 1 == n as usize {
            0
        } else {
            found_i + 1
        });
        let y4 = *y.add(if found_i + 1 == n as usize {
            0
        } else {
            found_i + 1
        });
        let numa = (x4 - x3) * (ym - y3) - (y4 - y3) * (xm - x3);
        let denom = (y4 - y3) * (x2 - xm) - (x4 - x3) * (y2 - ym);
        let ua = numa / denom;
        *edgex = xm + ua * (x2 - xm);
        *edgey = ym + ua * (y2 - ym);
    } else {
        *edgex = xm;
        *edgey = ym;
    }
}

/* ==============================
 * Drawing primitives: arrows
 * ============================== */

unsafe fn drawArrow(
    x: *const f64,
    y: *const f64,
    atype: SEXP,
    i: c_int,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    let nt = LENGTH(atype);
    match *INTEGER(atype).add((i % nt) as usize) {
        1 => GEPolyline(3, x, y, gc, dd),
        2 => GEPolygon(3, x, y, gc, dd),
        _ => {} // intentionally unhandled: unknown arrowhead type
    }
}

unsafe fn calcArrow(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    angle: SEXP,
    length: SEXP,
    i: c_int,
    _vpc: LViewportContext,
    _vpWidthCM: f64,
    _vpHeightCM: f64,
    vertx: *mut f64,
    verty: *mut f64,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    let l1 = 0.0; // STUB: transformWidthtoINCHES
    let l2 = 0.0; // STUB: transformHeighttoINCHES
    let l = fmin2(l1, l2);
    let na = LENGTH(angle);
    let a = DEG2RAD * *REAL(angle).add((i % na) as usize);
    let xc = x2 - x1;
    let yc = y2 - y1;
    let rot = (yc).atan2(xc);
    *vertx.add(0) = toDeviceX(x1 + l * (rot + a).cos(), GE_INCHES, dd);
    *verty.add(0) = toDeviceY(y1 + l * (rot + a).sin(), GE_INCHES, dd);
    *vertx.add(1) = toDeviceX(x1, GE_INCHES, dd);
    *verty.add(1) = toDeviceY(y1, GE_INCHES, dd);
    *vertx.add(2) = toDeviceX(x1 + l * (rot - a).cos(), GE_INCHES, dd);
    *verty.add(2) = toDeviceY(y1 + l * (rot - a).sin(), GE_INCHES, dd);
}

unsafe fn arrows(
    x: *const f64,
    y: *const f64,
    n: c_int,
    arrow: SEXP,
    i: c_int,
    start: bool,
    end: bool,
    vpc: LViewportContext,
    vpWidthCM: f64,
    vpHeightCM: f64,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    let ends = VECTOR_ELT(arrow, GRID_ARROWENDS as R_xlen_t);
    let ne = LENGTH(ends);
    let mut vertx = [0.0f64; 3];
    let mut verty = [0.0f64; 3];
    let mut first = true;
    let mut last = true;

    if n < 2 {
        return;
    }

    match *INTEGER(ends).add((i % ne) as usize) {
        2 => {
            first = false;
        }
        1 => {
            last = false;
        }
        _ => {} // intentionally unhandled: unknown grid boundary value
    }

    if first && start {
        calcArrow(
            fromDeviceX(*x.add(0), GE_INCHES, dd),
            fromDeviceY(*y.add(0), GE_INCHES, dd),
            fromDeviceX(*x.add(1), GE_INCHES, dd),
            fromDeviceY(*y.add(1), GE_INCHES, dd),
            VECTOR_ELT(arrow, GRID_ARROWANGLE as R_xlen_t),
            VECTOR_ELT(arrow, GRID_ARROWLENGTH as R_xlen_t),
            i,
            vpc,
            vpWidthCM,
            vpHeightCM,
            vertx.as_mut_ptr(),
            verty.as_mut_ptr(),
            gc,
            dd,
        );
        drawArrow(
            vertx.as_ptr(),
            verty.as_ptr(),
            VECTOR_ELT(arrow, GRID_ARROWTYPE as R_xlen_t),
            i,
            gc,
            dd,
        );
    }
    if last && end {
        calcArrow(
            fromDeviceX(*x.add((n - 1) as usize), GE_INCHES, dd),
            fromDeviceY(*y.add((n - 1) as usize), GE_INCHES, dd),
            fromDeviceX(*x.add((n - 2) as usize), GE_INCHES, dd),
            fromDeviceY(*y.add((n - 2) as usize), GE_INCHES, dd),
            VECTOR_ELT(arrow, GRID_ARROWANGLE as R_xlen_t),
            VECTOR_ELT(arrow, GRID_ARROWLENGTH as R_xlen_t),
            i,
            vpc,
            vpWidthCM,
            vpHeightCM,
            vertx.as_mut_ptr(),
            verty.as_mut_ptr(),
            gc,
            dd,
        );
        drawArrow(
            vertx.as_ptr(),
            verty.as_ptr(),
            VECTOR_ELT(arrow, GRID_ARROWTYPE as R_xlen_t),
            i,
            gc,
            dd,
        );
    }
}

/* ==============================
 * L_moveTo
 * ============================== */

pub unsafe fn L_moveTo(x: SEXP, y: SEXP) -> SEXP {
    // STUB: full implementation requires unit conversion
    R_NilValue()
}

/* ==============================
 * L_lineTo
 * ============================== */

pub unsafe fn L_lineTo(x: SEXP, y: SEXP, arrow: SEXP) -> SEXP {
    // STUB: full implementation requires unit conversion + GELine
    R_NilValue()
}

/* ==============================
 * L_lines
 * ============================== */

pub unsafe fn L_lines(x: SEXP, y: SEXP, index: SEXP, arrow: SEXP) -> SEXP {
    // STUB: full implementation requires unit conversion + GEPolyline
    R_NilValue()
}

/* ==============================
 * gridXspline (internal)
 * ============================== */

unsafe fn gridXspline(
    _x: SEXP,
    _y: SEXP,
    _s: SEXP,
    _o: SEXP,
    _a: SEXP,
    _rep: SEXP,
    _index: SEXP,
    _theta: c_double,
    _draw: bool,
    _trace: bool,
) -> SEXP {
    R_NilValue()
}

/* ==============================
 * L_xspline
 * ============================== */

pub unsafe fn L_xspline(
    x: SEXP,
    y: SEXP,
    s: SEXP,
    o: SEXP,
    a: SEXP,
    rep: SEXP,
    index: SEXP,
) -> SEXP {
    gridXspline(x, y, s, o, a, rep, index, 0.0, true, false);
    R_NilValue()
}

/* ==============================
 * L_xsplineBounds
 * ============================== */

pub unsafe fn L_xsplineBounds(
    x: SEXP,
    y: SEXP,
    s: SEXP,
    o: SEXP,
    a: SEXP,
    rep: SEXP,
    index: SEXP,
    theta: SEXP,
) -> SEXP {
    gridXspline(x, y, s, o, a, rep, index, *REAL(theta), false, false)
}

/* ==============================
 * L_xsplinePoints
 * ============================== */

pub unsafe fn L_xsplinePoints(
    x: SEXP,
    y: SEXP,
    s: SEXP,
    o: SEXP,
    a: SEXP,
    rep: SEXP,
    index: SEXP,
    theta: SEXP,
) -> SEXP {
    gridXspline(x, y, s, o, a, rep, index, *REAL(theta), false, true)
}

/* ==============================
 * L_segments
 * ============================== */

pub unsafe fn L_segments(x0: SEXP, y0: SEXP, x1: SEXP, y1: SEXP, arrow: SEXP) -> SEXP {
    // STUB: full implementation requires unit conversion + GELine
    R_NilValue()
}

/* ==============================
 * L_arrows
 * ============================== */

pub unsafe fn L_arrows(
    x1: SEXP,
    x2: SEXP,
    xnm1: SEXP,
    xn: SEXP,
    y1: SEXP,
    y2: SEXP,
    ynm1: SEXP,
    yn: SEXP,
    angle: SEXP,
    length: SEXP,
    ends: SEXP,
    r#type: SEXP,
) -> SEXP {
    // STUB: full implementation
    R_NilValue()
}

/* ==============================
 * L_polygon
 * ============================== */

pub unsafe fn L_polygon(x: SEXP, y: SEXP, index: SEXP) -> SEXP {
    // STUB: full implementation requires unit conversion + GEPolygon
    R_NilValue()
}

/* ==============================
 * gridCircle (internal)
 * ============================== */

unsafe fn gridCircle(_x: SEXP, _y: SEXP, _r: SEXP, _theta: c_double, _draw: bool) -> SEXP {
    R_NilValue()
}

/* ==============================
 * L_circle
 * ============================== */

pub unsafe fn L_circle(x: SEXP, y: SEXP, r: SEXP) -> SEXP {
    gridCircle(x, y, r, 0.0, true);
    R_NilValue()
}

/* ==============================
 * L_circleBounds
 * ============================== */

pub unsafe fn L_circleBounds(x: SEXP, y: SEXP, r: SEXP, theta: SEXP) -> SEXP {
    gridCircle(x, y, r, *REAL(theta), false)
}

/* ==============================
 * gridRect (internal)
 * ============================== */

unsafe fn gridRect(
    _x: SEXP,
    _y: SEXP,
    _w: SEXP,
    _h: SEXP,
    _hjust: SEXP,
    _vjust: SEXP,
    _theta: c_double,
    _draw: bool,
) -> SEXP {
    R_NilValue()
}

/* ==============================
 * L_rect
 * ============================== */

pub unsafe fn L_rect(x: SEXP, y: SEXP, w: SEXP, h: SEXP, hjust: SEXP, vjust: SEXP) -> SEXP {
    gridRect(x, y, w, h, hjust, vjust, 0.0, true);
    R_NilValue()
}

/* ==============================
 * L_rectBounds
 * ============================== */

pub unsafe fn L_rectBounds(
    x: SEXP,
    y: SEXP,
    w: SEXP,
    h: SEXP,
    hjust: SEXP,
    vjust: SEXP,
    theta: SEXP,
) -> SEXP {
    gridRect(x, y, w, h, hjust, vjust, *REAL(theta), false)
}

/* ==============================
 * L_path
 * ============================== */

pub unsafe fn L_path(x: SEXP, y: SEXP, index: SEXP, rule: SEXP) -> SEXP {
    // STUB: full implementation requires GEPath
    R_NilValue()
}

/* ==============================
 * L_raster
 * ============================== */

pub unsafe fn L_raster(
    raster: SEXP,
    x: SEXP,
    y: SEXP,
    w: SEXP,
    h: SEXP,
    hjust: SEXP,
    vjust: SEXP,
    interpolate: SEXP,
) -> SEXP {
    // STUB: full implementation requires GERaster
    R_NilValue()
}

/* ==============================
 * L_cap
 * ============================== */

pub unsafe fn L_cap() -> SEXP {
    let dd = getDevice();
    let raster = Rf_protect(GECap(dd));
    if isNull(raster) {
        Rf_unprotect(1);
        R_NilValue()
    } else {
        let image = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, LENGTH(raster)));
        Rf_unprotect(2);
        image
    }
}

/* ==============================
 * gridText (internal)
 * ============================== */

unsafe fn gridText(
    _label: SEXP,
    _x: SEXP,
    _y: SEXP,
    _hjust: SEXP,
    _vjust: SEXP,
    _rot: SEXP,
    _checkOverlap: SEXP,
    _theta: c_double,
    _draw: bool,
) -> SEXP {
    R_NilValue()
}

/* ==============================
 * L_text
 * ============================== */

pub unsafe fn L_text(
    label: SEXP,
    x: SEXP,
    y: SEXP,
    hjust: SEXP,
    vjust: SEXP,
    rot: SEXP,
    checkOverlap: SEXP,
) -> SEXP {
    gridText(label, x, y, hjust, vjust, rot, checkOverlap, 0.0, true);
    R_NilValue()
}

/* ==============================
 * L_textBounds
 * ============================== */

pub unsafe fn L_textBounds(
    label: SEXP,
    x: SEXP,
    y: SEXP,
    hjust: SEXP,
    vjust: SEXP,
    rot: SEXP,
    theta: SEXP,
) -> SEXP {
    let checkOverlap = Rf_allocVector(SEXPTYPE::LGLSXP.0, 1);
    *LOGICAL(checkOverlap) = 0;
    gridText(
        label,
        x,
        y,
        hjust,
        vjust,
        rot,
        checkOverlap,
        *REAL(theta),
        false,
    )
}

/* ==============================
 * symbolCoords (internal helper)
 * ============================== */

unsafe fn symbolCoords(x: *const f64, y: *const f64, n: c_int, _dd: pGEDevDesc) -> SEXP {
    let result = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, 2));
    let xs = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, n));
    let ys = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, n));
    for i in 0..n as usize {
        *REAL(xs).add(i) = *x.add(i);
        *REAL(ys).add(i) = *y.add(i);
    }
    SET_VECTOR_ELT(result, 0 as R_xlen_t, xs);
    SET_VECTOR_ELT(result, 1 as R_xlen_t, ys);
    Rf_unprotect(3);
    result
}

/* ==============================
 * symbolNumCoords (internal helper)
 * ============================== */

unsafe fn symbolNumCoords(pch: c_int, closed: bool) -> c_int {
    let mut result: c_int = 1;
    match pch {
        0 | 1 | 2 => {}
        3 | 4 => {
            if !closed {
                result = 2;
            }
        }
        5 | 6 => {}
        7 => {
            if closed {
                result = 1;
            } else {
                result = 2;
            }
        }
        8 => {
            if !closed {
                result = 4;
            }
        }
        9 | 10 | 12 | 13 => {
            if closed {
                result = 1;
            } else {
                result = 2;
            }
        }
        11 => {
            if closed {
                result = 2;
            }
        }
        14 => {
            if closed {
                result = 2;
            }
        }
        15 | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 => {}
        _ => {} // intentionally unhandled: unknown arrowhead type
    }
    result
}

/* ==============================
 * gridSymbol
 * ============================== */

pub unsafe fn gridSymbol(
    _x: f64,
    _y: f64,
    _pch: c_int,
    _size: f64,
    _draw: bool,
    _closed: bool,
    _numCoords: c_int,
    _gc: pGEcontext,
    _dd: pGEDevDesc,
) -> SEXP {
    R_NilValue()
}

/* ==============================
 * gridPoints (internal)
 * ============================== */

unsafe fn gridPoints(
    _x: SEXP,
    _y: SEXP,
    _pch: SEXP,
    _size: SEXP,
    _draw: bool,
    _closed: bool,
) -> SEXP {
    R_NilValue()
}

/* ==============================
 * L_points
 * ============================== */

pub unsafe fn L_points(x: SEXP, y: SEXP, pch: SEXP, size: SEXP) -> SEXP {
    gridPoints(x, y, pch, size, true, false)
}

/* ==============================
 * L_pointsPoints
 * ============================== */

pub unsafe fn L_pointsPoints(x: SEXP, y: SEXP, pch: SEXP, size: SEXP, closed: SEXP) -> SEXP {
    gridPoints(x, y, pch, size, false, asBool(closed) != 0)
}

/* ==============================
 * L_clip
 * ============================== */

pub unsafe fn L_clip(x: SEXP, y: SEXP, w: SEXP, h: SEXP, hjust: SEXP, vjust: SEXP) -> SEXP {
    // STUB: full implementation requires GESetClip
    R_NilValue()
}

/* ==============================
 * L_pretty
 * ============================== */

pub unsafe fn L_pretty(scale: SEXP) -> SEXP {
    let n_ = Rf_ScalarInteger(5);
    L_pretty2(scale, n_)
}

/* ==============================
 * L_pretty2
 * ============================== */

pub unsafe fn L_pretty2(scale: SEXP, n_: SEXP) -> SEXP {
    let mut min = numeric(scale, 0);
    let mut max = numeric(scale, 1);
    let mut n = crate::main::coerce::asInteger(n_);
    let mut temp: f64;

    let swap = min > max;
    if swap {
        temp = min;
        min = max;
        max = temp;
    }

    GEPretty(&mut min, &mut max, &mut n);

    if swap {
        temp = min;
        min = max;
        max = temp;
    }

    let mut axp = [min, max, n as f64];
    Rf_CreateAtVector(axp.as_mut_ptr(), ptr::null_mut(), n, 0)
}

/* ==============================
 * L_locator
 * ============================== */

pub unsafe fn L_locator() -> SEXP {
    let answer = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP.0, 2));
    *REAL(answer).add(0) = f64::NAN;
    *REAL(answer).add(1) = f64::NAN;
    Rf_unprotect(1);
    answer
}

/* ==============================
 * L_locnBounds
 * ============================== */

pub unsafe fn L_locnBounds(x: SEXP, y: SEXP, theta: SEXP) -> SEXP {
    // Full implementation requires unit conversion
    R_NilValue()
}

/* ==============================
 * L_stringMetric
 * ============================== */

pub unsafe fn L_stringMetric(label: SEXP) -> SEXP {
    let result = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, 3));
    SET_VECTOR_ELT(
        result,
        0 as R_xlen_t,
        Rf_allocVector(SEXPTYPE::REALSXP.0, 0),
    );
    SET_VECTOR_ELT(
        result,
        1 as R_xlen_t,
        Rf_allocVector(SEXPTYPE::REALSXP.0, 0),
    );
    SET_VECTOR_ELT(
        result,
        2 as R_xlen_t,
        Rf_allocVector(SEXPTYPE::REALSXP.0, 0),
    );
    Rf_unprotect(1);
    result
}

/* ==============================
 * L_convertToNative (deprecated)
 * ============================== */

pub unsafe fn L_convertToNative(_x: SEXP, _what: SEXP) -> SEXP {
    R_NilValue()
}
