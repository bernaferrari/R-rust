/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1997--2025  The R Core Team
 *  Copyright (C) 1995, 1996  Robert Gentleman and Ross Ihaka
 *  Copyright (C) 2002--2011  The R Foundation
 *
 *  Ported from r-source/src/library/graphics/src/graphics.c
 *
 *  Graphics Engine functions: coordinate transformations (GConvert,
 *  GConvertX, GConvertY, GConvertXUnits, GConvertYUnits),
 *  device-to-user conversions (xDevtoNDC, yDevtoNDC, xDevtoNFC,
 *  yDevtoNFC, xDevtoNPC, yDevtoNPC, xNPCtoUsr, yNPCtoUsr,
 *  xDevtoUsr, yDevtoUsr), figure/plot management (GMapWin2Fig,
 *  mapping, GReset, GNewPlot, GRecording), graphics parameter
 *  functions (GInit, copyGPar, GRestore, GSavePars, GRestorePars,
 *  GSetState, GCheckState), clipping (GClip, GForceClip,
 *  gcontextFromGP, setClipRect), primitives (GLine, GLocator,
 *  GMetricInfo, GMode, GClipPolygon, GPolygon, GPolyline, GCircle,
 *  GRect, GPath, GRaster, GStrWidth, GStrHeight, GText, GArrow,
 *  GBox, GPretty, GLPretty, GSymbol, GMtext), math text
 *  (GExpressionWidth, GExpressionHeight, GMathText, GMMathText),
 *  axis setup (GScale, GSetupAxis), layout functions, and
 *  unit mapping (GMapUnits).
 *
 *  Also includes R_Log10 (logarithm utility, from Rmath).
 *
 *  These all depend on the Graphics Engine (GPar, pGEDevDesc, GE
 *  functions) which is not yet fully ported. All functions are stubs.
 *
 *  Note: currentFigureLocation is already provided as a stub in
 *  par.rs and is NOT duplicated here.
 */

use std::ffi::c_void;
use std::os::raw::{c_char, c_double, c_int, c_uint};

use crate::mainutils::engine;

use super::par::{GPar, gpptr};
use crate::sexp::ffi::*;
use crate::sexp::globals::*;

fn graphics_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
}

/* ========================================================================
 * Type definitions
 * ======================================================================== */

/// pGEDevDesc is an opaque pointer to the graphics device descriptor.
type pGEDevDesc = *mut c_void;

/// pGEcontext is an opaque pointer to a graphics context.
type pGEcontext = *const c_void;

/// Rboolean type (0 = FALSE, 1 = TRUE).
type Rboolean = c_int;

/// cetype_t -- character encoding type.
type cetype_t = c_int;

/// GUnit -- graphics unit enumeration (DEVICE=0, NDC=1, ..., USER=10, etc.)
type GUnit = c_int;

/* ========================================================================
 * Constants
 * ======================================================================== */

const DEVICE: GUnit = 0;
const NDC: GUnit = 1;
const NIC: GUnit = 2;
const NFC: GUnit = 3;
const NPC: GUnit = 6;
const USER: GUnit = 10;
const INCHES: GUnit = 5;
const LINES: GUnit = 7;
const CHARS: GUnit = 8;
const OMA1: GUnit = 9;
const OMA2: GUnit = 10;
const OMA3: GUnit = 11;
const OMA4: GUnit = 12;
const MAR1: GUnit = 13;
const MAR2: GUnit = 14;
const MAR3: GUnit = 15;
const MAR4: GUnit = 16;

/// DEG2RAD: conversion factor from degrees to radians.
const DEG2RAD: c_double = std::f64::consts::PI / 180.0;

/// CE_SYMBOL: character encoding for symbol font.
const CE_SYMBOL: c_int = 2;

/* ========================================================================
 * R_Log10 -- base-10 logarithm with NA handling
 * ======================================================================== */

/// R_Log10 -- compute base-10 logarithm, returning NA_REAL for
/// non-positive or non-finite input.
pub fn R_Log10(x: c_double) -> c_double {
    if x.is_finite() && x > 0.0 {
        x.log10()
    } else {
        crate::sexp::ffi::NA_REAL
    }
}

/* ========================================================================
 * GMapUnits -- map R interpreted units to internal units
 * ======================================================================== */

/// GMapUnits -- map R interpreted unit codes to internal GUnit values.
/// In interpreted R: 1 = "user", 2 = "figure", 3 = "inches".
pub unsafe fn GMapUnits(runits: c_int) -> GUnit {
    match runits {
        1 => USER,
        2 => NFC,
        3 => INCHES,
        _ => 0,
    }
}

/* ========================================================================
 * Coordinate unit conversions (single-value, unit-to-unit)
 * ======================================================================== */

fn graphics_unit_to_engine(unit: GUnit) -> Option<c_int> {
    match unit {
        DEVICE => Some(engine::GE_DEVICE),
        NDC => Some(engine::GE_NDC),
        INCHES => Some(engine::GE_INCHES),
        _ => None,
    }
}

unsafe fn gpar_for_device(dd: pGEDevDesc) -> *mut GPar {
    unsafe { gpptr(dd) as *mut GPar }
}

