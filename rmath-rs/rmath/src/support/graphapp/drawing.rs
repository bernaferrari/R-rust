#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Drawing primitives for GraphApp.
//!
//! Ported from drawing.c - provides line, rectangle, ellipse, polygon drawing,
//! pixel operations, and bitmap transfer functions.

use std::os::raw::c_int;
use std::ptr;

use super::gdraw;
use super::runtime::with_graphapp_runtime;
use super::types::*;

#[repr(transparent)]
pub struct MutPtr<T>(*mut T);

impl<T> std::ops::Deref for MutPtr<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.0 }
    }
}

impl<T> std::ops::DerefMut for MutPtr<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.0 }
    }
}

pub unsafe fn get_current_drawstate() -> &'static drawstruct {
    let ptr = with_graphapp_runtime(|runtime| &runtime.current_drawstate as *const drawstruct);
    unsafe { &*ptr }
}

pub unsafe fn get_current_drawstate_mut() -> MutPtr<drawstruct> {
    MutPtr(with_graphapp_runtime(|runtime| {
        &mut runtime.current_drawstate as *mut drawstruct
    }))
}

/// Set the current RGB colour.
pub extern "C" fn setrgb(c: rgb) {
    with_graphapp_runtime(|runtime| runtime.current_drawstate.hue = c);
}

/// Get the current drawing destination.
pub extern "C" fn currentdrawing() -> drawing {
    with_graphapp_runtime(|runtime| runtime.current_drawstate.dest)
}

/// Get the current RGB colour.
pub extern "C" fn currentrgb() -> rgb {
    with_graphapp_runtime(|runtime| runtime.current_drawstate.hue)
}

/// Get the current drawing mode.
pub extern "C" fn currentmode() -> c_int {
    with_graphapp_runtime(|runtime| runtime.current_drawstate.mode)
}

/// Get the current drawing point.
pub extern "C" fn currentpoint() -> point {
    with_graphapp_runtime(|runtime| runtime.current_drawstate.p)
}

/// Get the current line width.
pub extern "C" fn currentlinewidth() -> c_int {
    with_graphapp_runtime(|runtime| runtime.current_drawstate.linewidth)
}

/// Get the current font.
pub extern "C" fn currentfont() -> font {
    with_graphapp_runtime(|runtime| runtime.current_drawstate.fnt)
}

/// Get the current cursor.
pub extern "C" fn currentcursor() -> cursor {
    with_graphapp_runtime(|runtime| runtime.current_drawstate.crsr)
}

/// Set the drawing mode.
pub extern "C" fn setdrawmode(mode: c_int) {
    with_graphapp_runtime(|runtime| runtime.current_drawstate.mode = mode);
}

/// Set the line width.
pub extern "C" fn setlinewidth(width: c_int) {
    with_graphapp_runtime(|runtime| runtime.current_drawstate.linewidth = width);
}

/// Move the current drawing point.
pub extern "C" fn moveto(p: point) {
    with_graphapp_runtime(|runtime| runtime.current_drawstate.p = p);
}

/// Draw a line from the current point to the given point.
pub extern "C" fn lineto(p: point) {
    let from = currentpoint();
    unsafe {
        drawline(from, p);
    }
    with_graphapp_runtime(|runtime| runtime.current_drawstate.p = p);
}

/// Draw a single point.
pub extern "C" fn drawpoint(p: point) {
    unsafe { setpixel(p, currentrgb()) };
}

/// Draw a line between two points.
pub unsafe fn drawline(p1: point, p2: point) {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::gdrawline(ds.dest, ds.linewidth, lSolid, ds.hue, p1, p2, 0, 0, 0, 0.0) };
}

/// Draw a rectangle outline.
pub unsafe fn drawrect(r: rect) {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::gdrawrect(ds.dest, ds.linewidth, lSolid, ds.hue, r, 0, 0, 0, 0.0) };
}

