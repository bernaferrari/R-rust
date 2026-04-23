#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Drawing primitives for GraphApp.
//!
//! Ported from drawing.c - provides line, rectangle, ellipse, polygon drawing,
//! pixel operations, and bitmap transfer functions.

use std::cell::RefCell;
use std::os::raw::c_int;
use std::ptr;

use super::gdraw;
use super::types::*;

thread_local! { static CURRENT_DRAWSTATE: RefCell<drawstruct> = RefCell::new(drawstruct {
    dest: ptr::null_mut(),
    hue: Black,
    mode: GA_S,
    p: point { x: 0, y: 0 },
    linewidth: 1,
    fnt: ptr::null_mut(),
    crsr: ptr::null_mut(),
}); }

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

pub fn get_current_drawstate() -> &'static drawstruct {
    unsafe { CURRENT_DRAWSTATE.with(|v| &*v.as_ptr()) }
}

pub fn get_current_drawstate_mut() -> MutPtr<drawstruct> {
    MutPtr(CURRENT_DRAWSTATE.with(|v| v.as_ptr() as *mut drawstruct))
}

/// Set the current RGB colour.
pub extern "C" fn setrgb(c: rgb) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().hue = c);
}

/// Get the current drawing destination.
pub extern "C" fn currentdrawing() -> drawing {
    CURRENT_DRAWSTATE.with(|v| v.borrow().dest)
}

/// Get the current RGB colour.
pub extern "C" fn currentrgb() -> rgb {
    CURRENT_DRAWSTATE.with(|v| v.borrow().hue)
}

/// Get the current drawing mode.
pub extern "C" fn currentmode() -> c_int {
    CURRENT_DRAWSTATE.with(|v| v.borrow().mode)
}

/// Get the current drawing point.
pub extern "C" fn currentpoint() -> point {
    CURRENT_DRAWSTATE.with(|v| v.borrow().p)
}

/// Get the current line width.
pub extern "C" fn currentlinewidth() -> c_int {
    CURRENT_DRAWSTATE.with(|v| v.borrow().linewidth)
}

/// Get the current font.
pub extern "C" fn currentfont() -> font {
    CURRENT_DRAWSTATE.with(|v| v.borrow().fnt)
}

/// Get the current cursor.
pub extern "C" fn currentcursor() -> cursor {
    CURRENT_DRAWSTATE.with(|v| v.borrow().crsr)
}

/// Set the drawing mode.
pub extern "C" fn setdrawmode(mode: c_int) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().mode = mode);
}

/// Set the line width.
pub extern "C" fn setlinewidth(width: c_int) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().linewidth = width);
}

/// Move the current drawing point.
pub extern "C" fn moveto(p: point) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().p = p);
}

/// Draw a line from the current point to the given point.
pub extern "C" fn lineto(p: point) {
    CURRENT_DRAWSTATE.with(|v| {
        drawline(v.borrow().p, p);
    });
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().p = p);
}

/// Draw a single point.
pub extern "C" fn drawpoint(p: point) {
    CURRENT_DRAWSTATE.with(|v| setpixel(p, v.borrow().hue));
}

/// Draw a line between two points.
pub fn drawline(p1: point, p2: point) {
    let ds = get_current_drawstate();
    gdraw::gdrawline(ds.dest, ds.linewidth, lSolid, ds.hue, p1, p2, 0, 0, 0, 0.0);
}

/// Draw a rectangle outline.
pub fn drawrect(r: rect) {
    let ds = get_current_drawstate();
    gdraw::gdrawrect(ds.dest, ds.linewidth, lSolid, ds.hue, r, 0, 0, 0, 0.0);
}

/// Fill a rectangle.
pub fn fillrect(r: rect) {
    let ds = get_current_drawstate();
    gdraw::gfillrect(ds.dest, ds.hue, r);
}

/// Draw an arc within the bounding rectangle.
///
/// Arc drawing is not exposed through the `gdraw` interface in this port.
/// Headless no-op: R's graphics engine uses `GEArc`/`GECircle` directly,
/// bypassing this GraphApp layer.
pub fn drawarc(_r: rect, _start_angle: c_int, _end_angle: c_int) {}

/// Fill an arc (pie slice) within the bounding rectangle.
///
/// Headless no-op: R's graphics engine uses its own arc primitives.
pub fn fillarc(_r: rect, _start_angle: c_int, _end_angle: c_int) {}

/// Draw an ellipse.
pub fn drawellipse(r: rect) {
    let ds = get_current_drawstate();
    gdraw::gdrawellipse(ds.dest, ds.linewidth, ds.hue, r, 0, 0, 0, 0.0);
}

/// Fill an ellipse.
pub fn fillellipse(r: rect) {
    let ds = get_current_drawstate();
    gdraw::gfillellipse(ds.dest, ds.hue, r);
}

/// Old fillellipse using platform Ellipse function.
pub fn oldfillellipse(r: rect) {
    fillellipse(r);
}

/// Draw a rounded rectangle outline.
pub fn drawroundrect(r: rect) {
    drawrect(r);
}

/// Fill a rounded rectangle.
pub fn fillroundrect(r: rect) {
    fillrect(r);
}

/// Draw a polygon.
pub unsafe fn drawpolygon(p: *mut point, n: c_int) {
    let ds = unsafe { get_current_drawstate() };
    gdraw::gdrawpolygon(ds.dest, ds.linewidth, lSolid, ds.hue, p, n, 0, 0, 0, 0.0);
}

/// Fill a polygon.
pub unsafe fn fillpolygon(p: *mut point, n: c_int) {
    let ds = unsafe { get_current_drawstate() };
    gdraw::gfillpolygon(ds.dest, ds.hue, p, n);
}

