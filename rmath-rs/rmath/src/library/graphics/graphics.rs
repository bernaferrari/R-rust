#![allow(unsafe_op_in_unsafe_fn)] // legacy C-port unsafe boundary; see docs/unsafe-op-allowlist.tsv.
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

use crate::sexp::ffi::*;
use crate::sexp::globals::*;

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
#[unsafe(no_mangle)]
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

/// GConvertXUnits -- convert a single x value between unit systems.
/// Stub: returns 0.0.
#[unsafe(no_mangle)]
pub unsafe fn GConvertXUnits(
    _x: c_double,
    _fromUnits: GUnit,
    _toUnits: GUnit,
    _dd: pGEDevDesc,
) -> c_double {
    0.0
}

/// GConvertYUnits -- convert a single y value between unit systems.
/// Stub: returns 0.0.
#[unsafe(no_mangle)]
pub unsafe fn GConvertYUnits(
    _y: c_double,
    _fromUnits: GUnit,
    _toUnits: GUnit,
    _dd: pGEDevDesc,
) -> c_double {
    0.0
}

/* ========================================================================
 * Coordinate conversions: DEVICE to other systems
 * ======================================================================== */

/// xDevtoNDC -- convert x from device coordinates to NDC.
/// Stub: returns 0.0.
pub unsafe fn xDevtoNDC(_x: c_double, _dd: pGEDevDesc) -> c_double {
    0.0
}

/// yDevtoNDC -- convert y from device coordinates to NDC.
/// Stub: returns 0.0.
pub unsafe fn yDevtoNDC(_y: c_double, _dd: pGEDevDesc) -> c_double {
    0.0
}

/// xDevtoNFC -- convert x from device coordinates to NFC.
/// Stub: returns 0.0.
pub unsafe fn xDevtoNFC(_x: c_double, _dd: pGEDevDesc) -> c_double {
    0.0
}

/// yDevtoNFC -- convert y from device coordinates to NFC.
/// Stub: returns 0.0.
pub unsafe fn yDevtoNFC(_y: c_double, _dd: pGEDevDesc) -> c_double {
    0.0
}

/// xDevtoNPC -- convert x from device coordinates to NPC.
/// Stub: returns 0.0.
pub unsafe fn xDevtoNPC(_x: c_double, _dd: pGEDevDesc) -> c_double {
    0.0
}

/// yDevtoNPC -- convert y from device coordinates to NPC.
/// Stub: returns 0.0.
pub unsafe fn yDevtoNPC(_y: c_double, _dd: pGEDevDesc) -> c_double {
    0.0
}

/// xNPCtoUsr -- convert x from NPC to user coordinates.
/// Stub: returns 0.0.
#[unsafe(no_mangle)]
pub unsafe fn xNPCtoUsr(_x: c_double, _dd: pGEDevDesc) -> c_double {
    0.0
}

/// yNPCtoUsr -- convert y from NPC to user coordinates.
/// Stub: returns 0.0.
#[unsafe(no_mangle)]
pub unsafe fn yNPCtoUsr(_y: c_double, _dd: pGEDevDesc) -> c_double {
    0.0
}

/// xDevtoUsr -- convert x from device coordinates to user coordinates.
/// Stub: returns 0.0.
pub unsafe fn xDevtoUsr(_x: c_double, _dd: pGEDevDesc) -> c_double {
    0.0
}

/// yDevtoUsr -- convert y from device coordinates to user coordinates.
/// Stub: returns 0.0.
pub unsafe fn yDevtoUsr(_y: c_double, _dd: pGEDevDesc) -> c_double {
    0.0
}

/* ========================================================================
 * GConvert -- convert a location (x,y) between coordinate systems
 * ======================================================================== */

/// GConvert -- convert a location (x, y) from one coordinate system to another.
/// Stub: sets both to 0.0.
#[unsafe(no_mangle)]
pub unsafe fn GConvert(
    x: *mut c_double,
    y: *mut c_double,
    _from: GUnit,
    _to: GUnit,
    _dd: pGEDevDesc,
) {
    if !x.is_null() {
        *x = 0.0;
    }
    if !y.is_null() {
        *y = 0.0;
    }
}

