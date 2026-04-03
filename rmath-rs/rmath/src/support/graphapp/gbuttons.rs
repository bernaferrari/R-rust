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
    _sb: scrollbar,
    _which: c_int,
    _where_: c_int,
    _max: c_int,
    _pagesize: c_int,
    _disablenoscroll: c_int,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsetcursor(_d: drawing, _c: cursor) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newtoolbar(_height: c_int) -> control {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newtoolbutton(_img: image, _r: rect, _fn_: actionfn) -> button {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scrolltext(_c: textbox, _lines: c_int) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ggetkeystate() -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scrollcaret(_c: textbox, _lines: c_int) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gsetmodified(_c: textbox, _modified: c_int) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ggetmodified(_c: textbox) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getlinelength(_c: textbox) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getcurrentline(
    _c: textbox,
    _line: *mut std::os::raw::c_char,
    _length: c_int,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getseltext(_c: textbox, _text: *mut std::os::raw::c_char) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setlimittext(_t: textbox, _limit: std::os::raw::c_long) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getlimittext(_t: textbox) -> std::os::raw::c_long {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn checklimittext(_t: textbox, _n: std::os::raw::c_long) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getpastelength() -> std::os::raw::c_long {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn textselectionex(
    _obj: control,
    _start: *mut std::os::raw::c_long,
    _end: *mut std::os::raw::c_long,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn selecttextex(
    _obj: control,
    _start: std::os::raw::c_long,
    _end: std::os::raw::c_long,
) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn finddialog(_t: textbox) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn replacedialog(_t: textbox) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn modeless_active() -> c_int {
    0
}