unsafe fn x_usr_to_npc(x: c_double, gp: *mut GPar) -> c_double {
    unsafe {
        if gp.is_null() {
            return x;
        }
        let left = (*gp).usr[0];
        let right = (*gp).usr[1];
        if left == right {
            return 0.0;
        }
        (x - left) / (right - left)
    }
}

unsafe fn y_usr_to_npc(y: c_double, gp: *mut GPar) -> c_double {
    unsafe {
        if gp.is_null() {
            return y;
        }
        let bottom = (*gp).usr[2];
        let top = (*gp).usr[3];
        if bottom == top {
            return 0.0;
        }
        (y - bottom) / (top - bottom)
    }
}

unsafe fn x_npc_to_usr(x: c_double, gp: *mut GPar) -> c_double {
    unsafe {
        if gp.is_null() {
            return x;
        }
        (*gp).usr[0] + x * ((*gp).usr[1] - (*gp).usr[0])
    }
}

unsafe fn y_npc_to_usr(y: c_double, gp: *mut GPar) -> c_double {
    unsafe {
        if gp.is_null() {
            return y;
        }
        (*gp).usr[2] + y * ((*gp).usr[3] - (*gp).usr[2])
    }
}

unsafe fn x_npc_to_ndc(x: c_double, gp: *mut GPar) -> c_double {
    unsafe {
        if gp.is_null() {
            return x;
        }
        (*gp).plt[0] + x * ((*gp).plt[1] - (*gp).plt[0])
    }
}

unsafe fn y_npc_to_ndc(y: c_double, gp: *mut GPar) -> c_double {
    unsafe {
        if gp.is_null() {
            return y;
        }
        (*gp).plt[2] + y * ((*gp).plt[3] - (*gp).plt[2])
    }
}

unsafe fn x_ndc_to_npc(x: c_double, gp: *mut GPar) -> c_double {
    unsafe {
        if gp.is_null() {
            return x;
        }
        let left = (*gp).plt[0];
        let right = (*gp).plt[1];
        if left == right {
            return 0.0;
        }
        (x - left) / (right - left)
    }
}

unsafe fn y_ndc_to_npc(y: c_double, gp: *mut GPar) -> c_double {
    unsafe {
        if gp.is_null() {
            return y;
        }
        let bottom = (*gp).plt[2];
        let top = (*gp).plt[3];
        if bottom == top {
            return 0.0;
        }
        (y - bottom) / (top - bottom)
    }
}

unsafe fn x_nfc_to_ndc(x: c_double, gp: *mut GPar) -> c_double {
    unsafe {
        if gp.is_null() {
            return x;
        }
        (*gp).fig[0] + x * ((*gp).fig[1] - (*gp).fig[0])
    }
}

unsafe fn y_nfc_to_ndc(y: c_double, gp: *mut GPar) -> c_double {
    unsafe {
        if gp.is_null() {
            return y;
        }
        (*gp).fig[2] + y * ((*gp).fig[3] - (*gp).fig[2])
    }
}

unsafe fn x_ndc_to_nfc(x: c_double, gp: *mut GPar) -> c_double {
    unsafe {
        if gp.is_null() {
            return x;
        }
        let left = (*gp).fig[0];
        let right = (*gp).fig[1];
        if left == right {
            return 0.0;
        }
        (x - left) / (right - left)
    }
}

unsafe fn y_ndc_to_nfc(y: c_double, gp: *mut GPar) -> c_double {
    unsafe {
        if gp.is_null() {
            return y;
        }
        let bottom = (*gp).fig[2];
        let top = (*gp).fig[3];
        if bottom == top {
            return 0.0;
        }
        (y - bottom) / (top - bottom)
    }
}

unsafe fn x_chars_to_device(x: c_double, gp: *mut GPar, dd: pGEDevDesc) -> c_double {
    unsafe {
        let gp = gp;
        let cex = if gp.is_null() { 1.0 } else { (*gp).cex };
        let width = GStrWidth(c"m".as_ptr(), 0, DEVICE, dd);
        x * cex * width
    }
}

unsafe fn y_chars_to_device(y: c_double, gp: *mut GPar, dd: pGEDevDesc) -> c_double {
    unsafe {
        let gp = gp;
        let cex = if gp.is_null() { 1.0 } else { (*gp).cex };
        let height = GStrHeight(c"m".as_ptr(), 0, DEVICE, dd);
        y * cex * height
    }
}

unsafe fn x_lines_to_device(x: c_double, gp: *mut GPar, dd: pGEDevDesc) -> c_double {
    unsafe {
        let lheight = if gp.is_null() { 1.0 } else { (*gp).lheight };
        x_chars_to_device(x * lheight, gp, dd)
    }
}

unsafe fn y_lines_to_device(y: c_double, gp: *mut GPar, dd: pGEDevDesc) -> c_double {
    unsafe {
        let lheight = if gp.is_null() { 1.0 } else { (*gp).lheight };
        y_chars_to_device(y * lheight, gp, dd)
    }
}

