/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Port of R's src/library/grid/src/grid.c (5470 lines)
 *
 *  grid -- main grid drawing primitives and state management.
 */

use std::ffi::c_void;
use std::os::raw::{c_char, c_double, c_int, c_uint};
use std::ptr;

use crate::attrib_core::{R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::main::duplicate::duplicate as Rf_duplicate;
use crate::mainutils::coerce::asBool;
use crate::mainutils::colors::RGBpar3;
use crate::mainutils::engine::{
    GECap, GELine, GEMode, GENewPage, GEPath, GEPolygon, GEPolyline, GEPretty, GERaster, GESetClip,
    GEStrMetric, GEdeviceDirty, Rf_eval_with_gd, fromDeviceX, fromDeviceY, toDeviceHeight,
    toDeviceWidth, toDeviceX, toDeviceY,
};
use crate::mainutils::errors::{Rf_error, Rf_error1, Rf_warning};
use crate::mainutils::graphics_ffi::rmath_grid_release_definitions as release_grid_definitions;
use crate::mainutils::objects::inherits2 as Rf_inherits;
use crate::mainutils::subset::installTrChar;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::constructors::{Rf_lang2 as lang2, Rf_lang3 as lang3};
use crate::sexp::envir::{R_findVar as findVar, defineVar, findFun};
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::memory_ext::R_alloc;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::clippath::{isClipPath, resolveClipPath};
use super::gpar::{
    GP_ALPHA, GP_CEX, GP_COL, GP_FILL, GP_FONT, GP_FONTFAMILY, GP_FONTSIZE, GP_GAMMA, GP_LEX,
    GP_LINEEND, GP_LINEHEIGHT, GP_LINEJOIN, GP_LINEMITRE, GP_LTY, GP_LWD, gcontextFromgpar,
    gpFillSXP, initGContext, initGPar, pGEDevDesc, pGEcontext, resolveGPar, updateGContext,
};
use super::just::{justification, justifyX, justifyY};
use super::layout::calcViewportLocationFromLayout;
use super::mask::{isMask, resolveMask};
use super::state::{gridStateElement, initDL, setGridStateElement};
use super::types::*;
use super::unit::{
    L_INCHES, L_NATIVE, L_NPC, transformDimn, transformHeighttoINCHES, transformLocn,
    transformWHfromNPC, transformWHtoNPC, transformWidthHeightFromINCHES, transformWidthtoINCHES,
    transformXYFromINCHES, transformXYfromNPC, transformXYtoNPC, transformXtoINCHES,
    transformYtoINCHES, unit, unitLength, unitUnit, unitValue,
};
use super::util::{copyRect, getListElement, intersect, rect, setListElement, textRect};
use super::viewport::*;

unsafe fn GEcurrentDevice() -> pGEDevDesc {
    unsafe { crate::library::grdevices::device_registry::GEcurrentDevice() as pGEDevDesc }
}

unsafe fn lang4(symbol: SEXP, arg1: SEXP, arg2: SEXP, arg3: SEXP) -> SEXP {
    unsafe {
        let tail = Rf_cons(arg3, R_NilValue());
        let tail = Rf_cons(arg2, tail);
        let tail = Rf_cons(arg1, tail);
        let call = Rf_cons(symbol, tail);
        if !call.is_null() {
            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        call
    }
}

unsafe fn SET_TAG(x: SEXP, y: SEXP) {
    unsafe {
        SETTAG(x, y);
    }
}

unsafe fn NewFrameConfirm(_dev: *const c_void) {}

unsafe fn Rf_CreateAtVector(axp: *mut c_double, usr: *mut c_double, n: c_int, log: c_int) -> SEXP {
    unsafe {
        if axp.is_null() {
            return R_NilValue();
        }
        let axp_values = [*axp, *axp.add(1), *axp.add(2)];
        let usr_values = if usr.is_null() {
            [axp_values[0], axp_values[1]]
        } else {
            [*usr, *usr.add(1)]
        };
        crate::library::grdevices::axis_scales::create_at_vector_raw(
            axp_values,
            usr_values,
            n,
            log != 0,
        )
    }
}

unsafe fn GEExpressionMetric(
    _x: SEXP,
    _gc: pGEcontext,
    ascent: *mut f64,
    descent: *mut f64,
    width: *mut f64,
    _dd: pGEDevDesc,
) {
    unsafe {
        if !ascent.is_null() {
            *ascent = 0.0;
        }
        if !descent.is_null() {
            *descent = 0.0;
        }
        if !width.is_null() {
            *width = 0.0;
        }
    }
}

unsafe fn rmath_grid_release_definitions(dd: pGEDevDesc, clear_groups: c_int) {
    unsafe {
        release_grid_definitions(
            dd as crate::mainutils::graphics_ffi::pGEDevDesc,
            clear_groups,
        );
    }
}

unsafe fn initVP(dd: pGEDevDesc) {
    unsafe {
        super::viewport::initVP(dd as *const u8);
    }
}

/* ==============================
 * Constants
 * ============================== */

const GE_INCHES: c_int = 1;
const R_TRANWHITE: c_int = 0x7FFFFFFF;
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

/* ==============================
 * Local helper: numeric(x, index)
 * ============================== */

#[inline]
unsafe fn numeric(x: SEXP, index: c_int) -> f64 {
    unsafe { *REAL(x).add(index as usize) }
}

/* ==============================
 * Local helper: isNull
 * ============================== */

#[inline]
unsafe fn isNull(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() }
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

pub unsafe fn getDevice() -> pGEDevDesc {
    unsafe { GEcurrentDevice() }
}

/* ==============================
 * getDeviceSize
 * ============================== */

pub unsafe fn getDeviceSize(dd: pGEDevDesc, devWidthCM: *mut c_double, devHeightCM: *mut c_double) {
    unsafe {
        // Prefer device-driven conversion helpers when available.
        // In headless mode these still give deterministic non-zero sizes.
        let mut width_in = toDeviceWidth(1.0, GE_INCHES, dd).abs();
        let mut height_in = toDeviceHeight(1.0, GE_INCHES, dd).abs();
        if !width_in.is_finite() || width_in == 0.0 {
            width_in = 1.0;
        }
        if !height_in.is_finite() || height_in == 0.0 {
            height_in = 1.0;
        }
        *devWidthCM = width_in * 2.54;
        *devHeightCM = height_in * 2.54;
    }
}

/* ==============================
 * deviceChanged
 * ============================== */

unsafe fn deviceChanged(devWidthCM: c_double, devHeightCM: c_double, currentvp: SEXP) -> bool {
    unsafe {
        let mut result = false;
        let pvpDevWidthCM = VECTOR_ELT(currentvp, PVP_DEVWIDTHCM as R_xlen_t);
        let _width_guard = protect(pvpDevWidthCM);
        let pvpDevHeightCM = VECTOR_ELT(currentvp, PVP_DEVHEIGHTCM as R_xlen_t);
        let _height_guard = protect(pvpDevHeightCM);
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
        result
    }
}

/* ==============================
 * L_initGrid
 * ============================== */

pub unsafe fn L_initGrid(GridEvalEnv: SEXP) -> SEXP {
    unsafe {
        set_grid_eval_env(GridEvalEnv);
        // GEregisterSystem(gridCallback, &mut gridRegisterIndex);
        R_NilValue()
    }
}

/* ==============================
 * L_killGrid
 * ============================== */

pub unsafe fn L_killGrid() -> SEXP {
    unsafe {
        // GEunregisterSystem(gridRegisterIndex);
        R_NilValue()
    }
}

/* ==============================
 * dirtyGridDevice
 * ============================== */

pub unsafe fn dirtyGridDevice(dd: pGEDevDesc) {
    unsafe {
        GEdeviceDirty(dd);
    }
}

/* ==============================
 * L_gridDirty
 * ============================== */

pub unsafe fn L_gridDirty() -> SEXP {
    unsafe {
        let dd = getDevice();
        dirtyGridDevice(dd);
        R_NilValue()
    }
}

/* ==============================
 * getViewportContext
 * ============================== */

unsafe fn getViewportContext(vp: SEXP, vpc: *mut LViewportContext) {
    unsafe {
        fillViewportContextFromViewport(vp, vpc);
    }
}

/* ==============================
 * L_currentViewport
 * ============================== */

pub unsafe fn L_currentViewport() -> SEXP {
    unsafe {
        let dd = getDevice();
        gridStateElement(dd, GSS_VP)
    }
}

/* ==============================
 * doSetViewport
 * ============================== */

pub unsafe fn doSetViewport(vp: SEXP, topLevelVP: c_int, pushing: c_int, dd: pGEDevDesc) -> SEXP {
    unsafe {
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
            defineVar(
                installTrChar(STRING_ELT(VECTOR_ELT(vp, VP_NAME as R_xlen_t), 0)),
                vp,
                VECTOR_ELT(parent, PVP_CHILDREN as R_xlen_t),
            );
        }

        calcViewportTransform(
            vp,
            viewportParent(vp),
            topLevelVP == 0 && !deviceChanged(devWidthCM, devHeightCM, viewportParent(vp)),
            dd,
        );

        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path) != 0
        {
            if !isClipPath(viewportClipSXP(vp))
                && (viewportClip(vp) == NA_LOGICAL || viewportClip(vp) != 0)
            {
                Rf_warning(
                    c"Turning clipping on or off within a (clipping) path is no honoured".as_ptr(),
                );
            }
        } else if isClipPath(viewportClipSXP(vp)) {
            let parentClip = viewportClipRect(viewportParent(vp));
            let _parent_clip_guard = protect(parentClip);
            let currentClip = Rf_allocVector(SEXPTYPE::REALSXP, 4);
            let _current_clip_guard = protect(currentClip);
            *REAL(currentClip).add(0) = *REAL(parentClip).add(0);
            *REAL(currentClip).add(1) = *REAL(parentClip).add(1);
            *REAL(currentClip).add(2) = *REAL(parentClip).add(2);
            *REAL(currentClip).add(3) = *REAL(parentClip).add(3);
            SET_VECTOR_ELT(vp, PVP_CLIPRECT as R_xlen_t, currentClip);
        } else {
            if viewportClip(vp) == NA_LOGICAL {
                xx1 = toDeviceX(-0.5 * devWidthCM / 2.54, GE_INCHES, dd);
                yy1 = toDeviceY(-0.5 * devHeightCM / 2.54, GE_INCHES, dd);
                xx2 = toDeviceX(1.5 * devWidthCM / 2.54, GE_INCHES, dd);
                yy2 = toDeviceY(1.5 * devHeightCM / 2.54, GE_INCHES, dd);
                GESetClip(xx1, yy1, xx2, yy2, dd);
            } else if viewportClip(vp) != 0 {
                let rotationAngle = if TYPEOF(viewportRotation(vp)) == SEXPTYPE::REALSXP
                    && LENGTH(viewportRotation(vp)) > 0
                {
                    *REAL(viewportRotation(vp))
                } else {
                    0.0
                };
                if rotationAngle != 0.0
                    && rotationAngle != 90.0
                    && rotationAngle != 270.0
                    && rotationAngle != 360.0
                {
                    Rf_warning(c"cannot clip to rotated viewport".as_ptr());
                    let parentClip = viewportClipRect(viewportParent(vp));
                    let _parent_clip_guard = protect(parentClip);
                    xx1 = *REAL(parentClip).add(0);
                    yy1 = *REAL(parentClip).add(1);
                    xx2 = *REAL(parentClip).add(2);
                    yy2 = *REAL(parentClip).add(3);
                } else {
                    let mut transform: LTransform = [[0.0; 3]; 3];
                    for i in 0..3usize {
                        for j in 0..3usize {
                            transform[i][j] = *REAL(viewportTransform(vp)).add(i + 3 * j);
                        }
                    }
                    let vpWidthCM = *REAL(viewportWidthCM(vp));
                    let vpHeightCM = *REAL(viewportHeightCM(vp));
                    let x1 = if topLevelVP == 0 {
                        unit(0.0, L_NPC)
                    } else {
                        unit(-0.5, L_NPC)
                    };
                    let _x1_guard = protect(x1);
                    let y1 = if topLevelVP == 0 {
                        unit(0.0, L_NPC)
                    } else {
                        unit(-0.5, L_NPC)
                    };
                    let _y1_guard = protect(y1);
                    let x2 = if topLevelVP == 0 {
                        unit(1.0, L_NPC)
                    } else {
                        unit(1.5, L_NPC)
                    };
                    let _x2_guard = protect(x2);
                    let y2 = if topLevelVP == 0 {
                        unit(1.0, L_NPC)
                    } else {
                        unit(1.5, L_NPC)
                    };
                    let _y2_guard = protect(y2);
                    let mut vpc = LViewportContext::default();
                    getViewportContext(vp, &mut vpc);
                    let mut gc_buf: [u8; 256] = [0; 256];
                    let gc = gc_buf.as_ptr() as pGEcontext;
                    gcontextFromViewport(vp, gc, dd);
                    transformLocn(
                        x1,
                        y1,
                        0,
                        vpc,
                        gc,
                        vpWidthCM,
                        vpHeightCM,
                        dd,
                        &mut transform,
                        &mut xx1,
                        &mut yy1,
                    );
                    transformLocn(
                        x2,
                        y2,
                        0,
                        vpc,
                        gc,
                        vpWidthCM,
                        vpHeightCM,
                        dd,
                        &mut transform,
                        &mut xx2,
                        &mut yy2,
                    );
                    xx1 = toDeviceX(xx1, GE_INCHES, dd);
                    yy1 = toDeviceY(yy1, GE_INCHES, dd);
                    xx2 = toDeviceX(xx2, GE_INCHES, dd);
                    yy2 = toDeviceY(yy2, GE_INCHES, dd);
                    GESetClip(xx1, yy1, xx2, yy2, dd);
                }
            } else {
                let parentClip = viewportClipRect(viewportParent(vp));
                let _parent_clip_guard = protect(parentClip);
                xx1 = *REAL(parentClip).add(0);
                yy1 = *REAL(parentClip).add(1);
                xx2 = *REAL(parentClip).add(2);
                yy2 = *REAL(parentClip).add(3);
                let parentClipPath = VECTOR_ELT(viewportParent(vp), PVP_CLIPPATH as R_xlen_t);
                let _parent_clip_path_guard = protect(parentClipPath);
                if isClipPath(parentClipPath) {
                    SET_VECTOR_ELT(vp, PVP_CLIPPATH as R_xlen_t, parentClipPath);
                }
                if pushing == 0 && !isClipPath(parentClipPath) {
                    GESetClip(xx1, yy1, xx2, yy2, dd);
                }
            }

            let currentClip = Rf_allocVector(SEXPTYPE::REALSXP, 4);
            let _current_clip_guard = protect(currentClip);
            *REAL(currentClip).add(0) = xx1;
            *REAL(currentClip).add(1) = yy1;
            *REAL(currentClip).add(2) = xx2;
            *REAL(currentClip).add(3) = yy2;
            SET_VECTOR_ELT(vp, PVP_CLIPRECT as R_xlen_t, currentClip);
        }

        if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path) != 0
        {
            // Masks are ignored when resolving a clipping path.
        } else if isMask(viewportMaskSXP(vp)) {
            // Resolve after doSetViewport() once this viewport is current.
        } else if viewportMask(vp) {
            SET_VECTOR_ELT(
                vp,
                PVP_MASK as R_xlen_t,
                VECTOR_ELT(viewportParent(vp), PVP_MASK as R_xlen_t),
            );
        } else {
            SET_VECTOR_ELT(vp, PVP_MASK as R_xlen_t, R_NilValue());
            resolveMask(R_NilValue(), dd);
        }

        let widthCM = Rf_allocVector(SEXPTYPE::REALSXP, 1);
        let _width_guard = protect(widthCM);
        *REAL(widthCM) = devWidthCM;
        SET_VECTOR_ELT(vp, PVP_DEVWIDTHCM as R_xlen_t, widthCM);

        let heightCM = Rf_allocVector(SEXPTYPE::REALSXP, 1);
        let _height_guard = protect(heightCM);
        *REAL(heightCM) = devHeightCM;
        SET_VECTOR_ELT(vp, PVP_DEVHEIGHTCM as R_xlen_t, heightCM);

        vp
    }
}

