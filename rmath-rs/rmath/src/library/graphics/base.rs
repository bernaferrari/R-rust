#![allow(unsafe_op_in_unsafe_fn)] // legacy C-port unsafe boundary; see docs/unsafe-op-allowlist.tsv.
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001-12   The R Core Team.
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 *
 *  Ported from r-source/src/library/graphics/src/base.c
 *
 *  Base graphics functions: text, mtext, title, axis, box, arrows,
 *  segments, rect, polygon, polypath, xspline, symbols, strWidth,
 *  strHeight, locator, identify, clip, abline, plot_window, plot_xy,
 *  etc.
 *
 *  These all depend on the Graphics Engine (GPar, pGEDevDesc, GE
 *  functions) which is not yet fully ported. All functions are stubs
 *  returning R_NilValue().
 */

use std::ffi::c_void;
use std::os::raw::{c_double, c_int};

use crate::sexp::ffi::*;
use crate::sexp::globals::*;

/// pGEDevDesc is an opaque pointer to the graphics device descriptor.
type pGEDevDesc = *mut c_void;

/* ========================================================================
 * String dimension functions
 * ======================================================================== */

/// C_strWidth -- compute the width of a string in the current device.
/// Stub: returns 0.0.
pub unsafe fn C_strWidth(_str: SEXP, _gc: SEXP, _dd: pGEDevDesc) -> c_double {
    0.0
}

/// C_strHeight -- compute the height of a string in the current device.
/// Stub: returns 0.0.
pub unsafe fn C_strHeight(_str: SEXP, _gc: SEXP, _dd: pGEDevDesc) -> c_double {
    0.0
}

/* ========================================================================
 * Text and annotation functions
 * ======================================================================== */

/// C_text -- draw text on the plot.
/// Stub: returns R_NilValue.
pub unsafe fn C_text(
    _x: SEXP,
    _y: SEXP,
    _labels: SEXP,
    _adj: SEXP,
    _pos: SEXP,
    _font: SEXP,
    _vfont: SEXP,
    _col: SEXP,
    _cex: SEXP,
    _rot: SEXP,
) -> SEXP {
    R_NilValue()
}

/// C_mtext -- draw text in the margins of the current plot.
/// Stub: returns R_NilValue.
pub unsafe fn C_mtext(
    _text: SEXP,
    _side: SEXP,
    _line: SEXP,
    _outer: SEXP,
    _at: SEXP,
    _adj: SEXP,
    _padj: SEXP,
    _col: SEXP,
    _cex: SEXP,
    _font: SEXP,
) -> SEXP {
    R_NilValue()
}

/// C_title -- add a title to the current plot.
/// Stub: returns R_NilValue.
pub unsafe fn C_title(
    _main: SEXP,
    _sub: SEXP,
    _xlab: SEXP,
    _ylab: SEXP,
    _line: SEXP,
    _outer: SEXP,
    _col: SEXP,
    _cex: SEXP,
    _font: SEXP,
) -> SEXP {
    R_NilValue()
}

/* ========================================================================
 * Axis functions
 * ======================================================================== */

/// C_axis -- draw an axis on the current plot.
/// Stub: returns R_NilValue.
pub unsafe fn C_axis(
    _side: SEXP,
    _at: SEXP,
    _labels: SEXP,
    _tick: SEXP,
    _line: SEXP,
    _pos: SEXP,
    _outer: SEXP,
    _font: SEXP,
    _lty: SEXP,
    _lwd: SEXP,
    _col: SEXP,
    _las: SEXP,
    _hadj: SEXP,
    _padj: SEXP,
    _gap_axis: SEXP,
) -> SEXP {
    R_NilValue()
}

/// C_box -- draw a box around the current plot.
/// Stub: returns R_NilValue.
pub unsafe fn C_box(_which: SEXP, _lty: SEXP, _lwd: SEXP, _col: SEXP, _fill: SEXP) -> SEXP {
    R_NilValue()
}

/* ========================================================================
 * Line and arrow functions
 * ======================================================================== */

