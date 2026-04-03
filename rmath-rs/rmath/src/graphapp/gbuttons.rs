#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Extended button/scrollbar functions for GraphApp.
//!
//! Ported from gbuttons.c.

use std::os::raw::c_int;
use std::ptr;

use super::types::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gchangescrollbar(
    sb: scrollbar,
    which: c_int,
    where_: c_int,
    max: c_int,
    pagesize: c_int,
    disablenoscroll: c_int,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsetcursor(d: drawing, c: cursor) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newtoolbar(height: c_int) -> control {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newtoolbutton(img: image, r: rect, fn_: actionfn) -> button {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scrolltext(c: textbox, lines: c_int) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ggetkeystate() -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scrollcaret(c: textbox, lines: c_int) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsetmodified(c: textbox, modified: c_int) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ggetmodified(c: textbox) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getlinelength(c: textbox) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getcurrentline(
    c: textbox,
    line: *mut std::os::raw::c_char,
    length: c_int,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getseltext(c: textbox, text: *mut std::os::raw::c_char) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setlimittext(t: textbox, limit: std::os::raw::c_long) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getlimittext(t: textbox) -> std::os::raw::c_long {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn checklimittext(t: textbox, n: std::os::raw::c_long) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getpastelength() -> std::os::raw::c_long {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn textselectionex(
    obj: control,
    start: *mut std::os::raw::c_long,
    end: *mut std::os::raw::c_long,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn selecttextex(
    obj: control,
    start: std::os::raw::c_long,
    end: std::os::raw::c_long,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn finddialog(t: textbox) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn replacedialog(t: textbox) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn modeless_active() -> c_int {
    0
}
