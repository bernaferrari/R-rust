/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1998--2026  The R Core Team
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
 *  Ported from r-source/src/library/graphics/src/plot3d.c
 *
 *  3D perspective plotting functions: persp, contour, filled.contour, image.
 *
 *  These depend heavily on R's Graphics Engine (GPar, GEdevice, etc.).
 *  Pure algorithm functions (transformation math, contour finding, cut points)
 *  have real implementations. Functions requiring the GE are stubs returning
 *  R_NilValue().
 */

use std::ffi::c_void;
use std::os::raw::{c_char, c_double, c_int};

use super::plot::{FixupCol, FixupLty, FixupLwd, FixupVFont};
use crate::main::coerce::{asInteger, asLogical, asReal, coerceVector};
use crate::mainutils::sort::rsort_with_index;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::protect::*;

/* ========================================================================
 * Stub declarations for Graphics Engine types and functions
 * ======================================================================== */

/// pGEDevDesc is an opaque pointer to the graphics device descriptor.
type pGEDevDesc = *mut c_void;

/// Opaque R color type (unsigned int).
type rcolor = u32;

/// cetype_t: character encoding type.
type cetype_t = c_int;

/// CE_NATIVE constant.
const CE_NATIVE: cetype_t = 1;

/// R_TRANWHITE: transparent white color constant.
const R_TRANWHITE: u32 = 0x00FFFFFF;

/// R_PosInf constant.
const R_PosInf: c_double = f64::INFINITY;

/// R_NegInf constant.
const R_NegInf: c_double = f64::NEG_INFINITY;

/// DBL_MAX constant.
const DBL_MAX: c_double = f64::MAX;

/// DBL_MIN constant (smallest positive normal f64).
const DBL_MIN: c_double = f64::MIN_POSITIVE;

/// DEG2RAD: conversion factor from degrees to radians.
const DEG2RAD: c_double = std::f64::consts::PI / 180.0;

/// USER coordinate system constant (for GLine, GPolygon, etc.).
const USER: c_int = 1;

/// NPC coordinate system constant.
const NPC: c_int = 2;

/// INCHES coordinate system constant.
const INCHES: c_int = 5;

/// NDC coordinate system constant.
const NDC: c_int = 6;

/// LTY_SOLID line type constant.
const LTY_SOLID: c_int = 1;

/// LTY_DOTTED line type constant.
const LTY_DOTTED: c_int = 3;

/// max_contour_segments: safety limit for contour tracing loops.
const max_contour_segments: c_int = 25000;

// NA_STRING sentinel (non-null pointer for NA character).
// This is a stub; the real value comes from R internals.
// Not used directly in our stubs; defined only to prevent linker errors.

/* ========================================================================
 * 3D Transformation Math (real implementations)
 * ======================================================================== */

/// Vector3d: a 4-element homogeneous 3D vector.
type Vector3d = [c_double; 4];

/// Trans3d: a 4x4 transformation matrix.
type Trans3d = [[c_double; 4]; 4];

pub(crate) struct Plot3dState {
    vt: Trans3d,
    light: [c_double; 4],
    shade: c_double,
    do_lighting: bool,
}

impl Default for Plot3dState {
    fn default() -> Self {
        Plot3dState {
            vt: [[0.0; 4]; 4],
            light: [0.0; 4],
            shade: 1.0,
            do_lighting: false,
        }
    }
}

#[inline]
fn with_plot3d_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut Plot3dState) -> R,
{
    with_required_current_instance(|instance| f(&mut instance.plot3d_state))
}

fn set_vt_identity() {
    with_plot3d_state(|state| {
        state.vt = [[0.0; 4]; 4];
        for i in 0..4 {
            state.vt[i][i] = 1.0;
        }
    });
}

/// Transform a 3D vector by a 4x4 transformation matrix.
/// Real implementation of the matrix-vector product.
fn TransVector(u: &Vector3d, t: *const Trans3d, v: &mut Vector3d) {
    unsafe {
        for i in 0..4 {
            let mut sum = 0.0;
            for j in 0..4 {
                sum += u[j] * (*t)[j][i];
            }
            v[i] = sum;
        }
    }
}

/// Accumulate (right-multiply) a transformation into the global VT matrix.
/// VT := VT * T
fn Accumulate(t: &Trans3d) {
    with_plot3d_state(|state| {
        let vt = state.vt;
        let mut u: Trans3d = [[0.0; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += vt[i][k] * t[k][j];
                }
                u[i][j] = sum;
            }
        }
        state.vt = u;
    });
}

/// Set a 4x4 transformation matrix to the identity.
fn SetToIdentity(t: *mut Trans3d) {
    unsafe {
        for i in 0..4 {
            for j in 0..4 {
                (*t)[i][j] = 0.0;
            }
            (*t)[i][i] = 1.0;
        }
    }
}

/// Apply a translation to the viewing transformation.
fn Translate(x: c_double, y: c_double, z: c_double) {
    let mut t: Trans3d = [[0.0; 4]; 4];
    SetToIdentity(t.as_mut_ptr() as *mut Trans3d);
    t[3][0] = x;
    t[3][1] = y;
    t[3][2] = z;
    Accumulate(&t);
}