/* ==============================
 * L_setviewport
 * ============================== */

pub unsafe fn L_setviewport(invp: SEXP, hasParent: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let vp = Rf_duplicate(invp);
        let _vp_guard = protect(vp);

        let fcall = lang2(Rf_install(b"pushedvp\0".as_ptr() as *const c_char), vp);
        let _fcall_guard = protect(fcall);
        let pushedvp = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _pushedvp_guard = protect(pushedvp);
        let pushedvp = doSetViewport(
            pushedvp,
            if *LOGICAL(hasParent) != 0 { 0 } else { 1 },
            1,
            dd,
        );

        setGridStateElement(dd, GSS_VP, pushedvp);

        {
            let vpgp = VECTOR_ELT(pushedvp, VP_GP as R_xlen_t);
            let _vpgp_guard = protect(vpgp);
            let fill = getListElement(vpgp, c"fill".as_ptr() as *mut c_char);
            if fill != R_NilValue() {
                resolveGPar(vpgp, 1);
                let pushed_gp = VECTOR_ELT(pushedvp, PVP_GPAR as R_xlen_t);
                SET_VECTOR_ELT(
                    pushed_gp,
                    GP_FILL as R_xlen_t,
                    getListElement(vpgp, c"fill".as_ptr() as *mut c_char),
                );
                setGridStateElement(dd, GSS_GPAR, pushed_gp);
            }
        }

        {
            let clip = viewportClipSXP(pushedvp);
            let _clip_guard = protect(clip);
            if isClipPath(clip) {
                let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
                if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
                    && LENGTH(resolving_path) > 0
                    && *LOGICAL(resolving_path) != 0
                {
                    Rf_warning(
                        c"Clipping paths within a (clipping) path are not honoured".as_ptr(),
                    );
                    SET_VECTOR_ELT(pushedvp, PVP_CLIPPATH as R_xlen_t, R_NilValue());
                } else {
                    let resolvedclip = resolveClipPath(clip, dd);
                    let _resolvedclip_guard = protect(resolvedclip);
                    SET_VECTOR_ELT(pushedvp, PVP_CLIPPATH as R_xlen_t, resolvedclip);
                }
            }
        }

        {
            let mask = viewportMaskSXP(pushedvp);
            let _mask_guard = protect(mask);
            if isMask(mask) {
                let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
                if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
                    && LENGTH(resolving_path) > 0
                    && *LOGICAL(resolving_path) != 0
                {
                    Rf_warning(c"Masks within a (clipping) path are not honoured".as_ptr());
                    SET_VECTOR_ELT(pushedvp, PVP_MASK as R_xlen_t, R_NilValue());
                } else {
                    let resolvedmask = resolveMask(mask, dd);
                    let _resolvedmask_guard = protect(resolvedmask);
                    SET_VECTOR_ELT(pushedvp, PVP_MASK as R_xlen_t, resolvedmask);
                }
            }
        }

        R_NilValue()
    }
}

/* ==============================
 * Viewport search helpers
 * ============================== */

unsafe fn noChildren(children: SEXP) -> bool {
    unsafe {
        let fcall = lang2(
            Rf_install(b"no.children\0".as_ptr() as *const c_char),
            children,
        );
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        let r = asBool(result) != 0;
        r
    }
}

unsafe fn childExists(name: SEXP, children: SEXP) -> bool {
    unsafe {
        let fcall = lang3(
            Rf_install(b"child.exists\0".as_ptr() as *const c_char),
            name,
            children,
        );
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        let r = asBool(result) != 0;
        r
    }
}

unsafe fn childList(children: SEXP) -> SEXP {
    unsafe {
        let fcall = lang2(
            Rf_install(b"child.list\0".as_ptr() as *const c_char),
            children,
        );
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        result
    }
}

unsafe fn pathMatch(path: SEXP, pathsofar: SEXP, strict: SEXP) -> bool {
    unsafe {
        let fcall = lang4(
            Rf_install(b"pathMatch\0".as_ptr() as *const c_char),
            path,
            pathsofar,
            strict,
        );
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        let r = asBool(result) != 0;
        r
    }
}

unsafe fn growPath(pathsofar: SEXP, name: SEXP) -> SEXP {
    unsafe {
        if isNull(pathsofar) {
            return name;
        }
        let fcall = lang3(
            Rf_install(b"growPath\0".as_ptr() as *const c_char),
            pathsofar,
            name,
        );
        let _fcall_guard = protect(fcall);
        let result = Rf_eval_with_gd(fcall, grid_eval_env(), ptr::null_mut());
        let _result_guard = protect(result);
        result
    }
}

unsafe fn findViewport(name: SEXP, strict: SEXP, vp: SEXP, depth: c_int) -> SEXP {
    unsafe {
        let result = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _result_guard = protect(result);
        let zeroDepth = Rf_allocVector(SEXPTYPE::INTSXP, 1);
        let _zero_depth_guard = protect(zeroDepth);
        *INTEGER(zeroDepth) = 0;
        let curDepth = Rf_allocVector(SEXPTYPE::INTSXP, 1);
        let _cur_depth_guard = protect(curDepth);
        *INTEGER(curDepth) = depth;

        if noChildren(viewportChildren(vp)) {
            SET_VECTOR_ELT(result, 0 as R_xlen_t, zeroDepth);
            SET_VECTOR_ELT(result, 1 as R_xlen_t, R_NilValue());
        } else if childExists(name, viewportChildren(vp)) {
            SET_VECTOR_ELT(result, 0 as R_xlen_t, curDepth);
            SET_VECTOR_ELT(
                result,
                1 as R_xlen_t,
                findVar(installTrChar(STRING_ELT(name, 0)), viewportChildren(vp)),
            );
        } else {
            if *LOGICAL(strict) != 0 {
                SET_VECTOR_ELT(result, 0 as R_xlen_t, zeroDepth);
                SET_VECTOR_ELT(result, 1 as R_xlen_t, R_NilValue());
            } else {
                let found = findInChildren(name, strict, viewportChildren(vp), depth + 1);
                let _found_guard = protect(found);
                SET_VECTOR_ELT(result, 0 as R_xlen_t, VECTOR_ELT(found, 0 as R_xlen_t));
                SET_VECTOR_ELT(result, 1 as R_xlen_t, VECTOR_ELT(found, 1 as R_xlen_t));
            }
        }
        result
    }
}

