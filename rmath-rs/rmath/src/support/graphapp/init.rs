#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Application initialization for GraphApp.
//!
//! Ported from init.c - library initialisation code.

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

/// Initialise the GraphApp library.
pub unsafe fn initapp(argc: c_int, _argv: *mut *mut c_char) -> c_int {
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

/// Clean up the GraphApp library.
pub unsafe fn app_cleanup() {
    if get_app_initialised() != 0 {
        set_app_initialised(0);
        context::finish_contexts();
        objects::finish_objects();
        events::finish_events();
    }
}

/// Exit the application.
pub unsafe fn exitapp() {
    app_cleanup();
    std::process::exit(0);
}

/// Play an error sound.
pub unsafe fn gabeep() {
    eprint!("\x07");
}

/// Main loop entry point.
pub unsafe fn gamainloop() {
    unsafe {
        events::mainloop();
    }
}

/// Start graphapp (for Windows entry point).
pub unsafe fn startgraphapp(
    instance: *mut c_void,
    previous_instance: *mut c_void,
    _cmd_show: c_int,
) {
    with_graphapp_runtime(|runtime| {
        runtime.app.this_instance = instance;
        runtime.app.prev_instance = previous_instance;
    });
    initapp(0, ptr::null_mut());
}

/// Check if topmost.
pub unsafe fn isTopmost(_w: window) -> c_int {
    0
}

/// Bring window to top.
pub unsafe fn BringToTop(w: window, stay: c_int) {
    if !w.is_null() {
        super::windows::set_current_window(w);
    }
    with_graphapp_runtime(|runtime| runtime.app.topmost_dialogs = i32::from(stay != 0));
}

/// Get window handle.
pub unsafe fn getHandle(w: window) -> *mut c_void {
    if w.is_null() {
        ptr::null_mut()
    } else {
        (*w).handle
    }
}

/// Send a message window.
pub unsafe fn GA_msgWindow(c: window, typ: c_int) {
    if typ != 0 && !c.is_null() {
        unsafe {
            BringToTop(c, typ);
        }
    }
}