/// Apply a scaling to the viewing transformation.
fn Scale(x: c_double, y: c_double, z: c_double) {
    let mut t: Trans3d = [[0.0; 4]; 4];
    SetToIdentity(t.as_mut_ptr() as *mut Trans3d);
    t[0][0] = x;
    t[1][1] = y;
    t[2][2] = z;
    Accumulate(&t);
}

/// Apply a rotation about the X axis to the viewing transformation.
fn XRotate(angle: c_double) {
    let mut t: Trans3d = [[0.0; 4]; 4];
    SetToIdentity(t.as_mut_ptr() as *mut Trans3d);
    let rad = DEG2RAD * angle;
    let c = rad.cos();
    let s = rad.sin();
    t[1][1] = c;
    t[2][1] = -s;
    t[2][2] = c;
    t[1][2] = s;
    Accumulate(&t);
}

/// Apply a rotation about the Y axis to the viewing transformation.
fn YRotate(angle: c_double) {
    let mut t: Trans3d = [[0.0; 4]; 4];
    SetToIdentity(t.as_mut_ptr() as *mut Trans3d);
    let rad = DEG2RAD * angle;
    let c = rad.cos();
    let s = rad.sin();
    t[0][0] = c;
    t[2][0] = s;
    t[2][2] = c;
    t[0][2] = -s;
    Accumulate(&t);
}

/// Apply a rotation about the Z axis to the viewing transformation.
fn ZRotate(angle: c_double) {
    let mut t: Trans3d = [[0.0; 4]; 4];
    SetToIdentity(t.as_mut_ptr() as *mut Trans3d);
    let rad = DEG2RAD * angle;
    let c = rad.cos();
    let s = rad.sin();
    t[0][0] = c;
    t[1][0] = -s;
    t[1][1] = c;
    t[0][1] = s;
    Accumulate(&t);
}

/// Apply a perspective projection to the viewing transformation.
fn Perspective(d: c_double) {
    let mut t: Trans3d = [[0.0; 4]; 4];
    SetToIdentity(t.as_mut_ptr() as *mut Trans3d);
    t[2][3] = -1.0 / d;
    Accumulate(&t);
}

/* ========================================================================
 * Lighting support (real implementations)
 * ======================================================================== */

/// Set up the light source direction from spherical coordinates.
fn SetUpLight(theta: c_double, phi: c_double) {
    let u: Vector3d = [0.0, -1.0, 0.0, 1.0];
    set_vt_identity();
    XRotate(-phi);
    ZRotate(theta);
    let vt_val = with_plot3d_state(|state| state.vt);
    let mut light: [c_double; 4] = [0.0; 4];
    TransVector(&u, &vt_val, &mut light);
    with_plot3d_state(|state| state.light = light);
}

/// Compute the shading factor for a facet given two edge vectors.
fn FacetShade(u: &[c_double], v: &[c_double]) -> c_double {
    let nx = u[1] * v[2] - u[2] * v[1];
    let ny = u[2] * v[0] - u[0] * v[2];
    let nz = u[0] * v[1] - u[1] * v[0];
    let mut sum = (nx * nx + ny * ny + nz * nz).sqrt();
    if sum == 0.0 {
        sum = 1.0;
    }
    let nx = nx / sum;
    let ny = ny / sum;
    let nz = nz / sum;
    let (light, shade) = with_plot3d_state(|state| (state.light, state.shade));
    let s = 0.5 * (nx * light[0] + ny * light[1] + nz * light[2] + 1.0);
    s.powf(shade)
}

/* ========================================================================
 * Filled Contour: FindCutPoints / FindPolygonVertices (real implementations)
 * ======================================================================== */