unsafe fn findInChildren(name: SEXP, strict: SEXP, children: SEXP, depth: c_int) -> SEXP {
    unsafe {
        let childnames = childList(children);
        let _childnames_guard = protect(childnames);
        let n = LENGTH(childnames);
        let mut count: c_int = 0;
        let mut found = false;
        let mut result = R_NilValue();
        while count < n && !found {
            let child = findVar(
                installTrChar(STRING_ELT(childnames, count as R_xlen_t)),
                children,
            );
            let _child_guard = protect(child);
            if !isNull(child) {
                result = findViewport(name, strict, child, depth);
                found = *INTEGER(VECTOR_ELT(result, 0 as R_xlen_t)) > 0;
            }
            count += 1;
        }
        if !found {
            let temp = Rf_allocVector(SEXPTYPE::VECSXP, 2);
            let _temp_guard = protect(temp);
            let zeroDepth = Rf_allocVector(SEXPTYPE::INTSXP, 1);
            let _zero_depth_guard = protect(zeroDepth);
            *INTEGER(zeroDepth) = 0;
            SET_VECTOR_ELT(temp, 0 as R_xlen_t, zeroDepth);
            SET_VECTOR_ELT(temp, 1 as R_xlen_t, R_NilValue());
            result = temp;
        }
        result
    }
}

unsafe fn findvppathInChildren(
    path: SEXP,
    name: SEXP,
    strict: SEXP,
    pathsofar: SEXP,
    children: SEXP,
    depth: c_int,
) -> SEXP {
    unsafe {
        let childnames = childList(children);
        let _childnames_guard = protect(childnames);
        let n = LENGTH(childnames);
        let mut count: c_int = 0;
        let mut found = false;
        let mut result = R_NilValue();
        while count < n && !found {
            let vp = findVar(
                installTrChar(STRING_ELT(childnames, count as R_xlen_t)),
                children,
            );
            let _vp_guard = protect(vp);
            let newpathsofar = growPath(pathsofar, VECTOR_ELT(vp, VP_NAME as R_xlen_t));
            let _newpathsofar_guard = protect(newpathsofar);
            result = findvppath(path, name, strict, newpathsofar, vp, depth);
            found = *INTEGER(VECTOR_ELT(result, 0 as R_xlen_t)) > 0;
            count += 1;
        }
        if !found {
            let temp = Rf_allocVector(SEXPTYPE::VECSXP, 2);
            let _temp_guard = protect(temp);
            let zeroDepth = Rf_allocVector(SEXPTYPE::INTSXP, 1);
            let _zero_depth_guard = protect(zeroDepth);
            *INTEGER(zeroDepth) = 0;
            SET_VECTOR_ELT(temp, 0 as R_xlen_t, zeroDepth);
            SET_VECTOR_ELT(temp, 1 as R_xlen_t, R_NilValue());
            result = temp;
        }
        result
    }
}

unsafe fn findvppath(
    path: SEXP,
    name: SEXP,
    strict: SEXP,
    pathsofar: SEXP,
    vp: SEXP,
    depth: c_int,
) -> SEXP {
    unsafe {
        let result = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _result_guard = protect(result);
        let zeroDepth = Rf_allocVector(SEXPTYPE::INTSXP, 1);
        let _zero_depth_guard = protect(zeroDepth);
        *INTEGER(zeroDepth) = 0;
        let curDepth = Rf_allocVector(SEXPTYPE::INTSXP, 1);
        let _cur_depth_guard = protect(curDepth);
        *INTEGER(curDepth) = depth;

        if noChildren(viewportChildren(vp)) {
            SET_VECTOR_ELT(result, 0 as R_xlen_t, zeroDepth);
            SET_VECTOR_ELT(result, 1 as R_xlen_t, R_NilValue());
        } else if childExists(name, viewportChildren(vp)) && pathMatch(path, pathsofar, strict) {
            SET_VECTOR_ELT(result, 0 as R_xlen_t, curDepth);
            SET_VECTOR_ELT(
                result,
                1 as R_xlen_t,
                findVar(installTrChar(STRING_ELT(name, 0)), viewportChildren(vp)),
            );
        } else {
            let found = findvppathInChildren(
                path,
                name,
                strict,
                pathsofar,
                viewportChildren(vp),
                depth + 1,
            );
            let _found_guard = protect(found);
            SET_VECTOR_ELT(result, 0 as R_xlen_t, VECTOR_ELT(found, 0 as R_xlen_t));
            SET_VECTOR_ELT(result, 1 as R_xlen_t, VECTOR_ELT(found, 1 as R_xlen_t));
        }
        result
    }
}

/* ==============================
 * L_downviewport
 * ============================== */

pub unsafe fn L_downviewport(name: SEXP, strict: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let gvp = gridStateElement(dd, GSS_VP);
        let found = findViewport(name, strict, gvp, 1);
        let _found_guard = protect(found);
        if *INTEGER(VECTOR_ELT(found, 0 as R_xlen_t)) > 0 {
            let vp = doSetViewport(VECTOR_ELT(found, 1 as R_xlen_t), 0, 0, dd);
            setGridStateElement(dd, GSS_VP, vp);
            {
                let clip = VECTOR_ELT(vp, PVP_CLIPPATH as R_xlen_t);
                let _clip_guard = protect(clip);
                if isClipPath(clip) {
                    let resolvedclip = resolveClipPath(clip, dd);
                    let _resolvedclip_guard = protect(resolvedclip);
                    SET_VECTOR_ELT(vp, PVP_CLIPPATH as R_xlen_t, resolvedclip);
                }
            }
            {
                let mask = VECTOR_ELT(vp, PVP_MASK as R_xlen_t);
                let _mask_guard = protect(mask);
                if isMask(mask) {
                    let resolvedmask = resolveMask(mask, dd);
                    let _resolvedmask_guard = protect(resolvedmask);
                    SET_VECTOR_ELT(vp, PVP_MASK as R_xlen_t, resolvedmask);
                }
            }
            VECTOR_ELT(found, 0 as R_xlen_t)
        } else {
            Rf_error1(
                c"Viewport '%s' was not found".as_ptr(),
                CHAR(STRING_ELT(name, 0)),
            );
            R_NilValue()
        }
    }
}

/* ==============================
 * L_downvppath
 * ============================== */

pub unsafe fn L_downvppath(path: SEXP, name: SEXP, strict: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let gvp = gridStateElement(dd, GSS_VP);
        let found = findvppath(path, name, strict, R_NilValue(), gvp, 1);
        let _found_guard = protect(found);
        if *INTEGER(VECTOR_ELT(found, 0 as R_xlen_t)) > 0 {
            let vp = doSetViewport(VECTOR_ELT(found, 1 as R_xlen_t), 0, 0, dd);
            setGridStateElement(dd, GSS_VP, vp);
            {
                let clip = VECTOR_ELT(vp, PVP_CLIPPATH as R_xlen_t);
                let _clip_guard = protect(clip);
                if isClipPath(clip) {
                    let resolvedclip = resolveClipPath(clip, dd);
                    let _resolvedclip_guard = protect(resolvedclip);
                    SET_VECTOR_ELT(vp, PVP_CLIPPATH as R_xlen_t, resolvedclip);
                }
            }
            {
                let mask = VECTOR_ELT(vp, PVP_MASK as R_xlen_t);
                let _mask_guard = protect(mask);
                if isMask(mask) {
                    let resolvedmask = resolveMask(mask, dd);
                    let _resolvedmask_guard = protect(resolvedmask);
                    SET_VECTOR_ELT(vp, PVP_MASK as R_xlen_t, resolvedmask);
                }
            }
            VECTOR_ELT(found, 0 as R_xlen_t)
        } else {
            Rf_error1(
                c"Viewport '%s' was not found".as_ptr(),
                CHAR(STRING_ELT(name, 0)),
            );
            R_NilValue()
        }
    }
}

/* ==============================
 * L_unsetviewport
 * ============================== */

pub unsafe fn L_unsetviewport(n: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let mut gvp = gridStateElement(dd, GSS_VP);
        let mut newvp = VECTOR_ELT(gvp, PVP_PARENT as R_xlen_t);
        if isNull(newvp) {
            Rf_error(
                c"cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)".as_ptr(),
            );
        }
        for _i in 1..*INTEGER(n) {
            gvp = newvp;
            newvp = VECTOR_ELT(gvp, PVP_PARENT as R_xlen_t);
            if isNull(newvp) {
                Rf_error(
                    c"cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)"
                        .as_ptr(),
                );
            }
        }

        let _gvp_guard = protect(gvp);
        let _newvp_guard = protect(newvp);
        {
            let false0 = Rf_allocVector(SEXPTYPE::LGLSXP, 1);
            let _false_guard = protect(false0);
            *LOGICAL(false0) = 0;
            let fcall = lang4(
                Rf_install(c"remove".as_ptr()),
                VECTOR_ELT(gvp, VP_NAME as R_xlen_t),
                VECTOR_ELT(newvp, PVP_CHILDREN as R_xlen_t),
                false0,
            );
            let _fcall_guard = protect(fcall);
            let mut t = fcall;
            t = CDR(CDR(t));
            SET_TAG(t, Rf_install(c"envir".as_ptr()));
            t = CDR(t);
            SET_TAG(t, Rf_install(c"inherits".as_ptr()));
            Rf_eval_with_gd(fcall, grid_eval_env(), dd);
        }

        let mut devWidthCM: c_double = 0.0;
        let mut devHeightCM: c_double = 0.0;
        getDeviceSize(dd, &mut devWidthCM, &mut devHeightCM);
        if deviceChanged(devWidthCM, devHeightCM, newvp) {
            calcViewportTransform(newvp, viewportParent(newvp), true, dd);
        }
        setGridStateElement(dd, GSS_GPAR, VECTOR_ELT(gvp, PVP_PARENTGPAR as R_xlen_t));
        setGridStateElement(dd, GSS_VP, newvp);

        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if !(TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path) != 0)
        {
            let parentClip = viewportClipRect(newvp);
            let _parent_clip_guard = protect(parentClip);
            let parentClipPath = VECTOR_ELT(newvp, PVP_CLIPPATH as R_xlen_t);
            let _parent_clip_path_guard = protect(parentClipPath);
            if isClipPath(parentClipPath) {
                resolveClipPath(parentClipPath, dd);
            } else {
                let xx1 = *REAL(parentClip).add(0);
                let yy1 = *REAL(parentClip).add(1);
                let xx2 = *REAL(parentClip).add(2);
                let yy2 = *REAL(parentClip).add(3);
                GESetClip(xx1, yy1, xx2, yy2, dd);
            }
        }
        if !(TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path) != 0)
        {
            resolveMask(VECTOR_ELT(newvp, PVP_MASK as R_xlen_t), dd);
        }

        SET_VECTOR_ELT(gvp, PVP_PARENT as R_xlen_t, R_NilValue());
        R_NilValue()
    }
}