/* ========================================================================
 * GConvertX / GConvertY -- single-axis location conversion
 * ======================================================================== */

/// GConvertX -- convert an x location from one coordinate system to another.
/// Stub: returns 0.0.
#[unsafe(no_mangle)]
pub unsafe fn GConvertX(_x: c_double, _from: GUnit, _to: GUnit, _dd: pGEDevDesc) -> c_double {
    0.0
}

/// GConvertY -- convert a y location from one coordinate system to another.
/// Stub: returns 0.0.
#[unsafe(no_mangle)]
pub unsafe fn GConvertY(_y: c_double, _from: GUnit, _to: GUnit, _dd: pGEDevDesc) -> c_double {
    0.0
}

/* ========================================================================
 * Figure/plot management
 * ======================================================================== */

/// GMapWin2Fig -- set up the transformation from user to NFC coordinates.
/// Stub: does nothing.
#[unsafe(no_mangle)]
pub unsafe fn GMapWin2Fig(_dd: pGEDevDesc) {
    /* Stub: full implementation sets win2fig.{ax,ay,bx,by} from
    gpptr(dd)->plt and usr/logusr arrays */
}

/// GNewPlot -- begin a new plot (advance to new frame if needed).
/// Stub: returns null pointer.
#[unsafe(no_mangle)]
pub unsafe fn GNewPlot(_recording: Rboolean) -> pGEDevDesc {
    std::ptr::null_mut()
}

/// GRecording -- check whether graphics operations should be recorded.
/// Stub: returns 0 (FALSE).
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub unsafe fn GRestore(_dd: pGEDevDesc) {
    /* Stub: full implementation calls copyGPar(dpptr(dd), gpptr(dd)) */
}

/// GSavePars -- save inline graphical parameters.
/// Stub: does nothing.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub unsafe fn GSetState(_newstate: c_int, _dd: pGEDevDesc) {
    /* Stub: full implementation sets dpptr(dd)->state = gpptr(dd)->state */
}

/// GCheckState -- enquire whether GNewPlot has been called.
/// Stub: does nothing (does not error).
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub unsafe fn GScale(_min: c_double, _max: c_double, _axis: c_int, _dd: pGEDevDesc) {
    /* Stub: full implementation computes axis parameters based on
    lab, xaxs/yaxs, xlog/ylog from gpptr(dd) and calls GAxisPars */
}

/// GSetupAxis -- set up default axis information when user specifies par(usr=...).
/// Stub: does nothing.
#[unsafe(no_mangle)]
pub unsafe fn GSetupAxis(_axis: c_int, _dd: pGEDevDesc) {
    /* Stub: full implementation calls GPretty and stores axp */
}

/* ========================================================================
 * Clipping
 * ======================================================================== */

/// GClip -- update the device clipping region (depends on GP->xpd).
/// Stub: does nothing.
#[unsafe(no_mangle)]
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

/// gcontextFromGP -- generate an R_GE_gcontext from gpptr info.
/// Stub: does nothing.
pub unsafe fn gcontextFromGP(_gc: pGEcontext, _dd: pGEDevDesc) {
    /* Stub: full implementation copies all graphics context fields
    from gpptr(dd) to the gc struct */
}

/* ========================================================================
 * Graphical primitives
 * ======================================================================== */

/// GLine -- draw a line from (x1,y1) to (x2,y2).
/// Stub: does nothing.
#[unsafe(no_mangle)]
pub unsafe fn GLine(
    _x1: c_double,
    _y1: c_double,
    _x2: c_double,
    _y2: c_double,
    _coords: c_int,
    _dd: pGEDevDesc,
) {
    /* Stub: full implementation converts to DEVICE, clips, calls GELine */
}