/// Find the points where the line segment from (x1,y1,z1) to (x2,y2,z2)
/// crosses the horizontal planes z = low and z = high.
fn FindCutPoints(
    low: c_double,
    high: c_double,
    x1: c_double,
    y1: c_double,
    z1: c_double,
    x2: c_double,
    y2: c_double,
    z2: c_double,
    x: &mut [c_double],
    y: &mut [c_double],
    z: &mut [c_double],
    npt: &mut c_int,
) {
    if z1 > z2 {
        if z2 > high || z1 < low {
            return;
        }
        if z1 < high {
            x[*npt as usize] = x1;
            y[*npt as usize] = y1;
            z[*npt as usize] = z1;
            *npt += 1;
        } else if z1 == R_PosInf {
            x[*npt as usize] = x2;
            y[*npt as usize] = y1;
            z[*npt as usize] = z2;
            *npt += 1;
        } else {
            /* z1 >= high, z2 in range */
            let c = (z1 - high) / (z1 - z2);
            x[*npt as usize] = x1 + c * (x2 - x1);
            y[*npt as usize] = y1;
            z[*npt as usize] = z1 + c * (z2 - z1);
            *npt += 1;
        }
        if z2 == R_NegInf {
            x[*npt as usize] = x1;
            y[*npt as usize] = y1;
            z[*npt as usize] = z1;
            *npt += 1;
        } else if z2 <= low {
            /* and z1 in range */
            let c = (z2 - low) / (z2 - z1);
            x[*npt as usize] = x2 - c * (x2 - x1);
            y[*npt as usize] = y1;
            z[*npt as usize] = z2 - c * (z2 - z1);
            *npt += 1;
        }
    } else if z1 < z2 {
        if z2 < low || z1 > high {
            return;
        }
        if z1 > low {
            x[*npt as usize] = x1;
            y[*npt as usize] = y1;
            z[*npt as usize] = z1;
            *npt += 1;
        } else if z1 == R_NegInf {
            x[*npt as usize] = x2;
            y[*npt as usize] = y1;
            z[*npt as usize] = z2;
            *npt += 1;
        } else {
            /* and z2 in range */
            let c = (z1 - low) / (z1 - z2);
            x[*npt as usize] = x1 + c * (x2 - x1);
            y[*npt as usize] = y1;
            z[*npt as usize] = z1 + c * (z2 - z1);
            *npt += 1;
        }
        if z2 < high {
            /* Don't repeat corner vertices (OMIT) */
        } else if z2 == R_PosInf {
            x[*npt as usize] = x1;
            y[*npt as usize] = y1;
            z[*npt as usize] = z1;
            *npt += 1;
        } else {
            /* z2 high, z1 in range */
            let c = (z2 - high) / (z2 - z1);
            x[*npt as usize] = x2 - c * (x2 - x1);
            y[*npt as usize] = y1;
            z[*npt as usize] = z2 - c * (z2 - z1);
            *npt += 1;
        }
    } else {
        /* z1 == z2 */
        if low <= z1 && z1 <= high {
            x[*npt as usize] = x1;
            y[*npt as usize] = y1;
            z[*npt as usize] = z1;
            *npt += 1;
        }
    }
}

/// Find the vertices of a polygon formed by clipping a grid cell to the
/// horizontal slab between z = low and z = high.
fn FindPolygonVertices(
    low: c_double,
    high: c_double,
    x1: c_double,
    x2: c_double,
    y1: c_double,
    y2: c_double,
    z11: c_double,
    z21: c_double,
    z12: c_double,
    z22: c_double,
    x: &mut [c_double],
    y: &mut [c_double],
    z: &mut [c_double],
    npt: &mut c_int,
) {
    *npt = 0;
    /* Bottom edge: (x1,y1,z11) -> (x2,y1,z21) */
    FindCutPoints(low, high, x1, y1, z11, x2, y1, z21, x, y, z, npt);
    /* Right edge: (x2,y1,z21) -> (x2,y2,z22) */
    FindCutPoints(low, high, y1, x2, z21, y2, x2, z22, y, x, z, npt);
    /* Top edge: (x2,y2,z22) -> (x1,y2,z12) */
    FindCutPoints(low, high, x2, y2, z22, x1, y2, z12, x, y, z, npt);
    /* Left edge: (x1,y2,z12) -> (x1,y1,z11) */
    FindCutPoints(low, high, y2, x1, z12, y1, x1, z11, y, x, z, npt);
}

/* ========================================================================
 * Perspective: depth ordering and other helpers (real implementations)
 * ======================================================================== */

/// For each facet, determine the farthest point from the eye.
/// Sorting facets by depth yields an occlusion-compatible ordering.
fn DepthOrder(
    z: *const c_double,
    x: *const c_double,
    y: *const c_double,
    nx: c_int,
    ny: c_int,
    depth: *mut c_double,
    indx: *mut c_int,
) {
    let nx1 = nx - 1;
    let ny1 = ny - 1;
    for i in 0..(nx1 * ny1) as usize {
        unsafe {
            *indx.add(i) = i as c_int;
        }
    }
    for i in 0..nx1 as usize {
        for j in 0..ny1 as usize {
            let mut d = -DBL_MAX;
            for ii in 0..=1 {
                for jj in 0..=1 {
                    let mut u: Vector3d = [0.0; 4];
                    let mut v: Vector3d = [0.0; 4];
                    unsafe {
                        u[0] = *x.add(i + ii as usize);
                        u[1] = *y.add(j + jj as usize);
                    }
                    u[2] = 0.0;
                    u[3] = 1.0;
                    if u[0].is_finite() && u[1].is_finite() && u[2].is_finite() {
                        let vt_val = with_plot3d_state(|state| state.vt);
                        TransVector(&u, &vt_val, &mut v);
                        if v[3] > d {
                            d = v[3];
                        }
                    }
                }
            }
            unsafe {
                *depth.add(i + j * nx1 as usize) = -d;
            }
        }
    }
    unsafe {
        rsort_with_index(depth, indx, nx1 * ny1);
    }
}

/// Check that limits are valid: both finite and strictly increasing.
/// Returns true if valid; sets center `c` and scale `s`.
fn LimitCheck(lim: *const c_double, c: &mut c_double, s: &mut c_double) -> bool {
    unsafe {
        let l0 = *lim.add(0);
        let l1 = *lim.add(1);
        if !l0.is_finite() || !l1.is_finite() || l0 >= l1 {
            return false;
        }
        *s = 0.5 * (l1 - l0).abs();
        *c = 0.5 * (l1 + l0);
        true
    }
}

