#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Event handling for GraphApp.
//!
//! Ported from events.c - winprocs, timers, and event dispatch.

use std::cell::Cell;
use std::os::raw::{c_int, c_long, c_uint, c_void};

use super::types::*;

thread_local! { static KEYSTATE: Cell<c_int> = Cell::new(0); }

pub unsafe fn init_events() { /* TODO */
}
pub unsafe fn finish_events() { /* TODO */
}
pub unsafe fn handle_control(_hwnd: *mut c_void, _message: c_uint) { /* TODO */
}

pub unsafe fn getkeystate() -> c_int {
    KEYSTATE.with(|v| v.get())
}

pub unsafe fn drawall() { /* TODO */
}
pub unsafe fn peekevent() -> c_int {
    0
}
pub unsafe fn waitevent() { /* TODO */
}
pub unsafe fn doevent() -> c_int {
    0
}
pub unsafe fn mainloop() { /* TODO */
}
pub unsafe fn execapp(_cmd: *mut std::os::raw::c_char) -> c_int {
    0
}
pub unsafe fn settimer(_millisec: c_uint) -> c_int {
    0
}
pub unsafe fn settimerfn(_timeout: timerfn, _data: *mut c_void) { /* TODO */
}
pub unsafe fn setmousetimer(_millisec: c_uint) -> c_int {
    0
}
pub unsafe fn delay(_millisec: c_uint) { /* TODO */
}
pub unsafe fn currenttime() -> c_long {
    0
}

pub unsafe fn toolbar_show() { /* TODO */
}
pub unsafe fn toolbar_hide() { /* TODO */
}
