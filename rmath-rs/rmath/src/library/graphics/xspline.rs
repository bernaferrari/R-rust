/*
 *  Source code from Xfig 3.2.4 modified to work with arrays of doubles
 *  instead of linked lists of F_points and to remove globals.
 *
 *  Ported from r-source/src/main/xspline.c.
 */

use std::ffi::{CStr, c_void};
use std::os::raw::{c_double, c_int};

use crate::mainutils::engine::{
    fromDeviceHeight, fromDeviceWidth, fromDeviceX, fromDeviceY, toDeviceHeight, toDeviceWidth,
    toDeviceX, toDeviceY,
};
use crate::mainutils::errors::Rf_error;

type PGEDevDesc = *mut c_void;

const MAXNUMPTS: usize = 25_000;
const HIGH_PRECISION: c_double = 0.5;
const MAX_SPLINE_STEP: c_double = 0.2;

const GE_INCHES: c_int = 13;
const GE_NDC: c_int = 7;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplinePoint {
    pub x: c_double,
    pub y: c_double,
}

struct SplineBuilder {
    points: Vec<SplinePoint>,
    dd: PGEDevDesc,
}

impl SplineBuilder {
    fn new(dd: PGEDevDesc) -> Self {
        Self {
            points: Vec::new(),
            dd,
        }
    }

    fn add_point(&mut self, x: c_double, y: c_double) {
        if self.points.len() >= MAXNUMPTS {
            r_error(c"add_point - reached MAXNUMPTS");
        }

        if self
            .points
            .last()
            .is_some_and(|prev| prev.x == x && prev.y == y)
        {
            return;
        }

        let point = unsafe {
            SplinePoint {
                x: toDeviceX(x / 1200.0, GE_INCHES, self.dd),
                y: toDeviceY(y / 1200.0, GE_INCHES, self.dd),
            }
        };
        self.points.push(point);
    }

    fn into_points(self) -> Vec<SplinePoint> {
        self.points
    }
}

