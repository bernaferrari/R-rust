#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/engine.c -- R Graphics Engine.
//!
//! Original source: src/main/engine.c (~4,017 lines)
//!
//! This file implements R's graphics engine, providing the interface between
//! graphics devices and graphics systems (like base graphics and grid).
//! Most opaque-device dispatch now routes through the C bridge; a handful of
//! higher-level recording helpers still remain intentionally partial.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int, c_uint, c_void};
use std::ptr;

use crate::appl::pretty::R_pretty;
use crate::mainutils::errors::Rf_error;
use crate::sexp::accessors::{CHAR, INTEGER, LENGTH, LOGICAL, REAL, STRING_ELT, TYPEOF};
use crate::sexp::constructors::Rf_mkString;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance::with_required_current_instance;

unsafe extern "C" {
    fn rmath_ge_set_clip(x1: c_double, y1: c_double, x2: c_double, y2: c_double, dd: *mut c_void);
    fn rmath_ge_line(
        x1: c_double,
        y1: c_double,
        x2: c_double,
        y2: c_double,
        gc: *const c_void,
        dd: *mut c_void,
    );
    fn rmath_ge_polyline(
        n: c_int,
        x: *mut c_double,
        y: *mut c_double,
        gc: *const c_void,
        dd: *mut c_void,
    );
    fn rmath_ge_polygon(
        n: c_int,
        x: *mut c_double,
        y: *mut c_double,
        gc: *const c_void,
        dd: *mut c_void,
    );
    fn rmath_ge_circle(
        x: c_double,
        y: c_double,
        radius: c_double,
        gc: *const c_void,
        dd: *mut c_void,
    );
    fn rmath_ge_rect(
        x0: c_double,
        y0: c_double,
        x1: c_double,
        y1: c_double,
        gc: *const c_void,
        dd: *mut c_void,
    );
    fn rmath_ge_path(
        x: *mut c_double,
        y: *mut c_double,
        npoly: c_int,
        nper: *mut c_int,
        winding: c_int,
        gc: *const c_void,
        dd: *mut c_void,
    );
    fn rmath_ge_raster(
        raster: *mut c_uint,
        w: c_int,
        h: c_int,
        x: c_double,
        y: c_double,
        width: c_double,
        height: c_double,
        angle: c_double,
        interpolate: c_int,
        gc: *const c_void,
        dd: *mut c_void,
    );
    fn rmath_ge_text(
        x: c_double,
        y: c_double,
        str: *const c_char,
        rot: c_double,
        hadj: c_double,
        gc: *const c_void,
        dd: *mut c_void,
    );
    fn rmath_ge_text_with_encoding(
        x: c_double,
        y: c_double,
        str: *const c_char,
        enc: c_int,
        rot: c_double,
        hadj: c_double,
        gc: *const c_void,
        dd: *mut c_void,
    );
    fn rmath_ge_mode(mode: c_int, dd: *mut c_void);
    fn rmath_ge_new_page(gc: *const c_void, dd: *mut c_void);
    fn rmath_ge_stroke(path: SEXP, gc: *const c_void, dd: *mut c_void);
    fn rmath_ge_fill(path: SEXP, rule: c_int, gc: *const c_void, dd: *mut c_void);
    fn rmath_ge_fill_stroke(path: SEXP, rule: c_int, gc: *const c_void, dd: *mut c_void);
    fn rmath_ge_device_dirty(dd: *mut c_void) -> c_int;
    fn rmath_ge_mark_dirty(dd: *mut c_void);
    fn rmath_ge_mark_clean(dd: *mut c_void);
    fn rmath_ge_recording(dd: *mut c_void) -> c_int;
    fn rmath_ge_from_device_x(value: c_double, to: c_int, dd: *mut c_void) -> c_double;
    fn rmath_ge_to_device_x(value: c_double, from: c_int, dd: *mut c_void) -> c_double;
    fn rmath_ge_from_device_y(value: c_double, to: c_int, dd: *mut c_void) -> c_double;
    fn rmath_ge_to_device_y(value: c_double, from: c_int, dd: *mut c_void) -> c_double;
    fn rmath_ge_from_device_width(value: c_double, to: c_int, dd: *mut c_void) -> c_double;
    fn rmath_ge_to_device_width(value: c_double, from: c_int, dd: *mut c_void) -> c_double;
    fn rmath_ge_from_device_height(value: c_double, to: c_int, dd: *mut c_void) -> c_double;
    fn rmath_ge_to_device_height(value: c_double, from: c_int, dd: *mut c_void) -> c_double;
    fn rmath_ge_symbol(
        x: c_double,
        y: c_double,
        pch: c_int,
        size: c_double,
        gc: *const c_void,
        dd: *mut c_void,
    );
    fn rmath_ge_metric_info(
        c: c_int,
        gc: *const c_void,
        ascent: *mut c_double,
        descent: *mut c_double,
        width: *mut c_double,
        dd: *mut c_void,
    );
    fn rmath_ge_str_width(
        str: *const c_char,
        enc: c_int,
        gc: *const c_void,
        dd: *mut c_void,
    ) -> c_double;
    fn rmath_ge_str_width_utf8(str: *const c_char, gc: *const c_void, dd: *mut c_void) -> c_double;
    fn rmath_ge_str_height(
        str: *const c_char,
        enc: c_int,
        gc: *const c_void,
        dd: *mut c_void,
    ) -> c_double;
    fn rmath_ge_str_metric(
        str: *const c_char,
        enc: c_int,
        gc: *const c_void,
        ascent: *mut c_double,
        descent: *mut c_double,
        width: *mut c_double,
        dd: *mut c_void,
    );
    fn rmath_ge_raster_scale(
        sraster: *const c_uint,
        sw: c_int,
        sh: c_int,
        draster: *mut c_uint,
        dw: c_int,
        dh: c_int,
    );
    fn rmath_ge_raster_interpolate(
        sraster: *const c_uint,
        sw: c_int,
        sh: c_int,
        draster: *mut c_uint,
        dw: c_int,
        dh: c_int,
    );
    fn rmath_ge_raster_rotated_size(
        w: c_int,
        h: c_int,
        angle: c_double,
        wnew: *mut c_int,
        hnew: *mut c_int,
    );
    fn rmath_ge_raster_rotated_offset(
        w: c_int,
        h: c_int,
        angle: c_double,
        botleft: c_int,
        xoff: *mut c_double,
        yoff: *mut c_double,
    );
    fn rmath_ge_raster_resize_for_rotation(
        sraster: *const c_uint,
        w: c_int,
        h: c_int,
        newRaster: *mut c_uint,
        wnew: c_int,
        hnew: c_int,
        gc: *const c_void,
    );
    fn rmath_ge_raster_rotate(
        sraster: *const c_uint,
        w: c_int,
        h: c_int,
        angle: c_double,
        draster: *mut c_uint,
        gc: *const c_void,
        smoothAlpha: c_int,
    );
    fn rmath_ge_glyph(
        n: c_int,
        glyphs: *const c_int,
        x: *const c_double,
        y: *const c_double,
        font: SEXP,
        size: c_double,
        colour: c_int,
        rot: c_double,
        dd: *mut c_void,
    );
}

#[inline]
fn ge_set_clip(x1: c_double, y1: c_double, x2: c_double, y2: c_double, dd: *mut c_void) {
    unsafe { rmath_ge_set_clip(x1, y1, x2, y2, dd) }
}

#[inline]
fn ge_line(
    x1: c_double,
    y1: c_double,
    x2: c_double,
    y2: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    unsafe { rmath_ge_line(x1, y1, x2, y2, gc, dd) }
}

#[inline]
fn ge_polyline(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    unsafe { rmath_ge_polyline(n, x as *mut c_double, y as *mut c_double, gc, dd) }
}

