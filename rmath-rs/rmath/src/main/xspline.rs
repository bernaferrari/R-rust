#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

/*
 * Source code from Xfig 3.2.4 modified to work with arrays of doubles
 * instead linked lists of F_points and to remove some globals(!)
 * See copyright etc below.
 *
 * Originally #included from engine.c.
 * That manages the R_alloc stack.
 *
 * Port of R's src/main/xspline.c (549 lines)
 */

use std::cell::Cell;
use std::os::raw::{c_double, c_int, c_long};

use crate::main::engine::{GE_INCHES, GE_NDC, pGEDevDesc};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// From w_drawprim.h
const MAXNUMPTS: c_int = 25000;

/// From u_draw_spline.c
const HIGH_PRECISION: c_double = 0.5;
const LOW_PRECISION: c_double = 1.0;
const ZOOM_PRECISION: c_double = 5.0;
const ARROW_START: c_int = 4;
const MAX_SPLINE_STEP: c_double = 0.2;

// ---------------------------------------------------------------------------
// Module-level state (static globals in C)
// ---------------------------------------------------------------------------

/// Current number of points accumulated
thread_local! { static npoints: Cell<c_int> = Cell::new(0); }
/// Current capacity of the point arrays
thread_local! { static max_points: Cell<c_int> = Cell::new(0); }
/// Array of x-coordinates (in 1200ppi space, later converted to device)
thread_local! { static xpoints: Cell<*mut c_double> = Cell::new(std::ptr::null_mut()); }
/// Array of y-coordinates (in 1200ppi space, later converted to device)
thread_local! { static ypoints: Cell<*mut c_double> = Cell::new(std::ptr::null_mut()); }

// ---------------------------------------------------------------------------
// Public accessors (used by engine.c after calling compute_*_spline)
// ---------------------------------------------------------------------------

/// Returns the number of points computed by the last spline computation.
pub fn xspline_npoints() -> c_int {
    npoints.with(|v| v.get())
}

/// Returns the x-coordinates array from the last spline computation.
pub fn xspline_xpoints() -> *mut c_double {
    xpoints.with(|v| v.get())
}

/// Returns the y-coordinates array from the last spline computation.
pub fn xspline_ypoints() -> *mut c_double {
    ypoints.with(|v| v.get())
}

/// Reset the spline point state.
pub fn xspline_reset() {
    npoints.with(|v| v.set(0));
    max_points.with(|v| v.set(0));
    xpoints.with(|v| v.set(std::ptr::null_mut()));
    ypoints.with(|v| v.set(std::ptr::null_mut()));
}

// ---------------------------------------------------------------------------
// Spline blend functions
// ---------------------------------------------------------------------------

/// Helper: Q(s) = -(s)
#[inline(always)]
const fn Q(s: c_double) -> c_double {
    -s
}

/// f_blend: cubic blend function for positive shape parameters.
fn f_blend(numerator: c_double, denominator: c_double) -> c_double {
    let p = 2.0 * denominator * denominator;
    let u = numerator / denominator;
    let u2 = u * u;
    u * u2 * (10.0 - p + (2.0 * p - 15.0) * u + (6.0 - p) * u2)
}

/// g_blend: quartic blend function (p=2 case).
fn g_blend(u: c_double, q: c_double) -> c_double {
    u * (q + u * (2.0 * q + u * (8.0 - 12.0 * q + u * (14.0 * q - 11.0 + u * (4.0 - 5.0 * q)))))
}

/// h_blend: quartic blend function (special case).
fn h_blend(u: c_double, q: c_double) -> c_double {
    let u2 = u * u;
    u * (q + u * (2.0 * q + u2 * (-2.0 * q - u * q)))
}

/// Compute influence of a negative s1 parameter at position t.
/// Returns (A0, A2) instead of writing through mutable references.
fn negative_s1_influence(t: c_double, s1: c_double) -> (c_double, c_double) {
    (h_blend(-t, Q(s1)), g_blend(t, Q(s1)))
}

