#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Drawing primitives for GraphApp.
//!
//! Ported from drawing.c - provides line, rectangle, ellipse, polygon drawing,
//! pixel operations, and bitmap transfer functions.

use std::os::raw::c_int;
use std::ptr;

use super::types::*;

/// Global drawing state. Initialized in context.rs.
static mut CURRENT_DRAWSTATE: drawstruct = drawstruct {
    dest: ptr::null_mut(),
    hue: Black,
    mode: GA_S,
    p: point { x: 0, y: 0 },
    linewidth: 1,
    fnt: ptr::null_mut(),
    crsr: ptr::null_mut(),
};

/// Get a reference to the global drawstate.
pub unsafe fn get_current_drawstate() -> &'static drawstruct {
    unsafe { &*std::ptr::addr_of!(CURRENT_DRAWSTATE) }
}

/// Get a mutable reference to the global drawstate.
pub unsafe fn get_current_drawstate_mut() -> &'static mut drawstruct {
    unsafe { &mut *std::ptr::addr_of_mut!(CURRENT_DRAWSTATE) }
}

/// Set the current RGB colour.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setrgb(c: rgb) {
    unsafe {
        CURRENT_DRAWSTATE.hue = c;
    }
}

/// Get the current drawing destination.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn currentdrawing() -> drawing {
    unsafe { CURRENT_DRAWSTATE.dest }
}

/// Get the current RGB colour.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn currentrgb() -> rgb {
    unsafe { CURRENT_DRAWSTATE.hue }
}

/// Get the current drawing mode.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn currentmode() -> c_int {
    unsafe { CURRENT_DRAWSTATE.mode }
}

/// Get the current drawing point.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn currentpoint() -> point {
    unsafe { CURRENT_DRAWSTATE.p }
}

/// Get the current line width.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn currentlinewidth() -> c_int {
    unsafe { CURRENT_DRAWSTATE.linewidth }
}

/// Get the current font.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn currentfont() -> font {
    unsafe { CURRENT_DRAWSTATE.fnt }
}

/// Get the current cursor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn currentcursor() -> cursor {
    unsafe { CURRENT_DRAWSTATE.crsr }
}

/// Set the drawing mode.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setdrawmode(mode: c_int) {
    unsafe {
        CURRENT_DRAWSTATE.mode = mode;
    }
}

/// Set the line width.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setlinewidth(width: c_int) {
    unsafe {
        CURRENT_DRAWSTATE.linewidth = width;
    }
}

/// Move the current drawing point.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moveto(p: point) {
    unsafe {
        CURRENT_DRAWSTATE.p = p;
    }
}

/// Draw a line from the current point to the given point.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lineto(p: point) {
    unsafe {
        drawline(CURRENT_DRAWSTATE.p, p);
        CURRENT_DRAWSTATE.p = p;
    }
}

/// Draw a single point.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawpoint(p: point) {
    unsafe {
        setpixel(p, CURRENT_DRAWSTATE.hue);
    }
}

/// Draw a line between two points.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawline(p1: point, p2: point) {
    // TODO: Platform-specific rendering
}

/// Draw a rectangle outline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawrect(r: rect) {
    // TODO: Platform-specific rendering
}

/// Fill a rectangle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fillrect(r: rect) {
    // TODO: Platform-specific rendering
}

/// Draw an arc.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawarc(r: rect, start_angle: c_int, end_angle: c_int) {
    // TODO: Platform-specific rendering
}

/// Fill an arc (pie slice).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fillarc(r: rect, start_angle: c_int, end_angle: c_int) {
    // TODO: Platform-specific rendering
}

/// Draw an ellipse.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawellipse(r: rect) {
    // TODO: Platform-specific rendering
}

/// Fill an ellipse.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fillellipse(r: rect) {
    // TODO: Platform-specific rendering
}

/// Old fillellipse using platform Ellipse function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oldfillellipse(r: rect) {
    unsafe {
        fillellipse(r);
    }
}

/// Draw a rounded rectangle outline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawroundrect(r: rect) {
    // TODO: Platform-specific rendering
}

/// Fill a rounded rectangle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fillroundrect(r: rect) {
    // TODO: Platform-specific rendering
}

/// Draw a polygon.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawpolygon(p: *mut point, n: c_int) {
    // TODO: Platform-specific rendering
}

/// Fill a polygon.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fillpolygon(p: *mut point, n: c_int) {
    // TODO: Platform-specific rendering
}