unsafe fn x_to_device_units(x: c_double, fromUnits: GUnit, dd: pGEDevDesc) -> c_double {
    unsafe {
        if fromUnits == DEVICE {
            return x;
        }
        let gp = gpar_for_device(dd);
        match fromUnits {
            USER => {
                let npc = x_usr_to_npc(x, gp);
                let ndc = x_npc_to_ndc(npc, gp);
                engine::toDeviceX(ndc, engine::GE_NDC, dd)
            }
            NFC => {
                let ndc = x_nfc_to_ndc(x, gp);
                engine::toDeviceX(ndc, engine::GE_NDC, dd)
            }
            NPC => {
                let ndc = x_npc_to_ndc(x, gp);
                engine::toDeviceX(ndc, engine::GE_NDC, dd)
            }
            LINES => x_lines_to_device(x, gp, dd),
            CHARS => x_chars_to_device(x, gp, dd),
            _ => {
                if let Some(from) = graphics_unit_to_engine(fromUnits) {
                    engine::toDeviceX(x, from, dd)
                } else {
                    graphics_error(format!(
                        "unsupported source unit {fromUnits} for x coordinate conversion"
                    ));
                }
            }
        }
    }
}

unsafe fn y_to_device_units(y: c_double, fromUnits: GUnit, dd: pGEDevDesc) -> c_double {
    unsafe {
        if fromUnits == DEVICE {
            return y;
        }
        let gp = gpar_for_device(dd);
        match fromUnits {
            USER => {
                let npc = y_usr_to_npc(y, gp);
                let ndc = y_npc_to_ndc(npc, gp);
                engine::toDeviceY(ndc, engine::GE_NDC, dd)
            }
            NFC => {
                let ndc = y_nfc_to_ndc(y, gp);
                engine::toDeviceY(ndc, engine::GE_NDC, dd)
            }
            NPC => {
                let ndc = y_npc_to_ndc(y, gp);
                engine::toDeviceY(ndc, engine::GE_NDC, dd)
            }
            LINES => y_lines_to_device(y, gp, dd),
            CHARS => y_chars_to_device(y, gp, dd),
            _ => {
                if let Some(from) = graphics_unit_to_engine(fromUnits) {
                    engine::toDeviceY(y, from, dd)
                } else {
                    graphics_error(format!(
                        "unsupported source unit {fromUnits} for y coordinate conversion"
                    ));
                }
            }
        }
    }
}

unsafe fn x_from_device_units(x: c_double, toUnits: GUnit, dd: pGEDevDesc) -> c_double {
    unsafe {
        if toUnits == DEVICE {
            return x;
        }
        let gp = gpar_for_device(dd);
        match toUnits {
            USER => {
                let ndc = engine::fromDeviceX(x, engine::GE_NDC, dd);
                let npc = x_ndc_to_npc(ndc, gp);
                x_npc_to_usr(npc, gp)
            }
            NFC => {
                let ndc = engine::fromDeviceX(x, engine::GE_NDC, dd);
                x_ndc_to_nfc(ndc, gp)
            }
            NPC => {
                let ndc = engine::fromDeviceX(x, engine::GE_NDC, dd);
                x_ndc_to_npc(ndc, gp)
            }
            LINES | CHARS => {
                let device = x;
                let chars = if device == 0.0 {
                    0.0
                } else {
                    let width = x_chars_to_device(1.0, gp, dd);
                    if width == 0.0 {
                        0.0
                    } else {
                        device / width
                    }
                };
                if toUnits == CHARS {
                    chars
                } else {
                    let lheight = if gp.is_null() { 1.0 } else { (*gp).lheight };
                    if lheight == 0.0 {
                        0.0
                    } else {
                        chars / lheight
                    }
                }
            }
            _ => {
                if let Some(to) = graphics_unit_to_engine(toUnits) {
                    engine::fromDeviceX(x, to, dd)
                } else {
                    graphics_error(format!(
                        "unsupported target unit {toUnits} for x coordinate conversion"
                    ));
                }
            }
        }
    }
}

unsafe fn y_from_device_units(y: c_double, toUnits: GUnit, dd: pGEDevDesc) -> c_double {
    unsafe {
        if toUnits == DEVICE {
            return y;
        }
        let gp = gpar_for_device(dd);
        match toUnits {
            USER => {
                let ndc = engine::fromDeviceY(y, engine::GE_NDC, dd);
                let npc = y_ndc_to_npc(ndc, gp);
                y_npc_to_usr(npc, gp)
            }
            NFC => {
                let ndc = engine::fromDeviceY(y, engine::GE_NDC, dd);
                y_ndc_to_nfc(ndc, gp)
            }
            NPC => {
                let ndc = engine::fromDeviceY(y, engine::GE_NDC, dd);
                y_ndc_to_npc(ndc, gp)
            }
            LINES | CHARS => {
                let device = y;
                let chars = if device == 0.0 {
                    0.0
                } else {
                    let height = y_chars_to_device(1.0, gp, dd);
                    if height == 0.0 {
                        0.0
                    } else {
                        device / height
                    }
                };
                if toUnits == CHARS {
                    chars
                } else {
                    let lheight = if gp.is_null() { 1.0 } else { (*gp).lheight };
                    if lheight == 0.0 {
                        0.0
                    } else {
                        chars / lheight
                    }
                }
            }
            _ => {
                if let Some(to) = graphics_unit_to_engine(toUnits) {
                    engine::fromDeviceY(y, to, dd)
                } else {
                    graphics_error(format!(
                        "unsupported target unit {toUnits} for y coordinate conversion"
                    ));
                }
            }
        }
    }
}