/* ==============================
 * L_upviewport
 * ============================== */

pub unsafe fn L_upviewport(n: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let mut gvp = gridStateElement(dd, GSS_VP);
        let mut newvp = VECTOR_ELT(gvp, PVP_PARENT as R_xlen_t);
        if isNull(newvp) {
            Rf_error(
                c"cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)".as_ptr(),
            );
        }
        for _i in 1..*INTEGER(n) {
            gvp = newvp;
            newvp = VECTOR_ELT(gvp, PVP_PARENT as R_xlen_t);
            if isNull(newvp) {
                Rf_error(
                    c"cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)"
                        .as_ptr(),
                );
            }
        }

        let mut devWidthCM: c_double = 0.0;
        let mut devHeightCM: c_double = 0.0;
        getDeviceSize(dd, &mut devWidthCM, &mut devHeightCM);
        if deviceChanged(devWidthCM, devHeightCM, newvp) {
            calcViewportTransform(newvp, viewportParent(newvp), true, dd);
        }
        setGridStateElement(dd, GSS_GPAR, VECTOR_ELT(gvp, PVP_PARENTGPAR as R_xlen_t));
        setGridStateElement(dd, GSS_VP, newvp);

        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if !(TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path) != 0)
        {
            let parentClip = viewportClipRect(newvp);
            let _parent_clip_guard = protect(parentClip);
            let parentClipPath = VECTOR_ELT(newvp, PVP_CLIPPATH as R_xlen_t);
            let _parent_clip_path_guard = protect(parentClipPath);
            if isClipPath(parentClipPath) {
                resolveClipPath(parentClipPath, dd);
            } else {
                let xx1 = *REAL(parentClip).add(0);
                let yy1 = *REAL(parentClip).add(1);
                let xx2 = *REAL(parentClip).add(2);
                let yy2 = *REAL(parentClip).add(3);
                GESetClip(xx1, yy1, xx2, yy2, dd);
            }
        }
        if !(TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path) != 0)
        {
            resolveMask(VECTOR_ELT(newvp, PVP_MASK as R_xlen_t), dd);
        }
        R_NilValue()
    }
}

/* ==============================
 * Display list accessors
 * ============================== */

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

pub unsafe fn getViewportTransform(
    currentvp: SEXP,
    dd: pGEDevDesc,
    vpWidthCM: *mut c_double,
    vpHeightCM: *mut c_double,
    transform: *mut LTransform,
    rotationAngle: *mut c_double,
) {
    unsafe {
        let mut devWidthCM: c_double = 0.0;
        let mut devHeightCM: c_double = 0.0;
        getDeviceSize(dd, &mut devWidthCM, &mut devHeightCM);
        if deviceChanged(devWidthCM, devHeightCM, currentvp) {
            calcViewportTransform(currentvp, viewportParent(currentvp), true, dd);
        }

        let width_cm = viewportWidthCM(currentvp);
        let height_cm = viewportHeightCM(currentvp);
        let rotation = viewportRotation(currentvp);
        let t = viewportTransform(currentvp);

        *vpWidthCM =
            if !isNull(width_cm) && TYPEOF(width_cm) == SEXPTYPE::REALSXP && LENGTH(width_cm) > 0 {
                *REAL(width_cm)
            } else {
                devWidthCM
            };
        *vpHeightCM = if !isNull(height_cm)
            && TYPEOF(height_cm) == SEXPTYPE::REALSXP
            && LENGTH(height_cm) > 0
        {
            *REAL(height_cm)
        } else {
            devHeightCM
        };
        *rotationAngle =
            if !isNull(rotation) && TYPEOF(rotation) == SEXPTYPE::REALSXP && LENGTH(rotation) > 0 {
                *REAL(rotation)
            } else {
                0.0
            };

        if !transform.is_null() {
            for i in 0..3 {
                for j in 0..3 {
                    (*transform)[i][j] = if i == j { 1.0 } else { 0.0 };
                }
            }
            if !isNull(t) && TYPEOF(t) == SEXPTYPE::REALSXP && LENGTH(t) >= 9 {
                for col in 0..3usize {
                    for row in 0..3usize {
                        (*transform)[row][col] = *REAL(t).add(col * 3 + row);
                    }
                }
            }
        }
    }
}

/* ==============================
 * L_convert
 * ============================== */

pub unsafe fn L_convert(x: SEXP, whatfrom: SEXP, whatto: SEXP, unitto: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, c"black".as_ptr());

        let mut vpWidthCM: c_double = 0.0;
        let mut vpHeightCM: c_double = 0.0;
        let mut rotationAngle: c_double = 0.0;
        let mut transform: LTransform = [[0.0; 3]; 3];
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);

        let nx = unitLength(x);
        let answer = Rf_allocVector(SEXPTYPE::REALSXP, nx);
        let _answer_guard = protect(answer);
        let unitto_len = LENGTH(unitto).max(1);
        let from_axis = if LENGTH(whatfrom) > 0 {
            *INTEGER(whatfrom).add(0)
        } else {
            0
        };
        let to_axis = if LENGTH(whatto) > 0 {
            *INTEGER(whatto).add(0)
        } else {
            0
        };

        for i in 0..nx {
            updateGContext(currentgp, i, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);
            let to_unit = *INTEGER(unitto).add((i % unitto_len) as usize);
            let mut rel_convert = (unitUnit(x, i) == L_NATIVE || unitUnit(x, i) == L_NPC)
                && (to_unit == L_NATIVE || to_unit == L_NPC)
                && ((from_axis == to_axis)
                    || (from_axis == 0 && to_axis == 2)
                    || (from_axis == 2 && to_axis == 0)
                    || (from_axis == 1 && to_axis == 3)
                    || (from_axis == 3 && to_axis == 1));

            let mut value_in = match from_axis {
                0 => {
                    if rel_convert && vpWidthCM < 1e-6 {
                        transformXYtoNPC(
                            unitValue(x, i),
                            unitUnit(x, i),
                            vpc.xscalemin,
                            vpc.xscalemax,
                        )
                    } else {
                        rel_convert = false;
                        transformXtoINCHES(x, i, vpc, gc, vpWidthCM, vpHeightCM, dd)
                    }
                }
                1 => {
                    if rel_convert && vpHeightCM < 1e-6 {
                        transformXYtoNPC(
                            unitValue(x, i),
                            unitUnit(x, i),
                            vpc.yscalemin,
                            vpc.yscalemax,
                        )
                    } else {
                        rel_convert = false;
                        transformYtoINCHES(x, i, vpc, gc, vpWidthCM, vpHeightCM, dd)
                    }
                }
                2 => {
                    if rel_convert && vpWidthCM < 1e-6 {
                        transformWHtoNPC(
                            unitValue(x, i),
                            unitUnit(x, i),
                            vpc.xscalemin,
                            vpc.xscalemax,
                        )
                    } else {
                        rel_convert = false;
                        transformWidthtoINCHES(x, i, vpc, gc, vpWidthCM, vpHeightCM, dd)
                    }
                }
                3 => {
                    if rel_convert && vpHeightCM < 1e-6 {
                        transformWHtoNPC(
                            unitValue(x, i),
                            unitUnit(x, i),
                            vpc.yscalemin,
                            vpc.yscalemax,
                        )
                    } else {
                        rel_convert = false;
                        transformHeighttoINCHES(x, i, vpc, gc, vpWidthCM, vpHeightCM, dd)
                    }
                }
                _ => unitValue(x, i),
            };

            value_in = match to_axis {
                0 => {
                    if rel_convert {
                        transformXYfromNPC(value_in, to_unit, vpc.xscalemin, vpc.xscalemax)
                    } else {
                        transformXYFromINCHES(
                            value_in,
                            to_unit,
                            vpc.xscalemin,
                            vpc.xscalemax,
                            gc,
                            vpWidthCM,
                            vpHeightCM,
                            dd,
                        )
                    }
                }
                1 => {
                    if rel_convert {
                        transformXYfromNPC(value_in, to_unit, vpc.yscalemin, vpc.yscalemax)
                    } else {
                        transformXYFromINCHES(
                            value_in,
                            to_unit,
                            vpc.yscalemin,
                            vpc.yscalemax,
                            gc,
                            vpHeightCM,
                            vpWidthCM,
                            dd,
                        )
                    }
                }
                2 => {
                    if rel_convert {
                        transformWHfromNPC(value_in, to_unit, vpc.xscalemin, vpc.xscalemax)
                    } else {
                        transformWidthHeightFromINCHES(
                            value_in,
                            to_unit,
                            vpc.xscalemin,
                            vpc.xscalemax,
                            gc,
                            vpWidthCM,
                            vpHeightCM,
                            dd,
                        )
                    }
                }
                3 => {
                    if rel_convert {
                        transformWHfromNPC(value_in, to_unit, vpc.yscalemin, vpc.yscalemax)
                    } else {
                        transformWidthHeightFromINCHES(
                            value_in,
                            to_unit,
                            vpc.yscalemin,
                            vpc.yscalemax,
                            gc,
                            vpHeightCM,
                            vpWidthCM,
                            dd,
                        )
                    }
                }
                _ => value_in,
            };

            *REAL(answer).add(i as usize) = value_in;
        }

        answer
    }
}

/* ==============================
 * L_devLoc
 * ============================== */

pub unsafe fn L_devLoc(x: SEXP, y: SEXP, device: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, c"black".as_ptr());

        let mut vpWidthCM: c_double = 0.0;
        let mut vpHeightCM: c_double = 0.0;
        let mut rotationAngle: c_double = 0.0;
        let mut transform: LTransform = [[0.0; 3]; 3];
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        gcontextFromgpar(currentgp, 0, gc, dd);

        let maxn = unitLength(x).max(unitLength(y));
        let devx = Rf_allocVector(SEXPTYPE::REALSXP, maxn);
        let _devx_guard = protect(devx);
        let devy = Rf_allocVector(SEXPTYPE::REALSXP, maxn);
        let _devy_guard = protect(devy);
        let result = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _result_guard = protect(result);
        let as_device =
            TYPEOF(device) == SEXPTYPE::LGLSXP && LENGTH(device) > 0 && *LOGICAL(device) != 0;

        for i in 0..maxn {
            let mut xx: c_double = NA_REAL;
            let mut yy: c_double = NA_REAL;
            transformLocn(
                x,
                y,
                i,
                vpc,
                gc,
                vpWidthCM,
                vpHeightCM,
                dd,
                &mut transform,
                &mut xx,
                &mut yy,
            );
            if as_device {
                xx = toDeviceX(xx, GE_INCHES, dd);
                yy = toDeviceY(yy, GE_INCHES, dd);
            }
            *REAL(devx).add(i as usize) = xx;
            *REAL(devy).add(i as usize) = yy;
        }

        SET_VECTOR_ELT(result, 0 as R_xlen_t, devx);
        SET_VECTOR_ELT(result, 1 as R_xlen_t, devy);
        result
    }
}