/// Compute influence of a negative s2 parameter at position t.
/// Returns (A1, A3) instead of writing through mutable references.
fn negative_s2_influence(t: c_double, s2: c_double) -> (c_double, c_double) {
    (g_blend(1.0 - t, Q(s2)), h_blend(t - 1.0, Q(s2)))
}

/// Compute influence of a positive s1 parameter at position t, segment k.
/// Returns (A0, A2).
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

/// Compute influence of a positive s2 parameter at position t, segment k.
/// Returns (A1, A3).
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

// ---------------------------------------------------------------------------
// Point management
// ---------------------------------------------------------------------------

/// Add a point (in 1200ppi coordinates) to the global point arrays.
/// Converts from 1200ppi to DEVICE coordinates before storing.
///
/// This is the equivalent of the C static `add_point` function.
unsafe fn add_point(x: c_double, y: c_double, dd: pGEDevDesc) {
    unsafe {
        let dd_void = dd as *mut std::ffi::c_void;
        if npoints.with(|v| v.get()) >= max_points.with(|v| v.get()) {
            let cur_max = max_points.with(|v| v.get());
            let tmp_n = cur_max + 200;
            // Too many points, error out
            if tmp_n > MAXNUMPTS {
                crate::main::errors::Rf_error(c"add_point - reached MAXNUMPTS".as_ptr() as *const _);
            }
            let tmp_px: *mut c_double;
            let tmp_py: *mut c_double;
            if cur_max == 0 {
                tmp_px = crate::sexp::memory_ext::R_alloc(
                    std::mem::size_of::<c_double>(),
                    tmp_n as usize,
                ) as *mut c_double;
                tmp_py = crate::sexp::memory_ext::R_alloc(
                    std::mem::size_of::<c_double>(),
                    tmp_n as usize,
                ) as *mut c_double;
            } else {
                let cur_xp = xpoints.with(|v| v.get());
                let cur_yp = ypoints.with(|v| v.get());
                tmp_px = crate::main::memory_main::S_realloc(
                    cur_xp as *mut i8,
                    tmp_n as c_long,
                    cur_max as c_long,
                    std::mem::size_of::<c_double>() as c_int,
                ) as *mut c_double;
                tmp_py = crate::main::memory_main::S_realloc(
                    cur_yp as *mut i8,
                    tmp_n as c_long,
                    cur_max as c_long,
                    std::mem::size_of::<c_double>() as c_int,
                ) as *mut c_double;
            }
            if tmp_px.is_null() || tmp_py.is_null() {
                crate::main::errors::Rf_error(
                    c"insufficient memory to allocate point array".as_ptr() as *const _,
                );
            }
            xpoints.with(|v| v.set(tmp_px));
            ypoints.with(|v| v.set(tmp_py));
            max_points.with(|v| v.set(tmp_n));
        }
        // Ignore identical points
        let cur_npoints = npoints.with(|v| v.get());
        let xp = xpoints.with(|v| v.get());
        let yp = ypoints.with(|v| v.get());
        if cur_npoints > 0
            && !xp.is_null()
            && !yp.is_null()
            && *xp.add((cur_npoints - 1) as usize) == x
            && *yp.add((cur_npoints - 1) as usize) == y
        {
            return;
        }
        // Convert back from 1200ppi to DEVICE coordinates
        *xp.add(cur_npoints as usize) =
            crate::main::engine::toDeviceX(x / 1200.0, GE_INCHES, dd_void);
        *yp.add(cur_npoints as usize) =
            crate::main::engine::toDeviceY(y / 1200.0, GE_INCHES, dd_void);
        npoints.with(|v| v.set(v.get() + 1));
    }
}

// ---------------------------------------------------------------------------
// Point computation helpers
// ---------------------------------------------------------------------------

/// Compute weighted numerator for one dimension.
#[inline(always)]
fn eqn_numerator(a_blend: &[c_double; 4], dim: &[c_double; 4]) -> c_double {
    a_blend[0] * dim[0] + a_blend[1] * dim[1] + a_blend[2] * dim[2] + a_blend[3] * dim[3]
}

