#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types,
    non_snake_case
)]

/*
 *  Source code from Xfig 3.2.4 modified to work with arrays of doubles
 *  instead of linked lists of F_points and to remove some globals.
 *
 *  Ported from r-source/src/main/xspline.c
 *
 *  X-spline curve computation (Blanc & Schlick, SIGGRAPH '95).
 *  Functions: compute_open_spline, compute_closed_spline,
 *  and supporting blend/step/segment helpers.
 *
 *  Copyright (c) 1985-1988 by Supoj Sutanthavibul
 *  Parts Copyright (c) 1989-2002 by Brian V. Smith
 *  Parts Copyright (c) 1991 by Paul King
 *  Parts Copyright (c) 1992 by James Tough
 *  Parts Copyright (c) 1998 by Georg Stemmer
 *  Parts Copyright (c) 1995 by C. Blanc and C. Schlick
 */

use std::ffi::c_void;
use std::os::raw::{c_double, c_int};

use crate::main::errors::Rf_error;
use crate::mainutils::engine::{
    fromDeviceHeight, fromDeviceWidth, fromDeviceX, fromDeviceY, toDeviceHeight, toDeviceX,
    toDeviceY, toDeviceWidth,
};

type pGEDevDesc = *mut c_void;

const MAXNUMPTS: usize = 25000;

const HIGH_PRECISION: c_double = 0.5;
const MAX_SPLINE_STEP: c_double = 0.2;

const GE_INCHES: c_int = 13;
const GE_NDC: c_int = 7;

use std::cell::Cell;

thread_local! {
    static NPOINTS: Cell<c_int> = Cell::new(0);
    static MAX_POINTS: Cell<c_int> = Cell::new(0);
    static XPOINTS: Cell<*mut c_double> = Cell::new(std::ptr::null_mut());
    static YPOINTS: Cell<*mut c_double> = Cell::new(std::ptr::null_mut());
}

unsafe fn r_alloc<T>(n: usize) -> *mut T {
    let layout = std::alloc::Layout::array::<T>(n).unwrap();
    std::alloc::alloc(layout) as *mut T
}

unsafe fn add_point(x: c_double, y: c_double, dd: pGEDevDesc) {
    let mut npoints = NPOINTS.get();
    let mut max_points = MAX_POINTS.get();
    let mut xpoints = XPOINTS.get();
    let mut ypoints = YPOINTS.get();

    if npoints >= max_points {
        let tmp_n = max_points + 200;
        if tmp_n > MAXNUMPTS as c_int {
            Rf_error(b"add_point - reached MAXNUMPTS\0".as_ptr() as *const std::os::raw::c_char);
        }
    }
    if npoints > 0
        && *xpoints.add((npoints - 1) as usize) == x
        && *ypoints.add((npoints - 1) as usize) == y
    {
        return;
    }
    *xpoints.add(npoints as usize) = toDeviceX(x / 1200.0, GE_INCHES, dd);
    *ypoints.add(npoints as usize) = toDeviceY(y / 1200.0, GE_INCHES, dd);
    npoints += 1;
    NPOINTS.set(npoints);
}

#[inline]
fn q(s: c_double) -> c_double {
    -s
}

fn f_blend(numerator: c_double, denominator: c_double) -> c_double {
    let p = 2.0 * denominator * denominator;
    let u = numerator / denominator;
    let u2 = u * u;
    u * u2 * (10.0 - p + (2.0 * p - 15.0) * u + (6.0 - p) * u2)
}

fn g_blend(u: c_double, q_val: c_double) -> c_double {
    u * (q_val + u * (2.0 * q_val + u * (8.0 - 12.0 * q_val + u * (14.0 * q_val - 11.0 + u * (4.0 - 5.0 * q_val)))))
}

fn h_blend(u: c_double, q_val: c_double) -> c_double {
    let u2 = u * u;
    u * (q_val + u * (2.0 * q_val + u2 * (-2.0 * q_val - u * q_val)))
}

