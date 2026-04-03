#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Extended drawing functions for GraphApp.
//!
//! Ported from gdraw.c - thread-safe and extended drawing functions.

use std::os::raw::c_int;
use std::ptr;

use super::types::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ggetcliprect(d: drawing) -> rect {
    rect::default()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsetcliprect(d: drawing, r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gbitblt(db: bitmap, sb: bitmap, p: point, r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gscroll(d: drawing, dp: point, r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ginvert(d: drawing, r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ggetpixel(d: drawing, p: point) -> rgb {
    Black
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsetpixel(d: drawing, p: point, c: rgb) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawline(
    d: drawing,
    width: c_int,
    style: c_int,
    c: rgb,
    p1: point,
    p2: point,
    fast: c_int,
    lend: c_int,
    ljoin: c_int,
    lmitre: f32,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawrect(
    d: drawing,
    width: c_int,
    style: c_int,
    c: rgb,
    r: rect,
    fast: c_int,
    lend: c_int,
    ljoin: c_int,
    lmitre: f32,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gfillrect(d: drawing, fill: rgb, r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcopy(d: drawing, d2: drawing, r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcopyalpha(d: drawing, d2: drawing, r: rect, alpha: c_int) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcopyalpha2(d: drawing, src: image, r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawellipse(
    d: drawing,
    width: c_int,
    border: rgb,
    r: rect,
    fast: c_int,
    lend: c_int,
    ljoin: c_int,
    lmitre: f32,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gfillellipse(d: drawing, fill: rgb, r: rect) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawpolyline(
    d: drawing,
    width: c_int,
    style: c_int,
    c: rgb,
    p: *mut point,
    n: c_int,
    closepath: c_int,
    fast: c_int,
    lend: c_int,
    ljoin: c_int,
    lmitre: f32,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawpolygon(
    d: drawing,
    width: c_int,
    style: c_int,
    c: rgb,
    p: *mut point,
    n: c_int,
    fast: c_int,
    lend: c_int,
    ljoin: c_int,
    lmitre: f32,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsetpolyfillmode(d: drawing, oddeven: c_int) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gfillpolygon(d: drawing, fill: rgb, p: *mut point, n: c_int) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gfillpolypolygon(
    d: drawing,
    fill: rgb,
    p: *mut point,
    npoly: c_int,
    nper: *mut c_int,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawimage(d: drawing, img: image, dr: rect, sr: rect) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gmaskimage(d: drawing, img: image, dr: rect, sr: rect, mask: image) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawstr(
    d: drawing,
    f: font,
    c: rgb,
    p: point,
    s: *const std::os::raw::c_char,
) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawstr1(
    d: drawing,
    f: font,
    c: rgb,
    p: point,
    s: *const std::os::raw::c_char,
    hadj: f64,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gstrrect(d: drawing, f: font, s: *const std::os::raw::c_char) -> rect {
    rect::default()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gstrsize(d: drawing, f: font, s: *const std::os::raw::c_char) -> point {
    point::default()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gstrwidth(d: drawing, f: font, s: *const std::os::raw::c_char) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gcharmetric(
    d: drawing,
    f: font,
    c: c_int,
    ascent: *mut c_int,
    descent: *mut c_int,
    width: *mut c_int,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gnewfont(
    d: drawing,
    face: *const std::os::raw::c_char,
    style: c_int,
    size: c_int,
    rot: f64,
    usePoints: c_int,
) -> font {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gnewfont2(
    d: drawing,
    face: *const std::os::raw::c_char,
    style: c_int,
    size: c_int,
    rot: f64,
    usePoints: c_int,
    quality: c_int,
) -> font {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ghasfixedwidth(f: font) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newfield_no_border(text: *const std::os::raw::c_char, r: rect) -> field {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gdrawwcs(
    d: drawing,
    f: font,
    c: rgb,
    p: point,
    s: *const std::os::raw::c_int,
) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gwcswidth(d: drawing, f: font, s: *const std::os::raw::c_int) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gwcharmetric(
    d: drawing,
    f: font,
    c: c_int,
    ascent: *mut c_int,
    descent: *mut c_int,
    width: *mut c_int,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gwdrawstr1(
    d: drawing,
    f: font,
    c: rgb,
    p: point,
    s: *const std::os::raw::c_int,
    cnt: c_int,
    hadj: f64,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gstrwidth1(
    d: drawing,
    f: font,
    s: *const std::os::raw::c_char,
    enc: c_int,
) -> c_int {
    0
}