/* ========================================================================
 * Contour label helpers (real implementations where possible)
 * ======================================================================== */

/// Check if y1 is the lowest of the four values.
fn lowest(y1: c_double, y2: c_double, y3: c_double, y4: c_double) -> bool {
    y1 <= y2 && y1 <= y3 && y1 <= y4
}

/// Compute the angle (in degrees) of a line segment.
fn labelAngle(x1: c_double, y1: c_double, x2: c_double, y2: c_double) -> c_double {
    let dx = (x2 - x1).abs();
    let dy = if x2 > x1 { y2 - y1 } else { y1 - y2 };
    if dx == 0.0 {
        if dy > 0.0 { 90.0 } else { 270.0 }
    } else {
        (180.0 / std::f64::consts::PI) * dy.atan2(dx)
    }
}

/// Minimum of two doubles (matching R's fmin2).
fn fmin2(a: c_double, b: c_double) -> c_double {
    if a < b { a } else { b }
}

/* ========================================================================
 * Static lookup tables for PerspBox / PerspAxes (real data)
 * ======================================================================== */

/// The vertices of the bounding box.
static VERTEX: [[c_int; 3]; 8] = [
    [0, 0, 0],
    [0, 0, 1],
    [0, 1, 0],
    [0, 1, 1],
    [1, 0, 0],
    [1, 0, 1],
    [1, 1, 0],
    [1, 1, 1],
];

/// The vertices visited when tracing a face.
static FACE: [[c_int; 4]; 6] = [
    [0, 1, 5, 4],
    [2, 6, 7, 3],
    [0, 2, 3, 1],
    [4, 5, 7, 6],
    [0, 4, 6, 2],
    [1, 3, 7, 5],
];

/// The edges drawn when tracing a face.
static EDGE: [[c_int; 4]; 6] = [
    [0, 1, 2, 3],
    [4, 5, 6, 7],
    [8, 7, 9, 0],
    [2, 10, 5, 11],
    [3, 11, 4, 8],
    [9, 6, 10, 1],
];

/// Starting vertex for possible axes.
static AXIS_START: [c_int; 8] = [0, 0, 2, 4, 0, 4, 2, 6];

/// Tick vector for possible axes.
static TICK_VECTOR: [[c_int; 3]; 8] = [
    [0, -1, -1],
    [-1, 0, -1],
    [0, 1, -1],
    [1, 0, -1],
    [-1, -1, 0],
    [1, -1, 0],
    [-1, 1, 0],
    [1, 1, 0],
];

/* ========================================================================
 * Stub module-level functions that depend on the GE
 * ======================================================================== */

/// Draw the perspective bounding box (stub).
fn PerspBox(
    _front: c_int,
    _x: *const c_double,
    _y: *const c_double,
    _z: *const c_double,
    _edge_done: *mut c_char,
    _dd: pGEDevDesc,
) {
    /* Stub: real implementation requires GE drawing primitives */
}

/// Set up the perspective plotting window (stub).
fn PerspWindow(
    _xlim: *const c_double,
    _ylim: *const c_double,
    _zlim: *const c_double,
    _dd: pGEDevDesc,
) {
    /* Stub: real implementation requires GE coordinate conversion */
}

/// Draw facets on the perspective surface (stub).
fn DrawFacets(
    _z: *const c_double,
    _x: *const c_double,
    _y: *const c_double,
    _nx: c_int,
    _ny: c_int,
    _indx: *const c_int,
    _xs: c_double,
    _ys: c_double,
    _zs: c_double,
    _col: *const c_int,
    _ncol: c_int,
    _border: c_int,
) {
    /* Stub: real implementation requires GE polygon drawing */
}

/// Draw a single perspective axis (stub).
fn PerspAxis(
    _x: *const c_double,
    _y: *const c_double,
    _z: *const c_double,
    _axis: c_int,
    _axis_type: c_int,
    _n_ticks: c_int,
    _tick_type: c_int,
    _label: *const c_char,
    _enc: cetype_t,
    _dd: pGEDevDesc,
) {
    /* Stub: real implementation requires GE text/line drawing */
}

/// Draw all perspective axes (stub).
fn PerspAxes(
    _x: *const c_double,
    _y: *const c_double,
    _z: *const c_double,
    _xlab: *const c_char,
    _xenc: cetype_t,
    _ylab: *const c_char,
    _yenc: cetype_t,
    _zlab: *const c_char,
    _zenc: cetype_t,
    _n_ticks: c_int,
    _tick_type: c_int,
    _dd: pGEDevDesc,
) {
    /* Stub: real implementation requires GE text/line drawing */
}

/// Find the corners of a contour label box (stub: requires GE).
fn FindCorners(
    _width: c_double,
    _height: c_double,
    _label: SEXP,
    _x0: c_double,
    _y0: c_double,
    _x1: c_double,
    _y1: c_double,
    _dd: pGEDevDesc,
) {
    /* Stub: requires GE coordinate conversion */
}