/// Compute and add a point from blend weights and control point coordinates.
unsafe fn point_adding(
    a_blend: &[c_double; 4],
    px: &[c_double; 4],
    py: &[c_double; 4],
    dd: pGEDevDesc,
) {
    unsafe {
        let weights_sum = a_blend[0] + a_blend[1] + a_blend[2] + a_blend[3];
        let x = eqn_numerator(a_blend, px) / weights_sum;
        let y = eqn_numerator(a_blend, py) / weights_sum;
        add_point(x, y, dd);
    }
}

/// Compute a point from blend weights and control point coordinates (without adding).
fn point_computing(
    a_blend: &[c_double; 4],
    px: &[c_double; 4],
    py: &[c_double; 4],
) -> (c_double, c_double) {
    let weights_sum = a_blend[0] + a_blend[1] + a_blend[2] + a_blend[3];
    let x = eqn_numerator(a_blend, px) / weights_sum;
    let y = eqn_numerator(a_blend, py) / weights_sum;
    (x, y)
}

// ---------------------------------------------------------------------------
// Step computation
// ---------------------------------------------------------------------------

/// Compute the step size for drawing a spline segment.
/// This determines how finely to tessellate the curve.
fn step_computing(
    k: c_int,
    px: &[c_double; 4],
    py: &[c_double; 4],
    s1: c_double,
    s2: c_double,
    precision: c_double,
    dd: pGEDevDesc,
) -> c_double {
    let k_f = k as c_double;
    let dd_void = dd as *mut std::ffi::c_void;

    // Only one step in case of linear segment
    if s1 == 0.0 && s2 == 0.0 {
        return 1.0;
    }

    // Compute coordinates of the origin
    let (xstart, ystart) = if s1 > 0.0 {
        let a_blend = if s2 < 0.0 {
            let (a0, a2) = positive_s1_influence(k_f, 0.0, s1);
            let (a1, a3) = negative_s2_influence(0.0, s2);
            [a0, a1, a2, a3]
        } else {
            let (a0, a2) = positive_s1_influence(k_f, 0.0, s1);
            let (a1, a3) = positive_s2_influence(k_f, 0.0, s2);
            [a0, a1, a2, a3]
        };
        point_computing(&a_blend, px, py)
    } else {
        (px[1], py[1])
    };

    // Compute coordinates of the extremity
    let (xend, yend) = if s2 > 0.0 {
        let a_blend = if s1 < 0.0 {
            let (a0, a2) = negative_s1_influence(1.0, s1);
            let (a1, a3) = positive_s2_influence(k_f, 1.0, s2);
            [a0, a1, a2, a3]
        } else {
            let (a0, a2) = positive_s1_influence(k_f, 1.0, s1);
            let (a1, a3) = positive_s2_influence(k_f, 1.0, s2);
            [a0, a1, a2, a3]
        };
        point_computing(&a_blend, px, py)
    } else {
        (px[2], py[2])
    };

    // Compute coordinates of the middle
    let (xmid, ymid) = if s2 > 0.0 {
        let a_blend = if s1 < 0.0 {
            let (a0, a2) = negative_s1_influence(0.5, s1);
            let (a1, a3) = positive_s2_influence(k_f, 0.5, s2);
            [a0, a1, a2, a3]
        } else {
            let (a0, a2) = positive_s1_influence(k_f, 0.5, s1);
            let (a1, a3) = positive_s2_influence(k_f, 0.5, s2);
            [a0, a1, a2, a3]
        };
        point_computing(&a_blend, px, py)
    } else if s1 < 0.0 {
        let (a0, a2) = negative_s1_influence(0.5, s1);
        let (a1, a3) = negative_s2_influence(0.5, s2);
        let a_blend = [a0, a1, a2, a3];
        point_computing(&a_blend, px, py)
    } else {
        let (a0, a2) = positive_s1_influence(k_f, 0.5, s1);
        let (a1, a3) = negative_s2_influence(0.5, s2);
        let a_blend = [a0, a1, a2, a3];
        point_computing(&a_blend, px, py)
    };

    let xv1 = xstart - xmid;
    let yv1 = ystart - ymid;
    let xv2 = xend - xmid;
    let yv2 = yend - ymid;

    let scal_prod = xv1 * xv2 + yv1 * yv2;
    let sides_length_prod = ((xv1 * xv1 + yv1 * yv1) * (xv2 * xv2 + yv2 * yv2)).sqrt();

    // Compute cosine of origin-middle-extremity angle, which approximates
    // the curve of the spline segment
    let angle_cos = if sides_length_prod == 0.0 {
        0.0
    } else {
        scal_prod / sides_length_prod
    };

    let xlength = xend - xstart;
    let ylength = yend - ystart;
    let mut start_to_end_dist = (xlength * xlength + ylength * ylength).sqrt();

    // It is possible for origin and extremity to be very remote indeed
    // (if the control points are located WAY off the device).
    // Limit the start_to_end_dist to the length of the diagonal of the device.
    let devWidth = crate::main::engine::fromDeviceWidth(
        crate::main::engine::toDeviceWidth(1.0, GE_NDC, dd_void),
        GE_INCHES,
        dd_void,
    ) * 1200.0;
    let devHeight = crate::main::engine::fromDeviceHeight(
        crate::main::engine::toDeviceHeight(1.0, GE_NDC, dd_void),
        GE_INCHES,
        dd_void,
    ) * 1200.0;
    let devDiag = (devWidth * devWidth + devHeight * devHeight).sqrt();
    if start_to_end_dist > devDiag {
        start_to_end_dist = devDiag;
    }

    // More steps if segment's origin and extremity are remote
    let mut number_of_steps = start_to_end_dist.sqrt() / 2.0;

    // More steps if the curve is high
    number_of_steps += ((1.0 + angle_cos) * 10.0) as c_int as c_double;

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

// ---------------------------------------------------------------------------
// Spline segment computation
// ---------------------------------------------------------------------------

/// Compute all the points along a spline segment.
unsafe fn spline_segment_computing(
    step: c_double,
    k: c_int,
    px: &[c_double; 4],
    py: &[c_double; 4],
    s1: c_double,
    s2: c_double,
    dd: pGEDevDesc,
) {
    unsafe {
        let k_f = k as c_double;

        if s1 < 0.0 {
            if s2 < 0.0 {
                let mut t = 0.0;
                while t < 1.0 {
                    let (a0, a2) = negative_s1_influence(t, s1);
                    let (a1, a3) = negative_s2_influence(t, s2);
                    let a_blend = [a0, a1, a2, a3];
                    point_adding(&a_blend, px, py, dd);
                    t += step;
                }
            } else {
                let mut t = 0.0;
                while t < 1.0 {
                    let (a0, a2) = negative_s1_influence(t, s1);
                    let (a1, a3) = positive_s2_influence(k_f, t, s2);
                    let a_blend = [a0, a1, a2, a3];
                    point_adding(&a_blend, px, py, dd);
                    t += step;
                }
            }
        } else if s2 < 0.0 {
            let mut t = 0.0;
            while t < 1.0 {
                let (a0, a2) = positive_s1_influence(k_f, t, s1);
                let (a1, a3) = negative_s2_influence(t, s2);
                let a_blend = [a0, a1, a2, a3];
                point_adding(&a_blend, px, py, dd);
                t += step;
            }
        } else {
            let mut t = 0.0;
            while t < 1.0 {
                let (a0, a2) = positive_s1_influence(k_f, t, s1);
                let (a1, a3) = positive_s2_influence(k_f, t, s2);
                let a_blend = [a0, a1, a2, a3];
                point_adding(&a_blend, px, py, dd);
                t += step;
            }
        }
    }
}

/// For adding last line segment when computing open spline
/// WITHOUT end control points repeated.
unsafe fn spline_last_segment_computing(
    step: c_double,
    k: c_int,
    px: &[c_double; 4],
    py: &[c_double; 4],
    s1: c_double,
    s2: c_double,
    dd: pGEDevDesc,
) {
    unsafe {
        let k_f = k as c_double;
        let t = 1.0;

        let (a0, a2) = if s1 < 0.0 {
            negative_s1_influence(t, s1)
        } else {
            positive_s1_influence(k_f, t, s1)
        };

        let (a1, a3) = if s2 < 0.0 {
            negative_s2_influence(t, s2)
        } else {
            positive_s2_influence(k_f, t, s2)
        };

        let a_blend = [a0, a1, a2, a3];
        point_adding(&a_blend, px, py, dd);
    }
}

// ---------------------------------------------------------------------------
// Control point macros (inlined as helper functions in Rust)
// ---------------------------------------------------------------------------

/// Copy a control point from the input arrays into the local px/py/ps arrays.
/// Equivalent to the C COPY_CONTROL_POINT macro.
///
/// x and y are in DEVICE coordinates; they are converted to 1200ppi.
unsafe fn copy_control_point(
    pi: usize,
    i: usize,
    n: usize,
    x: *const c_double,
    y: *const c_double,
    s: *const c_double,
    px: &mut [c_double; 4],
    py: &mut [c_double; 4],
    ps: &mut [c_double; 4],
    dd: pGEDevDesc,
) {
    unsafe {
        let dd_void = dd as *mut std::ffi::c_void;
        px[pi] = crate::main::engine::fromDeviceX(*x.add(i % n), GE_INCHES, dd_void) * 1200.0;
        py[pi] = crate::main::engine::fromDeviceY(*y.add(i % n), GE_INCHES, dd_void) * 1200.0;
        ps[pi] = *s.add(i % n);
    }
}

/// Load the next set of 4 control points for segment k.
/// Equivalent to the C NEXT_CONTROL_POINTS macro.
unsafe fn next_control_points(
    k: usize,
    n: usize,
    x: *const c_double,
    y: *const c_double,
    s: *const c_double,
    px: &mut [c_double; 4],
    py: &mut [c_double; 4],
    ps: &mut [c_double; 4],
    dd: pGEDevDesc,
) {
    unsafe {
        copy_control_point(0, k, n, x, y, s, px, py, ps, dd);
        copy_control_point(1, k + 1, n, x, y, s, px, py, ps, dd);
        copy_control_point(2, k + 2, n, x, y, s, px, py, ps, dd);
        copy_control_point(3, k + 3, n, x, y, s, px, py, ps, dd);
    }
}

/// Initialize control points for a closed spline.
/// Equivalent to the C INIT_CONTROL_POINTS macro.
unsafe fn init_control_points(
    n: usize,
    x: *const c_double,
    y: *const c_double,
    s: *const c_double,
    px: &mut [c_double; 4],
    py: &mut [c_double; 4],
    ps: &mut [c_double; 4],
    dd: pGEDevDesc,
) {
    unsafe {
        copy_control_point(0, n - 1, n, x, y, s, px, py, ps, dd);
        copy_control_point(1, 0, n, x, y, s, px, py, ps, dd);
        copy_control_point(2, 1, n, x, y, s, px, py, ps, dd);
        copy_control_point(3, 2, n, x, y, s, px, py, ps, dd);
    }
}

/// Compute step and run spline_segment_computing.
/// Equivalent to the C SPLINE_SEGMENT_LOOP macro.
unsafe fn spline_segment_loop(
    k: c_int,
    px: &[c_double; 4],
    py: &[c_double; 4],
    s1: c_double,
    s2: c_double,
    precision: c_double,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        let step = step_computing(k, px, py, s1, s2, precision, dd);
        spline_segment_computing(step, k, px, py, s1, s2, dd);
        step
    }
}