fn negative_s1_influence(t: c_double, s1: c_double) -> (c_double, c_double) {
    (h_blend(-t, q(s1)), g_blend(t, q(s1)))
}

fn negative_s2_influence(t: c_double, s2: c_double) -> (c_double, c_double) {
    (g_blend(1.0 - t, q(s2)), h_blend(t - 1.0, q(s2)))
}

fn positive_s1_influence(k: c_double, t: c_double, s1: c_double) -> (c_double, c_double) {
    let tk = k + 1.0 + s1;
    let a0 = if t + k + 1.0 < tk {
        f_blend(t + k + 1.0 - tk, k - tk)
    } else {
        0.0
    };
    let tk = k + 1.0 - s1;
    let a2 = f_blend(t + k + 1.0 - tk, k + 2.0 - tk);
    (a0, a2)
}

fn positive_s2_influence(k: c_double, t: c_double, s2: c_double) -> (c_double, c_double) {
    let tk = k + 2.0 + s2;
    let a1 = f_blend(t + k + 1.0 - tk, k + 1.0 - tk);
    let tk = k + 2.0 - s2;
    let a3 = if t + k + 1.0 > tk {
        f_blend(t + k + 1.0 - tk, k + 3.0 - tk)
    } else {
        0.0
    };
    (a1, a3)
}

unsafe fn eqn_numerator(a_blend: &[c_double; 4], dim: &[c_double; 4]) -> c_double {
    a_blend[0] * dim[0] + a_blend[1] * dim[1] + a_blend[2] * dim[2] + a_blend[3] * dim[3]
}

unsafe fn point_adding(a_blend: &[c_double; 4], px: &[c_double; 4], py: &[c_double; 4], dd: pGEDevDesc) {
    let weights_sum = a_blend[0] + a_blend[1] + a_blend[2] + a_blend[3];
    add_point(
        eqn_numerator(a_blend, px) / weights_sum,
        eqn_numerator(a_blend, py) / weights_sum,
        dd,
    );
}

fn point_computing(a_blend: &[c_double; 4], px: &[c_double; 4], py: &[c_double; 4]) -> (c_double, c_double) {
    let weights_sum = a_blend[0] + a_blend[1] + a_blend[2] + a_blend[3];
    let x = (a_blend[0] * px[0] + a_blend[1] * px[1] + a_blend[2] * px[2] + a_blend[3] * px[3]) / weights_sum;
    let y = (a_blend[0] * py[0] + a_blend[1] * py[1] + a_blend[2] * py[2] + a_blend[3] * py[3]) / weights_sum;
    (x, y)
}

