#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

//! Window management for GraphApp.
//!
//! Ported from windows.c - manipulating on-screen windows.

use std::os::raw::{c_char, c_int, c_long};
use std::ptr;

use super::runtime::with_graphapp_runtime;
use super::types::*;
use super::{memory, objects, strings};

pub fn get_current_window() -> window {
    with_graphapp_runtime(|runtime| runtime.windows.current)
}

pub fn set_current_window(w: window) {
    with_graphapp_runtime(|runtime| runtime.windows.current = w);
}

pub fn get_active_windows() -> c_int {
    with_graphapp_runtime(|runtime| runtime.windows.active_count)
}

pub fn set_active_windows(n: c_int) {
    with_graphapp_runtime(|runtime| runtime.windows.active_count = n);
}

pub fn decrement_active_windows() {
    with_graphapp_runtime(|runtime| runtime.windows.active_count -= 1);
}

fn alloc_drawstate(dest: drawing) -> drawstate {
    let state = memory::memalloc_bytes(std::mem::size_of::<drawstruct>()) as drawstate;
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let Some(data) = (unsafe { ptr_mut(state) }) else {
        return ptr::null_mut();
    };
    *data = drawstruct {
        dest,
        hue: Black,
        ..Default::default()
    };
    state
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newwindow(name: *const c_char, r: rect, flags: c_long) -> window {
    super::init::initapp(0, ptr::null_mut());
    objects::init_objects();

    // SAFETY: forwarded raw pointers satisfy `new_object`'s contract per this function's own `# Safety` requirements.
    let window = unsafe { objects::new_object(WindowObject, ptr::null_mut(), ptr::null_mut()) };
    if window.is_null() {
        return ptr::null_mut();
    }

    let drawstate = alloc_drawstate(window);
    if drawstate.is_null() {
        // SAFETY: forwarded raw pointers satisfy `delobj`'s contract per this function's own `# Safety` requirements.
        unsafe { objects::delobj(window) };
        objects::deletion_traversal();
        return ptr::null_mut();
    }

    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(data) = unsafe { obj_mut(window) } {
        data.rect = r;
        data.flags = flags;
        data.state = GA_Enabled;
        data.fg = Black;
        data.bg = White;
        // SAFETY: forwarded raw pointers satisfy `new_string`'s contract per this function's own `# Safety` requirements.
        data.text = unsafe { strings::new_string(name) };
        data.drawstate = drawstate;
    }

    set_active_windows(get_active_windows() + 1);
    window
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn show(w: window) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(data) = unsafe { obj_mut(w) } {
        data.state |= GA_Visible;
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn hide(w: window) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(data) = unsafe { obj_mut(w) } {
        data.state &= !GA_Visible;
    }
}

pub fn ismdi() -> c_int {
    0
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn isUnicodeWindow(obj: object) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(obj) }).map_or(0, |data| {
        if (data.flags & UseUnicode as c_long) != 0 {
            1
        } else {
            0
        }
    })
}

pub fn isiconic(_w: window) -> c_int {
    0
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn GetCurrentWinPos(obj: object) -> rect {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(obj) }).map_or(rect::default(), |data| data.rect)
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn show_window(obj: object) {
    // SAFETY: forwarded raw pointers satisfy `show`'s contract per this function's own `# Safety` requirements.
    unsafe { show(obj) };
    if !obj.is_null() {
        set_current_window(obj);
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn hide_window(obj: object) {
    // SAFETY: forwarded raw pointers satisfy `hide`'s contract per this function's own `# Safety` requirements.
    unsafe { hide(obj) };
    if get_current_window() == obj {
        set_current_window(ptr::null_mut());
    }
}
pub fn simple_window() -> window {
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    unsafe { newwindow(ptr::null(), rect::default(), SimpleWindow as c_long) }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn screen_coords(obj: object) -> rect {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(obj) }).map_or(rect::default(), |data| data.rect)
}