fn r_error(message: &'static CStr) -> ! {
    unsafe { Rf_error(message.as_ptr()) };
    unreachable!("Rf_error should not return")
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
    u * (q_val
        + u * (2.0 * q_val
            + u * (8.0 - 12.0 * q_val + u * (14.0 * q_val - 11.0 + u * (4.0 - 5.0 * q_val)))))
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

fn point_computing(
    a_blend: &[c_double; 4],
    px: &[c_double; 4],
    py: &[c_double; 4],
) -> (c_double, c_double) {
    let weights_sum = a_blend.iter().sum::<c_double>();
    let x = (a_blend[0] * px[0] + a_blend[1] * px[1] + a_blend[2] * px[2] + a_blend[3] * px[3])
        / weights_sum;
    let y = (a_blend[0] * py[0] + a_blend[1] * py[1] + a_blend[2] * py[2] + a_blend[3] * py[3])
        / weights_sum;
    (x, y)
}

fn point_adding(
    a_blend: &[c_double; 4],
    px: &[c_double; 4],
    py: &[c_double; 4],
    out: &mut SplineBuilder,
) {
    let (x, y) = point_computing(a_blend, px, py);
    out.add_point(x, y);
}

fn blended_point(k: c_double, t: c_double, s1: c_double, s2: c_double) -> [c_double; 4] {
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
    [a0, a1, a2, a3]
}

fn step_computing(
    k: c_double,
    px: &[c_double; 4],
    py: &[c_double; 4],
    s1: c_double,
    s2: c_double,
    precision: c_double,
    dd: PGEDevDesc,
) -> c_double {
    if s1 == 0.0 && s2 == 0.0 {
        return 1.0;
    }

    let (xstart, ystart) = if s1 > 0.0 {
        point_computing(&blended_point(k, 0.0, s1, s2), px, py)
    } else {
        (px[1], py[1])
    };

    let (xend, yend) = if s2 > 0.0 {
        point_computing(&blended_point(k, 1.0, s1, s2), px, py)
    } else {
        (px[2], py[2])
    };

    let (xmid, ymid) = point_computing(&blended_point(k, 0.5, s1, s2), px, py);

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

    let (dev_width, dev_height) = unsafe {
        (
            fromDeviceWidth(toDeviceWidth(1.0, GE_NDC, dd), GE_INCHES, dd) * 1200.0,
            fromDeviceHeight(toDeviceHeight(1.0, GE_NDC, dd), GE_INCHES, dd) * 1200.0,
        )
    };
    let dev_diag = (dev_width * dev_width + dev_height * dev_height).sqrt();
    if start_to_end_dist > dev_diag {
        start_to_end_dist = dev_diag;
    }

    let number_of_steps =
        start_to_end_dist.sqrt() / 2.0 + ((1.0 + angle_cos) * 10.0) as c_int as c_double;
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

fn spline_segment_computing(
    step: c_double,
    k: c_double,
    px: &[c_double; 4],
    py: &[c_double; 4],
    s1: c_double,
    s2: c_double,
    out: &mut SplineBuilder,
) {
    let mut t = 0.0;
    while t < 1.0 {
        point_adding(&blended_point(k, t, s1, s2), px, py, out);
        t += step;
    }
}

fn spline_last_segment_computing(
    k: c_double,
    px: &[c_double; 4],
    py: &[c_double; 4],
    s1: c_double,
    s2: c_double,
    out: &mut SplineBuilder,
) {
    point_adding(&blended_point(k, 1.0, s1, s2), px, py, out);
}

fn copy_control_point(
    pi: usize,
    i: usize,
    x: &[c_double],
    y: &[c_double],
    s: &[c_double],
    px: &mut [c_double; 4],
    py: &mut [c_double; 4],
    ps: &mut [c_double; 4],
    dd: PGEDevDesc,
) {
    let idx = i % x.len();
    unsafe {
        px[pi] = fromDeviceX(x[idx], GE_INCHES, dd) * 1200.0;
        py[pi] = fromDeviceY(y[idx], GE_INCHES, dd) * 1200.0;
    }
    ps[pi] = s[idx];
}

fn next_control_points(
    k: usize,
    x: &[c_double],
    y: &[c_double],
    s: &[c_double],
    px: &mut [c_double; 4],
    py: &mut [c_double; 4],
    ps: &mut [c_double; 4],
    dd: PGEDevDesc,
) {
    copy_control_point(0, k, x, y, s, px, py, ps, dd);
    copy_control_point(1, k + 1, x, y, s, px, py, ps, dd);
    copy_control_point(2, k + 2, x, y, s, px, py, ps, dd);
    copy_control_point(3, k + 3, x, y, s, px, py, ps, dd);
}

fn validate_inputs(x: &[c_double], y: &[c_double], s: &[c_double]) {
    if x.len() != y.len() || x.len() != s.len() {
        r_error(c"x, y, and shape vectors must have the same length");
    }
}

pub fn compute_open_spline_points(
    x: &[c_double],
    y: &[c_double],
    s: &[c_double],
    rep_ends: bool,
    precision: c_double,
    dd: PGEDevDesc,
) -> Vec<SplinePoint> {
    validate_inputs(x, y, s);
    if rep_ends && x.len() < 2 {
        r_error(c"there must be at least two control points");
    }
    if !rep_ends && x.len() < 4 {
        r_error(c"there must be at least four control points");
    }

    let mut out = SplineBuilder::new(dd);
    let mut px = [0.0; 4];
    let mut py = [0.0; 4];
    let mut ps = [0.0; 4];
    let mut step = 0.0;

    if rep_ends {
        copy_control_point(0, 0, x, y, s, &mut px, &mut py, &mut ps, dd);
        copy_control_point(1, 0, x, y, s, &mut px, &mut py, &mut ps, dd);
        copy_control_point(2, 1, x, y, s, &mut px, &mut py, &mut ps, dd);

        if x.len() == 2 {
            copy_control_point(3, 1, x, y, s, &mut px, &mut py, &mut ps, dd);
        } else {
            copy_control_point(3, 2, x, y, s, &mut px, &mut py, &mut ps, dd);
        }

        let mut k = 0usize;
        loop {
            step = step_computing(k as c_double, &px, &py, ps[1], ps[2], precision, dd);
            spline_segment_computing(step, k as c_double, &px, &py, ps[1], ps[2], &mut out);
            if k + 3 >= x.len() {
                break;
            }
            next_control_points(k, x, y, s, &mut px, &mut py, &mut ps, dd);
            k += 1;
        }

        if x.len() == 2 {
            copy_control_point(0, x.len() - 2, x, y, s, &mut px, &mut py, &mut ps, dd);
        } else {
            copy_control_point(0, x.len() - 3, x, y, s, &mut px, &mut py, &mut ps, dd);
        }
        copy_control_point(1, x.len() - 2, x, y, s, &mut px, &mut py, &mut ps, dd);
        copy_control_point(2, x.len() - 1, x, y, s, &mut px, &mut py, &mut ps, dd);
        copy_control_point(3, x.len() - 1, x, y, s, &mut px, &mut py, &mut ps, dd);
        step = step_computing(k as c_double, &px, &py, ps[1], ps[2], precision, dd);
        spline_segment_computing(step, k as c_double, &px, &py, ps[1], ps[2], &mut out);
        out.add_point(px[3], py[3]);
    } else {
        for k in 0..(x.len() - 3) {
            next_control_points(k, x, y, s, &mut px, &mut py, &mut ps, dd);
            step = step_computing(k as c_double, &px, &py, ps[1], ps[2], precision, dd);
            spline_segment_computing(step, k as c_double, &px, &py, ps[1], ps[2], &mut out);
        }
        spline_last_segment_computing((x.len() - 4) as c_double, &px, &py, ps[1], ps[2], &mut out);
    }

    out.into_points()
}

pub fn compute_closed_spline_points(
    x: &[c_double],
    y: &[c_double],
    s: &[c_double],
    precision: c_double,
    dd: PGEDevDesc,
) -> Vec<SplinePoint> {
    validate_inputs(x, y, s);
    if x.len() < 3 {
        r_error(c"There must be at least three control points");
    }

    let mut out = SplineBuilder::new(dd);
    let mut px = [0.0; 4];
    let mut py = [0.0; 4];
    let mut ps = [0.0; 4];

    copy_control_point(0, x.len() - 1, x, y, s, &mut px, &mut py, &mut ps, dd);
    copy_control_point(1, 0, x, y, s, &mut px, &mut py, &mut ps, dd);
    copy_control_point(2, 1, x, y, s, &mut px, &mut py, &mut ps, dd);
    copy_control_point(3, 2, x, y, s, &mut px, &mut py, &mut ps, dd);

    for k in 0..x.len() {
        let step = step_computing(k as c_double, &px, &py, ps[1], ps[2], precision, dd);
        spline_segment_computing(step, k as c_double, &px, &py, ps[1], ps[2], &mut out);
        next_control_points(k, x, y, s, &mut px, &mut py, &mut ps, dd);
    }

    out.into_points()
}

pub unsafe fn compute_open_spline(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    s: *const c_double,
    rep_ends: bool,
    precision: c_double,
    dd: PGEDevDesc,
) -> Vec<SplinePoint> {
    if n < 0 || x.is_null() || y.is_null() || s.is_null() {
        r_error(c"invalid xspline control point input");
    }
    let len = n as usize;
    let x = unsafe { std::slice::from_raw_parts(x, len) };
    let y = unsafe { std::slice::from_raw_parts(y, len) };
    let s = unsafe { std::slice::from_raw_parts(s, len) };
    compute_open_spline_points(x, y, s, rep_ends, precision, dd)
}

pub unsafe fn compute_closed_spline(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    s: *const c_double,
    precision: c_double,
    dd: PGEDevDesc,
) -> Vec<SplinePoint> {
    if n < 0 || x.is_null() || y.is_null() || s.is_null() {
        r_error(c"invalid xspline control point input");
    }
    let len = n as usize;
    let x = unsafe { std::slice::from_raw_parts(x, len) };
    let y = unsafe { std::slice::from_raw_parts(y, len) };
    let s = unsafe { std::slice::from_raw_parts(s, len) };
    compute_closed_spline_points(x, y, s, precision, dd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_spline_with_repeated_ends_returns_owned_points() {
        let x = [0.0, 1.0, 2.0];
        let y = [0.0, 1.0, 0.0];
        let s = [0.0, 0.0, 0.0];

        let points =
            compute_open_spline_points(&x, &y, &s, true, HIGH_PRECISION, std::ptr::null_mut());

        assert!(!points.is_empty());
        assert_eq!(points.last().unwrap().x, 2.0);
    }

    #[test]
    fn closed_spline_returns_owned_points_without_global_scratch() {
        let x = [0.0, 1.0, 0.0];
        let y = [0.0, 0.0, 1.0];
        let s = [0.0, 0.0, 0.0];

        let first = compute_closed_spline_points(&x, &y, &s, HIGH_PRECISION, std::ptr::null_mut());
        let second = compute_closed_spline_points(&x, &y, &s, HIGH_PRECISION, std::ptr::null_mut());

        assert_eq!(first, second);
        assert!(first.len() >= 3);
    }
}
