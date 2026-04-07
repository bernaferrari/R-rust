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
#[unsafe(no_mangle)]
pub extern "C" fn setrgb(c: rgb) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().hue = c);
}

/// Get the current drawing destination.
#[unsafe(no_mangle)]
pub extern "C" fn currentdrawing() -> drawing {
    CURRENT_DRAWSTATE.with(|v| v.borrow().dest)
}

/// Get the current RGB colour.
#[unsafe(no_mangle)]
pub extern "C" fn currentrgb() -> rgb {
    CURRENT_DRAWSTATE.with(|v| v.borrow().hue)
}

/// Get the current drawing mode.
#[unsafe(no_mangle)]
pub extern "C" fn currentmode() -> c_int {
    CURRENT_DRAWSTATE.with(|v| v.borrow().mode)
}

/// Get the current drawing point.
#[unsafe(no_mangle)]
pub extern "C" fn currentpoint() -> point {
    CURRENT_DRAWSTATE.with(|v| v.borrow().p)
}

/// Get the current line width.
#[unsafe(no_mangle)]
pub extern "C" fn currentlinewidth() -> c_int {
    CURRENT_DRAWSTATE.with(|v| v.borrow().linewidth)
}

/// Get the current font.
#[unsafe(no_mangle)]
pub extern "C" fn currentfont() -> font {
    CURRENT_DRAWSTATE.with(|v| v.borrow().fnt)
}

/// Get the current cursor.
#[unsafe(no_mangle)]
pub extern "C" fn currentcursor() -> cursor {
    CURRENT_DRAWSTATE.with(|v| v.borrow().crsr)
}

/// Set the drawing mode.
#[unsafe(no_mangle)]
pub extern "C" fn setdrawmode(mode: c_int) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().mode = mode);
}

/// Set the line width.
#[unsafe(no_mangle)]
pub extern "C" fn setlinewidth(width: c_int) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().linewidth = width);
}

/// Move the current drawing point.
#[unsafe(no_mangle)]
pub extern "C" fn moveto(p: point) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().p = p);
}

/// Draw a line from the current point to the given point.
#[unsafe(no_mangle)]
pub extern "C" fn lineto(p: point) {
    CURRENT_DRAWSTATE.with(|v| {
        let ds = v.borrow_mut();
        drawline(ds.p, p);
    });
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().p = p);
}

/// Draw a single point.
#[unsafe(no_mangle)]
pub extern "C" fn drawpoint(p: point) {
    CURRENT_DRAWSTATE.with(|v| setpixel(p, v.borrow().hue));
}

/// Draw a line between two points.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawline(_p1: point, _p2: point) {
    // TODO: Platform-specific rendering
}

/// Draw a rectangle outline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawrect(_r: rect) {
    // TODO: Platform-specific rendering
}

/// Fill a rectangle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fillrect(_r: rect) {
    // TODO: Platform-specific rendering
}

/// Draw an arc.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawarc(_r: rect, _start_angle: c_int, _end_angle: c_int) {
    // TODO: Platform-specific rendering
}

/// Fill an arc (pie slice).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fillarc(_r: rect, _start_angle: c_int, _end_angle: c_int) {
    // TODO: Platform-specific rendering
}

/// Draw an ellipse.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawellipse(_r: rect) {
    // TODO: Platform-specific rendering
}

/// Fill an ellipse.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fillellipse(_r: rect) {
    // TODO: Platform-specific rendering
}

/// Old fillellipse using platform Ellipse function.
#[unsafe(no_mangle)]
pub extern "C" fn oldfillellipse(r: rect) {
    fillellipse(r);
}

/// Draw a rounded rectangle outline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawroundrect(_r: rect) {
    // TODO: Platform-specific rendering
}

/// Fill a rounded rectangle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fillroundrect(_r: rect) {
    // TODO: Platform-specific rendering
}

/// Draw a polygon.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawpolygon(_p: *mut point, _n: c_int) {
    // TODO: Platform-specific rendering
}

/// Fill a polygon.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fillpolygon(_p: *mut point, _n: c_int) {
    // TODO: Platform-specific rendering
}

/// Draw a string at the given position.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawstr(_p: point, s: *const std::os::raw::c_char) -> c_int {
    // TODO: Platform-specific rendering
    if s.is_null() { 0 } else { 0 }
}