/// GLocator -- read the current pen position interactively.
/// Stub: returns 0 (FALSE).
#[unsafe(no_mangle)]
pub unsafe fn GLocator(
    x: *mut c_double,
    y: *mut c_double,
    _coords: c_int,
    _dd: pGEDevDesc,
) -> Rboolean {
    if !x.is_null() {
        *x = 0.0;
    }
    if !y.is_null() {
        *y = 0.0;
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

/// GMode -- set graphics mode (0=off, 1=on, 2=input on).
/// Stub: does nothing.
pub unsafe fn GMode(_mode: c_int, _dd: pGEDevDesc) {
    /* Stub: full implementation calls GEMode and updates devmode */
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
/// Stub: does nothing.
#[unsafe(no_mangle)]
pub unsafe fn GPolygon(
    _n: c_int,
    _x: *mut c_double,
    _y: *mut c_double,
    _coords: c_int,
    _bg: c_int,
    _fg: c_int,
    _dd: pGEDevDesc,
) {
    /* Stub: full implementation converts to DEVICE, clips, calls GEPolygon */
}

/// GPolyline -- draw a series of connected line segments.
/// Stub: does nothing.
pub unsafe fn GPolyline(
    _n: c_int,
    _x: *mut c_double,
    _y: *mut c_double,
    _coords: c_int,
    _dd: pGEDevDesc,
) {
    /* Stub: full implementation converts to DEVICE, clips, calls GEPolyline */
}

/// GCircle -- draw a circle. Filled with color bg and outlined with color fg.
/// Stub: does nothing.
#[unsafe(no_mangle)]
pub unsafe fn GCircle(
    _x: c_double,
    _y: c_double,
    _coords: c_int,
    _radius: c_double,
    _bg: c_int,
    _fg: c_int,
    _dd: pGEDevDesc,
) {
    /* Stub: full implementation converts to DEVICE, clips, calls GECircle */
}

/// GRect -- draw a rectangle. Filled with color bg and outlined with color fg.
/// Stub: does nothing.
pub unsafe fn GRect(
    _x0: c_double,
    _y0: c_double,
    _x1: c_double,
    _y1: c_double,
    _coords: c_int,
    _bg: c_int,
    _fg: c_int,
    _dd: pGEDevDesc,
) {
    /* Stub: full implementation converts to DEVICE, clips, calls GERect */
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
#[unsafe(no_mangle)]
pub unsafe fn GStrWidth(
    _str: *const c_char,
    _enc: cetype_t,
    _units: GUnit,
    _dd: pGEDevDesc,
) -> c_double {
    0.0
}

/// GStrHeight -- compute the height of a string.
/// Stub: returns 0.0.
#[unsafe(no_mangle)]
pub unsafe fn GStrHeight(
    _str: *const c_char,
    _enc: cetype_t,
    _units: GUnit,
    _dd: pGEDevDesc,
) -> c_double {
    0.0
}

/// GText -- draw text at a location.
/// Stub: does nothing.
pub unsafe fn GText(
    _x: c_double,
    _y: c_double,
    _coords: c_int,
    _str: *const c_char,
    _enc: cetype_t,
    _xc: c_double,
    _yc: c_double,
    _rot: c_double,
    _dd: pGEDevDesc,
) {
    /* Stub: full implementation converts to DEVICE, clips, calls GEText */
}

/// GArrow -- draw an arrow from (xfrom,yfrom) to (xto,yto).
/// The length parameter is in inches.
/// Stub: does nothing.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
/// Stub: does nothing.
pub unsafe fn GSymbol(_x: c_double, _y: c_double, _coords: c_int, _pch: c_int, _dd: pGEDevDesc) {
    /* Stub: full implementation converts to DEVICE, clips, calls GESymbol */
}

/* ========================================================================
 * Margin text
 * ======================================================================== */

/// GMtext -- draw text in the plot margins.
/// side: 1=bottom, 2=left, 3=top, 4=right.
/// las: 0=parallel to axis, 1=always horizontal, 2=perpendicular, 3=vertical.
/// Stub: does nothing.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
