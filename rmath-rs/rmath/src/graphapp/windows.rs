#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Window management for GraphApp.
//!
//! Ported from windows.c - manipulating on-screen windows.

use std::os::raw::{c_char, c_int, c_long};
use std::ptr;

use super::types::*;

static mut CURRENT_WINDOW: window = ptr::null_mut();
static mut ACTIVE_WINDOWS: c_int = 0;

pub unsafe fn get_current_window() -> window {
    unsafe { CURRENT_WINDOW }
}

pub unsafe fn set_current_window(w: window) {
    unsafe {
        CURRENT_WINDOW = w;
    }
}

pub unsafe fn get_active_windows() -> c_int {
    unsafe { ACTIVE_WINDOWS }
}

pub unsafe fn set_active_windows(n: c_int) {
    unsafe {
        ACTIVE_WINDOWS = n;
    }
}

pub unsafe fn decrement_active_windows() {
    unsafe {
        ACTIVE_WINDOWS -= 1;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newwindow(name: *const c_char, r: rect, flags: c_long) -> window {
    unsafe {
        super::init::initapp(0, ptr::null_mut());
        ptr::null_mut() // TODO: Platform-specific
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn show(w: window) {
    unsafe {
        if !w.is_null() {
            (*w).state |= GA_Visible;
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn hide(w: window) {
    unsafe {
        if !w.is_null() {
            (*w).state &= !GA_Visible;
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ismdi() -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn isUnicodeWindow(obj: object) -> c_int {
    unsafe {
        if obj.is_null() {
            0
        } else {
            if ((*obj).flags & UseUnicode as c_long) != 0 {
                1
            } else {
                0
            }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn isiconic(w: window) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn GetCurrentWinPos(obj: object) -> rect {
    unsafe {
        if obj.is_null() {
            rect::default()
        } else {
            (*obj).rect
        }
    }
}

pub unsafe fn show_window(obj: object) { /* TODO */
}
pub unsafe fn hide_window(obj: object) { /* TODO */
}
pub unsafe fn simple_window() -> window {
    ptr::null_mut()
}
pub unsafe fn screen_coords(obj: object) -> rect {
    unsafe {
        if obj.is_null() {
            rect::default()
        } else {
            (*obj).rect
        }
    }
}