/// Test if two label boxes intersect (real implementation).
fn TestLabelIntersection(label1: SEXP, label2: SEXP) -> c_int {
    unsafe {
        let r1 = REAL(label1);
        let r2 = REAL(label2);
        for i in 0..4 {
            let ax = *r1.add(i);
            let ay = *r1.add(i + 4);
            let bx = *r1.add((i + 1) % 4);
            let by = *r1.add((i + 1) % 4 + 4);
            for j in 0..4 {
                let cx = *r2.add(j);
                let cy = *r2.add(j + 4);
                let dx = *r2.add((j + 1) % 4);
                let dy = *r2.add((j + 1) % 4 + 4);

                let dom =
                    bx * dy - bx * ay - ax * dy + ax * ay - dx * by + dx * ay + cx * by - cx * ay;
                let (result1, result2) = if dom == 0.0 {
                    (-1.0, -1.0)
                } else {
                    let r1 = (dx * ay - cx * ay - ay * dx - ax * dy + ax * ay + dy * cx) / dom;
                    let r2 = if dx - cx == 0.0 {
                        if dy - cy == 0.0 {
                            -1.0
                        } else {
                            (ay + (by - ay) * r1 - cy) / (dy - cy)
                        }
                    } else {
                        (ax + (bx - ax) * r1 - cx) / (dx - cx)
                    };
                    (r1, r2)
                };
                let l1 = result1 >= 0.0 && result1 <= 1.0;
                let l2 = result2 >= 0.0 && result2 <= 1.0;
                if l1 && l2 {
                    return 1;
                }
            }
        }
        0
    }
}

/// Check if a label box is inside the window (stub: requires GE).
fn LabelInsideWindow(_label: SEXP, _dd: pGEDevDesc) -> c_int {
    0
}

/// Find a gap upward in the contour segment list (stub: requires GE).
fn findGapUp(
    _xxx: *const c_double,
    _yyy: *const c_double,
    _ns: c_int,
    _label_distance: c_double,
    _dd: pGEDevDesc,
) -> c_int {
    0
}

/// Find a gap downward in the contour segment list (stub: requires GE).
fn findGapDown(
    _xxx: *const c_double,
    _yyy: *const c_double,
    _ns: c_int,
    _label_distance: c_double,
    _dd: pGEDevDesc,
) -> c_int {
    0
}

/// Compute distance from a point to the nearest edge of the plot region (stub).
fn distFromEdge(
    _xxx: *const c_double,
    _yyy: *const c_double,
    _iii: c_int,
    _dd: pGEDevDesc,
) -> c_double {
    0.0
}

/// Determine whether to use the start of a contour for labelling (stub).
fn useStart(_xxx: *const c_double, _yyy: *const c_double, _ns: c_int, _dd: pGEDevDesc) -> bool {
    true
}

/// Draw a single contour level (stub: requires GE drawing and contourLines).
unsafe fn contour(
    _x: SEXP,
    _nx: c_int,
    _y: SEXP,
    _ny: c_int,
    _z: SEXP,
    _zc: c_double,
    _labels: SEXP,
    _cnum: c_int,
    _draw_labels: bool,
    _method: c_int,
    _atom: c_double,
    _dd: pGEDevDesc,
    _label_list: SEXP,
) -> SEXP {
    unsafe { R_NilValue() }
}

/* ========================================================================
 * C_filledcontour -- Filled Contour Plots
 * ======================================================================== */

/* ========================================================================
 * Module-private helpers (same as other modules define locally)
 * ======================================================================== */

/// Check if SEXP has a dim attribute with >= 2 elements (i.e., is a matrix).
unsafe fn isMatrix(x: SEXP) -> bool {
    unsafe {
        let dim = crate::attrib_core::getAttrib(x, crate::attrib_core::R_DimSymbol());
        !R_NilValue().is_null() && !dim.is_null() && LENGTH(dim) >= 2
    }
}

/// Get the number of rows from a matrix's dim attribute.
unsafe fn nrows(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let dim = ATTRIB(x);
        if dim.is_null() || dim == R_NilValue() {
            return 0;
        }
        if TYPEOF(dim) != SEXPTYPE::INTSXP {
            return 0;
        }
        let len = LENGTH(dim);
        if len < 2 {
            return if len == 1 { *INTEGER(dim).add(0) } else { 0 };
        }
        *INTEGER(dim).add(0)
    }
}

/// Get the number of columns from a matrix's dim attribute.
unsafe fn ncols(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        let dim = ATTRIB(x);
        if dim.is_null() || dim == R_NilValue() {
            return 0;
        }
        if TYPEOF(dim) != SEXPTYPE::INTSXP {
            return 0;
        }
        let len = LENGTH(dim);
        if len < 2 {
            return 1;
        }
        *INTEGER(dim).add(1)
    }
}