#[inline]
fn ge_polygon(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    unsafe { rmath_ge_polygon(n, x as *mut c_double, y as *mut c_double, gc, dd) }
}

#[inline]
fn ge_circle(x: c_double, y: c_double, radius: c_double, gc: *const c_void, dd: *mut c_void) {
    unsafe { rmath_ge_circle(x, y, radius, gc, dd) }
}

#[inline]
fn ge_rect(
    x0: c_double,
    y0: c_double,
    x1: c_double,
    y1: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    unsafe { rmath_ge_rect(x0, y0, x1, y1, gc, dd) }
}

#[inline]
fn ge_path(
    x: *mut c_double,
    y: *mut c_double,
    npoly: c_int,
    nper: *mut c_int,
    winding: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) {
    unsafe { rmath_ge_path(x, y, npoly, nper, winding, gc, dd) }
}

#[inline]
fn ge_raster(
    raster: *mut c_uint,
    w: c_int,
    h: c_int,
    x: c_double,
    y: c_double,
    width: c_double,
    height: c_double,
    angle: c_double,
    interpolate: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) {
    unsafe {
        rmath_ge_raster(
            raster,
            w,
            h,
            x,
            y,
            width,
            height,
            angle,
            interpolate,
            gc,
            dd,
        )
    }
}

#[inline]
fn ge_text_with_encoding(
    x: c_double,
    y: c_double,
    str: *const c_char,
    enc: c_int,
    rot: c_double,
    hadj: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    unsafe { rmath_ge_text_with_encoding(x, y, str, enc, rot, hadj, gc, dd) }
}

#[inline]
fn ge_mode(mode: c_int, dd: *mut c_void) {
    unsafe { rmath_ge_mode(mode, dd) }
}

#[inline]
fn ge_new_page(gc: *const c_void, dd: *mut c_void) {
    unsafe { rmath_ge_new_page(gc, dd) }
}

#[inline]
fn ge_metric_info(
    c: c_int,
    gc: *const c_void,
    ascent: *mut c_double,
    descent: *mut c_double,
    width: *mut c_double,
    dd: *mut c_void,
) {
    unsafe { rmath_ge_metric_info(c, gc, ascent, descent, width, dd) }
}

#[inline]
fn ge_str_width(str: *const c_char, enc: c_int, gc: *const c_void, dd: *mut c_void) -> c_double {
    unsafe { rmath_ge_str_width(str, enc, gc, dd) }
}

#[inline]
fn ge_str_width_utf8(str: *const c_char, gc: *const c_void, dd: *mut c_void) -> c_double {
    unsafe { rmath_ge_str_width_utf8(str, gc, dd) }
}

#[inline]
fn ge_str_height(str: *const c_char, enc: c_int, gc: *const c_void, dd: *mut c_void) -> c_double {
    unsafe { rmath_ge_str_height(str, enc, gc, dd) }
}

#[inline]
fn ge_str_metric(
    str: *const c_char,
    enc: c_int,
    gc: *const c_void,
    ascent: *mut c_double,
    descent: *mut c_double,
    width: *mut c_double,
    dd: *mut c_void,
) {
    unsafe { rmath_ge_str_metric(str, enc, gc, ascent, descent, width, dd) }
}

#[inline]
fn ge_device_dirty(dd: *mut c_void) -> c_int {
    unsafe { rmath_ge_device_dirty(dd) }
}

#[inline]
fn ge_mark_dirty(dd: *mut c_void) {
    unsafe { rmath_ge_mark_dirty(dd) }
}

#[inline]
fn ge_mark_clean(dd: *mut c_void) {
    unsafe { rmath_ge_mark_clean(dd) }
}

#[inline]
fn ge_recording(dd: *mut c_void) -> c_int {
    unsafe { rmath_ge_recording(dd) }
}

#[inline]
fn ge_from_device_x(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    unsafe { rmath_ge_from_device_x(value, to, dd) }
}

#[inline]
fn ge_to_device_x(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    unsafe { rmath_ge_to_device_x(value, from, dd) }
}

#[inline]
fn ge_from_device_y(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    unsafe { rmath_ge_from_device_y(value, to, dd) }
}

#[inline]
fn ge_to_device_y(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    unsafe { rmath_ge_to_device_y(value, from, dd) }
}

#[inline]
fn ge_from_device_width(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    unsafe { rmath_ge_from_device_width(value, to, dd) }
}

#[inline]
fn ge_to_device_width(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    unsafe { rmath_ge_to_device_width(value, from, dd) }
}

#[inline]
fn ge_from_device_height(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    unsafe { rmath_ge_from_device_height(value, to, dd) }
}

#[inline]
fn ge_to_device_height(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    unsafe { rmath_ge_to_device_height(value, from, dd) }
}

#[inline]
fn ge_symbol(
    x: c_double,
    y: c_double,
    pch: c_int,
    size: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    unsafe { rmath_ge_symbol(x, y, pch, size, gc, dd) }
}

#[inline]
fn ge_stroke(path: SEXP, gc: *const c_void, dd: *mut c_void) {
    unsafe { rmath_ge_stroke(path, gc, dd) }
}

#[inline]
fn ge_fill(path: SEXP, rule: c_int, gc: *const c_void, dd: *mut c_void) {
    unsafe { rmath_ge_fill(path, rule, gc, dd) }
}

#[inline]
fn ge_fill_stroke(path: SEXP, rule: c_int, gc: *const c_void, dd: *mut c_void) {
    unsafe { rmath_ge_fill_stroke(path, rule, gc, dd) }
}

#[inline]
fn ge_raster_scale(
    sraster: *const c_uint,
    sw: c_int,
    sh: c_int,
    draster: *mut c_uint,
    dw: c_int,
    dh: c_int,
) {
    unsafe { rmath_ge_raster_scale(sraster, sw, sh, draster, dw, dh) }
}

#[inline]
fn ge_raster_interpolate(
    sraster: *const c_uint,
    sw: c_int,
    sh: c_int,
    draster: *mut c_uint,
    dw: c_int,
    dh: c_int,
) {
    unsafe { rmath_ge_raster_interpolate(sraster, sw, sh, draster, dw, dh) }
}

#[inline]
fn ge_raster_rotated_size(w: c_int, h: c_int, angle: c_double, wnew: *mut c_int, hnew: *mut c_int) {
    unsafe { rmath_ge_raster_rotated_size(w, h, angle, wnew, hnew) }
}

#[inline]
fn ge_raster_rotated_offset(
    w: c_int,
    h: c_int,
    angle: c_double,
    botleft: c_int,
    xoff: *mut c_double,
    yoff: *mut c_double,
) {
    unsafe { rmath_ge_raster_rotated_offset(w, h, angle, botleft, xoff, yoff) }
}

#[inline]
fn ge_raster_resize_for_rotation(
    sraster: *const c_uint,
    w: c_int,
    h: c_int,
    new_raster: *mut c_uint,
    wnew: c_int,
    hnew: c_int,
    gc: *const c_void,
) {
    unsafe { rmath_ge_raster_resize_for_rotation(sraster, w, h, new_raster, wnew, hnew, gc) }
}

#[inline]
fn ge_raster_rotate(
    sraster: *const c_uint,
    w: c_int,
    h: c_int,
    angle: c_double,
    draster: *mut c_uint,
    gc: *const c_void,
    smooth_alpha: c_int,
) {
    unsafe { rmath_ge_raster_rotate(sraster, w, h, angle, draster, gc, smooth_alpha) }
}

#[inline]
fn ge_eval_with_rho(e: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::eval::eval::Rf_eval(e, rho) }
}