/// GConvertXUnits -- convert a single x value between unit systems.
pub unsafe fn GConvertXUnits(
    x: c_double,
    fromUnits: GUnit,
    toUnits: GUnit,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        if fromUnits == toUnits {
            return x;
        }
        if dd.is_null() {
            graphics_error("graphics coordinate conversion requires a graphics device backend");
        }
        let device = x_to_device_units(x, fromUnits, dd);
        x_from_device_units(device, toUnits, dd)
    }
}

/// GConvertYUnits -- convert a single y value between unit systems.
pub unsafe fn GConvertYUnits(
    y: c_double,
    fromUnits: GUnit,
    toUnits: GUnit,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        if fromUnits == toUnits {
            return y;
        }
        if dd.is_null() {
            graphics_error("graphics coordinate conversion requires a graphics device backend");
        }
        let device = y_to_device_units(y, fromUnits, dd);
        y_from_device_units(device, toUnits, dd)
    }
}

/* ========================================================================
 * Coordinate conversions: DEVICE to other systems
 * ======================================================================== */

/// xDevtoNDC -- convert x from device coordinates to NDC.
pub unsafe fn xDevtoNDC(x: c_double, dd: pGEDevDesc) -> c_double {
    unsafe { engine::fromDeviceX(x, 1, dd) }
}

/// yDevtoNDC -- convert y from device coordinates to NDC.
pub unsafe fn yDevtoNDC(y: c_double, dd: pGEDevDesc) -> c_double {
    unsafe { engine::fromDeviceY(y, 1, dd) }
}

/// xDevtoNFC -- convert x from device coordinates to NFC.
pub unsafe fn xDevtoNFC(x: c_double, dd: pGEDevDesc) -> c_double {
    unsafe { x_from_device_units(x, NFC, dd) }
}

/// yDevtoNFC -- convert y from device coordinates to NFC.
pub unsafe fn yDevtoNFC(y: c_double, dd: pGEDevDesc) -> c_double {
    unsafe { y_from_device_units(y, NFC, dd) }
}

/// xDevtoNPC -- convert x from device coordinates to NPC.
pub unsafe fn xDevtoNPC(x: c_double, dd: pGEDevDesc) -> c_double {
    unsafe { x_from_device_units(x, NPC, dd) }
}

/// yDevtoNPC -- convert y from device coordinates to NPC.
pub unsafe fn yDevtoNPC(y: c_double, dd: pGEDevDesc) -> c_double {
    unsafe { y_from_device_units(y, NPC, dd) }
}

/// xNPCtoUsr -- convert x from NPC to user coordinates.
pub unsafe fn xNPCtoUsr(x: c_double, dd: pGEDevDesc) -> c_double {
    unsafe {
        let gp = gpar_for_device(dd);
        x_npc_to_usr(x, gp)
    }
}

/// yNPCtoUsr -- convert y from NPC to user coordinates.
pub unsafe fn yNPCtoUsr(y: c_double, dd: pGEDevDesc) -> c_double {
    unsafe {
        let gp = gpar_for_device(dd);
        y_npc_to_usr(y, gp)
    }
}

/// xDevtoUsr -- convert x from device coordinates to user coordinates.
pub unsafe fn xDevtoUsr(x: c_double, dd: pGEDevDesc) -> c_double {
    unsafe { x_from_device_units(x, USER, dd) }
}

/// yDevtoUsr -- convert y from device coordinates to user coordinates.
pub unsafe fn yDevtoUsr(y: c_double, dd: pGEDevDesc) -> c_double {
    unsafe { y_from_device_units(y, USER, dd) }
}

/* ========================================================================
 * GConvert -- convert a location (x,y) between coordinate systems
 * ======================================================================== */

/// GConvert -- convert a location (x, y) from one coordinate system to another.
pub unsafe fn GConvert(x: *mut c_double, y: *mut c_double, from: GUnit, to: GUnit, dd: pGEDevDesc) {
    unsafe {
        if !x.is_null() {
            *x = GConvertX(*x, from, to, dd);
        }
        if !y.is_null() {
            *y = GConvertY(*y, from, to, dd);
        }
    }
}

/* ========================================================================
 * GConvertX / GConvertY -- single-axis location conversion
 * ======================================================================== */

/// GConvertX -- convert an x location from one coordinate system to another.
pub unsafe fn GConvertX(x: c_double, from: GUnit, to: GUnit, dd: pGEDevDesc) -> c_double {
    unsafe { GConvertXUnits(x, from, to, dd) }
}

/// GConvertY -- convert a y location from one coordinate system to another.
pub unsafe fn GConvertY(y: c_double, from: GUnit, to: GUnit, dd: pGEDevDesc) -> c_double {
    unsafe { GConvertYUnits(y, from, to, dd) }
}

/* ========================================================================
 * Figure/plot management
 * ======================================================================== */