/// Get the bounding rectangle of a string with the given font.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strrect(_f: font, _s: *const std::os::raw::c_char) -> rect {
    // TODO: Platform-specific font measurement
    rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    }
}

/// Get the size of a string with the given font.
#[unsafe(no_mangle)]
pub extern "C" fn strsize(f: font, s: *const std::os::raw::c_char) -> point {
    let r = strrect(f, s);
    point {
        x: r.width,
        y: r.height,
    }
}

/// Get the width of a string with the given font.
#[unsafe(no_mangle)]
pub extern "C" fn strwidth(f: font, s: *const std::os::raw::c_char) -> c_int {
    strrect(f, s).width
}

/// Get a pixel colour.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getpixel(_p: point) -> rgb {
    // TODO: Platform-specific
    Black
}

/// Set a pixel colour.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setpixel(_p: point, _c: rgb) {
    // TODO: Platform-specific
}

/// Bit-block transfer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bitblt(_db: bitmap, _sb: bitmap, _p: point, _r: rect, _mode: c_int) {
    // TODO: Platform-specific
}

/// Scroll a rectangle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scrollrect(_dp: point, _r: rect) {
    // TODO: Platform-specific
}

/// Copy a rectangle from source to current destination.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn copyrect(_sb: bitmap, _p: point, _r: rect) {
    // TODO: Platform-specific
}

/// Texture-fill a rectangle with a bitmap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn texturerect(_sb: bitmap, _dr: rect) {
    // TODO: Platform-specific
}

/// Invert a rectangle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn invertrect(_r: rect) {
    // TODO: Platform-specific
}

/// Draw an image.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawimage(_img: image, _dr: rect, _sr: rect) {
    // TODO: Platform-specific
}

/// Draw an image in monochrome.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawmonochrome(_img: image, _dr: rect, _sr: rect) {
    // TODO: Platform-specific
}

/// Draw an image in greyscale.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawgreyscale(_img: image, _dr: rect, _sr: rect) {
    // TODO: Platform-specific
}

/// Draw an image darker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawdarker(_img: image, _dr: rect, _sr: rect) {
    // TODO: Platform-specific
}

/// Draw an image brighter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawbrighter(_img: image, _dr: rect, _sr: rect) {
    // TODO: Platform-specific
}

/// Get the clipping rectangle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getcliprect() -> rect {
    // TODO: Platform-specific
    rect::default()
}

/// Set the clipping rectangle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setcliprect(_r: rect) {
    // TODO: Platform-specific
}

/// Copy the current draw state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn copydrawstate() -> drawstate {
    unsafe {
        let ds = super::memory::memalloc(std::mem::size_of::<drawstruct>() as i64) as drawstate;
        if !ds.is_null() {
            *ds = CURRENT_DRAWSTATE.with(|v| v.borrow().clone());
        }
        ds
    }
}

/// Set the draw state.
#[unsafe(no_mangle)]
pub extern "C" fn setdrawstate(saved: drawstate) {
    if !saved.is_null() {
        CURRENT_DRAWSTATE.with(|v| *v.borrow_mut() = (*saved).clone());
    }
}

/// Restore a draw state (alias for setdrawstate).
#[unsafe(no_mangle)]
pub extern "C" fn restoredrawstate(saved: drawstate) {
    setdrawstate(saved);
}

/// Reset the draw state to defaults.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn drawto(dest: drawing) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().dest = dest);
}

/// Add a control to the current window.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn addto(_dest: control) {
    // TODO: Platform-specific
}

/// Set the current cursor.
#[unsafe(no_mangle)]
pub extern "C" fn setcursor(c: cursor) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().crsr = c);
}

/// Set the current font.
#[unsafe(no_mangle)]
pub extern "C" fn setfont(f: font) {
    CURRENT_DRAWSTATE.with(|v| v.borrow_mut().fnt = f);
}

/// Set the caret.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setcaret(
    _c: control,
    _x: c_int,
    _y: c_int,
    _width: c_int,
    _height: c_int,
) {
    // TODO: Platform-specific
}

/// Show/hide the caret.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn showcaret(_c: control, _showing: c_int) {
    // TODO: Platform-specific
}