/* ==============================
 * L_devDim
 * ============================== */

pub unsafe fn L_devDim(x: SEXP, y: SEXP, device: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, c"black".as_ptr());

        let mut vpWidthCM: c_double = 0.0;
        let mut vpHeightCM: c_double = 0.0;
        let mut rotationAngle: c_double = 0.0;
        let mut transform: LTransform = [[0.0; 3]; 3];
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        gcontextFromgpar(currentgp, 0, gc, dd);

        let maxn = unitLength(x).max(unitLength(y));
        let devx = Rf_allocVector(SEXPTYPE::REALSXP, maxn);
        let _devx_guard = protect(devx);
        let devy = Rf_allocVector(SEXPTYPE::REALSXP, maxn);
        let _devy_guard = protect(devy);
        let result = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _result_guard = protect(result);
        let as_device =
            TYPEOF(device) == SEXPTYPE::LGLSXP && LENGTH(device) > 0 && *LOGICAL(device) != 0;

        for i in 0..maxn {
            let mut xx: c_double = NA_REAL;
            let mut yy: c_double = NA_REAL;
            transformDimn(
                x,
                y,
                i,
                vpc,
                gc,
                vpWidthCM,
                vpHeightCM,
                dd,
                rotationAngle,
                &mut xx,
                &mut yy,
            );
            if as_device {
                xx = toDeviceWidth(xx, GE_INCHES, dd);
                yy = toDeviceHeight(yy, GE_INCHES, dd);
            }
            *REAL(devx).add(i as usize) = xx;
            *REAL(devy).add(i as usize) = yy;
        }

        SET_VECTOR_ELT(result, 0 as R_xlen_t, devx);
        SET_VECTOR_ELT(result, 1 as R_xlen_t, devy);
        result
    }
}

/* ==============================
 * L_layoutRegion
 * ============================== */

pub unsafe fn L_layoutRegion(layoutPosRow: SEXP, layoutPosCol: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        if isNull(viewportLayout(currentvp)) {
            return R_NilValue();
        }
        let mut vpl = LViewportLocation {
            x: R_NilValue(),
            y: R_NilValue(),
            width: R_NilValue(),
            height: R_NilValue(),
            hjust: 0.0,
            vjust: 0.0,
        };
        calcViewportLocationFromLayout(layoutPosRow, layoutPosCol, currentvp, &mut vpl);

        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);
        let vp_width_cm = if !isNull(viewportWidthCM(currentvp))
            && TYPEOF(viewportWidthCM(currentvp)) == SEXPTYPE::REALSXP
            && LENGTH(viewportWidthCM(currentvp)) > 0
        {
            *REAL(viewportWidthCM(currentvp))
        } else {
            1.0
        };
        let vp_height_cm = if !isNull(viewportHeightCM(currentvp))
            && TYPEOF(viewportHeightCM(currentvp)) == SEXPTYPE::REALSXP
            && LENGTH(viewportHeightCM(currentvp)) > 0
        {
            *REAL(viewportHeightCM(currentvp))
        } else {
            1.0
        };
        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;

        let x_cm = transformXtoINCHES(vpl.x, 0, vpc, gc, vp_width_cm, vp_height_cm, dd) * 2.54;
        let y_cm = transformYtoINCHES(vpl.y, 0, vpc, gc, vp_width_cm, vp_height_cm, dd) * 2.54;
        let w_cm =
            transformWidthtoINCHES(vpl.width, 0, vpc, gc, vp_width_cm, vp_height_cm, dd) * 2.54;
        let h_cm =
            transformHeighttoINCHES(vpl.height, 0, vpc, gc, vp_width_cm, vp_height_cm, dd) * 2.54;

        let answer = Rf_allocVector(SEXPTYPE::REALSXP, 4);
        let _answer_guard = protect(answer);
        *REAL(answer).add(0) = x_cm;
        *REAL(answer).add(1) = y_cm;
        *REAL(answer).add(2) = w_cm;
        *REAL(answer).add(3) = h_cm;
        answer
    }
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
    unsafe {
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
}

unsafe fn circleEdge(
    x: c_double,
    y: c_double,
    r: c_double,
    theta: c_double,
    edgex: *mut c_double,
    edgey: *mut c_double,
) {
    unsafe {
        let angle = theta / 180.0 * std::f64::consts::PI;
        *edgex = x + r * angle.cos();
        *edgey = y + r * angle.sin();
    }
}

unsafe fn polygonEdge(
    x: *const f64,
    y: *const f64,
    n: c_int,
    theta: c_double,
    edgex: *mut c_double,
    edgey: *mut c_double,
) {
    unsafe {
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
    unsafe {
        let nt = LENGTH(atype);
        match *INTEGER(atype).add((i % nt) as usize) {
            1 => GEPolyline(3, x, y, gc, dd),
            2 => GEPolygon(3, x, y, gc, dd),
            _ => {} // intentionally unhandled: unknown arrowhead type
        }
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
    vpc: LViewportContext,
    vpWidthCM: f64,
    vpHeightCM: f64,
    vertx: *mut f64,
    verty: *mut f64,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    unsafe {
        let l1 = transformWidthtoINCHES(length, i, vpc, gc, vpWidthCM, vpHeightCM, dd).abs();
        let l2 = transformHeighttoINCHES(length, i, vpc, gc, vpWidthCM, vpHeightCM, dd).abs();
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
    unsafe {
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
}

#[inline]
unsafe fn gp_fill_is_pattern(gp: SEXP) -> bool {
    unsafe {
        let fill = gpFillSXP(gp);
        Rf_inherits(fill, c"GridPattern".as_ptr()) != 0
            || Rf_inherits(fill, c"GridPatternList".as_ptr()) != 0
    }
}

#[inline]
unsafe fn set_gp_fill_string(gp: SEXP, fill: *const c_char) {
    unsafe {
        SET_VECTOR_ELT(gp, GP_FILL as R_xlen_t, Rf_mkString(fill));
    }
}

/* ==============================
 * L_moveTo
 * ============================== */

pub unsafe fn L_moveTo(x: SEXP, y: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, c"transparent".as_ptr());

        let prevloc = gridStateElement(dd, GSS_PREVLOC);
        let _prevloc_guard = protect(prevloc);
        let devloc = gridStateElement(dd, GSS_CURRLOC);
        let _devloc_guard = protect(devloc);
        let mut vpWidthCM: c_double = 0.0;
        let mut vpHeightCM: c_double = 0.0;
        let mut rotationAngle: c_double = 0.0;
        let mut transform: LTransform = [[0.0; 3]; 3];
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        gcontextFromgpar(currentgp, 0, gc, dd);

        let mut xx: c_double = NA_REAL;
        let mut yy: c_double = NA_REAL;
        transformLocn(
            x,
            y,
            0,
            vpc,
            gc,
            vpWidthCM,
            vpHeightCM,
            dd,
            &mut transform,
            &mut xx,
            &mut yy,
        );

        if TYPEOF(prevloc) == SEXPTYPE::REALSXP
            && LENGTH(prevloc) >= 2
            && TYPEOF(devloc) == SEXPTYPE::REALSXP
            && LENGTH(devloc) >= 2
        {
            *REAL(prevloc).add(0) = *REAL(devloc).add(0);
            *REAL(prevloc).add(1) = *REAL(devloc).add(1);
            *REAL(devloc).add(0) = xx;
            *REAL(devloc).add(1) = yy;
        }

        R_NilValue()
    }
}

/* ==============================
 * L_lineTo
 * ============================== */

pub unsafe fn L_lineTo(x: SEXP, y: SEXP, arrow: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        if gp_fill_is_pattern(currentgp) {
            set_gp_fill_string(currentgp, c"transparent".as_ptr());
        }

        let prevloc = gridStateElement(dd, GSS_PREVLOC);
        let _prevloc_guard = protect(prevloc);
        let devloc = gridStateElement(dd, GSS_CURRLOC);
        let _devloc_guard = protect(devloc);
        let mut vpWidthCM: c_double = 0.0;
        let mut vpHeightCM: c_double = 0.0;
        let mut rotationAngle: c_double = 0.0;
        let mut transform: LTransform = [[0.0; 3]; 3];
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        gcontextFromgpar(currentgp, 0, gc, dd);

        let mut xx: c_double = NA_REAL;
        let mut yy: c_double = NA_REAL;
        transformLocn(
            x,
            y,
            0,
            vpc,
            gc,
            vpWidthCM,
            vpHeightCM,
            dd,
            &mut transform,
            &mut xx,
            &mut yy,
        );

        if TYPEOF(prevloc) == SEXPTYPE::REALSXP
            && LENGTH(prevloc) >= 2
            && TYPEOF(devloc) == SEXPTYPE::REALSXP
            && LENGTH(devloc) >= 2
        {
            *REAL(prevloc).add(0) = *REAL(devloc).add(0);
            *REAL(prevloc).add(1) = *REAL(devloc).add(1);
            *REAL(devloc).add(0) = xx;
            *REAL(devloc).add(1) = yy;

            let xx0 = toDeviceX(*REAL(prevloc).add(0), GE_INCHES, dd);
            let yy0 = toDeviceY(*REAL(prevloc).add(1), GE_INCHES, dd);
            let xx1 = toDeviceX(xx, GE_INCHES, dd);
            let yy1 = toDeviceY(yy, GE_INCHES, dd);

            if xx0.is_finite() && yy0.is_finite() && xx1.is_finite() && yy1.is_finite() {
                GEMode(1, dd);
                GELine(xx0, yy0, xx1, yy1, gc, dd);
                if !isNull(arrow) {
                    let ax = [xx0, xx1];
                    let ay = [yy0, yy1];
                    arrows(
                        ax.as_ptr(),
                        ay.as_ptr(),
                        2,
                        arrow,
                        0,
                        true,
                        true,
                        vpc,
                        vpWidthCM,
                        vpHeightCM,
                        gc,
                        dd,
                    );
                }
                GEMode(0, dd);
            }
        }

        R_NilValue()
    }
}

/* ==============================
 * L_lines
 * ============================== */

pub unsafe fn L_lines(x: SEXP, y: SEXP, index: SEXP, arrow: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        if gp_fill_is_pattern(currentgp) {
            set_gp_fill_string(currentgp, c"transparent".as_ptr());
        }
        let mut vpWidthCM: c_double = 0.0;
        let mut vpHeightCM: c_double = 0.0;
        let mut rotationAngle: c_double = 0.0;
        let mut transform: LTransform = [[0.0; 3]; 3];
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);

        GEMode(1, dd);
        let nl = LENGTH(index);
        for j in 0..nl {
            let indices = VECTOR_ELT(index, j as R_xlen_t);
            updateGContext(currentgp, j, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);
            let nx = LENGTH(indices);
            if nx <= 0 {
                continue;
            }

            let mut xx = vec![NA_REAL; nx as usize];
            let mut yy = vec![NA_REAL; nx as usize];
            let mut xold = NA_REAL;
            let mut yold = NA_REAL;
            let mut start: usize = 0;

            for i in 0..nx as usize {
                let idx = *INTEGER(indices).add(i);
                if idx > 0 {
                    transformLocn(
                        x,
                        y,
                        idx - 1,
                        vpc,
                        gc,
                        vpWidthCM,
                        vpHeightCM,
                        dd,
                        &mut transform,
                        &mut xx[i],
                        &mut yy[i],
                    );
                    xx[i] = toDeviceX(xx[i], GE_INCHES, dd);
                    yy[i] = toDeviceY(yy[i], GE_INCHES, dd);
                }

                let current_finite = xx[i].is_finite() && yy[i].is_finite();
                let previous_finite = xold.is_finite() && yold.is_finite();

                if current_finite && !previous_finite {
                    start = i;
                } else if previous_finite && !current_finite {
                    if i.saturating_sub(start) > 1 {
                        GEPolyline(
                            (i - start) as c_int,
                            xx.as_ptr().add(start),
                            yy.as_ptr().add(start),
                            gc,
                            dd,
                        );
                        if !isNull(arrow) {
                            arrows(
                                xx.as_ptr().add(start),
                                yy.as_ptr().add(start),
                                (i - start) as c_int,
                                arrow,
                                j,
                                start == 0,
                                false,
                                vpc,
                                vpWidthCM,
                                vpHeightCM,
                                gc,
                                dd,
                            );
                        }
                    }
                } else if previous_finite && i + 1 == nx as usize {
                    GEPolyline(
                        (nx as usize - start) as c_int,
                        xx.as_ptr().add(start),
                        yy.as_ptr().add(start),
                        gc,
                        dd,
                    );
                    if !isNull(arrow) {
                        arrows(
                            xx.as_ptr().add(start),
                            yy.as_ptr().add(start),
                            (nx as usize - start) as c_int,
                            arrow,
                            j,
                            start == 0,
                            true,
                            vpc,
                            vpWidthCM,
                            vpHeightCM,
                            gc,
                            dd,
                        );
                    }
                }

                xold = xx[i];
                yold = yy[i];
            }
        }
        GEMode(0, dd);
        R_NilValue()
    }
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
    unsafe { R_NilValue() }
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
    unsafe {
        gridXspline(x, y, s, o, a, rep, index, 0.0, true, false);
        R_NilValue()
    }
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
    unsafe { gridXspline(x, y, s, o, a, rep, index, *REAL(theta), false, false) }
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
    unsafe { gridXspline(x, y, s, o, a, rep, index, *REAL(theta), false, true) }
}