/// GMapWin2Fig -- set up the transformation from user to NFC coordinates.
/// Stub: does nothing.
pub unsafe fn GMapWin2Fig(_dd: pGEDevDesc) {
    /* Stub: full implementation sets win2fig.{ax,ay,bx,by} from
    gpptr(dd)->plt and usr/logusr arrays */
}

/// GNewPlot -- begin a new plot (advance to new frame if needed).
/// Stub: returns null pointer.
pub unsafe fn GNewPlot(_recording: Rboolean) -> pGEDevDesc {
    std::ptr::null_mut()
}

/// GRecording -- check whether graphics operations should be recorded.
/// Stub: returns 0 (FALSE).
pub unsafe fn GRecording(_call: SEXP, _dd: pGEDevDesc) -> Rboolean {
    0
}

/// GReset -- reset graphics parameters after device resize.
/// Stub: does nothing.
pub unsafe fn GReset(_dd: pGEDevDesc) {
    /* Stub: full implementation recalculates all mappings */
}

/* ========================================================================
 * Graphics parameter management
 * ======================================================================== */

/// GInit -- initialize default graphics parameter values in a GPar struct.
/// Stub: does nothing.
pub unsafe fn GInit(_dp: *mut c_void) {
    /* Stub: full implementation sets all fields of GPar to defaults */
}

/// copyGPar -- copy a GPar structure from source to dest.
/// Stub: does nothing.
pub unsafe fn copyGPar(_source: *mut c_void, _dest: *mut c_void) {
    /* Stub: full implementation does memcpy(dest, source, sizeof(GPar)) */
}

/// GRestore -- restore graphics parameters from the device copy.
/// Stub: does nothing.
pub unsafe fn GRestore(_dd: pGEDevDesc) {
    /* Stub: full implementation calls copyGPar(dpptr(dd), gpptr(dd)) */
}

/// GSavePars -- save inline graphical parameters.
/// Stub: does nothing.
pub unsafe fn GSavePars(_dd: pGEDevDesc) {
    /* Stub: full implementation saves all inline pars to static vars */
}

/// GRestorePars -- restore inline graphical parameters.
/// Stub: does nothing.
pub unsafe fn GRestorePars(_dd: pGEDevDesc) {
    /* Stub: full implementation restores all inline pars from static vars */
}

/// GSetState -- set the graphics state flag (records whether GNewPlot called).
/// Stub: does nothing.
pub unsafe fn GSetState(_newstate: c_int, _dd: pGEDevDesc) {
    /* Stub: full implementation sets dpptr(dd)->state = gpptr(dd)->state */
}

/// GCheckState -- enquire whether GNewPlot has been called.
/// Stub: does nothing (does not error).
pub unsafe fn GCheckState(_dd: pGEDevDesc) {
    /* Stub: full implementation errors if state==0 or !valid */
}

/* ========================================================================
 * Axis setup
 * ======================================================================== */

/// GScale -- compute default axis information (axp, usr, log, n).
/// Provides default axis information i.e., if user has NOT specified
/// par(usr=.., {x,y}axp= ..).
/// Stub: does nothing.
pub unsafe fn GScale(_min: c_double, _max: c_double, _axis: c_int, _dd: pGEDevDesc) {
    /* Stub: full implementation computes axis parameters based on
    lab, xaxs/yaxs, xlog/ylog from gpptr(dd) and calls GAxisPars */
}

/// GSetupAxis -- set up default axis information when user specifies par(usr=...).
/// Stub: does nothing.
pub unsafe fn GSetupAxis(_axis: c_int, _dd: pGEDevDesc) {
    /* Stub: full implementation calls GPretty and stores axp */
}

/* ========================================================================
 * Clipping
 * ======================================================================== */

/// GClip -- update the device clipping region (depends on GP->xpd).
/// Stub: does nothing.
pub unsafe fn GClip(_dd: pGEDevDesc) {
    /* Stub: full implementation calls setClipRect + GESetClip */
}

/// GForceClip -- forced update of the device clipping region.
/// Stub: does nothing.
pub unsafe fn GForceClip(_dd: pGEDevDesc) {
    /* Stub: full implementation calls setClipRect + GESetClip */
}

/// setClipRect -- set the clipping rectangle for a given coordinate system.
/// Stub: sets all outputs to 0.0.
pub unsafe fn setClipRect(
    x1: *mut c_double,
    y1: *mut c_double,
    x2: *mut c_double,
    y2: *mut c_double,
    _coords: GUnit,
    _dd: pGEDevDesc,
) {
    unsafe {
        if !x1.is_null() {
            *x1 = 0.0;
        }
        if !y1.is_null() {
            *y1 = 0.0;
        }
        if !x2.is_null() {
            *x2 = 0.0;
        }
        if !y2.is_null() {
            *y2 = 0.0;
        }
    }
}

/// gcontextFromGP -- generate an R_GE_gcontext from gpptr info.
/// Stub: does nothing.
pub unsafe fn gcontextFromGP(_gc: pGEcontext, _dd: pGEDevDesc) {
    /* Stub: full implementation copies all graphics context fields
    from gpptr(dd) to the gc struct */
}

/* ========================================================================
 * Graphical primitives
 * ======================================================================== */