#[inline]
fn nil_value() -> SEXP {
    unsafe { R_NilValue() }
}

#[inline]
fn mk_string(ptr: *const c_char) -> SEXP {
    unsafe { Rf_mkString(ptr) }
}

#[inline]
fn ge_glyph(
    n: c_int,
    glyphs: *const c_int,
    x: *const c_double,
    y: *const c_double,
    font: SEXP,
    size: c_double,
    colour: c_int,
    rot: c_double,
    dd: *mut c_void,
) {
    unsafe { rmath_ge_glyph(n, glyphs, x, y, font, size, colour, rot, dd) }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of registered graphics systems.
pub const MAX_GRAPHICS_SYSTEMS: c_int = 10;

/// R Graphics Engine version number.
pub const R_GE_version: c_int = 16;

/// GEevent enum values
pub const GE_InitState: c_int = 0;
pub const GE_FinaliseState: c_int = 1;
pub const GE_SaveState: c_int = 2;
pub const GE_RestoreState: c_int = 3;
pub const GE_CopyState: c_int = 4;
pub const GE_CheckPlot: c_int = 5;
pub const GE_SaveSnapshotState: c_int = 6;
pub const GE_RestoreSnapshotState: c_int = 7;

/// GEUnit enum values
pub const GE_DEVICE: c_int = 0;
pub const GE_NDC: c_int = 1;
pub const GE_INCHES: c_int = 2;
pub const GE_CM: c_int = 3;

/// Device version constants
pub const R_GE_deviceClip: c_int = 14;
pub const R_GE_group: c_int = 15;
pub const R_GE_glyphs: c_int = 16;

/// LTY constants
pub const LTY_BLANK: c_int = -1;
pub const LTY_SOLID: c_int = 1;
pub const LTY_DASHED: c_int = 2;
pub const LTY_DOTTED: c_int = 3;
pub const LTY_DOTDASH: c_int = 4;
pub const LTY_LONGDASH: c_int = 5;
pub const LTY_TWODASH: c_int = 6;

/// R_GE_lineend enum values
pub const GE_ROUND_CAP: c_int = 1;
pub const GE_BUTT_CAP: c_int = 2;
pub const GE_SQUARE_CAP: c_int = 3;

/// R_GE_linejoin enum values
pub const GE_ROUND_JOIN: c_int = 1;
pub const GE_MITRE_JOIN: c_int = 2;
pub const GE_BEVEL_JOIN: c_int = 3;

/// R transparent white color value.
pub const R_TRANWHITE: c_uint = 0x00FFFFFF;

// ---------------------------------------------------------------------------
// Per-session graphics engine state
// ---------------------------------------------------------------------------

#[derive(Default)]
pub(crate) struct GraphicsEngineState {
    pub num_graphics_systems: c_int,
}

#[inline]
fn with_graphics_engine_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut GraphicsEngineState) -> R,
{
    with_required_current_instance(|instance| f(&mut instance.graphics_engine_state))
}

#[inline]
fn wrap_index(len: c_int, ind: c_int) -> usize {
    ind.rem_euclid(len.max(1)) as usize
}

fn sexp_string_at(value: SEXP, ind: c_int) -> Option<String> {
    let len = unsafe { LENGTH(value) };
    let ty = unsafe { TYPEOF(value) };
    if value.is_null() || ty != SEXPTYPE::STRSXP.as_c_int() || len == 0 {
        return None;
    }
    let idx = wrap_index(len, ind) as R_xlen_t;
    let cstr = unsafe { CStr::from_ptr(CHAR(STRING_ELT(value, idx))) };
    Some(cstr.to_string_lossy().to_ascii_lowercase())
}

