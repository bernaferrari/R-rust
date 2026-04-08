#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Extended drawing functions for GraphApp.
//!
//! Ported from gdraw.c - thread-safe and extended drawing functions.

use std::os::raw::c_int;
use std::ptr;

use super::types::*;

pub unsafe fn ggetcliprect(_d: drawing) -> rect {
    rect::default()
}
pub unsafe fn gsetcliprect(_d: drawing, _r: rect) { /* TODO */
}
pub unsafe fn gbitblt(_db: bitmap, _sb: bitmap, _p: point, _r: rect) { /* TODO */
}
pub unsafe fn gscroll(_d: drawing, _dp: point, _r: rect) { /* TODO */
}
pub unsafe fn ginvert(_d: drawing, _r: rect) { /* TODO */
}
pub unsafe fn ggetpixel(_d: drawing, _p: point) -> rgb {
    Black
}
pub unsafe fn gsetpixel(_d: drawing, _p: point, _c: rgb) { /* TODO */
}
pub unsafe fn gdrawline(
    _d: drawing,
    _width: c_int,
    _style: c_int,
    _c: rgb,
    _p1: point,
    _p2: point,
    _fast: c_int,
    _lend: c_int,
    _ljoin: c_int,
    _lmitre: f32,
) { /* TODO */
}
pub unsafe fn gdrawrect(
    _d: drawing,
    _width: c_int,
    _style: c_int,
    _c: rgb,
    _r: rect,
    _fast: c_int,
    _lend: c_int,
    _ljoin: c_int,
    _lmitre: f32,
) { /* TODO */
}
pub unsafe fn gfillrect(_d: drawing, _fill: rgb, _r: rect) { /* TODO */
}
pub unsafe fn gcopy(_d: drawing, _d2: drawing, _r: rect) { /* TODO */
}
pub unsafe fn gcopyalpha(_d: drawing, _d2: drawing, _r: rect, _alpha: c_int) {
    /* TODO */
}
pub unsafe fn gcopyalpha2(_d: drawing, _src: image, _r: rect) { /* TODO */
}
pub unsafe fn gdrawellipse(
    _d: drawing,
    _width: c_int,
    _border: rgb,
    _r: rect,
    _fast: c_int,
    _lend: c_int,
    _ljoin: c_int,
    _lmitre: f32,
) { /* TODO */
}
pub unsafe fn gfillellipse(_d: drawing, _fill: rgb, _r: rect) { /* TODO */
}
pub unsafe fn gdrawpolyline(
    _d: drawing,
    _width: c_int,
    _style: c_int,
    _c: rgb,
    _p: *mut point,
    _n: c_int,
    _closepath: c_int,
    _fast: c_int,
    _lend: c_int,
    _ljoin: c_int,
    _lmitre: f32,
) { /* TODO */
}
pub unsafe fn gdrawpolygon(
    _d: drawing,
    _width: c_int,
    _style: c_int,
    _c: rgb,
    _p: *mut point,
    _n: c_int,
    _fast: c_int,
    _lend: c_int,
    _ljoin: c_int,
    _lmitre: f32,
) { /* TODO */
}
pub unsafe fn gsetpolyfillmode(_d: drawing, _oddeven: c_int) { /* TODO */
}
pub unsafe fn gfillpolygon(_d: drawing, _fill: rgb, _p: *mut point, _n: c_int) {
    /* TODO */
}
pub unsafe fn gfillpolypolygon(
    _d: drawing,
    _fill: rgb,
    _p: *mut point,
    _npoly: c_int,
    _nper: *mut c_int,
) { /* TODO */
}
pub unsafe fn gdrawimage(_d: drawing, _img: image, _dr: rect, _sr: rect) {
    /* TODO */
}
pub unsafe fn gmaskimage(_d: drawing, _img: image, _dr: rect, _sr: rect, _mask: image) {
    /* TODO */
}
pub unsafe fn gdrawstr(
    _d: drawing,
    _f: font,
    _c: rgb,
    _p: point,
    _s: *const std::os::raw::c_char,
) -> c_int {
    0
}
pub unsafe fn gdrawstr1(
    _d: drawing,
    _f: font,
    _c: rgb,
    _p: point,
    _s: *const std::os::raw::c_char,
    _hadj: f64,
) { /* TODO */
}
pub unsafe fn gstrrect(_d: drawing, _f: font, _s: *const std::os::raw::c_char) -> rect {
    rect::default()
}
pub unsafe fn gstrsize(_d: drawing, _f: font, _s: *const std::os::raw::c_char) -> point {
    point::default()
}
pub unsafe fn gstrwidth(_d: drawing, _f: font, _s: *const std::os::raw::c_char) -> c_int {
    0
}
pub unsafe fn gcharmetric(
    _d: drawing,
    _f: font,
    _c: c_int,
    _ascent: *mut c_int,
    _descent: *mut c_int,
    _width: *mut c_int,
) { /* TODO */
}
pub unsafe fn gnewfont(
    _d: drawing,
    _face: *const std::os::raw::c_char,
    _style: c_int,
    _size: c_int,
    _rot: f64,
    _usePoints: c_int,
) -> font {
    ptr::null_mut()
}
pub unsafe fn gnewfont2(
    _d: drawing,
    _face: *const std::os::raw::c_char,
    _style: c_int,
    _size: c_int,
    _rot: f64,
    _usePoints: c_int,
    _quality: c_int,
) -> font {
    ptr::null_mut()
}
pub unsafe fn ghasfixedwidth(_f: font) -> c_int {
    0
}
pub unsafe fn newfield_no_border(_text: *const std::os::raw::c_char, _r: rect) -> field {
    ptr::null_mut()
}
pub unsafe fn gdrawwcs(
    _d: drawing,
    _f: font,
    _c: rgb,
    _p: point,
    _s: *const std::os::raw::c_int,
) -> c_int {
    0
}
pub unsafe fn gwcswidth(_d: drawing, _f: font, _s: *const std::os::raw::c_int) -> c_int {
    0
}
pub unsafe fn gwcharmetric(
    _d: drawing,
    _f: font,
    _c: c_int,
    _ascent: *mut c_int,
    _descent: *mut c_int,
    _width: *mut c_int,
) { /* TODO */
}
pub unsafe fn gwdrawstr1(
    _d: drawing,
    _f: font,
    _c: rgb,
    _p: point,
    _s: *const std::os::raw::c_int,
    _cnt: c_int,
    _hadj: f64,
) { /* TODO */
}
pub unsafe fn gstrwidth1(
    _d: drawing,
    _f: font,
    _s: *const std::os::raw::c_char,
    _enc: c_int,
) -> c_int {
    0
}