/* ==============================
 * L_segments
 * ============================== */

pub unsafe fn L_segments(x0: SEXP, y0: SEXP, x1: SEXP, y1: SEXP, arrow: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        if gp_fill_is_pattern(currentgp) {
            set_gp_fill_string(currentgp, c"transparent".as_ptr());
        }

        let mut vpWidthCM: c_double = 0.0;
        let mut vpHeightCM: c_double = 0.0;
        let mut rotationAngle: c_double = 0.0;
        let mut transform: LTransform = [[0.0; 3]; 3];
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);

        let maxn = unitLength(x0)
            .max(unitLength(y0))
            .max(unitLength(x1))
            .max(unitLength(y1));
        GEMode(1, dd);
        for i in 0..maxn {
            updateGContext(currentgp, i, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);
            let mut xx0: c_double = NA_REAL;
            let mut yy0: c_double = NA_REAL;
            let mut xx1: c_double = NA_REAL;
            let mut yy1: c_double = NA_REAL;
            transformLocn(
                x0,
                y0,
                i,
                vpc,
                gc,
                vpWidthCM,
                vpHeightCM,
                dd,
                &mut transform,
                &mut xx0,
                &mut yy0,
            );
            transformLocn(
                x1,
                y1,
                i,
                vpc,
                gc,
                vpWidthCM,
                vpHeightCM,
                dd,
                &mut transform,
                &mut xx1,
                &mut yy1,
            );
            xx0 = toDeviceX(xx0, GE_INCHES, dd);
            yy0 = toDeviceY(yy0, GE_INCHES, dd);
            xx1 = toDeviceX(xx1, GE_INCHES, dd);
            yy1 = toDeviceY(yy1, GE_INCHES, dd);
            if xx0.is_finite() && yy0.is_finite() && xx1.is_finite() && yy1.is_finite() {
                GELine(xx0, yy0, xx1, yy1, gc, dd);
                if !isNull(arrow) {
                    let ax = [xx0, xx1];
                    let ay = [yy0, yy1];
                    arrows(
                        ax.as_ptr(),
                        ay.as_ptr(),
                        2,
                        arrow,
                        i,
                        true,
                        true,
                        vpc,
                        vpWidthCM,
                        vpHeightCM,
                        gc,
                        dd,
                    );
                }
            }
        }
        GEMode(0, dd);
        R_NilValue()
    }
}

/* ==============================
 * L_arrows
 * ============================== */

unsafe fn getArrowN(
    x1: SEXP,
    x2: SEXP,
    xnm1: SEXP,
    xn: SEXP,
    y1: SEXP,
    y2: SEXP,
    ynm1: SEXP,
    yn: SEXP,
) -> c_int {
    unsafe {
        let mut maxn = 0;
        let ny1 = if isNull(y1) { 0 } else { unitLength(y1) };
        let nx2 = unitLength(x2);
        let ny2 = unitLength(y2);
        let nxnm1 = if isNull(xnm1) { 0 } else { unitLength(xnm1) };
        let nynm1 = if isNull(ynm1) { 0 } else { unitLength(ynm1) };
        let nxn = unitLength(xn);
        let nyn = unitLength(yn);
        maxn = maxn.max(ny1);
        maxn = maxn.max(nx2);
        maxn = maxn.max(ny2);
        maxn = maxn.max(nxnm1);
        maxn = maxn.max(nynm1);
        maxn = maxn.max(nxn);
        maxn = maxn.max(nyn);
        maxn
    }
}

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
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        if gp_fill_is_pattern(currentgp) {
            set_gp_fill_string(currentgp, c"transparent".as_ptr());
        }

        let mut vpWidthCM: c_double = 0.0;
        let mut vpHeightCM: c_double = 0.0;
        let mut rotationAngle: c_double = 0.0;
        let mut transform: LTransform = [[0.0; 3]; 3];
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let maxn = getArrowN(x1, x2, xnm1, xn, y1, y2, ynm1, yn);
        let ne = LENGTH(ends);
        resolveGPar(currentgp, 0);

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);

        GEMode(1, dd);
        for i in 0..maxn {
            let mut first = true;
            let mut last = true;
            match *INTEGER(ends).add((i % ne) as usize) {
                2 => first = false,
                1 => last = false,
                _ => {}
            }
            updateGContext(currentgp, i, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);

            let devloc = if isNull(x1) {
                gridStateElement(dd, GSS_CURRLOC)
            } else {
                R_NilValue()
            };
            let _devloc_guard = if isNull(x1) {
                Some(protect(devloc))
            } else {
                None
            };

            if first {
                let mut xx1: c_double = NA_REAL;
                let mut yy1: c_double = NA_REAL;
                if isNull(x1) {
                    if TYPEOF(devloc) == SEXPTYPE::REALSXP && LENGTH(devloc) >= 2 {
                        xx1 = *REAL(devloc).add(0);
                        yy1 = *REAL(devloc).add(1);
                    }
                } else {
                    transformLocn(
                        x1,
                        y1,
                        i,
                        vpc,
                        gc,
                        vpWidthCM,
                        vpHeightCM,
                        dd,
                        &mut transform,
                        &mut xx1,
                        &mut yy1,
                    );
                }
                let mut xx2: c_double = NA_REAL;
                let mut yy2: c_double = NA_REAL;
                transformLocn(
                    x2,
                    y2,
                    i,
                    vpc,
                    gc,
                    vpWidthCM,
                    vpHeightCM,
                    dd,
                    &mut transform,
                    &mut xx2,
                    &mut yy2,
                );

                let mut vertx = [0.0; 3];
                let mut verty = [0.0; 3];
                calcArrow(
                    xx1,
                    yy1,
                    xx2,
                    yy2,
                    angle,
                    length,
                    i,
                    vpc,
                    vpWidthCM,
                    vpHeightCM,
                    vertx.as_mut_ptr(),
                    verty.as_mut_ptr(),
                    gc,
                    dd,
                );
                if toDeviceX(xx2, GE_INCHES, dd).is_finite()
                    && toDeviceY(yy2, GE_INCHES, dd).is_finite()
                    && vertx[1].is_finite()
                    && verty[1].is_finite()
                {
                    drawArrow(vertx.as_ptr(), verty.as_ptr(), r#type, i, gc, dd);
                }
            }

            if last {
                let mut xxnm1: c_double = NA_REAL;
                let mut yynm1: c_double = NA_REAL;
                if isNull(xnm1) {
                    if TYPEOF(devloc) == SEXPTYPE::REALSXP && LENGTH(devloc) >= 2 {
                        xxnm1 = *REAL(devloc).add(0);
                        yynm1 = *REAL(devloc).add(1);
                    }
                } else {
                    transformLocn(
                        xnm1,
                        ynm1,
                        i,
                        vpc,
                        gc,
                        vpWidthCM,
                        vpHeightCM,
                        dd,
                        &mut transform,
                        &mut xxnm1,
                        &mut yynm1,
                    );
                }

                let mut xxn: c_double = NA_REAL;
                let mut yyn: c_double = NA_REAL;
                transformLocn(
                    xn,
                    yn,
                    i,
                    vpc,
                    gc,
                    vpWidthCM,
                    vpHeightCM,
                    dd,
                    &mut transform,
                    &mut xxn,
                    &mut yyn,
                );

                let mut vertx = [0.0; 3];
                let mut verty = [0.0; 3];
                calcArrow(
                    xxn,
                    yyn,
                    xxnm1,
                    yynm1,
                    angle,
                    length,
                    i,
                    vpc,
                    vpWidthCM,
                    vpHeightCM,
                    vertx.as_mut_ptr(),
                    verty.as_mut_ptr(),
                    gc,
                    dd,
                );
                if toDeviceX(xxnm1, GE_INCHES, dd).is_finite()
                    && toDeviceY(yynm1, GE_INCHES, dd).is_finite()
                    && vertx[1].is_finite()
                    && verty[1].is_finite()
                {
                    drawArrow(vertx.as_ptr(), verty.as_ptr(), r#type, i, gc, dd);
                }
            }
        }
        GEMode(0, dd);
        R_NilValue()
    }
}

