#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Extended drawing functions for GraphApp.
//!
//! Ported from gdraw.c - thread-safe and extended drawing functions.

use std::os::raw::c_int;
use std::ptr;

use super::types::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ggetcliprect(_d: drawing) -> rect {
    rect::default()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsetcliprect(_d: drawing, _r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gbitblt(_db: bitmap, _sb: bitmap, _p: point, _r: rect) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gscroll(_d: drawing, _dp: point, _r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ginvert(_d: drawing, _r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ggetpixel(_d: drawing, _p: point) -> rgb {
    Black
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsetpixel(_d: drawing, _p: point, _c: rgb) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawline(
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawrect(
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gfillrect(_d: drawing, _fill: rgb, _r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcopy(_d: drawing, _d2: drawing, _r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcopyalpha(_d: drawing, _d2: drawing, _r: rect, _alpha: c_int) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcopyalpha2(_d: drawing, _src: image, _r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawellipse(
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gfillellipse(_d: drawing, _fill: rgb, _r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawpolyline(
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawpolygon(
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsetpolyfillmode(_d: drawing, _oddeven: c_int) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gfillpolygon(_d: drawing, _fill: rgb, _p: *mut point, _n: c_int) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gfillpolypolygon(
    _d: drawing,
    _fill: rgb,
    _p: *mut point,
    _npoly: c_int,
    _nper: *mut c_int,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawimage(_d: drawing, _img: image, _dr: rect, _sr: rect) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gmaskimage(_d: drawing, _img: image, _dr: rect, _sr: rect, _mask: image) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawstr(
    _d: drawing,
    _f: font,
    _c: rgb,
    _p: point,
    _s: *const std::os::raw::c_char,
) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawstr1(
    _d: drawing,
    _f: font,
    _c: rgb,
    _p: point,
    _s: *const std::os::raw::c_char,
    _hadj: f64,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gstrrect(_d: drawing, _f: font, _s: *const std::os::raw::c_char) -> rect {
    rect::default()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gstrsize(_d: drawing, _f: font, _s: *const std::os::raw::c_char) -> point {
    point::default()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gstrwidth(
    _d: drawing,
    _f: font,
    _s: *const std::os::raw::c_char,
) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcharmetric(
    _d: drawing,
    _f: font,
    _c: c_int,
    _ascent: *mut c_int,
    _descent: *mut c_int,
    _width: *mut c_int,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gnewfont(
    _d: drawing,
    _face: *const std::os::raw::c_char,
    _style: c_int,
    _size: c_int,
    _rot: f64,
    _usePoints: c_int,
) -> font {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gnewfont2(
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ghasfixedwidth(_f: font) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newfield_no_border(_text: *const std::os::raw::c_char, _r: rect) -> field {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawwcs(
    _d: drawing,
    _f: font,
    _c: rgb,
    _p: point,
    _s: *const std::os::raw::c_int,
) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gwcswidth(_d: drawing, _f: font, _s: *const std::os::raw::c_int) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gwcharmetric(
    _d: drawing,
    _f: font,
    _c: c_int,
    _ascent: *mut c_int,
    _descent: *mut c_int,
    _width: *mut c_int,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gwdrawstr1(
    _d: drawing,
    _f: font,
    _c: rgb,
    _p: point,
    _s: *const std::os::raw::c_int,
    _cnt: c_int,
    _hadj: f64,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gstrwidth1(
    _d: drawing,
    _f: font,
    _s: *const std::os::raw::c_char,
    _enc: c_int,
) -> c_int {
    0
}