/// Fill a rectangle.
pub unsafe fn fillrect(r: rect) {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::gfillrect(ds.dest, ds.hue, r) };
}

/// Draw an arc within the bounding rectangle.
///
/// Arc drawing is not exposed through the `gdraw` interface in this port.
/// Headless no-op: R's graphics engine uses `GEArc`/`GECircle` directly,
/// bypassing this GraphApp layer.
pub unsafe fn drawarc(_r: rect, _start_angle: c_int, _end_angle: c_int) {}

/// Fill an arc (pie slice) within the bounding rectangle.
///
/// Headless no-op: R's graphics engine uses its own arc primitives.
pub unsafe fn fillarc(_r: rect, _start_angle: c_int, _end_angle: c_int) {}

/// Draw an ellipse.
pub unsafe fn drawellipse(r: rect) {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::gdrawellipse(ds.dest, ds.linewidth, ds.hue, r, 0, 0, 0, 0.0) };
}

/// Fill an ellipse.
pub unsafe fn fillellipse(r: rect) {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::gfillellipse(ds.dest, ds.hue, r) };
}

/// Old fillellipse using platform Ellipse function.
pub unsafe fn oldfillellipse(r: rect) {
    unsafe {
        fillellipse(r);
    }
}

/// Draw a rounded rectangle outline.
pub unsafe fn drawroundrect(r: rect) {
    unsafe { drawrect(r) };
}

/// Fill a rounded rectangle.
pub unsafe fn fillroundrect(r: rect) {
    unsafe { fillrect(r) };
}

/// Draw a polygon.
pub unsafe fn drawpolygon(p: *mut point, n: c_int) {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::gdrawpolygon(ds.dest, ds.linewidth, lSolid, ds.hue, p, n, 0, 0, 0, 0.0) };
}

/// Fill a polygon.
pub unsafe fn fillpolygon(p: *mut point, n: c_int) {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::gfillpolygon(ds.dest, ds.hue, p, n) };
}

/// Draw a string at the given position.
#[allow(clippy::if_same_then_else)]
pub unsafe fn drawstr(p: point, s: *const std::os::raw::c_char) -> c_int {
    if s.is_null() {
        return 0;
    }
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::gdrawstr(ds.dest, ds.fnt, ds.hue, p, s) }
}

/// Get the bounding rectangle of a string with the given font.
pub unsafe fn strrect(f: font, s: *const std::os::raw::c_char) -> rect {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::gstrrect(ds.dest, f, s) }
}

/// Get the size of a string with the given font.
pub unsafe fn strsize(f: font, s: *const std::os::raw::c_char) -> point {
    unsafe {
        let r = strrect(f, s);
        point {
            x: r.width,
            y: r.height,
        }
    }
}

/// Get the width of a string with the given font.
pub unsafe fn strwidth(f: font, s: *const std::os::raw::c_char) -> c_int {
    unsafe { strrect(f, s).width }
}

/// Get a pixel colour.
pub unsafe fn getpixel(p: point) -> rgb {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::ggetpixel(ds.dest, p) }
}

/// Set a pixel colour.
pub unsafe fn setpixel(p: point, c: rgb) {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::gsetpixel(ds.dest, p, c) };
}

/// Bit-block transfer.
pub unsafe fn bitblt(db: bitmap, sb: bitmap, p: point, r: rect, _mode: c_int) {
    unsafe { gdraw::gbitblt(db, sb, p, r) };
}

/// Scroll a rectangle.
pub unsafe fn scrollrect(dp: point, r: rect) {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::gscroll(ds.dest, dp, r) };
}

/// Copy a rectangle from source to current destination.
pub unsafe fn copyrect(sb: bitmap, p: point, r: rect) {
    unsafe { bitblt(currentdrawing() as bitmap, sb, p, r, currentmode()) };
}