// ---------------------------------------------------------------------------
// Main public functions
// ---------------------------------------------------------------------------

/// Compute an open spline.
///
/// x and y are in DEVICE coordinates.
/// xfig works in 1200ppi (http://www.csit.fsu.edu/~burkardt/data/fig/fig_format.html)
/// so we convert to 1200ppi so that step calculations are correct.
///
/// # Safety
/// - x, y, s must be valid pointers with at least n elements.
/// - dd must be a valid pGEDevDesc pointer.
#[allow(clippy::too_many_arguments)]
pub unsafe fn compute_open_spline(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    s: *const c_double,
    rep_ends: bool,
    precision: c_double,
    dd: pGEDevDesc,
) {
    unsafe {
        let n = n as usize;
        let mut step = 0.0;
        let mut px = [0.0, 0.0, 0.0, 0.0];
        let mut py = [0.0, 0.0, 0.0, 0.0];
        let mut ps = [0.0, 0.0, 0.0, 0.0];

        npoints.with(|v| v.set(0));
        max_points.with(|v| v.set(0));
        xpoints.with(|v| v.set(std::ptr::null_mut()));
        ypoints.with(|v| v.set(std::ptr::null_mut()));

        if rep_ends && n < 2 {
            crate::main::errors::Rf_error(
                c"there must be at least two control points".as_ptr() as *const _
            );
        }
        if !rep_ends && n < 4 {
            crate::main::errors::Rf_error(
                c"there must be at least four control points".as_ptr() as *const _
            );
        }

        if rep_ends {
            // First control point is needed twice for the first segment
            copy_control_point(0, 0, n, x, y, s, &mut px, &mut py, &mut ps, dd);
            copy_control_point(1, 0, n, x, y, s, &mut px, &mut py, &mut ps, dd);
            copy_control_point(2, 1, n, x, y, s, &mut px, &mut py, &mut ps, dd);

            if n == 2 {
                copy_control_point(3, 1, n, x, y, s, &mut px, &mut py, &mut ps, dd);
            } else {
                copy_control_point(3, 2, n, x, y, s, &mut px, &mut py, &mut ps, dd);
            }

            let mut k = 0usize;
            loop {
                step = spline_segment_loop(k as c_int, &px, &py, ps[1], ps[2], precision, dd);
                // >= rather than == to handle special case of n == 2
                if k + 3 >= n {
                    break;
                }
                next_control_points(k, n, x, y, s, &mut px, &mut py, &mut ps, dd);
                k += 1;
            }

            // Last control point is needed twice for the last segment
            if n == 2 {
                copy_control_point(0, n - 2, n, x, y, s, &mut px, &mut py, &mut ps, dd);
            } else {
                copy_control_point(0, n - 3, n, x, y, s, &mut px, &mut py, &mut ps, dd);
            }
            copy_control_point(1, n - 2, n, x, y, s, &mut px, &mut py, &mut ps, dd);
            copy_control_point(2, n - 1, n, x, y, s, &mut px, &mut py, &mut ps, dd);
            copy_control_point(3, n - 1, n, x, y, s, &mut px, &mut py, &mut ps, dd);
            spline_segment_loop(k as c_int, &px, &py, ps[1], ps[2], precision, dd);

            add_point(px[3], py[3], dd);
        } else {
            let mut k = 0usize;
            while k + 3 < n {
                next_control_points(k, n, x, y, s, &mut px, &mut py, &mut ps, dd);
                step = spline_segment_loop(k as c_int, &px, &py, ps[1], ps[2], precision, dd);
                k += 1;
            }
            spline_last_segment_computing(step, (n - 4) as c_int, &px, &py, ps[1], ps[2], dd);
        }
    }
}