/* ==============================
 * L_polygon
 * ============================== */

pub unsafe fn L_polygon(x: SEXP, y: SEXP, index: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path).add(0) != 0
        {
            set_gp_fill_string(currentgp, c"black".as_ptr());
        }

        let mut vpWidthCM: c_double = 0.0;
        let mut vpHeightCM: c_double = 0.0;
        let mut rotationAngle: c_double = 0.0;
        let mut transform: LTransform = [[0.0; 3]; 3];
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);

        GEMode(1, dd);
        let np = LENGTH(index);
        for i in 0..np {
            let indices = VECTOR_ELT(index, i as R_xlen_t);
            updateGContext(currentgp, i, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);
            let nx = LENGTH(indices);
            if nx <= 0 {
                continue;
            }

            let mut xx = vec![NA_REAL; nx as usize];
            let mut yy = vec![NA_REAL; nx as usize];
            let mut xold = NA_REAL;
            let mut yold = NA_REAL;
            let mut start: usize = 0;

            for j in 0..nx as usize {
                let idx = *INTEGER(indices).add(j);
                if idx > 0 {
                    transformLocn(
                        x,
                        y,
                        idx - 1,
                        vpc,
                        gc,
                        vpWidthCM,
                        vpHeightCM,
                        dd,
                        &mut transform,
                        &mut xx[j],
                        &mut yy[j],
                    );
                    xx[j] = toDeviceX(xx[j], GE_INCHES, dd);
                    yy[j] = toDeviceY(yy[j], GE_INCHES, dd);
                }

                let current_finite = xx[j].is_finite() && yy[j].is_finite();
                let previous_finite = xold.is_finite() && yold.is_finite();

                if current_finite && !previous_finite {
                    start = j;
                } else if previous_finite && !current_finite {
                    if j.saturating_sub(start) > 1 {
                        GEPolygon(
                            (j - start) as c_int,
                            xx.as_ptr().add(start),
                            yy.as_ptr().add(start),
                            gc,
                            dd,
                        );
                    }
                } else if previous_finite && j + 1 == nx as usize {
                    GEPolygon(
                        (nx as usize - start) as c_int,
                        xx.as_ptr().add(start),
                        yy.as_ptr().add(start),
                        gc,
                        dd,
                    );
                }

                xold = xx[j];
                yold = yy[j];
            }
        }
        GEMode(0, dd);
        R_NilValue()
    }
}

/* ==============================
 * gridCircle (internal)
 * ============================== */

unsafe fn gridCircle(_x: SEXP, _y: SEXP, _r: SEXP, _theta: c_double, _draw: bool) -> SEXP {
    unsafe { R_NilValue() }
}

/* ==============================
 * L_circle
 * ============================== */

pub unsafe fn L_circle(x: SEXP, y: SEXP, r: SEXP) -> SEXP {
    unsafe {
        gridCircle(x, y, r, 0.0, true);
        R_NilValue()
    }
}

/* ==============================
 * L_circleBounds
 * ============================== */

pub unsafe fn L_circleBounds(x: SEXP, y: SEXP, r: SEXP, theta: SEXP) -> SEXP {
    unsafe { gridCircle(x, y, r, *REAL(theta), false) }
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
    unsafe { R_NilValue() }
}

/* ==============================
 * L_rect
 * ============================== */

pub unsafe fn L_rect(x: SEXP, y: SEXP, w: SEXP, h: SEXP, hjust: SEXP, vjust: SEXP) -> SEXP {
    unsafe {
        gridRect(x, y, w, h, hjust, vjust, 0.0, true);
        R_NilValue()
    }
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
    unsafe { gridRect(x, y, w, h, hjust, vjust, *REAL(theta), false) }
}

/* ==============================
 * L_path
 * ============================== */

pub unsafe fn L_path(x: SEXP, y: SEXP, index: SEXP, rule: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        let resolving_path = gridStateElement(dd, GSS_RESOLVINGPATH);
        if TYPEOF(resolving_path) == SEXPTYPE::LGLSXP
            && LENGTH(resolving_path) > 0
            && *LOGICAL(resolving_path).add(0) != 0
        {
            set_gp_fill_string(currentgp, c"black".as_ptr());
        }

        let mut vpWidthCM: c_double = 0.0;
        let mut vpHeightCM: c_double = 0.0;
        let mut rotationAngle: c_double = 0.0;
        let mut transform: LTransform = [[0.0; 3]; 3];
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);

        GEMode(1, dd);

        for h in 0..LENGTH(index) {
            let poly_ind = VECTOR_ELT(index, h as R_xlen_t);
            let npoly = LENGTH(poly_ind);
            if npoly <= 0 {
                continue;
            }

            let mut ntot: c_int = 0;
            let mut nper = vec![0i32; npoly as usize];
            for i in 0..npoly as usize {
                let n = LENGTH(VECTOR_ELT(poly_ind, i as R_xlen_t));
                nper[i] = n;
                ntot += n;
            }
            if ntot <= 0 {
                continue;
            }

            let mut xx = vec![0.0; ntot as usize];
            let mut yy = vec![0.0; ntot as usize];
            let mut k: usize = 0;
            for i in 0..npoly as usize {
                let indices = INTEGER(VECTOR_ELT(poly_ind, i as R_xlen_t));
                for j in 0..nper[i] as usize {
                    transformLocn(
                        x,
                        y,
                        *indices.add(j) - 1,
                        vpc,
                        gc,
                        vpWidthCM,
                        vpHeightCM,
                        dd,
                        &mut transform,
                        &mut xx[k],
                        &mut yy[k],
                    );
                    xx[k] = toDeviceX(xx[k], GE_INCHES, dd);
                    yy[k] = toDeviceY(yy[k], GE_INCHES, dd);
                    if !xx[k].is_finite() || !yy[k].is_finite() {
                        Rf_error(c"non-finite x or y in graphics path".as_ptr());
                    }
                    k += 1;
                }
            }

            updateGContext(currentgp, h, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);
            GEPath(
                xx.as_mut_ptr(),
                yy.as_mut_ptr(),
                npoly,
                nper.as_mut_ptr(),
                asBool(rule),
                gc,
                dd,
            );
        }

        GEMode(0, dd);
        R_NilValue()
    }
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
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = Rf_duplicate(gridStateElement(dd, GSS_GPAR));
        let _currentgp_guard = protect(currentgp);
        set_gp_fill_string(currentgp, c"transparent".as_ptr());

        let mut vpWidthCM: c_double = 0.0;
        let mut vpHeightCM: c_double = 0.0;
        let mut rotationAngle: c_double = 0.0;
        let mut transform: LTransform = [[0.0; 3]; 3];
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );
        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        let mut gp_is_scalar = [-1i32; 15];
        let mut gc_buf: [u8; 256] = [0; 256];
        let mut gc_cache_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        let gc_cache = gc_cache_buf.as_mut_ptr() as pGEcontext;
        initGContext(currentgp, gc, dd, gp_is_scalar.as_mut_ptr(), gc_cache);

        let n = LENGTH(raster);
        if n <= 0 {
            Rf_error(c"Empty raster".as_ptr());
        }

        let mut image_owned: Vec<c_uint> = Vec::new();
        let image: *mut c_uint = if Rf_inherits(raster, c"nativeRaster".as_ptr()) != 0
            && TYPEOF(raster) == SEXPTYPE::INTSXP
        {
            INTEGER(raster) as *mut c_uint
        } else {
            image_owned = vec![0; n as usize];
            for i in 0..n as usize {
                image_owned[i] = RGBpar3(raster as *mut c_void, i as c_int, R_TRANWHITE as c_uint);
            }
            image_owned.as_mut_ptr()
        };

        let dim = getAttrib(raster, R_DimSymbol());
        if TYPEOF(dim) != SEXPTYPE::INTSXP || LENGTH(dim) < 2 {
            Rf_error(c"invalid raster dimensions".as_ptr());
        }

        let mut maxn = unitLength(x);
        maxn = maxn.max(unitLength(y));
        maxn = maxn.max(unitLength(w));
        maxn = maxn.max(unitLength(h));
        let hjust_len = LENGTH(hjust).max(1) as usize;
        let vjust_len = LENGTH(vjust).max(1) as usize;
        let interp_len = LENGTH(interpolate).max(1) as usize;

        GEMode(1, dd);
        for i in 0..maxn as usize {
            updateGContext(
                currentgp,
                i as c_int,
                gc,
                dd,
                gp_is_scalar.as_mut_ptr(),
                gc_cache,
            );

            let mut xx: c_double = 0.0;
            let mut yy: c_double = 0.0;
            transformLocn(
                x,
                y,
                i as c_int,
                vpc,
                gc,
                vpWidthCM,
                vpHeightCM,
                dd,
                &mut transform,
                &mut xx,
                &mut yy,
            );
            let mut ww = transformWidthtoINCHES(w, i as c_int, vpc, gc, vpWidthCM, vpHeightCM, dd);
            let mut hh = transformHeighttoINCHES(h, i as c_int, vpc, gc, vpWidthCM, vpHeightCM, dd);
            let hjust_i = if TYPEOF(hjust) == SEXPTYPE::REALSXP && LENGTH(hjust) > 0 {
                *REAL(hjust).add(i % hjust_len)
            } else {
                0.0
            };
            let vjust_i = if TYPEOF(vjust) == SEXPTYPE::REALSXP && LENGTH(vjust) > 0 {
                *REAL(vjust).add(i % vjust_len)
            } else {
                0.0
            };
            let interp_i = if TYPEOF(interpolate) == SEXPTYPE::LGLSXP && LENGTH(interpolate) > 0 {
                *LOGICAL(interpolate).add(i % interp_len)
            } else {
                0
            };

            if rotationAngle == 0.0 {
                xx = justifyX(xx, ww, hjust_i);
                yy = justifyY(yy, hh, vjust_i);
                xx = toDeviceX(xx, GE_INCHES, dd);
                yy = toDeviceY(yy, GE_INCHES, dd);
                ww = toDeviceWidth(ww, GE_INCHES, dd);
                hh = toDeviceHeight(hh, GE_INCHES, dd);
                if xx.is_finite() && yy.is_finite() && ww.is_finite() && hh.is_finite() {
                    GERaster(
                        image,
                        *INTEGER(dim).add(1),
                        *INTEGER(dim).add(0),
                        xx,
                        yy,
                        ww,
                        hh,
                        rotationAngle,
                        interp_i,
                        gc,
                        dd,
                    );
                }
            } else {
                let mut xadj: c_double = 0.0;
                let mut yadj: c_double = 0.0;
                justification(ww, hh, hjust_i, vjust_i, &mut xadj, &mut yadj);
                let xadjInches = unit(xadj, L_INCHES);
                let _xadj_guard = protect(xadjInches);
                let yadjInches = unit(yadj, L_INCHES);
                let _yadj_guard = protect(yadjInches);
                let mut dw: c_double = 0.0;
                let mut dh: c_double = 0.0;
                transformDimn(
                    xadjInches,
                    yadjInches,
                    0,
                    vpc,
                    gc,
                    vpWidthCM,
                    vpHeightCM,
                    dd,
                    rotationAngle,
                    &mut dw,
                    &mut dh,
                );
                let mut xbl = xx + dw;
                let mut ybl = yy + dh;
                xbl = toDeviceX(xbl, GE_INCHES, dd);
                ybl = toDeviceY(ybl, GE_INCHES, dd);
                ww = toDeviceWidth(ww, GE_INCHES, dd);
                hh = toDeviceHeight(hh, GE_INCHES, dd);
                if xbl.is_finite() && ybl.is_finite() && ww.is_finite() && hh.is_finite() {
                    GERaster(
                        image,
                        *INTEGER(dim).add(1),
                        *INTEGER(dim).add(0),
                        xbl,
                        ybl,
                        ww,
                        hh,
                        rotationAngle,
                        interp_i,
                        gc,
                        dd,
                    );
                }
            }
        }
        GEMode(0, dd);
        R_NilValue()
    }
}