/// Draw a string at the given position.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawstr(p: point, s: *const std::os::raw::c_char) -> c_int {
    // TODO: Platform-specific rendering
    if s.is_null() { 0 } else { 0 }
}

/// Get the bounding rectangle of a string with the given font.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strrect(f: font, s: *const std::os::raw::c_char) -> rect {
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
pub unsafe extern "C" fn strsize(f: font, s: *const std::os::raw::c_char) -> point {
    unsafe {
        let r = strrect(f, s);
        point {
            x: r.width,
            y: r.height,
        }
    }
}

/// Get the width of a string with the given font.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strwidth(f: font, s: *const std::os::raw::c_char) -> c_int {
    unsafe { strrect(f, s).width }
}

/// Get a pixel colour.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getpixel(p: point) -> rgb {
    // TODO: Platform-specific
    Black
}

/// Set a pixel colour.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setpixel(p: point, c: rgb) {
    // TODO: Platform-specific
}

/// Bit-block transfer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bitblt(db: bitmap, sb: bitmap, p: point, r: rect, mode: c_int) {
    // TODO: Platform-specific
}

/// Scroll a rectangle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scrollrect(dp: point, r: rect) {
    // TODO: Platform-specific
}

/// Copy a rectangle from source to current destination.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn copyrect(sb: bitmap, p: point, r: rect) {
    // TODO: Platform-specific
}

/// Texture-fill a rectangle with a bitmap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn texturerect(sb: bitmap, dr: rect) {
    // TODO: Platform-specific
}

/// Invert a rectangle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn invertrect(r: rect) {
    // TODO: Platform-specific
}

/// Draw an image.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawimage(img: image, dr: rect, sr: rect) {
    // TODO: Platform-specific
}

/// Draw an image in monochrome.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawmonochrome(img: image, dr: rect, sr: rect) {
    // TODO: Platform-specific
}

/// Draw an image in greyscale.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawgreyscale(img: image, dr: rect, sr: rect) {
    // TODO: Platform-specific
}

/// Draw an image darker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawdarker(img: image, dr: rect, sr: rect) {
    // TODO: Platform-specific
}

/// Draw an image brighter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawbrighter(img: image, dr: rect, sr: rect) {
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
pub unsafe extern "C" fn setcliprect(r: rect) {
    // TODO: Platform-specific
}

/// Copy the current draw state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn copydrawstate() -> drawstate {
    unsafe {
        let ds = super::memory::memalloc(std::mem::size_of::<drawstruct>() as i64) as drawstate;
        if !ds.is_null() {
            *ds = (*std::ptr::addr_of!(CURRENT_DRAWSTATE)).clone();
        }
        ds
    }
}

/// Set the draw state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setdrawstate(saved: drawstate) {
    unsafe {
        if !saved.is_null() {
            CURRENT_DRAWSTATE = (*saved).clone();
        }
    }
}

/// Restore a draw state (alias for setdrawstate).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn restoredrawstate(saved: drawstate) {
    unsafe {
        setdrawstate(saved);
    }
}

/// Reset the draw state to defaults.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn resetdrawstate() {
    unsafe {
        CURRENT_DRAWSTATE.dest = ptr::null_mut();
        CURRENT_DRAWSTATE.hue = Black;
        CURRENT_DRAWSTATE.mode = GA_S;
        CURRENT_DRAWSTATE.p = point { x: 0, y: 0 };
        CURRENT_DRAWSTATE.linewidth = 1;
        CURRENT_DRAWSTATE.fnt = ptr::null_mut();
        CURRENT_DRAWSTATE.crsr = ptr::null_mut();
    }
}

/// Set the draw destination.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawto(dest: drawing) {
    unsafe {
        CURRENT_DRAWSTATE.dest = dest;
    }
}

/// Add a control to the current window.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn addto(dest: control) {
    // TODO: Platform-specific
}

/// Set the current cursor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setcursor(c: cursor) {
    unsafe {
        CURRENT_DRAWSTATE.crsr = c;
    }
}

/// Set the current font.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setfont(f: font) {
    unsafe {
        CURRENT_DRAWSTATE.fnt = f;
    }
}

/// Set the caret.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setcaret(c: control, x: c_int, y: c_int, width: c_int, height: c_int) {
    // TODO: Platform-specific
}

/// Show/hide the caret.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn showcaret(c: control, showing: c_int) {
    // TODO: Platform-specific
}