/// Compute a closed spline.
///
/// x and y are in DEVICE coordinates.
///
/// # Safety
/// - x, y, s must be valid pointers with at least n elements.
/// - dd must be a valid pGEDevDesc pointer.
pub unsafe fn compute_closed_spline(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    s: *const c_double,
    precision: c_double,
    dd: pGEDevDesc,
) {
    unsafe {
        let n = n as usize;
        let mut px = [0.0, 0.0, 0.0, 0.0];
        let mut py = [0.0, 0.0, 0.0, 0.0];
        let mut ps = [0.0, 0.0, 0.0, 0.0];

        npoints.with(|v| v.set(0));
        max_points.with(|v| v.set(0));
        xpoints.with(|v| v.set(std::ptr::null_mut()));
        ypoints.with(|v| v.set(std::ptr::null_mut()));

        if n < 3 {
            crate::main::errors::Rf_error(
                c"There must be at least three control points".as_ptr() as *const _
            );
        }

        init_control_points(n, x, y, s, &mut px, &mut py, &mut ps, dd);

        for k in 0..n {
            spline_segment_loop(k as c_int, &px, &py, ps[1], ps[2], precision, dd);
            next_control_points(k, n, x, y, s, &mut px, &mut py, &mut ps, dd);
        }
    }
}