/// C_filledcontour -- draw a filled contour plot.
/// Ported from plot3d.c C_filledcontour().
/// Uses real FindPolygonVertices algorithm; GE drawing calls are stubs.
pub unsafe fn C_filledcontour(args: SEXP) -> SEXP {
    unsafe {
        let mut _args = CDR(args);
        if LENGTH(_args) < 5 {
            /* too few arguments - stub: no error reporting available */
            return R_NilValue();
        }

        let sx = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        let _nx = LENGTH(sx);
        _args = CDR(_args);

        let sy = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        let _ny = LENGTH(sy);
        _args = CDR(_args);

        if _nx < 2 || _ny < 2 {
            /* insufficient x or y values */
            Rf_unprotect(2);
            return R_NilValue();
        }

        let sz = CAR(_args);
        if nrows(sz) != _nx || ncols(sz) != _ny {
            /* dimension mismatch */
            Rf_unprotect(2);
            return R_NilValue();
        }
        let _sz = Rf_protect(coerceVector(sz, SEXPTYPE::REALSXP.into()));
        _args = CDR(_args);

        let _sc = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        let _nc = LENGTH(_sc);
        _args = CDR(_args);

        if _nc < 1 {
            /* no contour values */
            Rf_unprotect(4);
            return R_NilValue();
        }

        let _scol = Rf_protect(FixupCol(CAR(_args), R_TRANWHITE));
        let _ncol = LENGTH(_scol);

        /* Real algorithm: FindPolygonVertices for each cell/level pair.
         * GE drawing calls (GPolygon) are stubs so no visible output. */
        let _x = REAL(sx);
        let _y = REAL(sy);
        let _z = REAL(_sz);
        let _c = REAL(_sc);
        let _col: *const u32 = INTEGER(_scol) as *const u32;

        let mut _px: [c_double; 8] = [0.0; 8];
        let mut _py: [c_double; 8] = [0.0; 8];
        let mut _pz: [c_double; 8] = [0.0; 8];
        let mut _npt: c_int = 0;

        for i in 1.._nx {
            for j in 1.._ny {
                for k in 1.._nc {
                    _npt = 0;
                    FindPolygonVertices(
                        *_c.add((k - 1) as usize),
                        *_c.add(k as usize),
                        *_x.add((i - 1) as usize),
                        *_x.add(i as usize),
                        *_y.add((j - 1) as usize),
                        *_y.add(j as usize),
                        *_z.add((i - 1 + (j - 1) * _nx) as usize),
                        *_z.add((i + (j - 1) * _nx) as usize),
                        *_z.add((i - 1 + j * _nx) as usize),
                        *_z.add((i + j * _nx) as usize),
                        &mut _px,
                        &mut _py,
                        &mut _pz,
                        &mut _npt,
                    );
                    if _npt > 2 { /* GPolygon call would go here -- stub */ }
                }
            }
        }

        Rf_unprotect(5);
        R_NilValue()
    }
}

/* ========================================================================
 * C_image -- Image Rendering
 * ======================================================================== */

/// C_image -- draw an image plot.
/// Ported from plot3d.c C_image().
/// GE drawing calls (GRect) are stubs.
pub unsafe fn C_image(args: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::*;

        let mut _args = CDR(args);

        let sx = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        let _nx = LENGTH(sx);
        _args = CDR(_args);

        let sy = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        let _ny = LENGTH(sy);
        _args = CDR(_args);

        let _sz = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::INTSXP.into()));
        _args = CDR(_args);

        let _sc = Rf_protect(FixupCol(CAR(_args), R_TRANWHITE));
        let _nc = LENGTH(_sc);

        let _x = REAL(sx);
        let _y = REAL(sy);
        let _z = INTEGER(_sz);
        let _c = INTEGER(_sc) as *const u32;

        for i in 0..(_nx - 1) {
            for j in 0..(_ny - 1) {
                let tmp = *_z.add((i + j * (_nx - 1)) as usize);
                if tmp >= 0 && tmp < _nc && tmp != crate::sexp::ffi::NA_INTEGER {
                    /* GRect call would go here -- stub */
                }
            }
        }

        Rf_unprotect(4);
        R_NilValue()
    }
}

/* ========================================================================
 * C_persp -- Perspective Surface Plots
 * ======================================================================== */

