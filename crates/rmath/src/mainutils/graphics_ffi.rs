#![allow(non_snake_case, non_camel_case_types)]

//! Graphics-engine value types and pure raster helpers.
//!
//! GNU R dispatches drawing through a `DevDesc` table of C callbacks. This
//! runtime has no such drivers: every device is a registry-owned
//! [`GEDeviceDesc`](crate::library::grdevices::device_registry) and drawing is
//! routed through the registry by `mainutils::engine`. Device pointers are
//! therefore opaque registry keys and are never dereferenced here.

use std::os::raw::{c_char, c_double, c_int, c_uint};

use crate::sexp::ffi::SEXP;

/// Fill used where engine.c would read a null context (no fill colour).
const NULL_CONTEXT_FILL: c_uint = 0;

/// Raw graphics-context pointer kept for API parity; the engine takes
/// `Option<&R_GE_gcontext>` and never dereferences this alias.
pub type pGEcontext = *const R_GE_gcontext;
/// Device descriptor key: a registry pointer that is only ever compared.
pub type pGEDevDesc = crate::library::grdevices::device_registry::pGEDevDesc;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct R_GE_gcontext {
    pub col: c_int,
    pub fill: c_int,
    pub gamma: c_double,
    pub lwd: c_double,
    pub lty: c_int,
    pub lend: c_int,
    pub ljoin: c_int,
    pub lmitre: c_double,
    pub cex: c_double,
    pub ps: c_double,
    pub lineheight: c_double,
    pub fontface: c_int,
    pub fontfamily: [c_char; 201],
    pub patternFill: SEXP,
}

impl R_GE_gcontext {
    /// All-zero context: the value grid's scratch context buffers hold.
    pub const fn zeroed() -> Self {
        Self {
            col: 0,
            fill: 0,
            gamma: 0.0,
            lwd: 0.0,
            lty: 0,
            lend: 0,
            ljoin: 0,
            lmitre: 0.0,
            cex: 0.0,
            ps: 0.0,
            lineheight: 0.0,
            fontface: 0,
            fontfamily: [0; 201],
            patternFill: std::ptr::null_mut(),
        }
    }
}

#[inline]
const fn red(col: c_uint) -> c_int {
    (col & 0xFF) as c_int
}

#[inline]
const fn green(col: c_uint) -> c_int {
    ((col >> 8) & 0xFF) as c_int
}

#[inline]
const fn blue(col: c_uint) -> c_int {
    ((col >> 16) & 0xFF) as c_int
}

#[inline]
const fn alpha(col: c_uint) -> c_int {
    ((col >> 24) & 0xFF) as c_int
}

#[inline]
const fn rgba(r: c_int, g: c_int, b: c_int, a: c_int) -> c_uint {
    (r as c_uint & 0xFF)
        | ((g as c_uint & 0xFF) << 8)
        | ((b as c_uint & 0xFF) << 16)
        | ((a as c_uint & 0xFF) << 24)
}

#[inline]
fn gc_fill(gc: Option<&R_GE_gcontext>) -> c_uint {
    gc.map_or(NULL_CONTEXT_FILL, |gc| gc.fill as c_uint)
}

/// Nearest-neighbour scaling of an `sw`x`sh` raster into `dw`x`dh`.
pub(crate) fn raster_scale(
    sraster: &[c_uint],
    sw: c_int,
    sh: c_int,
    draster: &mut [c_uint],
    dw: c_int,
    dh: c_int,
) {
    for i in 0..dh {
        for j in 0..dw {
            let sy = i * sh / dh;
            let sx = j * sw / dw;
            let mut pixel = 0;
            if sx >= 0 && sx < sw && sy >= 0 && sy < sh {
                pixel = sraster[(sy * sw + sx) as usize];
            }
            draster[(i * dw + j) as usize] = pixel;
        }
    }
}

