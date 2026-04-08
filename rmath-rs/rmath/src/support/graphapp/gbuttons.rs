#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Extended button/scrollbar functions for GraphApp.
//!
//! Ported from gbuttons.c.

use std::os::raw::c_int;
use std::ptr;

use super::types::*;

pub unsafe fn gchangescrollbar(
    _sb: scrollbar,
    _which: c_int,
    _where_: c_int,
    _max: c_int,
    _pagesize: c_int,
    _disablenoscroll: c_int,
) { /* TODO */
}
pub unsafe fn gsetcursor(_d: drawing, _c: cursor) { /* TODO */
}
pub unsafe fn newtoolbar(_height: c_int) -> control {
    ptr::null_mut()
}
pub unsafe fn newtoolbutton(_img: image, _r: rect, _fn_: actionfn) -> button {
    ptr::null_mut()
}
pub unsafe fn scrolltext(_c: textbox, _lines: c_int) { /* TODO */
}
pub unsafe fn ggetkeystate() -> c_int {
    0
}
pub unsafe fn scrollcaret(_c: textbox, _lines: c_int) { /* TODO */
}
pub unsafe fn gsetmodified(_c: textbox, _modified: c_int) { /* TODO */
}
pub unsafe fn ggetmodified(_c: textbox) -> c_int {
    0
}
pub unsafe fn getlinelength(_c: textbox) -> c_int {
    0
}
pub unsafe fn getcurrentline(
    _c: textbox,
    _line: *mut std::os::raw::c_char,
    _length: c_int,
) { /* TODO */
}
pub unsafe fn getseltext(_c: textbox, _text: *mut std::os::raw::c_char) {
    /* TODO */
}
pub unsafe fn setlimittext(_t: textbox, _limit: std::os::raw::c_long) {
    /* TODO */
}
pub unsafe fn getlimittext(_t: textbox) -> std::os::raw::c_long {
    0
}
pub unsafe fn checklimittext(_t: textbox, _n: std::os::raw::c_long) {
    /* TODO */
}
pub unsafe fn getpastelength() -> std::os::raw::c_long {
    0
}
pub unsafe fn textselectionex(
    _obj: control,
    _start: *mut std::os::raw::c_long,
    _end: *mut std::os::raw::c_long,
) { /* TODO */
}
pub unsafe fn selecttextex(
    _obj: control,
    _start: std::os::raw::c_long,
    _end: std::os::raw::c_long,
) { /* TODO */
}
pub unsafe fn finddialog(_t: textbox) { /* TODO */
}
pub unsafe fn replacedialog(_t: textbox) { /* TODO */
}
pub unsafe fn modeless_active() -> c_int {
    0
}
