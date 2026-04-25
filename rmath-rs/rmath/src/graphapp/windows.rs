#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Window management for GraphApp.
//!
//! Ported from windows.c - manipulating on-screen windows.

use std::cell::Cell;
use std::os::raw::{c_char, c_int, c_long};
use std::ptr;

use super::types::*;
use super::{memory, objects, strings};

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

unsafe fn alloc_drawstate(dest: drawing) -> drawstate {
    unsafe {
        let state = memory::memalloc(std::mem::size_of::<drawstruct>() as i64) as drawstate;
        if state.is_null() {
            return ptr::null_mut();
        }
        ptr::write_bytes(state as *mut u8, 0, std::mem::size_of::<drawstruct>());
        (*state).dest = dest;
        (*state).hue = Black;
        state
    }
}

pub unsafe fn newwindow(name: *const c_char, r: rect, flags: c_long) -> window {
    unsafe {
        super::init::initapp(0, ptr::null_mut());
        objects::init_objects();

        let window = objects::new_object(WindowObject, ptr::null_mut(), ptr::null_mut());
        if window.is_null() {
            return ptr::null_mut();
        }

        let drawstate = alloc_drawstate(window);
        if drawstate.is_null() {
            objects::delobj(window);
            objects::deletion_traversal();
            return ptr::null_mut();
        }

        (*window).rect = r;
        (*window).flags = flags;
        (*window).state = GA_Enabled;
        (*window).fg = Black;
        (*window).bg = White;
        (*window).text = strings::new_string(name);
        (*window).drawstate = drawstate;

        set_active_windows(get_active_windows() + 1);
        window
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

pub fn ismdi() -> c_int {
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

pub fn isiconic(_w: window) -> c_int {
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

pub unsafe fn show_window(obj: object) {
    unsafe {
        show(obj);
        if !obj.is_null() {
            set_current_window(obj);
        }
    }
}
pub unsafe fn hide_window(obj: object) {
    unsafe {
        hide(obj);
        if get_current_window() == obj {
            set_current_window(ptr::null_mut());
        }
    }
}
pub fn simple_window() -> window {
    unsafe { newwindow(ptr::null(), rect::default(), SimpleWindow as c_long) }
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