/// C_arrows -- draw arrows between pairs of points.
/// Stub: returns R_NilValue.
pub unsafe fn C_arrows(
    _x1: SEXP,
    _y1: SEXP,
    _x2: SEXP,
    _y2: SEXP,
    _angle: SEXP,
    _length: SEXP,
    _code: SEXP,
    _col: SEXP,
    _lty: SEXP,
    _lwd: SEXP,
) -> SEXP {
    R_NilValue()
}

/// C_segments -- draw line segments between pairs of points.
/// Stub: returns R_NilValue.
pub unsafe fn C_segments(
    _x1: SEXP,
    _y1: SEXP,
    _x2: SEXP,
    _y2: SEXP,
    _col: SEXP,
    _lty: SEXP,
    _lwd: SEXP,
) -> SEXP {
    R_NilValue()
}

/// C_abline -- add a line (or lines) to the current plot.
/// Stub: returns R_NilValue.
pub unsafe fn C_abline(
    _a: SEXP,
    _b: SEXP,
    _h: SEXP,
    _v: SEXP,
    _untf: SEXP,
    _col: SEXP,
    _lty: SEXP,
    _lwd: SEXP,
) -> SEXP {
    R_NilValue()
}

/* ========================================================================
 * Rectangle and polygon functions
 * ======================================================================== */

/// C_rect -- draw rectangles on the current plot.
/// Stub: returns R_NilValue.
pub unsafe fn C_rect(
    _xleft: SEXP,
    _ybottom: SEXP,
    _xright: SEXP,
    _ytop: SEXP,
    _density: SEXP,
    _angle: SEXP,
    _border: SEXP,
    _col: SEXP,
    _lty: SEXP,
    _lwd: SEXP,
) -> SEXP {
    R_NilValue()
}

/// C_polygon -- draw a polygon on the current plot.
/// Stub: returns R_NilValue.
pub unsafe fn C_polygon(
    _x: SEXP,
    _y: SEXP,
    _density: SEXP,
    _angle: SEXP,
    _border: SEXP,
    _col: SEXP,
    _lty: SEXP,
    _lwd: SEXP,
) -> SEXP {
    R_NilValue()
}

/// C_polypath -- draw a path (possibly with holes) on the current plot.
/// Stub: returns R_NilValue.
pub unsafe fn C_polypath(
    _x: SEXP,
    _y: SEXP,
    _perimeter: SEXP,
    _rule: SEXP,
    _col: SEXP,
    _border: SEXP,
    _lty: SEXP,
    _lwd: SEXP,
) -> SEXP {
    R_NilValue()
}

/// C_xspline -- draw an X-spline (smooth curve) on the current plot.
/// Stub: returns R_NilValue.
pub unsafe fn C_xspline(
    _x: SEXP,
    _y: SEXP,
    _s: SEXP,
    _open: SEXP,
    _repEnds: SEXP,
    _col: SEXP,
    _border: SEXP,
    _lty: SEXP,
    _lwd: SEXP,
) -> SEXP {
    R_NilValue()
}

/* ========================================================================
 * Symbol plotting functions
 * ======================================================================== */

/// C_symbols -- draw plotting symbols on the current plot.
/// Stub: returns R_NilValue.
pub unsafe fn C_symbols(
    _x: SEXP,
    _y: SEXP,
    _inches: SEXP,
    _circles: SEXP,
    _squares: SEXP,
    _rectangles: SEXP,
    _stars: SEXP,
    _thermometers: SEXP,
    _boxplots: SEXP,
    _add: SEXP,
    _fg: SEXP,
    _bg: SEXP,
    _xlim: SEXP,
    _ylim: SEXP,
) -> SEXP {
    R_NilValue()
}

/* ========================================================================
 * Interactive functions
 * ======================================================================== */

/// C_locator -- identify points on the plot interactively.
/// Stub: returns R_NilValue.
pub unsafe fn C_locator(
    _call: SEXP,
    _untyped: SEXP,
    _n: SEXP,
    _type_: SEXP,
    _tol: SEXP,
    _x: SEXP,
    _y: SEXP,
) -> SEXP {
    R_NilValue()
}