/// C_persp -- draw a 3D perspective surface plot.
/// Ported from plot3d.c C_persp().
/// GE drawing calls are stubs; transformation math is real.
pub unsafe fn C_persp(args: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::*;

        let mut _args = CDR(args);
        if LENGTH(_args) < 24 {
            /* too few parameters -- stub */
            return R_NilValue();
        }

        let x = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        if LENGTH(x) < 2 {
            Rf_unprotect(1);
            return R_NilValue();
        }
        _args = CDR(_args);

        let y = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        if LENGTH(y) < 2 {
            Rf_unprotect(2);
            return R_NilValue();
        }
        _args = CDR(_args);

        let z = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        if !isMatrix(z) || nrows(z) != LENGTH(x) || ncols(z) != LENGTH(y) {
            Rf_unprotect(3);
            return R_NilValue();
        }
        _args = CDR(_args);

        let xlim = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        if LENGTH(xlim) != 2 {
            Rf_unprotect(4);
            return R_NilValue();
        }
        _args = CDR(_args);

        let ylim = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        if LENGTH(ylim) != 2 {
            Rf_unprotect(5);
            return R_NilValue();
        }
        _args = CDR(_args);

        let zlim = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        if LENGTH(zlim) != 2 {
            Rf_unprotect(6);
            return R_NilValue();
        }
        _args = CDR(_args);

        /* Check limits */
        let mut _xc = 0.0;
        let mut _xs = 0.0;
        let mut _yc = 0.0;
        let mut _ys = 0.0;
        let mut _zc = 0.0;
        let mut _zs = 0.0;

        if !LimitCheck(REAL(xlim), &mut _xc, &mut _xs) {
            Rf_unprotect(6);
            return R_NilValue();
        }
        if !LimitCheck(REAL(ylim), &mut _yc, &mut _ys) {
            Rf_unprotect(6);
            return R_NilValue();
        }
        if !LimitCheck(REAL(zlim), &mut _zc, &mut _zs) {
            Rf_unprotect(6);
            return R_NilValue();
        }

        let _theta = asReal(CAR(_args));
        _args = CDR(_args);
        let _phi = asReal(CAR(_args));
        _args = CDR(_args);
        let _r = asReal(CAR(_args));
        _args = CDR(_args);
        let _d = asReal(CAR(_args));
        _args = CDR(_args);
        let _scale = asLogical(CAR(_args));
        _args = CDR(_args);
        let _expand = asReal(CAR(_args));
        _args = CDR(_args);
        let _col = CAR(_args);
        _args = CDR(_args);
        let _border = CAR(_args);
        _args = CDR(_args);
        let _ltheta = asReal(CAR(_args));
        _args = CDR(_args);
        let _lphi = asReal(CAR(_args));
        _args = CDR(_args);
        with_plot3d_state(|state| state.shade = asReal(CAR(_args)));
        _args = CDR(_args);
        let _dobox = asLogical(CAR(_args));
        _args = CDR(_args);
        let _doaxes = asLogical(CAR(_args));
        _args = CDR(_args);
        let _nTicks = asInteger(CAR(_args));
        _args = CDR(_args);
        let _tickType = asInteger(CAR(_args));
        _args = CDR(_args);
        let _xlab = CAR(_args);
        _args = CDR(_args);
        let _ylab = CAR(_args);
        _args = CDR(_args);
        let _zlab = CAR(_args);
        _args = CDR(_args);

        let shade_val = with_plot3d_state(|state| state.shade);
        if shade_val.is_finite() && shade_val <= 0.0 {
            with_plot3d_state(|state| state.shade = 1.0);
        }
        if _ltheta.is_finite()
            && _lphi.is_finite()
            && with_plot3d_state(|state| state.shade).is_finite()
        {
            with_plot3d_state(|state| state.do_lighting = true);
        } else {
            with_plot3d_state(|state| state.do_lighting = false);
        }

        let mut _xs2 = _xs;
        let mut _ys2 = _ys;
        let mut _zs2 = _zs;
        if _scale == 0 {
            let mut s = _xs2;
            if s < _ys2 {
                s = _ys2;
            }
            if s < _zs2 {
                s = _zs2;
            }
            _xs2 = s;
            _ys2 = s;
            _zs2 = s;
        }

        /* Parameter checks */
        if !_theta.is_finite()
            || !_phi.is_finite()
            || !_r.is_finite()
            || !_d.is_finite()
            || _d < 0.0
            || _r < 0.0
        {
            Rf_unprotect(6);
            return R_NilValue();
        }
        if !_expand.is_finite() || _expand < 0.0 {
            Rf_unprotect(6);
            return R_NilValue();
        }

        /* Set up the viewing transformation (real math) */
        set_vt_identity();
        Translate(-_xc, -_yc, -_zc);
        Scale(1.0 / _xs2, 1.0 / _ys2, _expand / _zs2);
        XRotate(-90.0);
        YRotate(-_theta);
        XRotate(_phi);
        Translate(0.0, 0.0, -_r - _d);
        Perspective(_d);

        /* Set up lighting (real math) */
        if with_plot3d_state(|state| state.do_lighting) {
            /* Save VT, set up light direction, then restore VT */
            let saved_vt = with_plot3d_state(|state| state.vt);
            SetUpLight(_ltheta, _lphi);
            with_plot3d_state(|state| state.vt = saved_vt);
        }

        /* Compute depth order (real algorithm) */
        let nr = nrows(z);
        let nc = ncols(z);
        let depth = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, (nr - 1) * (nc - 1)));
        let indx = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP, (nr - 1) * (nc - 1)));

        DepthOrder(
            REAL(z),
            REAL(x),
            REAL(y),
            nr,
            nc,
            REAL(depth),
            INTEGER(indx),
        );

        /* Build the result: 4x4 viewing transformation matrix */
        let result = Rf_protect(Rf_allocVector(SEXPTYPE::REALSXP, 16));
        let dim = Rf_protect(Rf_allocVector(SEXPTYPE::INTSXP, 2));
        for i in 0..4 {
            for j in 0..4 {
                *REAL(result).add(i + j * 4) = with_plot3d_state(|state| state.vt)[i][j];
            }
        }
        *INTEGER(dim).add(0) = 4;
        *INTEGER(dim).add(1) = 4;
        crate::attrib_core::setAttrib(result, crate::attrib_core::R_DimSymbol(), dim);

        Rf_unprotect(10);
        result
    }
}

/* ========================================================================
 * C_contourDef -- check if device supports rotated text in contour
 * ======================================================================== */

