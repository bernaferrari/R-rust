#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

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

pub unsafe fn get_app_name() -> *mut c_char {
    with_graphapp_runtime(|runtime| runtime.app.name)
}

pub fn get_app_initialised() -> c_int {
    with_graphapp_runtime(|runtime| runtime.app.initialised)
}

pub fn set_app_initialised(val: c_int) {
    with_graphapp_runtime(|runtime| runtime.app.initialised = val);
}

pub unsafe fn initapp(argc: c_int, _argv: *mut *mut c_char) -> c_int {
    unsafe {
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
}

pub unsafe fn app_cleanup() {
    unsafe {
        if get_app_initialised() != 0 {
            set_app_initialised(0);
            context::finish_contexts();
            objects::finish_objects();
            events::finish_events();
        }
    }
}

pub unsafe fn exitapp() {
    unsafe {
        app_cleanup();
        std::process::exit(0);
    }
}

pub fn gabeep() {
    eprint!("\x07");
}

pub fn gamainloop() {
    events::mainloop();
}

pub unsafe fn startgraphapp(
    instance: *mut c_void,
    previous_instance: *mut c_void,
    _cmd_show: c_int,
) {
    with_graphapp_runtime(|runtime| {
        runtime.app.this_instance = instance;
        runtime.app.prev_instance = previous_instance;
    });
    unsafe {
        initapp(0, ptr::null_mut());
    }
}

pub unsafe fn isTopmost(_w: window) -> c_int {
    0
}

pub unsafe fn BringToTop(w: window, stay: c_int) {
    if !w.is_null() {
        super::windows::set_current_window(w);
    }
    with_graphapp_runtime(|runtime| runtime.app.topmost_dialogs = i32::from(stay != 0));
}

pub unsafe fn getHandle(w: window) -> *mut c_void {
    unsafe {
        if w.is_null() {
            ptr::null_mut()
        } else {
            (*w).handle
        }
    }
}

pub unsafe fn GA_msgWindow(c: window, typ: c_int) {
    if typ != 0 && !c.is_null() {
        unsafe {
            BringToTop(c, typ);
        }
    }
}