unsafe fn convert_xy_points_to_device(
    n: c_int,
    x: *mut c_double,
    y: *mut c_double,
    coords: c_int,
    dd: pGEDevDesc,
) -> Option<(Vec<c_double>, Vec<c_double>)> {
    unsafe {
        if n <= 0 || x.is_null() || y.is_null() || dd.is_null() {
            return None;
        }
        let mut xs = Vec::with_capacity(n as usize);
        let mut ys = Vec::with_capacity(n as usize);
        for i in 0..n as isize {
            let mut xi = *x.offset(i);
            let mut yi = *y.offset(i);
            GConvert(&mut xi, &mut yi, coords, DEVICE, dd);
            xs.push(xi);
            ys.push(yi);
        }
        Some((xs, ys))
    }
}

/// GLine -- draw a line from (x1,y1) to (x2,y2).
pub unsafe fn GLine(
    x1: c_double,
    y1: c_double,
    x2: c_double,
    y2: c_double,
    coords: c_int,
    dd: pGEDevDesc,
) {
    unsafe {
        if dd.is_null() {
            return;
        }
        let mut x1 = x1;
        let mut y1 = y1;
        let mut x2 = x2;
        let mut y2 = y2;
        GConvert(&mut x1, &mut y1, coords, DEVICE, dd);
        GConvert(&mut x2, &mut y2, coords, DEVICE, dd);
        engine::GELine(x1, y1, x2, y2, std::ptr::null(), dd);
    }
}

/// GLocator -- read the current pen position interactively.
/// Stub: returns 0 (FALSE).
pub unsafe fn GLocator(
    x: *mut c_double,
    y: *mut c_double,
    _coords: c_int,
    _dd: pGEDevDesc,
) -> Rboolean {
    unsafe {
        if !x.is_null() {
            *x = 0.0;
        }
        if !y.is_null() {
            *y = 0.0;
        }
    }
    0
}

/// GMetricInfo -- access character font metric information.
/// Stub: sets all outputs to 0.0.
pub unsafe fn GMetricInfo(
    _c: c_int,
    ascent: *mut c_double,
    descent: *mut c_double,
    width: *mut c_double,
    _units: GUnit,
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

/// GMode -- set graphics mode (0=off, 1=on, 2=input on).
pub unsafe fn GMode(mode: c_int, dd: pGEDevDesc) {
    unsafe {
        if dd.is_null() {
            return;
        }
        engine::GEMode(mode, dd);
    }
}

/// GClipPolygon -- clip a polygon to the current clip region.
/// Uses Sutherland-Hodgman polygon clipping algorithm.
/// Stub: returns 0.
pub unsafe fn GClipPolygon(
    _x: *mut c_double,
    _y: *mut c_double,
    _n: c_int,
    _coords: c_int,
    _store: c_int,
    _xout: *mut c_double,
    _yout: *mut c_double,
    _dd: pGEDevDesc,
) -> c_int {
    0
}

/// GPolygon -- draw a filled polygon with border.
/// Filled with color bg and outlined with color fg.
pub unsafe fn GPolygon(
    n: c_int,
    x: *mut c_double,
    y: *mut c_double,
    coords: c_int,
    _bg: c_int,
    _fg: c_int,
    dd: pGEDevDesc,
) {
    unsafe {
        let Some((xs, ys)) = convert_xy_points_to_device(n, x, y, coords, dd) else {
            return;
        };
        engine::GEPolygon(
            n,
            xs.as_ptr(),
            ys.as_ptr(),
            std::ptr::null(),
            dd,
        );
    }
}

/// GPolyline -- draw a series of connected line segments.
pub unsafe fn GPolyline(
    n: c_int,
    x: *mut c_double,
    y: *mut c_double,
    coords: c_int,
    dd: pGEDevDesc,
) {
    unsafe {
        let Some((xs, ys)) = convert_xy_points_to_device(n, x, y, coords, dd) else {
            return;
        };
        engine::GEPolyline(
            n,
            xs.as_ptr(),
            ys.as_ptr(),
            std::ptr::null(),
            dd,
        );
    }
}

/// GCircle -- draw a circle. Filled with color bg and outlined with color fg.
pub unsafe fn GCircle(
    x: c_double,
    y: c_double,
    coords: c_int,
    radius: c_double,
    _bg: c_int,
    _fg: c_int,
    dd: pGEDevDesc,
) {
    unsafe {
        if dd.is_null() {
            return;
        }
        let mut cx = x;
        let mut cy = y;
        GConvert(&mut cx, &mut cy, coords, DEVICE, dd);
        let device_radius = if coords == DEVICE {
            radius
        } else {
            let edge_x = x_to_device_units(x + radius, coords, dd);
            edge_x - cx
        };
        engine::GECircle(cx, cy, device_radius, std::ptr::null(), dd);
    }
}

/// GRect -- draw a rectangle. Filled with color bg and outlined with color fg.
pub unsafe fn GRect(
    x0: c_double,
    y0: c_double,
    x1: c_double,
    y1: c_double,
    coords: c_int,
    _bg: c_int,
    _fg: c_int,
    dd: pGEDevDesc,
) {
    unsafe {
        if dd.is_null() {
            return;
        }
        let mut left = x0;
        let mut bottom = y0;
        let mut right = x1;
        let mut top = y1;
        GConvert(&mut left, &mut bottom, coords, DEVICE, dd);
        GConvert(&mut right, &mut top, coords, DEVICE, dd);
        engine::GERect(left, bottom, right, top, std::ptr::null(), dd);
    }
}

/// GPath -- draw a path (possibly with holes).
/// Stub: does nothing.
pub unsafe fn GPath(
    _x: *mut c_double,
    _y: *mut c_double,
    _npoly: c_int,
    _nper: *mut c_int,
    _winding: Rboolean,
    _bg: c_int,
    _fg: c_int,
    _dd: pGEDevDesc,
) {
    /* Stub: full implementation clips and calls GEPath */
}

/// GRaster -- draw a raster image.
/// Stub: does nothing.
pub unsafe fn GRaster(
    _image: *mut c_uint,
    _w: c_int,
    _h: c_int,
    _x0: c_double,
    _y0: c_double,
    _x1: c_double,
    _y1: c_double,
    _angle: c_double,
    _interpolate: Rboolean,
    _dd: pGEDevDesc,
) {
    /* Stub: full implementation clips and calls GERaster */
}

/* ========================================================================
 * String dimension and text functions
 * ======================================================================== */

/// GStrWidth -- compute the width of a string.
/// Stub: returns 0.0.
pub unsafe fn GStrWidth(
    str: *const c_char,
    enc: cetype_t,
    units: GUnit,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        let width = engine::GEStrWidth(str, enc, std::ptr::null(), dd);
        match units {
            DEVICE => width,
            NDC => engine::fromDeviceWidth(width, 1, dd),
            INCHES => engine::fromDeviceWidth(width, 2, dd),
            _ => width,
        }
    }
}

