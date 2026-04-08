#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Window management for GraphApp.
//!
//! Ported from windows.c - manipulating on-screen windows.

use std::cell::Cell;
use std::os::raw::{c_char, c_int, c_long};
use std::ptr;

use super::types::*;

thread_local! { static CURRENT_WINDOW: Cell<window> = Cell::new(ptr::null_mut()); }
thread_local! { static ACTIVE_WINDOWS: Cell<c_int> = Cell::new(0); }

pub fn get_current_window() -> window {
    CURRENT_WINDOW.with(|v| v.get())
}

pub fn set_current_window(w: window) {
    CURRENT_WINDOW.with(|v| v.set(w));
}

pub fn get_active_windows() -> c_int {
    ACTIVE_WINDOWS.with(|v| v.get())
}

pub fn set_active_windows(n: c_int) {
    ACTIVE_WINDOWS.with(|v| v.set(n));
}

pub fn decrement_active_windows() {
    ACTIVE_WINDOWS.with(|v| v.set(v.get() - 1));
}

pub unsafe fn newwindow(_name: *const c_char, _r: rect, _flags: c_long) -> window {
    unsafe {
        super::init::initapp(0, ptr::null_mut());
        ptr::null_mut() // TODO: Platform-specific
    }
}

pub unsafe fn show(w: window) {
    unsafe {
        if !w.is_null() {
            (*w).state |= GA_Visible;
        }
    }
}

pub unsafe fn hide(w: window) {
    unsafe {
        if !w.is_null() {
            (*w).state &= !GA_Visible;
        }
    }
}

pub unsafe fn ismdi() -> c_int {
    0
}

pub unsafe fn isUnicodeWindow(obj: object) -> c_int {
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

pub unsafe fn isiconic(_w: window) -> c_int {
    0
}

pub unsafe fn GetCurrentWinPos(obj: object) -> rect {
    unsafe {
        if obj.is_null() {
            rect::default()
        } else {
            (*obj).rect
        }
    }
}

pub unsafe fn show_window(_obj: object) { /* TODO */
}
pub unsafe fn hide_window(_obj: object) { /* TODO */
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