/// C_identify -- identify points on the plot interactively.
/// Stub: returns R_NilValue.
pub unsafe fn C_identify(
    _call: SEXP,
    _x: SEXP,
    _y: SEXP,
    _labels: SEXP,
    _n: SEXP,
    _plot: SEXP,
    _offset: SEXP,
    _tol: SEXP,
    _atpen: SEXP,
    _order: SEXP,
) -> SEXP {
    R_NilValue()
}

/* ========================================================================
 * Clipping and coordinate functions
 * ======================================================================== */

/// C_clip -- set clipping region on the current plot.
/// Stub: returns R_NilValue.
pub unsafe fn C_clip(_x1: SEXP, _y1: SEXP, _x2: SEXP, _y2: SEXP) -> SEXP {
    R_NilValue()
}

/* ========================================================================
 * Plot window and coordinate setup
 * ======================================================================== */

/// C_plot_window -- set up the plot window (plot.new() / frame()).
/// Stub: returns R_NilValue.
pub unsafe fn C_plot_window(
    _xlim: SEXP,
    _ylim: SEXP,
    _log: SEXP,
    _asp: SEXP,
    _xaxs: SEXP,
    _yaxs: SEXP,
) -> SEXP {
    R_NilValue()
}

/// C_plot_xy -- set up the plot coordinates and draw the axes/frame.
/// Stub: returns R_NilValue.
pub unsafe fn C_plot_xy(
    _x: SEXP,
    _y: SEXP,
    _type_: SEXP,
    _xlim: SEXP,
    _ylim: SEXP,
    _log: SEXP,
    _main: SEXP,
    _sub: SEXP,
    _xlab: SEXP,
    _ylab: SEXP,
    _ann: SEXP,
    _asp: SEXP,
    _xaxs: SEXP,
    _yaxs: SEXP,
) -> SEXP {
    R_NilValue()
}

/* ========================================================================
 * Raster / image functions
 * ======================================================================== */

/// C_raster -- draw a raster image on the plot.
/// Stub: returns R_NilValue.
pub unsafe fn C_raster(
    _x: SEXP,
    _y: SEXP,
    _width: SEXP,
    _height: SEXP,
    _img: SEXP,
    _interpolate: SEXP,
) -> SEXP {
    R_NilValue()
}

/* ========================================================================
 * Grid / grid lines
 * ======================================================================== */

/// C_grid -- draw a grid on the current plot.
/// Stub: returns R_NilValue.
pub unsafe fn C_grid(_nx: SEXP, _ny: SEXP, _col: SEXP, _lty: SEXP, _lwd: SEXP) -> SEXP {
    R_NilValue()
}

/* ========================================================================
 * Contour lines
 * ======================================================================== */

/// C_contourLines -- compute contour lines.
/// Stub: returns R_NilValue.
pub unsafe fn C_contourLines(_x: SEXP, _y: SEXP, _z: SEXP, _levels: SEXP) -> SEXP {
    R_NilValue()
}

/* ========================================================================
 * Hershey vector font support (used by text, mtext, title, axis)
 * ======================================================================== */

/// C_HersheyStroke -- get the stroke for a Hershey glyph.
/// Stub: returns R_NilValue.
pub unsafe fn C_HersheyStroke(_which: SEXP, _index: SEXP) -> SEXP {
    R_NilValue()
}

/// C_HersheyWidth -- get the width of a Hershey glyph.
/// Stub: returns 0.
pub unsafe fn C_HersheyWidth(_which: SEXP, _index: SEXP) -> c_double {
    0.0
}

/// C_HersheyList -- list all available Hershey fonts and glyphs.
/// Stub: returns R_NilValue.
pub unsafe fn C_HersheyList() -> SEXP {
    R_NilValue()
}

/// C_HersheyGlyph -- get a Hershey glyph as a character string.
/// Stub: returns R_NilValue.
pub unsafe fn C_HersheyGlyph(_which: SEXP, _index: SEXP) -> SEXP {
    R_NilValue()
}