/// C_contourDef -- return whether the current device supports rotated
/// contour text. Stub: returns FALSE (NA_LOGICAL).
pub unsafe fn C_contourDef() -> SEXP {
    unsafe {
        use crate::sexp::constructors::*;
        Rf_ScalarLogical(0) /* FALSE: no rotated text support */
    }
}

/* ========================================================================
 * C_contour -- Contour Plots
 * ======================================================================== */

/// C_contour -- draw a contour plot.
/// Ported from plot3d.c C_contour().
/// Validation logic is real; GE drawing calls are stubs.
pub unsafe fn C_contour(args: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::*;

        let mut _args = CDR(args);
        if LENGTH(_args) < 12 {
            /* too few arguments */
            return R_NilValue();
        }

        let x = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        let nx = LENGTH(x);
        _args = CDR(_args);

        let y = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        let ny = LENGTH(y);
        _args = CDR(_args);

        let z = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        _args = CDR(_args);

        /* levels */
        let c = Rf_protect(coerceVector(CAR(_args), SEXPTYPE::REALSXP.into()));
        let nc = LENGTH(c);
        _args = CDR(_args);

        let _labels = CAR(_args);
        _args = CDR(_args);

        let _labcex = asReal(CAR(_args));
        _args = CDR(_args);

        let _drawLabels = asLogical(CAR(_args));
        _args = CDR(_args);

        let _method = asInteger(CAR(_args));
        _args = CDR(_args);

        if _method < 1 || _method > 3 {
            Rf_unprotect(4);
            return R_NilValue();
        }

        let _vfont = Rf_protect(FixupVFont(CAR(_args)));
        _args = CDR(_args);

        let _rawcol = CAR(_args);
        let _col = Rf_protect(FixupCol(_rawcol, R_TRANWHITE));
        let _ncol = LENGTH(_col);
        _args = CDR(_args);

        let _lty = Rf_protect(FixupLty(CAR(_args), LTY_SOLID));
        let _nlty = LENGTH(_lty);
        _args = CDR(_args);

        let _lwd = Rf_protect(FixupLwd(CAR(_args), 1.0));
        let _nlwd = LENGTH(_lwd);
        _args = CDR(_args);

        /* Validation */
        if nx < 2 || ny < 2 {
            Rf_unprotect(8);
            return R_NilValue();
        }

        if nrows(z) != nx || ncols(z) != ny {
            Rf_unprotect(8);
            return R_NilValue();
        }

        if nc < 1 {
            Rf_unprotect(8);
            return R_NilValue();
        }

        /* Check x values are finite and increasing */
        let xr = REAL(x);
        let yr = REAL(y);
        let cr = REAL(c);
        let zr = REAL(z);

        let mut valid = true;
        for i in 0..nx {
            if !(*xr.add(i as usize)).is_finite() {
                valid = false;
                break;
            }
            if i > 0 && *xr.add(i as usize) < *xr.add((i - 1) as usize) {
                valid = false;
                break;
            }
        }
        if !valid {
            Rf_unprotect(8);
            return R_NilValue();
        }

        for i in 0..ny {
            if !(*yr.add(i as usize)).is_finite() {
                valid = false;
                break;
            }
            if i > 0 && *yr.add(i as usize) < *yr.add((i - 1) as usize) {
                valid = false;
                break;
            }
        }
        if !valid {
            Rf_unprotect(8);
            return R_NilValue();
        }

        for i in 0..nc {
            if !(*cr.add(i as usize)).is_finite() {
                valid = false;
                break;
            }
        }
        if !valid {
            Rf_unprotect(8);
            return R_NilValue();
        }

        /* Find z range */
        let mut zmin = DBL_MAX;
        let mut zmax = f64::MIN_POSITIVE;
        for i in 0..(nx * ny) {
            let zi = *zr.add(i as usize);
            if zi.is_finite() {
                if zmax < zi {
                    zmax = zi;
                }
                if zmin > zi {
                    zmin = zi;
                }
            }
        }

        if zmin >= zmax {
            Rf_unprotect(8);
            return R_NilValue();
        }

        let _atom = 1e-3 * (zmax - zmin);

        /* Contour drawing would happen here -- stubs */
        /* The real implementation calls contourLines(), then traces segments,
         * draws polylines, and optionally draws labels */

        Rf_unprotect(8);
        R_NilValue()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn plot3d_state_is_session_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        left.with_protected(|| {
            with_plot3d_state(|state| {
                state.vt[0][0] = 42.0;
                state.light = [1.0, 2.0, 3.0, 4.0];
                state.shade = 3.5;
                state.do_lighting = true;
            });
        });

        right.with_protected(|| {
            with_plot3d_state(|state| {
                assert_eq!(state.vt, [[0.0; 4]; 4]);
                assert_eq!(state.light, [0.0; 4]);
                assert_eq!(state.shade, 1.0);
                assert!(!state.do_lighting);
            });
        });

        left.with_protected(|| {
            with_plot3d_state(|state| {
                assert_eq!(state.vt[0][0], 42.0);
                assert_eq!(state.light, [1.0, 2.0, 3.0, 4.0]);
                assert_eq!(state.shade, 3.5);
                assert!(state.do_lighting);
            });
        });
    }
}