/// Texture-fill a rectangle with a bitmap.
///
/// No `gdraw` texture callback exists. Headless no-op.
pub unsafe fn texturerect(_sb: bitmap, _dr: rect) {}

/// Invert a rectangle.
pub unsafe fn invertrect(r: rect) {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::ginvert(ds.dest, r) };
}

/// Draw an image.
pub unsafe fn drawimage(img: image, dr: rect, sr: rect) {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::gdrawimage(ds.dest, img, dr, sr) };
}

/// Draw an image in monochrome.
///
/// No `gdraw` monochrome callback. Headless no-op.
pub unsafe fn drawmonochrome(_img: image, _dr: rect, _sr: rect) {}

/// Draw an image in greyscale.
///
/// No `gdraw` greyscale callback. Headless no-op.
pub unsafe fn drawgreyscale(_img: image, _dr: rect, _sr: rect) {}

/// Draw an image darker.
///
/// No `gdraw` darker callback. Headless no-op.
pub unsafe fn drawdarker(_img: image, _dr: rect, _sr: rect) {}

/// Draw an image brighter.
///
/// No `gdraw` brighter callback. Headless no-op.
pub unsafe fn drawbrighter(_img: image, _dr: rect, _sr: rect) {}

/// Get the clipping rectangle.
pub unsafe fn getcliprect() -> rect {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::ggetcliprect(ds.dest) }
}

/// Set the clipping rectangle.
pub unsafe fn setcliprect(r: rect) {
    let ds = unsafe { get_current_drawstate() };
    unsafe { gdraw::gsetcliprect(ds.dest, r) };
}

/// Copy the current draw state.
pub unsafe fn copydrawstate() -> drawstate {
    unsafe {
        let ds = super::memory::memalloc(std::mem::size_of::<drawstruct>() as i64) as drawstate;
        if !ds.is_null() {
            *ds = with_graphapp_runtime(|runtime| runtime.current_drawstate.clone());
        }
        ds
    }
}

/// Set the draw state.
pub extern "C" fn setdrawstate(saved: drawstate) {
    if !saved.is_null() {
        unsafe {
            with_graphapp_runtime(|runtime| runtime.current_drawstate = (*saved).clone());
        }
    }
}

/// Restore a draw state (alias for setdrawstate).
pub extern "C" fn restoredrawstate(saved: drawstate) {
    setdrawstate(saved);
}

/// Reset the draw state to defaults.
pub extern "C" fn resetdrawstate() {
    with_graphapp_runtime(|runtime| {
        runtime.current_drawstate.dest = ptr::null_mut();
        runtime.current_drawstate.hue = Black;
        runtime.current_drawstate.mode = GA_S;
        runtime.current_drawstate.p = point { x: 0, y: 0 };
        runtime.current_drawstate.linewidth = 1;
        runtime.current_drawstate.fnt = ptr::null_mut();
        runtime.current_drawstate.crsr = ptr::null_mut();
    });
}

/// Set the draw destination.
pub extern "C" fn drawto(dest: drawing) {
    with_graphapp_runtime(|runtime| runtime.current_drawstate.dest = dest);
}

/// Add a control to the current window.
///
/// Headless no-op: no window system to add controls to.
pub unsafe fn addto(_dest: control) {}

/// Set the current cursor.
pub extern "C" fn setcursor(c: cursor) {
    with_graphapp_runtime(|runtime| runtime.current_drawstate.crsr = c);
}

/// Set the current font.
pub extern "C" fn setfont(f: font) {
    with_graphapp_runtime(|runtime| runtime.current_drawstate.fnt = f);
}

/// Set the caret position and size.
///
/// Headless no-op: no text input caret in a headless rendering environment.
pub unsafe fn setcaret(_c: control, _x: c_int, _y: c_int, _width: c_int, _height: c_int) {}

/// Show/hide the caret.
///
/// Headless no-op: no text input caret in a headless rendering environment.
pub unsafe fn showcaret(_c: control, _showing: c_int) {}
