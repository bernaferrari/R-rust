
//! X11 data entry widget (dataentry.c)
//!
//! Port of R's X11 spreadsheet-style data editor / data viewer.
//! This file is loaded as a separate module by the X11 dynamic loader
//! (R_deRoutines).
//!
//! All functions are stubs returning safe defaults since we do not
//! link against X11/Xlib/Xt.

use crate::sexp::ffi::SEXP;
use core::ffi::{c_char, c_int, c_void};

// ── Notes ─────────────────────────────────────────────────────────────
//
// The primary entry points (in_RX11_dataentry, in_R_X11_dataviewer)
// are exported from dev_x11.rs to avoid duplicate #[unsafe(no_mangle)]
// symbols.  This file provides only module-private helpers that would
// be needed if real X11 support were added.

// ── Module-private stubs ──────────────────────────────────────────────

/// closewin – close the data entry window.
/// Module-private stub.
unsafe fn closewin(_de: *mut c_void) {
    // no-op stub
}

/// popupmenu – show the context / popup menu in the data entry widget.
/// Module-private stub.
unsafe fn popupmenu(_de: *mut c_void, _x_pos: c_int, _y_pos: c_int, _col: c_int, _row: c_int) {
    // no-op stub
}

/// popdownmenu – dismiss the popup menu.
/// Module-private stub.
unsafe fn popdownmenu(_de: *mut c_void) {
    // no-op stub
}

/// R_ProcessX11Events – process pending X11 events (event loop).
/// Module-private stub.
unsafe fn R_ProcessX11Events(_data: *mut c_void) {
    // no-op stub
}

/// advancerect – advance the cell selection rectangle in a given direction.
unsafe fn advancerect(_de: *mut c_void, _dir: c_int) {
    // no-op stub
}

/// findcell – find the currently selected cell.
unsafe fn findcell(_de: *mut c_void) -> c_int {
    0
}

/// drawwindow – redraw the entire data entry window.
unsafe fn drawwindow(_de: *mut c_void) {
    // no-op stub
}

/// drawcol – redraw a single column in the data entry window.
unsafe fn drawcol(_de: *mut c_void, _col: c_int) {
    // no-op stub
}

/// drawrow – redraw a single row in the data entry window.
unsafe fn drawrow(_de: *mut c_void, _row: c_int) {
    // no-op stub
}

/// eventloop – main X11 event loop for the data entry widget.
unsafe fn eventloop(_de: *mut c_void) {
    // no-op stub
}

/// handlechar – process a character input event.
unsafe fn handlechar(_de: *mut c_void, _buf: *mut c_char) {
    // no-op stub
}

/// highlightrect – highlight the current cell rectangle.
unsafe fn highlightrect(_de: *mut c_void) {
    // no-op stub
}

/// downlightrect – remove highlight from the current cell rectangle.
unsafe fn downlightrect(_de: *mut c_void) {
    // no-op stub
}

/// clearrect – clear the current cell rectangle.
unsafe fn clearrect(_de: *mut c_void) {
    // no-op stub
}

/// printstring – draw a string at a given cell position.
unsafe fn printstring(
    _de: *mut c_void,
    _text: *const c_char,
    _row: c_int,
    _col: c_int,
    _col0: c_int,
    _is_row_name: c_int,
) {
    // no-op stub
}

/// initwin – initialise the data entry X11 window.
/// Returns FALSE (failure).
unsafe fn initwin(_de: *mut c_void, _title: *const c_char) -> c_int {
    0
}

/// copycell – copy current cell contents to clipboard buffer.
unsafe fn copycell(_de: *mut c_void) {
    // no-op stub
}

/// pastecell – paste clipboard buffer into a cell.
unsafe fn pastecell(_de: *mut c_void, _row: c_int, _col: c_int) {
    // no-op stub
}
