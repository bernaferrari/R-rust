#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use super::context;
use super::cursors;
use super::events;
use super::fonts;
use super::menus;
use super::objects;
use super::runtime::with_graphapp_runtime;
use super::types::*;

pub fn get_app_name() -> *mut c_char {
    with_graphapp_runtime(|runtime| runtime.app.name)
}

pub fn get_app_initialised() -> c_int {
    with_graphapp_runtime(|runtime| runtime.app.initialised)
}

pub fn set_app_initialised(val: c_int) {
    with_graphapp_runtime(|runtime| runtime.app.initialised = val);
}

pub fn initapp(argc: c_int, _argv: *mut *mut c_char) -> c_int {
    if get_app_initialised() == 0 {
        set_app_initialised(1);
        objects::init_objects();
        events::init_events();
        fonts::init_fonts();
        cursors::init_cursors();
        context::init_contexts();
        menus::init_menus();
    }
    if argc < 1 { 1 } else { argc }
}

pub fn app_cleanup() {
    if get_app_initialised() != 0 {
        set_app_initialised(0);
        context::finish_contexts();
        objects::finish_objects();
        events::finish_events();
    }
}

pub fn exitapp() {
    app_cleanup();
    std::process::exit(0);
}

pub fn gabeep() {
    eprint!("\x07");
}

pub fn gamainloop() {
    events::mainloop();
}

pub fn startgraphapp(instance: *mut c_void, previous_instance: *mut c_void, _cmd_show: c_int) {
    with_graphapp_runtime(|runtime| {
        runtime.app.this_instance = instance;
        runtime.app.prev_instance = previous_instance;
    });

    initapp(0, ptr::null_mut());
}

pub fn isTopmost(_w: window) -> c_int {
    0
}

pub fn BringToTop(w: window, stay: c_int) {
    if !w.is_null() {
        super::windows::set_current_window(w);
    }
    with_graphapp_runtime(|runtime| runtime.app.topmost_dialogs = i32::from(stay != 0));
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getHandle(w: window) -> *mut c_void {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(w) }).map_or(ptr::null_mut(), |data| data.handle)
}

pub fn GA_msgWindow(c: window, typ: c_int) {
    if typ != 0 && !c.is_null() {
        BringToTop(c, typ);
    }
}