/// GStrHeight -- compute the height of a string.
/// Stub: returns 0.0.
pub unsafe fn GStrHeight(
    str: *const c_char,
    enc: cetype_t,
    units: GUnit,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        let height = engine::GEStrHeight(str, enc, std::ptr::null(), dd);
        match units {
            DEVICE => height,
            NDC => engine::fromDeviceHeight(height, 1, dd),
            INCHES => engine::fromDeviceHeight(height, 2, dd),
            _ => height,
        }
    }
}

/// GText -- draw text at a location.
pub unsafe fn GText(
    x: c_double,
    y: c_double,
    coords: c_int,
    str: *const c_char,
    enc: cetype_t,
    xc: c_double,
    yc: c_double,
    rot: c_double,
    dd: pGEDevDesc,
) {
    unsafe {
        if dd.is_null() {
            return;
        }
        let mut px = x;
        let mut py = y;
        GConvert(&mut px, &mut py, coords, DEVICE, dd);
        engine::GEText(px, py, str, enc, xc, yc, rot, std::ptr::null(), dd);
    }
}

/// GArrow -- draw an arrow from (xfrom,yfrom) to (xto,yto).
/// The length parameter is in inches.
/// Stub: does nothing.
pub unsafe fn GArrow(
    _xfrom: c_double,
    _yfrom: c_double,
    _xto: c_double,
    _yto: c_double,
    _coords: c_int,
    _length: c_double,
    _angle: c_double,
    _code: c_int,
    _dd: pGEDevDesc,
) {
    /* Stub: full implementation draws line + arrowhead polylines */
}

/// GBox -- draw a box around one of several regions (box(which)).
/// which=1: plot region (with bty styles), 2: figure, 3: inner, 4: outer/device.
/// Stub: does nothing.
pub unsafe fn GBox(_which: c_int, _dd: pGEDevDesc) {
    /* Stub: full implementation draws a box around plot/figure/device region
    with various bty styles (o, l, 7, c, [, ], u, n) */
}

/* ========================================================================
 * Pretty labeling -- GLPretty and GPretty are now in src/main/graphics_main.rs
 * ======================================================================== */

/* ========================================================================
 * Symbol drawing
 * ======================================================================== */

/// GSymbol -- draw one of the R special symbols.
pub unsafe fn GSymbol(x: c_double, y: c_double, coords: c_int, pch: c_int, dd: pGEDevDesc) {
    unsafe {
        if dd.is_null() {
            return;
        }
        let mut px = x;
        let mut py = y;
        GConvert(&mut px, &mut py, coords, DEVICE, dd);
        engine::GESymbol(px, py, pch, 1.0, std::ptr::null(), dd);
    }
}

/* ========================================================================
 * Margin text
 * ======================================================================== */

/// GMtext -- draw text in the plot margins.
/// side: 1=bottom, 2=left, 3=top, 4=right.
/// las: 0=parallel to axis, 1=always horizontal, 2=perpendicular, 3=vertical.
/// Stub: does nothing.
pub unsafe fn GMtext(
    _str: *const c_char,
    _enc: cetype_t,
    _side: c_int,
    _line: c_double,
    _outer: c_int,
    _at: c_double,
    _las: c_int,
    _yadj: c_double,
    _dd: pGEDevDesc,
) {
    /* Stub: full implementation calculates coords and calls GText */
}

/* ========================================================================
 * Mathematical expression text
 * ======================================================================== */

