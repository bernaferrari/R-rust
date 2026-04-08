#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Drawing primitives for GraphApp.
//!
//! Ported from drawing.c - provides line, rectangle, ellipse, polygon drawing,
//! pixel operations, and bitmap transfer functions.

use std::cell::RefCell;
use std::os::raw::c_int;
use std::ptr;

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
struct MutPtr<T>(*mut T);

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

/// Get a reference to the global drawstate.
pub unsafe fn get_current_drawstate() -> &'static drawstruct {
    unsafe { CURRENT_DRAWSTATE.with(|v| &*v.as_ptr()) }
}

/// Get a mutable reference to the global drawstate.
pub unsafe fn get_current_drawstate_mut() -> MutPtr<drawstruct> {
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
        let ds = v.borrow_mut();
        drawline(ds.p, p);
    });
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().p = p);
}

/// Draw a single point.
pub extern "C" fn drawpoint(p: point) {
    CURRENT_DRAWSTATE.with(|v| setpixel(p, v.borrow().hue));
}

/// Draw a line between two points.
pub unsafe fn drawline(_p1: point, _p2: point) {
    // TODO: Platform-specific rendering
}

/// Draw a rectangle outline.
pub unsafe fn drawrect(_r: rect) {
    // TODO: Platform-specific rendering
}

/// Fill a rectangle.
pub unsafe fn fillrect(_r: rect) {
    // TODO: Platform-specific rendering
}

/// Draw an arc.
pub unsafe fn drawarc(_r: rect, _start_angle: c_int, _end_angle: c_int) {
    // TODO: Platform-specific rendering
}

/// Fill an arc (pie slice).
pub unsafe fn fillarc(_r: rect, _start_angle: c_int, _end_angle: c_int) {
    // TODO: Platform-specific rendering
}

/// Draw an ellipse.
pub unsafe fn drawellipse(_r: rect) {
    // TODO: Platform-specific rendering
}

/// Fill an ellipse.
pub unsafe fn fillellipse(_r: rect) {
    // TODO: Platform-specific rendering
}

/// Old fillellipse using platform Ellipse function.
pub extern "C" fn oldfillellipse(r: rect) {
    fillellipse(r);
}

/// Draw a rounded rectangle outline.
pub unsafe fn drawroundrect(_r: rect) {
    // TODO: Platform-specific rendering
}

/// Fill a rounded rectangle.
pub unsafe fn fillroundrect(_r: rect) {
    // TODO: Platform-specific rendering
}

/// Draw a polygon.
pub unsafe fn drawpolygon(_p: *mut point, _n: c_int) {
    // TODO: Platform-specific rendering
}

/// Fill a polygon.
pub unsafe fn fillpolygon(_p: *mut point, _n: c_int) {
    // TODO: Platform-specific rendering
}

/// Draw a string at the given position.
pub unsafe fn drawstr(_p: point, s: *const std::os::raw::c_char) -> c_int {
    // TODO: Platform-specific rendering
    if s.is_null() { 0 } else { 0 }
}

/// Get the bounding rectangle of a string with the given font.
pub unsafe fn strrect(_f: font, _s: *const std::os::raw::c_char) -> rect {
    // TODO: Platform-specific font measurement
    rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    }
}

/// Get the size of a string with the given font.
pub extern "C" fn strsize(f: font, s: *const std::os::raw::c_char) -> point {
    let r = strrect(f, s);
    point {
        x: r.width,
        y: r.height,
    }
}

/// Get the width of a string with the given font.
pub extern "C" fn strwidth(f: font, s: *const std::os::raw::c_char) -> c_int {
    strrect(f, s).width
}

/// Get a pixel colour.
pub unsafe fn getpixel(_p: point) -> rgb {
    // TODO: Platform-specific
    Black
}

/// Set a pixel colour.
pub unsafe fn setpixel(_p: point, _c: rgb) {
    // TODO: Platform-specific
}

/// Bit-block transfer.
pub unsafe fn bitblt(_db: bitmap, _sb: bitmap, _p: point, _r: rect, _mode: c_int) {
    // TODO: Platform-specific
}

/// Scroll a rectangle.
pub unsafe fn scrollrect(_dp: point, _r: rect) {
    // TODO: Platform-specific
}

/// Copy a rectangle from source to current destination.
pub unsafe fn copyrect(_sb: bitmap, _p: point, _r: rect) {
    // TODO: Platform-specific
}

/// Texture-fill a rectangle with a bitmap.
pub unsafe fn texturerect(_sb: bitmap, _dr: rect) {
    // TODO: Platform-specific
}

/// Invert a rectangle.
pub unsafe fn invertrect(_r: rect) {
    // TODO: Platform-specific
}

/// Draw an image.
pub unsafe fn drawimage(_img: image, _dr: rect, _sr: rect) {
    // TODO: Platform-specific
}

/// Draw an image in monochrome.
pub unsafe fn drawmonochrome(_img: image, _dr: rect, _sr: rect) {
    // TODO: Platform-specific
}

/// Draw an image in greyscale.
pub unsafe fn drawgreyscale(_img: image, _dr: rect, _sr: rect) {
    // TODO: Platform-specific
}

/// Draw an image darker.
pub unsafe fn drawdarker(_img: image, _dr: rect, _sr: rect) {
    // TODO: Platform-specific
}

/// Draw an image brighter.
pub unsafe fn drawbrighter(_img: image, _dr: rect, _sr: rect) {
    // TODO: Platform-specific
}

/// Get the clipping rectangle.
pub unsafe fn getcliprect() -> rect {
    // TODO: Platform-specific
    rect::default()
}

/// Set the clipping rectangle.
pub unsafe fn setcliprect(_r: rect) {
    // TODO: Platform-specific
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
        CURRENT_DRAWSTATE.with(|v| *v.borrow_mut() = (*saved).clone());
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
pub unsafe fn addto(_dest: control) {
    // TODO: Platform-specific
}

/// Set the current cursor.
pub extern "C" fn setcursor(c: cursor) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().crsr = c);
}

/// Set the current font.
pub extern "C" fn setfont(f: font) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().fnt = f);
}

/// Set the caret.
pub unsafe fn setcaret(
    _c: control,
    _x: c_int,
    _y: c_int,
    _width: c_int,
    _height: c_int,
) {
    // TODO: Platform-specific
}

/// Show/hide the caret.
pub unsafe fn showcaret(_c: control, _showing: c_int) {
    // TODO: Platform-specific
}