/// Bilinear scaling of an `sw`x`sh` raster into `dw`x`dh`.
pub(crate) fn raster_interpolate(
    sraster: &[c_uint],
    sw: c_int,
    sh: c_int,
    draster: &mut [c_uint],
    dw: c_int,
    dh: c_int,
) {
    let scx = (16.0 * sw as c_double) / dw as c_double;
    let scy = (16.0 * sh as c_double) / dh as c_double;
    let wm2 = sw - 2;
    let hm2 = sh - 2;

    for i in 0..dh {
        let ypm = (scy * i as c_double - 8.0).max(0.0) as c_int;
        let yp = ypm >> 4;
        let yf = ypm & 0x0f;
        let dline = (i * dw) as usize;
        let sline = (yp * sw) as usize;
        let src = |offset: c_int| sraster[sline + offset as usize];
        for j in 0..dw {
            let xpm = (scx * j as c_double - 8.0).max(0.0) as c_int;
            let xp = xpm >> 4;
            let xf = xpm & 0x0f;
            let pixels1 = src(xp);
            let (pixels2, pixels3, pixels4) = if xp > wm2 || yp > hm2 {
                if yp > hm2 && xp <= wm2 {
                    let p2 = src(xp + 1);
                    (p2, pixels1, p2)
                } else if xp > wm2 && yp <= hm2 {
                    let p3 = src(sw + xp);
                    (pixels1, p3, p3)
                } else {
                    (pixels1, pixels1, pixels1)
                }
            } else {
                (src(xp + 1), src(sw + xp), src(sw + xp + 1))
            };

            let area00 = (16 - xf) * (16 - yf);
            let area10 = xf * (16 - yf);
            let area01 = (16 - xf) * yf;
            let area11 = xf * yf;

            let pixel = (((area00 * red(pixels1)
                + area10 * red(pixels2)
                + area01 * red(pixels3)
                + area11 * red(pixels4)
                + 128)
                >> 8)
                & 0x0000_00ff_u32 as c_int) as c_uint
                | ((area00 * green(pixels1)
                    + area10 * green(pixels2)
                    + area01 * green(pixels3)
                    + area11 * green(pixels4)
                    + 128)
                    & 0x0000_ff00_u32 as c_int) as c_uint
                | ((((area00 * blue(pixels1)
                    + area10 * blue(pixels2)
                    + area01 * blue(pixels3)
                    + area11 * blue(pixels4)
                    + 128)
                    << 8)
                    & 0x00ff_0000_u32 as c_int) as c_uint)
                | ((((area00 * alpha(pixels1)
                    + area10 * alpha(pixels2)
                    + area01 * alpha(pixels3)
                    + area11 * alpha(pixels4)
                    + 128)
                    << 16)
                    & 0xff00_0000_u32 as c_int) as c_uint);

            draster[dline + j as usize] = pixel;
        }
    }
}

/// Bounding size of a `w`x`h` raster rotated by `angle` radians.
pub(crate) fn raster_rotated_size(w: c_int, h: c_int, angle: c_double) -> (c_int, c_int) {
    let diag = ((w * w + h * h) as c_double).sqrt();
    let theta = (h as c_double).atan2(w as c_double);
    let trx1 = diag * (theta + angle).cos();
    let trx2 = diag * (theta - angle).cos();
    let try1 = diag * (theta + angle).sin();
    let try2 = diag * (angle - theta).sin();

    let mut rotated_width = trx1.abs().max(trx2.abs()).round() as c_int;
    let mut rotated_height = try1.abs().max(try2.abs()).round() as c_int;
    if rotated_width < w {
        rotated_width = w;
    }
    if rotated_height < h {
        rotated_height = h;
    }

    (rotated_width, rotated_height)
}

/// Offset of a rotated raster's corner relative to its centre.
pub(crate) fn raster_rotated_offset(
    w: c_int,
    h: c_int,
    angle: c_double,
    botleft: bool,
) -> (c_double, c_double) {
    let hypot = 0.5 * ((w * w + h * h) as c_double).sqrt();
    let theta = if botleft {
        std::f64::consts::PI + (h as c_double).atan2(w as c_double)
    } else {
        -std::f64::consts::PI - (h as c_double).atan2(w as c_double)
    };
    let dw = hypot * (theta + angle).cos();
    let dh = hypot * (theta + angle).sin();
    if botleft {
        (dw + w as c_double / 2.0, dh + h as c_double / 2.0)
    } else {
        (dw + w as c_double / 2.0, dh - h as c_double / 2.0)
    }
}