unsafe fn step_computing(
    k: c_double,
    px: &[c_double; 4],
    py: &[c_double; 4],
    s1: c_double,
    s2: c_double,
    precision: c_double,
    dd: pGEDevDesc,
) -> c_double {
    if s1 == 0.0 && s2 == 0.0 {
        return 1.0;
    }

    let (xstart, ystart);
    if s1 > 0.0 {
        let (a0, a2) = if s2 < 0.0 {
            positive_s1_influence(k, 0.0, s1)
        } else {
            positive_s1_influence(k, 0.0, s1)
        };
        let (a1, a3) = if s2 < 0.0 {
            negative_s2_influence(0.0, s2)
        } else {
            positive_s2_influence(k, 0.0, s2)
        };
        let ab = [a0, a1, a2, a3];
        let (xs, ys) = point_computing(&ab, px, py);
        xstart = xs;
        ystart = ys;
    } else {
        xstart = px[1];
        ystart = py[1];
    }

    let (xend, yend);
    if s2 > 0.0 {
        let (a0, a2) = if s1 < 0.0 {
            negative_s1_influence(1.0, s1)
        } else {
            positive_s1_influence(k, 1.0, s1)
        };
        let (a1, a3) = if s1 < 0.0 {
            positive_s2_influence(k, 1.0, s2)
        } else {
            positive_s2_influence(k, 1.0, s2)
        };
        let ab = [a0, a1, a2, a3];
        let (xe, ye) = point_computing(&ab, px, py);
        xend = xe;
        yend = ye;
    } else {
        xend = px[2];
        yend = py[2];
    }

    let (xmid, ymid);
    {
        let (a0, a2, a1, a3) = if s2 > 0.0 {
            if s1 < 0.0 {
                let (a0, a2) = negative_s1_influence(0.5, s1);
                let (a1, a3) = positive_s2_influence(k, 0.5, s2);
                (a0, a2, a1, a3)
            } else {
                let (a0, a2) = positive_s1_influence(k, 0.5, s1);
                let (a1, a3) = positive_s2_influence(k, 0.5, s2);
                (a0, a2, a1, a3)
            }
        };
        let ab = [a0, a1, a2, a3];
        let (xm, ym) = point_computing(&ab, px, py);
        xmid = xm;
        ymid = ym;
    }

    let xv1 = xstart - xmid;
    let yv1 = ystart - ymid;
    let xv2 = xend - xmid;
    let yv2 = yend - ymid;

    let scal_prod = xv1 * xv2 + yv1 * yv2;
    let sides_length_prod = ((xv1 * xv1 + yv1 * yv1) * (xv2 * xv2 + yv2 * yv2)).sqrt();

    let angle_cos = if sides_length_prod == 0.0 {
        0.0
    } else {
        scal_prod / sides_length_prod
    };

    let xlength = xend - xstart;
    let ylength = yend - ystart;
    let mut start_to_end_dist = (xlength * xlength + ylength * ylength).sqrt();

    let dev_width = fromDeviceWidth(toDeviceWidth(1.0, GE_NDC, dd), GE_INCHES, dd) * 1200.0;
    let dev_height = fromDeviceHeight(toDeviceHeight(1.0, GE_NDC, dd), GE_INCHES, dd) * 1200.0;
    let dev_diag = (dev_width * dev_width + dev_height * dev_height).sqrt();
    if start_to_end_dist > dev_diag {
        start_to_end_dist = dev_diag;
    }

    let mut number_of_steps = start_to_end_dist.sqrt() / 2.0;
    number_of_steps += ((1.0 + angle_cos) * 10.0) as c_double;

    let step = if number_of_steps == 0.0 {
        1.0
    } else {
        precision / number_of_steps
    };

    if step > MAX_SPLINE_STEP || step == 0.0 {
        MAX_SPLINE_STEP
    } else {
        step
    }
}

unsafe fn spline_segment_computing(
    step: c_double,
    k: c_double,
    px: &[c_double; 4],
    py: &[c_double; 4],
    s1: c_double,
    s2: c_double,
    dd: pGEDevDesc,
) {
    let mut t = 0.0;
    while t < 1.0 {
        let (a0, a2) = if s1 < 0.0 {
            negative_s1_influence(t, s1)
        } else {
            positive_s1_influence(k, t, s1)
        };
        let (a1, a3) = if s2 < 0.0 {
            negative_s2_influence(t, s2)
        } else {
            positive_s2_influence(k, t, s2)
        };
        let ab = [a0, a1, a2, a3];
        point_adding(&ab, px, py, dd);
        t += step;
    }
}

unsafe fn spline_last_segment_computing(
    _step: c_double,
    k: c_double,
    px: &[c_double; 4],
    py: &[c_double; 4],
    s1: c_double,
    s2: c_double,
    dd: pGEDevDesc,
) {
    let t = 1.0;
    let (a0, a2) = if s1 < 0.0 {
        negative_s1_influence(t, s1)
    } else {
        positive_s1_influence(k, t, s1)
    };
    let (a1, a3) = if s2 < 0.0 {
        negative_s2_influence(t, s2)
    } else {
        positive_s2_influence(k, t, s2)
    };
    let ab = [a0, a1, a2, a3];
    point_adding(&ab, px, py, dd);
}

