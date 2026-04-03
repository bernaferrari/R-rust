#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Event handling for GraphApp.
//!
//! Ported from events.c - winprocs, timers, and event dispatch.

use std::os::raw::{c_int, c_long, c_uint, c_void};

use super::types::*;

static mut KEYSTATE: c_int = 0;

pub unsafe fn init_events() { /* TODO */
}
pub unsafe fn finish_events() { /* TODO */
}
pub unsafe fn handle_control(_hwnd: *mut c_void, _message: c_uint) { /* TODO */
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getkeystate() -> c_int {
    unsafe { KEYSTATE }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawall() { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn peekevent() -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn waitevent() { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn doevent() -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mainloop() { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn execapp(_cmd: *mut std::os::raw::c_char) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn settimer(_millisec: c_uint) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn settimerfn(_timeout: timerfn, _data: *mut c_void) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setmousetimer(_millisec: c_uint) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn delay(_millisec: c_uint) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn currenttime() -> c_long {
    0
}

pub unsafe fn toolbar_show() { /* TODO */
}
pub unsafe fn toolbar_hide() { /* TODO */
}