/// Draw a string at the given position.
#[allow(clippy::if_same_then_else)]
pub unsafe fn drawstr(p: point, s: *const std::os::raw::c_char) -> c_int {
    if s.is_null() {
        return 0;
    }
    let ds = unsafe { get_current_drawstate() };
    gdraw::gdrawstr(ds.dest, ds.fnt, ds.hue, p, s)
}

/// Get the bounding rectangle of a string with the given font.
pub unsafe fn strrect(f: font, s: *const std::os::raw::c_char) -> rect {
    let ds = unsafe { get_current_drawstate() };
    gdraw::gstrrect(ds.dest, f, s)
}

/// Get the size of a string with the given font.
pub unsafe fn strsize(f: font, s: *const std::os::raw::c_char) -> point {
    let r = strrect(f, s);
    point {
        x: r.width,
        y: r.height,
    }
}

/// Get the width of a string with the given font.
pub unsafe fn strwidth(f: font, s: *const std::os::raw::c_char) -> c_int {
    strrect(f, s).width
}

/// Get a pixel colour.
pub fn getpixel(p: point) -> rgb {
    let ds = get_current_drawstate();
    gdraw::ggetpixel(ds.dest, p)
}

/// Set a pixel colour.
pub fn setpixel(p: point, c: rgb) {
    let ds = get_current_drawstate();
    gdraw::gsetpixel(ds.dest, p, c);
}

/// Bit-block transfer.
pub fn bitblt(db: bitmap, sb: bitmap, p: point, r: rect, _mode: c_int) {
    gdraw::gbitblt(db, sb, p, r);
}

/// Scroll a rectangle.
pub fn scrollrect(dp: point, r: rect) {
    let ds = get_current_drawstate();
    gdraw::gscroll(ds.dest, dp, r);
}

/// Copy a rectangle from source to current destination.
pub fn copyrect(sb: bitmap, p: point, r: rect) {
    bitblt(currentdrawing() as bitmap, sb, p, r, currentmode());
}

/// Texture-fill a rectangle with a bitmap.
///
/// No `gdraw` texture callback exists. Headless no-op.
pub fn texturerect(_sb: bitmap, _dr: rect) {}

/// Invert a rectangle.
pub fn invertrect(r: rect) {
    let ds = get_current_drawstate();
    gdraw::ginvert(ds.dest, r);
}

/// Draw an image.
pub fn drawimage(img: image, dr: rect, sr: rect) {
    let ds = get_current_drawstate();
    gdraw::gdrawimage(ds.dest, img, dr, sr);
}

/// Draw an image in monochrome.
///
/// No `gdraw` monochrome callback. Headless no-op.
pub fn drawmonochrome(_img: image, _dr: rect, _sr: rect) {}

/// Draw an image in greyscale.
///
/// No `gdraw` greyscale callback. Headless no-op.
pub fn drawgreyscale(_img: image, _dr: rect, _sr: rect) {}

/// Draw an image darker.
///
/// No `gdraw` darker callback. Headless no-op.
pub fn drawdarker(_img: image, _dr: rect, _sr: rect) {}

/// Draw an image brighter.
///
/// No `gdraw` brighter callback. Headless no-op.
pub fn drawbrighter(_img: image, _dr: rect, _sr: rect) {}

/// Get the clipping rectangle.
pub fn getcliprect() -> rect {
    let ds = get_current_drawstate();
    gdraw::ggetcliprect(ds.dest)
}

/// Set the clipping rectangle.
pub fn setcliprect(r: rect) {
    let ds = get_current_drawstate();
    gdraw::gsetcliprect(ds.dest, r);
}

/// Copy the current draw state.
pub unsafe fn copydrawstate() -> drawstate {
    unsafe {
        let ds = super::memory::memalloc(std::mem::size_of::<drawstruct>() as i64) as drawstate;
        if !ds.is_null() {
            *ds = CURRENT_DRAWSTATE.with(|v| v.borrow().clone());
        }
        ds
    }
}

/// Set the draw state.
pub extern "C" fn setdrawstate(saved: drawstate) {
    if !saved.is_null() {
        CURRENT_DRAWSTATE.with(|v| *v.borrow_mut() = unsafe { (*saved).clone() });
    }
}

/// Restore a draw state (alias for setdrawstate).
pub extern "C" fn restoredrawstate(saved: drawstate) {
    setdrawstate(saved);
}

/// Reset the draw state to defaults.
pub extern "C" fn resetdrawstate() {
    CURRENT_DRAWSTATE.with(|v| {
        let mut ds = v.borrow_mut();
        ds.dest = ptr::null_mut();
        ds.hue = Black;
        ds.mode = GA_S;
        ds.p = point { x: 0, y: 0 };
        ds.linewidth = 1;
        ds.fnt = ptr::null_mut();
        ds.crsr = ptr::null_mut();
    });
}

/// Set the draw destination.
pub extern "C" fn drawto(dest: drawing) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().dest = dest);
}

/// Add a control to the current window.
///
/// Headless no-op: no window system to add controls to.
pub fn addto(_dest: control) {}

/// Set the current cursor.
pub extern "C" fn setcursor(c: cursor) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().crsr = c);
}

/// Set the current font.
pub extern "C" fn setfont(f: font) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().fnt = f);
}

/// Set the caret position and size.
///
/// Headless no-op: no text input caret in a headless rendering environment.
pub fn setcaret(_c: control, _x: c_int, _y: c_int, _width: c_int, _height: c_int) {}

/// Show/hide the caret.
///
/// Headless no-op: no text input caret in a headless rendering environment.
pub fn showcaret(_c: control, _showing: c_int) {}