unsafe fn copy_control_point(
    pi: usize,
    i: c_int,
    n: c_int,
    px: &mut [c_double; 4],
    py: &mut [c_double; 4],
    ps: &mut [c_double; 4],
    x: *const c_double,
    y: *const c_double,
    s: *const c_double,
    dd: pGEDevDesc,
) {
    let idx = (i as usize) % (n as usize);
    px[pi] = fromDeviceX(*x.add(idx), GE_INCHES, dd) * 1200.0;
    py[pi] = fromDeviceY(*y.add(idx), GE_INCHES, dd) * 1200.0;
    ps[pi] = *s.add(idx);
}

unsafe fn next_control_points(
    k: c_int,
    n: c_int,
    px: &mut [c_double; 4],
    py: &mut [c_double; 4],
    ps: &mut [c_double; 4],
    x: *const c_double,
    y: *const c_double,
    s: *const c_double,
    dd: pGEDevDesc,
) {
    copy_control_point(0, k, n, px, py, ps, x, y, s, dd);
    copy_control_point(1, k + 1, n, px, py, ps, x, y, s, dd);
    copy_control_point(2, k + 2, n, px, py, ps, x, y, s, dd);
    copy_control_point(3, k + 3, n, px, py, ps, x, y, s, dd);
}

#[unsafe(no_mangle)]
pub unsafe fn compute_open_spline(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    s: *const c_double,
    rep_ends: bool,
    precision: c_double,
    dd: pGEDevDesc,
) {
    let mut px = [0.0f64; 4];
    let mut py = [0.0f64; 4];
    let mut ps = [0.0f64; 4];

    MAX_POINTS.set(0);
    NPOINTS.set(0);
    XPOINTS.set(std::ptr::null_mut());
    YPOINTS.set(std::ptr::null_mut());

    if rep_ends && n < 2 {
        Rf_error(b"there must be at least two control points\0".as_ptr() as *const std::os::raw::c_char);
    }
    if !rep_ends && n < 4 {
        Rf_error(b"there must be at least four control points\0".as_ptr() as *const std::os::raw::c_char);
    }

    if rep_ends {
        copy_control_point(0, 0, n, &mut px, &mut py, &mut ps, x, y, s, dd);
        copy_control_point(1, 0, n, &mut px, &mut py, &mut ps, x, y, s, dd);
        copy_control_point(2, 1, n, &mut px, &mut py, &mut ps, x, y, s, dd);

        if n == 2 {
            copy_control_point(3, 1, n, &mut px, &mut py, &mut ps, x, y, s, dd);
        } else {
            copy_control_point(3, 2, n, &mut px, &mut py, &mut ps, x, y, s, dd);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe fn compute_closed_spline(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    s: *const c_double,
    precision: c_double,
    dd: pGEDevDesc,
) {
    let mut px = [0.0f64; 4];
    let mut py = [0.0f64; 4];
    let mut ps = [0.0f64; 4];

    MAX_POINTS.set(0);
    NPOINTS.set(0);
    XPOINTS.set(std::ptr::null_mut());
    YPOINTS.set(std::ptr::null_mut());

    if n < 3 {
        Rf_error(b"There must be at least three control points\0".as_ptr() as *const std::os::raw::c_char);
    }

    copy_control_point(0, n - 1, n, &mut px, &mut py, &mut ps, x, y, s, dd);
    copy_control_point(1, 0, n, &mut px, &mut py, &mut ps, x, y, s, dd);
    copy_control_point(2, 1, n, &mut px, &mut py, &mut ps, x, y, s, dd);
    copy_control_point(3, 2, n, &mut px, &mut py, &mut ps, x, y, s, dd);

    for k in 0..n {
        let step = step_computing(k as c_double, &px, &py, ps[1], ps[2], precision, dd);
        spline_segment_computing(step, k as c_double, &px, &py, ps[1], ps[2], dd);
        next_control_points(k, n, &mut px, &mut py, &mut ps, x, y, s, dd);
    }
}

#[unsafe(no_mangle)]
pub unsafe fn get_spline_points() -> (*mut c_double, *mut c_double, c_int) {
    (XPOINTS.get(), YPOINTS.get(), NPOINTS.get())
}