/// Centre a `w`x`h` raster inside a `wnew`x`hnew` raster filled with `gc.fill`.
pub(crate) fn raster_resize_for_rotation(
    sraster: &[c_uint],
    w: c_int,
    h: c_int,
    new_raster: &mut [c_uint],
    wnew: c_int,
    hnew: c_int,
    gc: Option<&R_GE_gcontext>,
) {
    let xoff = (wnew - w) / 2;
    let yoff = (hnew - h) / 2;
    let fill = gc_fill(gc);
    for i in 0..hnew {
        for j in 0..wnew {
            new_raster[(i * wnew + j) as usize] = fill;
        }
    }
    for i in 0..h {
        for j in 0..w {
            let inew = i + yoff;
            let jnew = j + xoff;
            new_raster[(inew * wnew + jnew) as usize] = sraster[(i * w + j) as usize];
        }
    }
}

/// Rotate a `w`x`h` raster by `angle` radians about its centre.
pub(crate) fn raster_rotate(
    sraster: &[c_uint],
    w: c_int,
    h: c_int,
    angle: c_double,
    draster: &mut [c_uint],
    gc: Option<&R_GE_gcontext>,
    smooth_alpha: bool,
) {
    let angle = -angle;
    let xcen = w / 2;
    let wm2 = w - 2;
    let ycen = h / 2;
    let hm2 = h - 2;
    let sina = 16.0 * angle.sin();
    let cosa = 16.0 * angle.cos();
    let fill = gc_fill(gc);

    for i in 0..h {
        let ydif = ycen - i;
        let dline = (i * w) as usize;
        for j in 0..w {
            let xdif = xcen - j;
            let xpm = (-xdif as c_double * cosa - ydif as c_double * sina) as c_int;
            let ypm = (-ydif as c_double * cosa + xdif as c_double * sina) as c_int;
            let xp = xcen + (xpm >> 4);
            let yp = ycen + (ypm >> 4);
            let xf = xpm & 0x0f;
            let yf = ypm & 0x0f;
            if xp < 0 || yp < 0 || xp > wm2 || yp > hm2 {
                draster[dline + j as usize] = fill;
                continue;
            }
            let sline = (yp * w) as usize;
            let word00 = sraster[sline + xp as usize];
            let word10 = sraster[sline + (xp + 1) as usize];
            let word01 = sraster[sline + (w + xp) as usize];
            let word11 = sraster[sline + (w + xp + 1) as usize];

            let rval = ((16 - xf) * (16 - yf) * red(word00)
                + xf * (16 - yf) * red(word10)
                + (16 - xf) * yf * red(word01)
                + xf * yf * red(word11)
                + 128)
                / 256;
            let gval = ((16 - xf) * (16 - yf) * green(word00)
                + xf * (16 - yf) * green(word10)
                + (16 - xf) * yf * green(word01)
                + xf * yf * green(word11)
                + 128)
                / 256;
            let bval = ((16 - xf) * (16 - yf) * blue(word00)
                + xf * (16 - yf) * blue(word10)
                + (16 - xf) * yf * blue(word01)
                + xf * yf * blue(word11)
                + 128)
                / 256;
            let aval = if smooth_alpha {
                ((16 - xf) * (16 - yf) * alpha(word00)
                    + xf * (16 - yf) * alpha(word10)
                    + (16 - xf) * yf * alpha(word01)
                    + xf * yf * alpha(word11)
                    + 128)
                    / 256
            } else {
                alpha(word00)
                    .max(alpha(word10))
                    .max(alpha(word01))
                    .max(alpha(word11))
            };
            draster[dline + j as usize] = rgba(rval, gval, bval, aval);
        }
    }
}

/// Registry devices define no patterns, clip paths, masks or groups, so the
/// GNU `release*` device hooks have nothing to free.
pub(crate) fn rmath_grid_release_pattern(_dd: pGEDevDesc, _ref: SEXP) {}

/// See [`rmath_grid_release_pattern`].
pub(crate) fn rmath_grid_release_definitions(_dd: pGEDevDesc, _clear_groups: c_int) {}