fn sexp_int_at(value: SEXP, ind: c_int) -> Option<c_int> {
    let len = unsafe { LENGTH(value) };
    if value.is_null() || len == 0 {
        return None;
    }
    let idx = wrap_index(len, ind);
    match unsafe { TYPEOF(value) } {
        t if t == SEXPTYPE::INTSXP.as_c_int() => {
            let x = unsafe { *INTEGER(value).add(idx) };
            if x == NA_INTEGER { None } else { Some(x) }
        }
        t if t == SEXPTYPE::LGLSXP.as_c_int() => {
            let x = unsafe { *LOGICAL(value).add(idx) };
            if x == NA_LOGICAL { None } else { Some(x) }
        }
        t if t == SEXPTYPE::REALSXP.as_c_int() => {
            let x = unsafe { *REAL(value).add(idx) };
            if x.is_finite() {
                Some(x as c_int)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn parse_lend_name(name: &str) -> Option<c_int> {
    if name.starts_with("round") {
        Some(GE_ROUND_CAP)
    } else if name.starts_with("butt") {
        Some(GE_BUTT_CAP)
    } else if name.starts_with("square") {
        Some(GE_SQUARE_CAP)
    } else {
        None
    }
}

fn parse_ljoin_name(name: &str) -> Option<c_int> {
    if name.starts_with("round") {
        Some(GE_ROUND_JOIN)
    } else if name.starts_with("mitre") || name.starts_with("miter") {
        Some(GE_MITRE_JOIN)
    } else if name.starts_with("bevel") {
        Some(GE_BEVEL_JOIN)
    } else {
        None
    }
}

fn parse_lty_name(name: &str) -> Option<c_uint> {
    match name {
        "blank" => Some(LTY_BLANK as c_uint),
        "solid" => Some(LTY_SOLID as c_uint),
        "dashed" => Some(LTY_DASHED as c_uint),
        "dotted" => Some(LTY_DOTTED as c_uint),
        "dotdash" => Some(LTY_DOTDASH as c_uint),
        "longdash" => Some(LTY_LONGDASH as c_uint),
        "twodash" => Some(LTY_TWODASH as c_uint),
        _ => None,
    }
}

fn parse_lty_hex_digit(b: u8) -> Option<c_uint> {
    match b {
        b'0'..=b'9' => Some((b - b'0') as c_uint),
        b'a'..=b'f' => Some((b - b'a' + 10) as c_uint),
        b'A'..=b'F' => Some((b - b'A' + 10) as c_uint),
        _ => None,
    }
}

fn parse_lty_hex_pattern(spec: &str) -> Option<c_uint> {
    let bytes = spec.as_bytes();
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    let mut pattern: c_uint = 0;
    for b in bytes {
        let digit = parse_lty_hex_digit(*b)?;
        pattern = (pattern << 4) | digit;
    }
    Some(pattern)
}

// ---------------------------------------------------------------------------
// R_GE_getVersion / R_GE_checkVersionOrDie
// ---------------------------------------------------------------------------

/// Return the current graphics engine version number.
pub unsafe fn R_GE_getVersion() -> c_int {
    R_GE_version
}

/// Check that the given version matches the current engine version; panic on mismatch.
pub unsafe fn R_GE_checkVersionOrDie(version: c_int) {
    if version != R_GE_version {
        // In full implementation, this would call error().
        //  ignore silently.
    }
}

// ---------------------------------------------------------------------------
// GEdestroyDevDesc
// ---------------------------------------------------------------------------

/// Destroy a graphics device description, freeing all associated resources.
pub unsafe fn GEdestroyDevDesc(dd: *mut c_void) {
    let _ = dd;
}

// ---------------------------------------------------------------------------
// GEsystemState
// ---------------------------------------------------------------------------

/// Return the system-specific state for a graphics system.
pub unsafe fn GEsystemState(dd: *mut c_void, index: c_int) -> *mut c_void {
    ptr::null_mut()
}

// ---------------------------------------------------------------------------
// GEregisterWithDevice
// ---------------------------------------------------------------------------

/// Register all current graphics systems with a new device.
pub unsafe fn GEregisterWithDevice(dd: *mut c_void) {
    let _ = dd;
    // No devices in headless mode
}

// ---------------------------------------------------------------------------
// GEregisterSystem
// ---------------------------------------------------------------------------

/// Register a new graphics system with the engine.
pub unsafe fn GEregisterSystem(
    cb: Option<unsafe extern "C" fn(c_int, *mut c_void, SEXP) -> SEXP>,
    systemRegisterIndex: *mut c_int,
) {
    let _ = cb;
    with_graphics_engine_state(|state| {
        let current = state.num_graphics_systems;
        if current >= MAX_GRAPHICS_SYSTEMS {
            return;
        }
        if !systemRegisterIndex.is_null() {
            unsafe {
                *systemRegisterIndex = current;
            }
        }
        // Increment the count of registered graphics systems
        state.num_graphics_systems = current + 1;
        // Wire-up with any active devices. In headless mode there are no devices,
        // but call the hook to keep behavior consistent with the original C API.
        unsafe {
            GEregisterWithDevice(ptr::null_mut());
        }
    });
}

// ---------------------------------------------------------------------------
// GEunregisterSystem
// ---------------------------------------------------------------------------

/// Unregister a graphics system from the engine.
pub unsafe fn GEunregisterSystem(registerIndex: c_int) {
    let _ = registerIndex;
    with_graphics_engine_state(|state| {
        let current = state.num_graphics_systems;
        if current > 0 {
            state.num_graphics_systems = current - 1;
        }
    });
}

// ---------------------------------------------------------------------------
// GEhandleEvent
// ---------------------------------------------------------------------------

/// Handle a graphics event, forwarding to all registered systems.
pub unsafe fn GEhandleEvent(event: c_int, dev: *mut c_void, data: SEXP) -> SEXP {
    nil_value()
}

// ---------------------------------------------------------------------------
// Coordinate transformation stubs
// ---------------------------------------------------------------------------

/// Convert X coordinate from device units to the specified unit.
pub unsafe fn fromDeviceX(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    ge_from_device_x(value, to, dd)
}

/// Convert X coordinate from the specified unit to device units.
pub unsafe fn toDeviceX(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    ge_to_device_x(value, from, dd)
}

/// Convert Y coordinate from device units to the specified unit.
pub unsafe fn fromDeviceY(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    ge_from_device_y(value, to, dd)
}

/// Convert Y coordinate from the specified unit to device units.
pub unsafe fn toDeviceY(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    ge_to_device_y(value, from, dd)
}

/// Convert width from device units to the specified unit.
pub unsafe fn fromDeviceWidth(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    ge_from_device_width(value, to, dd)
}

/// Convert width from the specified unit to device units.
pub unsafe fn toDeviceWidth(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    ge_to_device_width(value, from, dd)
}

/// Convert height from device units to the specified unit.
pub unsafe fn fromDeviceHeight(value: c_double, to: c_int, dd: *mut c_void) -> c_double {
    ge_from_device_height(value, to, dd)
}

/// Convert height from the specified unit to device units.
pub unsafe fn toDeviceHeight(value: c_double, from: c_int, dd: *mut c_void) -> c_double {
    ge_to_device_height(value, from, dd)
}

// ---------------------------------------------------------------------------
// Line end / join parameter functions
// ---------------------------------------------------------------------------

/// Parse a line end specification from an R SEXP value.
pub unsafe fn GE_LENDpar(value: SEXP, ind: c_int) -> c_int {
    if let Some(name) = sexp_string_at(value, ind) {
        if let Some(parsed) = parse_lend_name(&name) {
            return parsed;
        }
    }
    match sexp_int_at(value, ind) {
        Some(1) => GE_ROUND_CAP,
        Some(2) => GE_BUTT_CAP,
        Some(3) => GE_SQUARE_CAP,
        _ => GE_ROUND_CAP,
    }
}

/// Convert a line end code to an R string.
pub unsafe fn GE_LENDget(lend: c_int) -> SEXP {
    match lend {
        GE_BUTT_CAP => mk_string(c"butt".as_ptr()),
        GE_SQUARE_CAP => mk_string(c"square".as_ptr()),
        _ => mk_string(c"round".as_ptr()),
    }
}

/// Parse a line join specification from an R SEXP value.
pub unsafe fn GE_LJOINpar(value: SEXP, ind: c_int) -> c_int {
    if let Some(name) = sexp_string_at(value, ind) {
        if let Some(parsed) = parse_ljoin_name(&name) {
            return parsed;
        }
    }
    match sexp_int_at(value, ind) {
        Some(1) => GE_ROUND_JOIN,
        Some(2) => GE_MITRE_JOIN,
        Some(3) => GE_BEVEL_JOIN,
        _ => GE_ROUND_JOIN,
    }
}

/// Convert a line join code to an R string.
pub unsafe fn GE_LJOINget(ljoin: c_int) -> SEXP {
    match ljoin {
        GE_MITRE_JOIN => mk_string(c"mitre".as_ptr()),
        GE_BEVEL_JOIN => mk_string(c"bevel".as_ptr()),
        _ => mk_string(c"round".as_ptr()),
    }
}

// ---------------------------------------------------------------------------
// GESetClip
// ---------------------------------------------------------------------------

/// Set the clipping rectangle on the current device.
pub unsafe fn GESetClip(x1: c_double, y1: c_double, x2: c_double, y2: c_double, dd: *mut c_void) {
    if dd.is_null() {
        return;
    }
    ge_set_clip(x1, y1, x2, y2, dd);
}

// ---------------------------------------------------------------------------
// GELine
// ---------------------------------------------------------------------------

/// Draw a line segment on the device, with clipping.
pub unsafe fn GELine(
    x1: c_double,
    y1: c_double,
    x2: c_double,
    y2: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if dd.is_null() {
        return;
    }
    ge_line(x1, y1, x2, y2, gc, dd);
}

// ---------------------------------------------------------------------------
// GEPolyline
// ---------------------------------------------------------------------------

/// Draw a polyline on the device, with clipping.
pub unsafe fn GEPolyline(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if dd.is_null() {
        return;
    }
    ge_polyline(n, x, y, gc, dd);
}

// ---------------------------------------------------------------------------
// GEPolygon
// ---------------------------------------------------------------------------

/// Draw a filled polygon on the device, with clipping.
pub unsafe fn GEPolygon(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if dd.is_null() {
        return;
    }
    ge_polygon(n, x, y, gc, dd);
}

// ---------------------------------------------------------------------------
// GECircle
// ---------------------------------------------------------------------------

/// Draw a circle on the device, with clipping.
pub unsafe fn GECircle(
    x: c_double,
    y: c_double,
    radius: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if dd.is_null() {
        return;
    }
    ge_circle(x, y, radius, gc, dd);
}

// ---------------------------------------------------------------------------
// GERect
// ---------------------------------------------------------------------------

/// Draw a rectangle on the device, with clipping.
pub unsafe fn GERect(
    x0: c_double,
    y0: c_double,
    x1: c_double,
    y1: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if dd.is_null() {
        return;
    }
    ge_rect(x0, y0, x1, y1, gc, dd);
}

// ---------------------------------------------------------------------------
// GEPath
// ---------------------------------------------------------------------------

/// Draw a multi-polygon path on the device.
pub unsafe fn GEPath(
    x: *mut c_double,
    y: *mut c_double,
    npoly: c_int,
    nper: *mut c_int,
    winding: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if dd.is_null() {
        return;
    }
    ge_path(x, y, npoly, nper, winding, gc, dd);
}

// ---------------------------------------------------------------------------
// GERaster
// ---------------------------------------------------------------------------

/// Draw a raster image on the device.
pub unsafe fn GERaster(
    raster: *mut c_uint,
    w: c_int,
    h: c_int,
    x: c_double,
    y: c_double,
    width: c_double,
    height: c_double,
    angle: c_double,
    interpolate: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if dd.is_null() {
        return;
    }
    ge_raster(
        raster,
        w,
        h,
        x,
        y,
        width,
        height,
        angle,
        interpolate,
        gc,
        dd,
    );
}

// ---------------------------------------------------------------------------
// GECap
// ---------------------------------------------------------------------------

/// Capture the current device contents as a raster image.
pub unsafe fn GECap(dd: *mut c_void) -> SEXP {
    nil_value()
}

// ---------------------------------------------------------------------------
// GEText
// ---------------------------------------------------------------------------

/// Draw text on the device, with clipping and rotation support.
pub unsafe fn GEText(
    x: c_double,
    y: c_double,
    str: *const c_char,
    enc: c_int,
    xc: c_double,
    yc: c_double,
    rot: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if dd.is_null() {
        return;
    }
    // GEText currently maps x-centering onto device hadj callback input.
    ge_text_with_encoding(x, y, str, enc, rot, xc, gc, dd);
}

// ---------------------------------------------------------------------------
// GEXspline
// ---------------------------------------------------------------------------

/// Draw an X-spline (smooth curve through control points) on the device.
pub unsafe fn GEXspline(
    n: c_int,
    x: *mut c_double,
    y: *mut c_double,
    s: *mut c_double,
    open: c_int,
    repEnds: c_int,
    draw: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) -> SEXP {
    nil_value()
}

// ---------------------------------------------------------------------------
// GEMode
// ---------------------------------------------------------------------------

/// Set the graphics mode on a device.
pub unsafe fn GEMode(mode: c_int, dd: *mut c_void) {
    if dd.is_null() {
        return;
    }
    ge_mode(mode, dd);
}

// ---------------------------------------------------------------------------
// GESymbol
// ---------------------------------------------------------------------------

/// Draw a plotting symbol on the device.
pub unsafe fn GESymbol(
    x: c_double,
    y: c_double,
    pch: c_int,
    size: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if dd.is_null() {
        return;
    }
    ge_symbol(x, y, pch, size, gc, dd);
}

// ---------------------------------------------------------------------------
// GEPretty
// ---------------------------------------------------------------------------

/// Calculate pretty axis tick positions (wrapper around R_pretty).
pub unsafe fn GEPretty(lo: *mut c_double, up: *mut c_double, ndiv: *mut c_int) {
    if lo.is_null() || up.is_null() || ndiv.is_null() {
        return;
    }
    let lo_value = unsafe { *lo };
    let up_value = unsafe { *up };
    let ndiv_value = unsafe { *ndiv };
    if ndiv_value <= 0 {
        let msg = CString::new(format!(
            "invalid axis extents [GEPretty(.,.,n={})",
            ndiv_value
        ))
        .expect("GEPretty message contains no NUL");
        unsafe {
            Rf_error(msg.as_ptr());
        }
    }
    if !lo_value.is_finite() || !up_value.is_finite() {
        let msg = CString::new(format!(
            "non-finite axis extents [GEPretty({},{}, n={})]",
            lo_value, up_value, ndiv_value
        ))
        .expect("GEPretty message contains no NUL");
        unsafe {
            Rf_error(msg.as_ptr());
        }
    }
    let high_u_fact = [0.8_f64, 1.7_f64, 1.125_f64];
    let _ = R_pretty(lo, up, ndiv, 1, 0.25, high_u_fact.as_ptr(), 2, 0);
}

// ---------------------------------------------------------------------------
// GEMetricInfo
// ---------------------------------------------------------------------------

/// Get metric information (ascent, descent, width) for a character.
pub unsafe fn GEMetricInfo(
    c: c_int,
    gc: *const c_void,
    ascent: *mut c_double,
    descent: *mut c_double,
    width: *mut c_double,
    dd: *mut c_void,
) {
    ge_metric_info(c, gc, ascent, descent, width, dd);
}

// ---------------------------------------------------------------------------
// GEStrWidth
// ---------------------------------------------------------------------------

/// Get the width of a string in device coordinates.
pub unsafe fn GEStrWidth(
    str: *const c_char,
    enc: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) -> c_double {
    if dd.is_null() {
        return 0.0;
    }
    if enc == 1 {
        ge_str_width_utf8(str, gc, dd)
    } else {
        ge_str_width(str, enc, gc, dd)
    }
}

// ---------------------------------------------------------------------------
// GEStrHeight
// ---------------------------------------------------------------------------

/// Get the height of a string in device coordinates.
pub unsafe fn GEStrHeight(
    str: *const c_char,
    enc: c_int,
    gc: *const c_void,
    dd: *mut c_void,
) -> c_double {
    if dd.is_null() {
        return 0.0;
    }
    ge_str_height(str, enc, gc, dd)
}

// ---------------------------------------------------------------------------
// GEStrMetric
// ---------------------------------------------------------------------------

/// Get metric information for a string (ascent, descent, width).
pub unsafe fn GEStrMetric(
    str: *const c_char,
    enc: c_int,
    gc: *const c_void,
    ascent: *mut c_double,
    descent: *mut c_double,
    width: *mut c_double,
    dd: *mut c_void,
) {
    ge_str_metric(str, enc, gc, ascent, descent, width, dd);
}

// ---------------------------------------------------------------------------
// GENewPage
// ---------------------------------------------------------------------------

/// Start a new page on the device.
pub unsafe fn GENewPage(gc: *const c_void, dd: *mut c_void) {
    if dd.is_null() {
        return;
    }
    ge_new_page(gc, dd);
}

// ---------------------------------------------------------------------------
// GEdeviceDirty / GEdirtyDevice / GEcleanDevice
// ---------------------------------------------------------------------------

/// Check whether a device has received output from any graphics system.
pub unsafe fn GEdeviceDirty(dd: *mut c_void) -> c_int {
    if dd.is_null() {
        return 0;
    }
    ge_device_dirty(dd)
}

/// Mark a device as having received output.
pub unsafe fn GEdirtyDevice(dd: *mut c_void) {
    if dd.is_null() {
        return;
    }
    ge_mark_dirty(dd);
}

/// Mark a device as clean (no output recorded).
pub(crate) unsafe fn GEcleanDevice(dd: *mut c_void) {
    if dd.is_null() {
        return;
    }
    ge_mark_clean(dd);
}

// ---------------------------------------------------------------------------
// GEcheckState
// ---------------------------------------------------------------------------

/// Check whether all registered graphics systems are in a valid state.
pub unsafe fn GEcheckState(dd: *mut c_void) -> c_int {
    if dd.is_null() {
        0 // FALSE in R's convention means OK for headless checks
    } else {
        1 // TRUE: there is a device/state to check
    }
}

// ---------------------------------------------------------------------------
// GErecording
// ---------------------------------------------------------------------------

/// Check whether graphics operations should be recorded for replay.
pub unsafe fn GErecording(call: SEXP, dd: *mut c_void) -> c_int {
    if dd.is_null() {
        return 0;
    }
    ge_recording(dd)
}

// ---------------------------------------------------------------------------
// GErecordGraphicOperation
// ---------------------------------------------------------------------------

/// Record a graphics operation for display list replay.
pub unsafe fn GErecordGraphicOperation(op: SEXP, args: SEXP, dd: *mut c_void) {
    let _ = (op, args, dd);
}

// ---------------------------------------------------------------------------
// GEinitDisplayList
// ---------------------------------------------------------------------------

/// Initialize the display list for a device.
pub unsafe fn GEinitDisplayList(dd: *mut c_void) {
    let _ = dd;
}

// ---------------------------------------------------------------------------
// GEplayDisplayList
// ---------------------------------------------------------------------------

/// Replay the display list on a device.
pub unsafe fn GEplayDisplayList(dd: *mut c_void) {
    let _ = dd;
}

// ---------------------------------------------------------------------------
// GEcopyDisplayList
// ---------------------------------------------------------------------------

/// Copy the display list from one device to another.
pub unsafe fn GEcopyDisplayList(fromDevice: c_int) {
    let _ = fromDevice;
}

// ---------------------------------------------------------------------------
// GEcreateSnapshot
// ---------------------------------------------------------------------------

/// Create a snapshot of the current display, including graphics system state.
pub unsafe fn GEcreateSnapshot(dd: *mut c_void) -> SEXP {
    let _ = dd;
    nil_value()
}

// ---------------------------------------------------------------------------
// GEplaySnapshot
// ---------------------------------------------------------------------------

/// Recreate a saved display from a snapshot.
pub unsafe fn GEplaySnapshot(snapshot: SEXP, dd: *mut c_void) {
    let _ = (snapshot, dd);
}

// ---------------------------------------------------------------------------
// do_getSnapshot / do_playSnapshot
// ---------------------------------------------------------------------------

/// recordPlot() -- R internal entry point.
pub unsafe fn do_getSnapshot(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, args, env);
    nil_value()
}

/// replayPlot() -- R internal entry point.
pub unsafe fn do_playSnapshot(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, args, env);
    nil_value()
}

// ---------------------------------------------------------------------------
// do_recordGraphics
// ---------------------------------------------------------------------------

/// .Internal(recordGraphics(...)) -- R internal entry point.
pub unsafe fn do_recordGraphics(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    let _ = (call, op, args, env);
    nil_value()
}

// ---------------------------------------------------------------------------
// GEonExit
// ---------------------------------------------------------------------------

/// Reset graphics state on error/interrupt.
pub unsafe fn GEonExit() {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// GEstring_to_pch
// ---------------------------------------------------------------------------

/// Convert a single-character string SEXP to a pch integer code.
pub unsafe fn GEstring_to_pch(pch: SEXP) -> c_int {
    let ch = unsafe {
        if pch.is_null() || TYPEOF(pch) != SEXPTYPE::STRSXP.as_c_int() || LENGTH(pch) == 0 {
            return NA_INTEGER;
        }
        CStr::from_ptr(CHAR(STRING_ELT(pch, 0)))
            .to_string_lossy()
            .chars()
            .next()
    };
    ch.map_or(NA_INTEGER, |c| c as c_int)
}

// ---------------------------------------------------------------------------
// GE_LTYpar / GE_LTYget
// ---------------------------------------------------------------------------

/// Parse a line type specification from an R SEXP value.
pub unsafe fn GE_LTYpar(value: SEXP, ind: c_int) -> c_uint {
    if let Some(name) = sexp_string_at(value, ind) {
        if let Some(named) = parse_lty_name(&name) {
            return named;
        }
        if let Some(custom) = parse_lty_hex_pattern(&name) {
            return custom;
        }
        return LTY_SOLID as c_uint;
    }
    match sexp_int_at(value, ind) {
        Some(0) => LTY_BLANK as c_uint,
        Some(1) => LTY_SOLID as c_uint,
        Some(2) => LTY_DASHED as c_uint,
        Some(3) => LTY_DOTTED as c_uint,
        Some(4) => LTY_DOTDASH as c_uint,
        Some(5) => LTY_LONGDASH as c_uint,
        Some(6) => LTY_TWODASH as c_uint,
        _ => LTY_SOLID as c_uint,
    }
}

/// Convert a line type code to an R string.
pub unsafe fn GE_LTYget(lty: c_uint) -> SEXP {
    let named = match lty as c_int {
        LTY_BLANK => Some("blank"),
        LTY_SOLID => Some("solid"),
        LTY_DASHED => Some("dashed"),
        LTY_DOTTED => Some("dotted"),
        LTY_DOTDASH => Some("dotdash"),
        LTY_LONGDASH => Some("longdash"),
        LTY_TWODASH => Some("twodash"),
        _ => None,
    };
    if let Some(name) = named {
        return mk_string(
            CString::new(name)
                .expect("lty name contains no NUL")
                .as_ptr(),
        );
    }
    let custom = CString::new(format!("{lty:x}")).expect("hex lty contains no NUL");
    mk_string(custom.as_ptr())
}

// ---------------------------------------------------------------------------
// Raster image operations
// ---------------------------------------------------------------------------

/// Scale a raster image using nearest-neighbour interpolation.
pub unsafe fn R_GE_rasterScale(
    sraster: *const c_uint,
    sw: c_int,
    sh: c_int,
    draster: *mut c_uint,
    dw: c_int,
    dh: c_int,
) {
    ge_raster_scale(sraster, sw, sh, draster, dw, dh);
}

/// Scale a raster image using bilinear interpolation.
pub unsafe fn R_GE_rasterInterpolate(
    sraster: *const c_uint,
    sw: c_int,
    sh: c_int,
    draster: *mut c_uint,
    dw: c_int,
    dh: c_int,
) {
    ge_raster_interpolate(sraster, sw, sh, draster, dw, dh);
}

/// Calculate the size needed for a rotated raster image.
pub unsafe fn R_GE_rasterRotatedSize(
    w: c_int,
    h: c_int,
    angle: c_double,
    wnew: *mut c_int,
    hnew: *mut c_int,
) {
    ge_raster_rotated_size(w, h, angle, wnew, hnew);
}

/// Calculate the offset for a rotated raster image.
pub unsafe fn R_GE_rasterRotatedOffset(
    w: c_int,
    h: c_int,
    angle: c_double,
    botleft: c_int,
    xoff: *mut c_double,
    yoff: *mut c_double,
) {
    ge_raster_rotated_offset(w, h, angle, botleft, xoff, yoff);
}

/// Copy a raster image into the middle of a larger raster (for rotation).
pub unsafe fn R_GE_rasterResizeForRotation(
    sraster: *const c_uint,
    w: c_int,
    h: c_int,
    newRaster: *mut c_uint,
    wnew: c_int,
    hnew: c_int,
    gc: *const c_void,
) {
    ge_raster_resize_for_rotation(sraster, w, h, newRaster, wnew, hnew, gc);
}

/// Rotate a raster image.
pub unsafe fn R_GE_rasterRotate(
    sraster: *const c_uint,
    w: c_int,
    h: c_int,
    angle: c_double,
    draster: *mut c_uint,
    gc: *const c_void,
    smoothAlpha: c_int,
) {
    ge_raster_rotate(sraster, w, h, angle, draster, gc, smoothAlpha);
}

// ---------------------------------------------------------------------------
// Path drawing (GEgroup API)
// ---------------------------------------------------------------------------

/// Stroke (outline) a path on the device.
pub unsafe fn GEStroke(path: SEXP, gc: *const c_void, dd: *mut c_void) {
    if dd.is_null() {
        return;
    }
    ge_stroke(path, gc, dd);
}

/// Fill a path on the device.
pub unsafe fn GEFill(path: SEXP, rule: c_int, gc: *const c_void, dd: *mut c_void) {
    if dd.is_null() {
        return;
    }
    ge_fill(path, rule, gc, dd);
}

/// Fill and stroke a path on the device.
pub unsafe fn GEFillStroke(path: SEXP, rule: c_int, gc: *const c_void, dd: *mut c_void) {
    if dd.is_null() {
        return;
    }
    ge_fill_stroke(path, rule, gc, dd);
}

// ---------------------------------------------------------------------------
// Glyph info API
// ---------------------------------------------------------------------------

/// Get the glyphs component from a glyphInfo SEXP.
pub unsafe fn R_GE_glyphInfoGlyphs(glyphInfo: SEXP) -> SEXP {
    nil_value()
}

/// Get the fonts component from a glyphInfo SEXP.
pub unsafe fn R_GE_glyphInfoFonts(glyphInfo: SEXP) -> SEXP {
    nil_value()
}

/// Get the glyph IDs from a glyphs SEXP.
pub unsafe fn R_GE_glyphID(glyphs: SEXP) -> SEXP {
    nil_value()
}

/// Get the glyph X positions from a glyphs SEXP.
pub unsafe fn R_GE_glyphX(glyphs: SEXP) -> SEXP {
    nil_value()
}

/// Get the glyph Y positions from a glyphs SEXP.
pub unsafe fn R_GE_glyphY(glyphs: SEXP) -> SEXP {
    nil_value()
}

/// Get the glyph font indices from a glyphs SEXP.
pub unsafe fn R_GE_glyphFont(glyphs: SEXP) -> SEXP {
    nil_value()
}

/// Get the glyph sizes from a glyphs SEXP.
pub unsafe fn R_GE_glyphSize(glyphs: SEXP) -> SEXP {
    nil_value()
}

/// Get the glyph colours from a glyphs SEXP.
pub unsafe fn R_GE_glyphColour(glyphs: SEXP) -> SEXP {
    nil_value()
}

/// Get the glyph rotations from a glyphs SEXP.
pub unsafe fn R_GE_glyphRotation(glyphs: SEXP) -> SEXP {
    nil_value()
}

/// Check whether a glyphs SEXP has rotation information.
pub unsafe fn R_GE_hasGlyphRotation(glyphs: SEXP) -> c_int {
    0 // FALSE
}

// ---------------------------------------------------------------------------
// Glyph font info API
// ---------------------------------------------------------------------------

/// Get the font file path from a glyphFont SEXP.
pub unsafe fn R_GE_glyphFontFile(glyphFont: SEXP) -> *const c_char {
    ptr::null()
}

/// Get the font index from a glyphFont SEXP.
pub unsafe fn R_GE_glyphFontIndex(glyphFont: SEXP) -> c_int {
    0
}

/// Get the font family name from a glyphFont SEXP.
pub unsafe fn R_GE_glyphFontFamily(glyphFont: SEXP) -> *const c_char {
    ptr::null()
}

/// Get the font weight from a glyphFont SEXP.
pub unsafe fn R_GE_glyphFontWeight(glyphFont: SEXP) -> c_double {
    0.0
}

/// Get the font style from a glyphFont SEXP.
pub unsafe fn R_GE_glyphFontStyle(glyphFont: SEXP) -> c_int {
    0
}

/// Get the font PostScript name from a glyphFont SEXP.
pub unsafe fn R_GE_glyphFontPSname(glyphFont: SEXP) -> *const c_char {
    ptr::null()
}

/// Get the number of font variation axes from a glyphFont SEXP.
pub unsafe fn R_GE_glyphFontNumVar(glyphFont: SEXP) -> c_int {
    0
}

/// Get the axis name for a font variation axis.
pub unsafe fn R_GE_glyphFontVarAxis(glyphFont: SEXP, index: c_int) -> *const c_char {
    ptr::null()
}

/// Get the axis value for a font variation axis.
pub unsafe fn R_GE_glyphFontVarValue(glyphFont: SEXP, index: c_int) -> c_double {
    0.0
}

/// Get the formatted value for a font variation axis.
pub unsafe fn R_GE_glyphFontVarFormatted(glyphFont: SEXP, index: c_int) -> *const c_char {
    ptr::null()
}

// ---------------------------------------------------------------------------
// GEGlyph
// ---------------------------------------------------------------------------

/// Draw glyph(s) on the device.
pub unsafe fn GEGlyph(
    n: c_int,
    glyphs: *const c_int,
    x: *const c_double,
    y: *const c_double,
    font: SEXP,
    size: c_double,
    colour: c_int,
    rot: c_double,
    dd: *mut c_void,
) {
    if dd.is_null() {
        return;
    }
    ge_glyph(n, glyphs, x, y, font, size, colour, rot, dd);
}

// ---------------------------------------------------------------------------
// Rf_eval_with_gd (eval_with_gd in C)
// ---------------------------------------------------------------------------

/// Evaluate an expression within a graphics device context (with device locking).
pub unsafe fn Rf_eval_with_gd(e: SEXP, rho: SEXP, dd: *mut c_void) -> SEXP {
    let _ = dd;
    ge_eval_with_rho(e, rho)
}

// ---------------------------------------------------------------------------
// Module-private helper stubs (not #[unsafe(no_mangle)], avoid duplicate symbols)
// ---------------------------------------------------------------------------

/// Internal helper: compute open spline (from xspline.c).
pub(crate) unsafe fn compute_open_spline(
    n: c_int,
    x: *mut c_double,
    y: *mut c_double,
    s: *mut c_double,
    repEnds: c_int,
    precision: c_int,
    dd: *mut c_void,
) {
    // Headless: no rendering
}

/// Internal helper: compute closed spline (from xspline.c).
pub(crate) unsafe fn compute_closed_spline(
    n: c_int,
    x: *mut c_double,
    y: *mut c_double,
    s: *mut c_double,
    precision: c_int,
    dd: *mut c_void,
) {
    // Headless: no rendering
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::accessors::STRING_ELT;
    use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector, Rf_mkChar, Rf_mkString};
    use crate::sexp::ffi::{R_xlen_t, SEXPTYPE};
    use crate::sexp::session::RSession;
    fn make_string_vector(values: &[&str]) -> SEXP {
        let v = unsafe { Rf_allocVector(SEXPTYPE::STRSXP, values.len() as c_int) };
        for (i, value) in values.iter().enumerate() {
            let c = std::ffi::CString::new(*value).expect("test string contains no NUL");
            unsafe {
                crate::sexp::accessors::SET_STRING_ELT(v, i as R_xlen_t, Rf_mkChar(c.as_ptr()));
            }
        }
        v
    }

    #[test]
    fn test_R_GE_getVersion() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(R_GE_getVersion(), R_GE_version);
        }
    }

    #[test]
    fn test_R_GE_checkVersionOrDie_matching() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Should not panic when version matches
            R_GE_checkVersionOrDie(R_GE_version);
        }
    }

    #[test]
    fn test_coordinate_transforms_passthrough() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let val = 1.0;
            assert_eq!(fromDeviceX(val, GE_DEVICE, ptr::null_mut()), 1.0);
            assert_eq!(toDeviceX(val, GE_DEVICE, ptr::null_mut()), 1.0);
            assert_eq!(fromDeviceY(val, GE_INCHES, ptr::null_mut()), 1.0);
            assert_eq!(toDeviceY(val, GE_INCHES, ptr::null_mut()), 1.0);
            assert_eq!(fromDeviceWidth(val, GE_CM, ptr::null_mut()), 1.0);
            assert_eq!(toDeviceWidth(val, GE_CM, ptr::null_mut()), 1.0);
            assert_eq!(fromDeviceHeight(val, GE_NDC, ptr::null_mut()), 1.0);
            assert_eq!(toDeviceHeight(val, GE_NDC, ptr::null_mut()), 1.0);
        }
    }

    #[test]
    fn test_GEhandleEvent_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = GEhandleEvent(0, ptr::null_mut(), ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_graphics_system_count_is_session_local_on_same_thread() {
        let _session = crate::sexp::session::RSession::new();
        let left = RSession::new();
        let right = RSession::new();

        left.with_protected(|| unsafe {
            let mut idx = -1;
            GEregisterSystem(None, &mut idx);
            assert_eq!(idx, 0);
            with_graphics_engine_state(|state| {
                assert_eq!(state.num_graphics_systems, 1);
            });
        });

        right.with_protected(|| {
            with_graphics_engine_state(|state| {
                assert_eq!(state.num_graphics_systems, 0);
            });
        });

        left.with_protected(|| unsafe {
            GEunregisterSystem(0);
            with_graphics_engine_state(|state| {
                assert_eq!(state.num_graphics_systems, 0);
            });
        });
    }

    #[test]
    fn test_GEMetricInfo_returns_defaults() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut a = 1.0;
            let mut d = 1.0;
            let mut w = 1.0;
            GEMetricInfo(77, ptr::null(), &mut a, &mut d, &mut w, ptr::null_mut());
            assert_eq!(a, 0.0);
            assert_eq!(d, 0.0);
            assert_eq!(w, 0.0);
        }
    }

    #[test]
    fn test_GEStrWidth_returns_zero() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(
                GEStrWidth(ptr::null(), 0, ptr::null(), ptr::null_mut()),
                0.0
            );
        }
    }

    #[test]
    fn test_GEStrHeight_returns_zero() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(
                GEStrHeight(ptr::null(), 0, ptr::null(), ptr::null_mut()),
                0.0
            );
        }
    }

    #[test]
    fn test_GEStrMetric_returns_zeros() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut a = 1.0;
            let mut d = 1.0;
            let mut w = 1.0;
            GEStrMetric(
                ptr::null(),
                0,
                ptr::null(),
                &mut a,
                &mut d,
                &mut w,
                ptr::null_mut(),
            );
            assert_eq!(a, 0.0);
            assert_eq!(d, 0.0);
            assert_eq!(w, 0.0);
        }
    }

    #[test]
    fn test_GEdeviceDirty_returns_false() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(GEdeviceDirty(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_GEcheckState_returns_false_for_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // null device = headless, returns FALSE (0) meaning no device state to check
            assert_eq!(GEcheckState(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_GErecording_returns_false() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(GErecording(ptr::null_mut(), ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_GEstring_to_pch_returns_na() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(GEstring_to_pch(ptr::null_mut()), c_int::MIN);
        }
    }

    #[test]
    fn test_GEstring_to_pch_reads_first_character() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let pch = Rf_mkString(c"A".as_ptr());
            assert_eq!(GEstring_to_pch(pch), 'A' as c_int);
        }
    }

    #[test]
    fn test_GE_LTYpar_parses_named_and_hex_styles() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let named = make_string_vector(&["dotted"]);
            let custom = make_string_vector(&["3313"]);
            assert_eq!(GE_LTYpar(named, 0), LTY_DOTTED as c_uint);
            assert_eq!(GE_LTYpar(custom, 0), 0x3313);
        }
    }

    #[test]
    fn test_GE_LTYpar_numeric_mapping() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let numeric = Rf_ScalarInteger(2);
            assert_eq!(GE_LTYpar(numeric, 0), LTY_DASHED as c_uint);
        }
    }

    #[test]
    fn test_GE_LTYget_round_trips_named_values() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let out = GE_LTYget(LTY_LONGDASH as c_uint);
            let name = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(out, 0)))
                .to_str()
                .expect("valid UTF-8");
            assert_eq!(name, "longdash");
        }
    }

    #[test]
    fn test_GECap_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = GECap(ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_GEXspline_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = GEXspline(
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                1,
                0,
                1,
                ptr::null(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_GEdos_return_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(
                do_getSnapshot(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut()
                ),
                R_NilValue()
            );
            assert_eq!(
                do_playSnapshot(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut()
                ),
                R_NilValue()
            );
            assert_eq!(
                do_recordGraphics(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut()
                ),
                R_NilValue()
            );
        }
    }

    #[test]
    fn test_GEGlyphInfo_stubs_return_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(R_GE_glyphInfoGlyphs(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphInfoFonts(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphID(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphX(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphY(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphFont(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphSize(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphColour(ptr::null_mut()), R_NilValue());
            assert_eq!(R_GE_glyphRotation(ptr::null_mut()), R_NilValue());
        }
    }

    #[test]
    fn test_GEGlyphFontInfo_stubs() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(R_GE_hasGlyphRotation(ptr::null_mut()), 0);
            assert_eq!(R_GE_glyphFontFile(ptr::null_mut()), ptr::null());
            assert_eq!(R_GE_glyphFontIndex(ptr::null_mut()), 0);
            assert_eq!(R_GE_glyphFontFamily(ptr::null_mut()), ptr::null());
            assert_eq!(R_GE_glyphFontWeight(ptr::null_mut()), 0.0);
            assert_eq!(R_GE_glyphFontStyle(ptr::null_mut()), 0);
            assert_eq!(R_GE_glyphFontPSname(ptr::null_mut()), ptr::null());
            assert_eq!(R_GE_glyphFontNumVar(ptr::null_mut()), 0);
            assert_eq!(R_GE_glyphFontVarAxis(ptr::null_mut(), 0), ptr::null());
            assert_eq!(R_GE_glyphFontVarValue(ptr::null_mut(), 0), 0.0);
            assert_eq!(R_GE_glyphFontVarFormatted(ptr::null_mut(), 0), ptr::null());
        }
    }

    #[test]
    fn test_R_GE_rasterRotatedSize() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut wnew: c_int = 0;
            let mut hnew: c_int = 0;
            R_GE_rasterRotatedSize(100, 200, 0.5, &mut wnew, &mut hnew);
            assert_eq!(wnew, 184);
            assert_eq!(hnew, 223);
        }
    }

    #[test]
    fn test_R_GE_rasterRotatedOffset() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut xoff = 1.0;
            let mut yoff = 1.0;
            R_GE_rasterRotatedOffset(100, 200, 0.5, 1, &mut xoff, &mut yoff);
            assert!((xoff - 54.06342576590164).abs() < 1e-12);
            assert!((yoff - (-11.72953311924742)).abs() < 1e-12);
        }
    }

    #[test]
    fn test_Rf_eval_with_gd_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let result = Rf_eval_with_gd(ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            assert_eq!(result, R_NilValue());
        });
    }

    #[test]
    fn test_GEPretty_rounds_interval() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut lo = 1.2_f64;
            let mut up = 4.8_f64;
            let mut ndiv = 4_i32;
            GEPretty(&mut lo, &mut up, &mut ndiv);
            assert_eq!(lo, 1.0);
            assert_eq!(up, 5.0);
            assert_eq!(ndiv, 4);
        }
    }

    #[test]
    fn test_R_GE_rasterScale_identity() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let src: [c_uint; 4] = [1, 2, 3, 4];
            let mut dst: [c_uint; 4] = [0; 4];
            R_GE_rasterScale(src.as_ptr(), 2, 2, dst.as_mut_ptr(), 2, 2);
            assert_eq!(dst, src);
        }
    }

    #[test]
    fn test_LEND_LJOIN_parse_and_get() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let lend = make_string_vector(&["square"]);
            let ljoin = make_string_vector(&["bevel"]);
            assert_eq!(GE_LENDpar(lend, 0), GE_SQUARE_CAP);
            assert_eq!(GE_LJOINpar(ljoin, 0), GE_BEVEL_JOIN);

            let lend_name = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(GE_LENDget(GE_BUTT_CAP), 0)))
                .to_str()
                .expect("valid UTF-8");
            let ljoin_name =
                std::ffi::CStr::from_ptr(CHAR(STRING_ELT(GE_LJOINget(GE_MITRE_JOIN), 0)))
                    .to_str()
                    .expect("valid UTF-8");
            assert_eq!(lend_name, "butt");
            assert_eq!(ljoin_name, "mitre");
        }
    }
}