/// GExpressionWidth -- compute the width of a mathematical expression.
/// Stub: returns 0.0.
pub unsafe fn GExpressionWidth(_expr: SEXP, _units: GUnit, _dd: pGEDevDesc) -> c_double {
    0.0
}

/// GExpressionHeight -- compute the height of a mathematical expression.
/// Stub: returns 0.0.
pub unsafe fn GExpressionHeight(_expr: SEXP, _units: GUnit, _dd: pGEDevDesc) -> c_double {
    0.0
}

/// GMathText -- draw a mathematical expression at a location.
/// Stub: does nothing.
pub unsafe fn GMathText(
    _x: c_double,
    _y: c_double,
    _coords: c_int,
    _expr: SEXP,
    _xc: c_double,
    _yc: c_double,
    _rot: c_double,
    _dd: pGEDevDesc,
) {
    /* Stub: full implementation converts to DEVICE, clips, calls GEMathText */
}

/// GMMathText -- draw a mathematical expression in the plot margins.
/// Stub: does nothing.
pub unsafe fn GMMathText(
    _str: SEXP,
    _side: c_int,
    _line: c_double,
    _outer: c_int,
    _at: c_double,
    _las: c_int,
    _yadj: c_double,
    _dd: pGEDevDesc,
) {
    /* Stub: full implementation calculates coords and calls GMathText */
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;

    #[test]
    fn test_identity_coordinate_conversion_preserves_values() {
        unsafe {
            assert_eq!(GConvertX(2.5, USER, USER, ptr::null_mut()), 2.5);
            assert_eq!(GConvertY(-1.25, NFC, NFC, ptr::null_mut()), -1.25);
            let mut x = 3.0;
            let mut y = 4.0;
            GConvert(&mut x, &mut y, INCHES, INCHES, ptr::null_mut());
            assert_eq!((x, y), (3.0, 4.0));
        }
    }

    #[test]
    fn test_unsupported_coordinate_conversion_errors() {
        let err = std::panic::catch_unwind(|| unsafe {
            GConvertX(2.5, USER, DEVICE, ptr::null_mut());
        });
        assert!(err.is_err());
        let payload = err.unwrap_err();
        let r_error = payload
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("coordinate conversion should raise RError");
        assert!(
            r_error
                .message
                .contains("requires a graphics device backend")
        );
    }

    #[test]
    fn string_metrics_delegate_to_engine_and_tolerate_no_device() {
        unsafe {
            assert_eq!(GStrWidth(c"abc".as_ptr(), 0, DEVICE, ptr::null_mut()), 0.0);
            assert_eq!(GStrHeight(c"abc".as_ptr(), 0, DEVICE, ptr::null_mut()), 0.0);
        }
    }

    #[test]
    fn device_to_ndc_delegates_to_engine_bridge() {
        unsafe {
            assert_eq!(xDevtoNDC(12.0, ptr::null_mut()), 12.0);
            assert_eq!(yDevtoNDC(7.5, ptr::null_mut()), 7.5);
        }
    }

    #[test]
    fn user_and_nfc_conversions_use_gpar_state() {
        let _session = crate::sexp::session::RSession::new();
        crate::library::grdevices::device_registry::reset_registry_for_tests();
        unsafe {
            let dd = crate::library::grdevices::device_registry::GEcurrentDevice();
            assert_eq!(GConvertX(0.5, USER, USER, dd.cast()), 0.5);
            assert_eq!(GConvertX(0.0, NFC, NFC, dd.cast()), 0.0);
            let device_x = GConvertX(0.5, USER, DEVICE, dd.cast());
            assert!(device_x > 0.0);
            assert!((GConvertX(device_x, DEVICE, USER, dd.cast()) - 0.5).abs() < 1e-9);
        }
    }

    #[test]
    fn gline_draws_on_headless_device_surface() {
        let _session = crate::sexp::session::RSession::new();
        crate::library::grdevices::device_registry::reset_registry_for_tests();
        unsafe {
            let dd = crate::library::grdevices::device_registry::GEcurrentDevice();
            crate::mainutils::engine::GENewPage(ptr::null(), dd.cast());
            GLine(0.0, 0.0, 10.0, 0.0, DEVICE, dd.cast());
            let result = crate::mainutils::engine::GECap(dd.cast());
            assert_eq!(crate::sexp::accessors::TYPEOF(result), crate::sexp::ffi::SEXPTYPE::INTSXP);
            assert_eq!(*crate::sexp::accessors::INTEGER(result), 0x0000_0000);
            assert_eq!(*crate::sexp::accessors::INTEGER(result).add(11), 0x00ff_ffff);
        }
    }

    #[test]
    fn inches_to_device_conversion_uses_headless_backend() {
        let _session = crate::sexp::session::RSession::new();
        crate::library::grdevices::device_registry::reset_registry_for_tests();
        unsafe {
            let dd = crate::library::grdevices::device_registry::GEcurrentDevice();
            assert_eq!(GConvertXUnits(1.0, INCHES, DEVICE, dd.cast()), 72.0);
            assert_eq!(GConvertYUnits(1.0, INCHES, DEVICE, dd.cast()), 432.0);
        }
    }
}