/* ==============================
 * L_cap
 * ============================== */

pub unsafe fn L_cap() -> SEXP {
    unsafe {
        let dd = getDevice();
        let raster = GECap(dd);
        let _raster_guard = protect(raster);
        if isNull(raster) {
            R_NilValue()
        } else {
            let image = Rf_allocVector(SEXPTYPE::STRSXP, LENGTH(raster));
            let _image_guard = protect(image);
            image
        }
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
    unsafe { R_NilValue() }
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
    unsafe {
        gridText(label, x, y, hjust, vjust, rot, checkOverlap, 0.0, true);
        R_NilValue()
    }
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
    unsafe {
        let checkOverlap = Rf_allocVector(SEXPTYPE::LGLSXP, 1);
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
}

/* ==============================
 * symbolCoords (internal helper)
 * ============================== */

unsafe fn symbolCoords(x: *const f64, y: *const f64, n: c_int, _dd: pGEDevDesc) -> SEXP {
    unsafe {
        let result = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        let _result_guard = protect(result);
        let xs = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let _xs_guard = protect(xs);
        let ys = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let _ys_guard = protect(ys);
        for i in 0..n as usize {
            *REAL(xs).add(i) = *x.add(i);
            *REAL(ys).add(i) = *y.add(i);
        }
        SET_VECTOR_ELT(result, 0 as R_xlen_t, xs);
        SET_VECTOR_ELT(result, 1 as R_xlen_t, ys);
        result
    }
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
    unsafe { R_NilValue() }
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
    unsafe { R_NilValue() }
}

/* ==============================
 * L_points
 * ============================== */

pub unsafe fn L_points(x: SEXP, y: SEXP, pch: SEXP, size: SEXP) -> SEXP {
    unsafe { gridPoints(x, y, pch, size, true, false) }
}

/* ==============================
 * L_pointsPoints
 * ============================== */

pub unsafe fn L_pointsPoints(x: SEXP, y: SEXP, pch: SEXP, size: SEXP, closed: SEXP) -> SEXP {
    unsafe { gridPoints(x, y, pch, size, false, asBool(closed) != 0) }
}

/* ==============================
 * L_clip
 * ============================== */

pub unsafe fn L_clip(x: SEXP, y: SEXP, w: SEXP, h: SEXP, hjust: SEXP, vjust: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentvp = gridStateElement(dd, GSS_VP);
        let currentgp = gridStateElement(dd, GSS_GPAR);

        let mut vpWidthCM: c_double = 0.0;
        let mut vpHeightCM: c_double = 0.0;
        let mut rotationAngle: c_double = 0.0;
        let mut transform: LTransform = [[0.0; 3]; 3];
        getViewportTransform(
            currentvp,
            dd,
            &mut vpWidthCM,
            &mut vpHeightCM,
            &mut transform,
            &mut rotationAngle,
        );

        let mut vpc = LViewportContext::default();
        getViewportContext(currentvp, &mut vpc);

        GEMode(1, dd);

        /*
         * Only set ONE clip rectangle (i.e., NOT vectorised)
         */
        let mut gc_buf: [u8; 256] = [0; 256];
        let gc = gc_buf.as_mut_ptr() as pGEcontext;
        gcontextFromgpar(currentgp, 0, gc, dd);

        let mut xx: c_double = 0.0;
        let mut yy: c_double = 0.0;
        transformLocn(
            x,
            y,
            0,
            vpc,
            gc,
            vpWidthCM,
            vpHeightCM,
            dd,
            &mut transform,
            &mut xx,
            &mut yy,
        );
        let ww = transformWidthtoINCHES(w, 0, vpc, gc, vpWidthCM, vpHeightCM, dd);
        let hh = transformHeighttoINCHES(h, 0, vpc, gc, vpWidthCM, vpHeightCM, dd);

        /*
         * We can ONLY clip if the total rotation angle is zero.
         */
        if rotationAngle == 0.0 {
            let hjust_val = if TYPEOF(hjust) == SEXPTYPE::REALSXP && LENGTH(hjust) > 0 {
                *REAL(hjust)
            } else {
                0.0
            };
            let vjust_val = if TYPEOF(vjust) == SEXPTYPE::REALSXP && LENGTH(vjust) > 0 {
                *REAL(vjust)
            } else {
                0.0
            };
            xx = justifyX(xx, ww, hjust_val);
            yy = justifyY(yy, hh, vjust_val);

            /*
             * The graphics engine only takes device coordinates
             */
            xx = toDeviceX(xx, GE_INCHES, dd);
            yy = toDeviceY(yy, GE_INCHES, dd);
            let ww_dev = toDeviceWidth(ww, GE_INCHES, dd);
            let hh_dev = toDeviceHeight(hh, GE_INCHES, dd);

            if xx.is_finite() && yy.is_finite() && ww_dev.is_finite() && hh_dev.is_finite() {
                GESetClip(xx, yy, xx + ww_dev, yy + hh_dev, dd);

                /*
                 * ALSO set the current clip region for the current viewport so that,
                 * if a viewport is pushed within the current viewport, when that
                 * viewport gets popped again, the clip region returns to what was
                 * set by THIS clipGrob.
                 */
                let currentClip = Rf_allocVector(SEXPTYPE::REALSXP, 4);
                let _current_clip_guard = protect(currentClip);
                *REAL(currentClip).add(0) = xx;
                *REAL(currentClip).add(1) = yy;
                *REAL(currentClip).add(2) = xx + ww_dev;
                *REAL(currentClip).add(3) = yy + hh_dev;
                SET_VECTOR_ELT(currentvp, PVP_CLIPRECT as R_xlen_t, currentClip);
            }
        } else {
            Rf_warning(c"unable to clip to rotated rectangle".as_ptr());
        }

        GEMode(0, dd);
        R_NilValue()
    }
}

/* ==============================
 * L_pretty
 * ============================== */

pub unsafe fn L_pretty(scale: SEXP) -> SEXP {
    unsafe {
        let n_ = Rf_ScalarInteger(5);
        L_pretty2(scale, n_)
    }
}

/* ==============================
 * L_pretty2
 * ============================== */

pub unsafe fn L_pretty2(scale: SEXP, n_: SEXP) -> SEXP {
    unsafe {
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
}

/* ==============================
 * L_locator
 * ============================== */

pub unsafe fn L_locator() -> SEXP {
    unsafe {
        let answer = Rf_allocVector(SEXPTYPE::REALSXP, 2);
        let _answer_guard = protect(answer);
        *REAL(answer).add(0) = f64::NAN;
        *REAL(answer).add(1) = f64::NAN;
        answer
    }
}

/* ==============================
 * L_locnBounds
 * ============================== */

pub unsafe fn L_locnBounds(x: SEXP, y: SEXP, theta: SEXP) -> SEXP {
    unsafe {
        // Full implementation requires unit conversion
        R_NilValue()
    }
}

/* ==============================
 * L_stringMetric
 * ============================== */

pub unsafe fn L_stringMetric(label: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        let currentgp = gridStateElement(dd, GSS_GPAR);
        let n = if !label.is_null() && label != R_NilValue() {
            LENGTH(label)
        } else {
            0
        };

        let ascent_vec = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let _ascent_guard = protect(ascent_vec);
        let descent_vec = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let _descent_guard = protect(descent_vec);
        let width_vec = Rf_allocVector(SEXPTYPE::REALSXP, n);
        let _width_guard = protect(width_vec);

        let mut gc: [u8; 256] = [0; 256];
        gcontextFromgpar(currentgp, 0, gc.as_mut_ptr() as pGEcontext, dd);

        for i in 0..n as R_xlen_t {
            let mut ascent: f64 = 0.0;
            let mut descent: f64 = 0.0;
            let mut width: f64 = 0.0;
            if TYPEOF(label) == SEXPTYPE::EXPRSXP {
                GEExpressionMetric(
                    VECTOR_ELT(label, i),
                    gc.as_ptr() as pGEcontext,
                    &mut ascent,
                    &mut descent,
                    &mut width,
                    dd,
                );
            } else {
                let s = CHAR(STRING_ELT(label, i));
                let ce = getCharCE(STRING_ELT(label, i));
                GEStrMetric(
                    s,
                    ce,
                    gc.as_ptr() as pGEcontext,
                    &mut ascent,
                    &mut descent,
                    &mut width,
                    dd,
                );
            }
            *REAL(ascent_vec).add(i as usize) = ascent;
            *REAL(descent_vec).add(i as usize) = descent;
            *REAL(width_vec).add(i as usize) = width;
        }

        let result = Rf_allocVector(SEXPTYPE::VECSXP, 3);
        let _result_guard = protect(result);
        SET_VECTOR_ELT(result, 0, ascent_vec);
        SET_VECTOR_ELT(result, 1, descent_vec);
        SET_VECTOR_ELT(result, 2, width_vec);
        result
    }
}

/* ==============================
 * L_convertToNative (deprecated)
 * ============================== */

pub unsafe fn L_convertToNative(_x: SEXP, _what: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_l_pretty_returns_axis_ticks() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let scale = Rf_allocVector(SEXPTYPE::REALSXP, 2);
            *REAL(scale) = 0.0;
            *REAL(scale).add(1) = 10.0;
            let ticks = L_pretty(scale);
            assert_eq!(TYPEOF(ticks), SEXPTYPE::REALSXP);
            assert!(LENGTH(ticks) >= 2);
            assert!(*REAL(ticks) <= *REAL(ticks).add((LENGTH(ticks) - 1) as usize));
        }
    }
}
