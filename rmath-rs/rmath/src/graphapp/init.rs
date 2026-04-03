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
use super::types::*;

static mut APP_INITIALISED: c_int = 0;
static mut APP_NAME: *mut c_char = ptr::null_mut();

pub unsafe fn get_app_name() -> *mut c_char {
    unsafe { APP_NAME }
}

pub unsafe fn get_app_initialised() -> c_int {
    unsafe { APP_INITIALISED }
}

pub unsafe fn set_app_initialised(val: c_int) {
    unsafe {
        APP_INITIALISED = val;
    }
}

/// Initialise the GraphApp library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn initapp(argc: c_int, argv: *mut *mut c_char) -> c_int {
    unsafe {
        if APP_INITIALISED == 0 {
            APP_INITIALISED = 1;
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

/// Clean up the GraphApp library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn app_cleanup() {
    unsafe {
        if APP_INITIALISED != 0 {
            APP_INITIALISED = 0;
            context::finish_contexts();
            objects::finish_objects();
            events::finish_events();
        }
    }
}

/// Exit the application.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn exitapp() {
    unsafe {
        app_cleanup();
        std::process::exit(0);
    }
}

/// Play an error sound.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gabeep() {
    // TODO: Platform-specific
}

/// Main loop entry point.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gamainloop() {
    // TODO: Platform-specific event loop
}

/// Start graphapp (for Windows entry point).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn startgraphapp(
    instance: *mut c_void,
    _prev_instance: *mut c_void,
    cmd_show: c_int,
) {
    unsafe {
        // TODO: Platform-specific
        initapp(0, ptr::null_mut());
    }
}

/// Check if topmost.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn isTopmost(w: window) -> c_int {
    0
}

/// Bring window to top.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn BringToTop(w: window, stay: c_int) { /* TODO */
}

/// Get window handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getHandle(w: window) -> *mut c_void {
    unsafe {
        if w.is_null() {
            ptr::null_mut()
        } else {
            (*w).handle
        }
    }
}

/// Send a message window.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn GA_msgWindow(c: window, typ: c_int) { /* TODO */
}

/// Topmost dialogs flag.
pub static mut TopmostDialogs: c_int = 0;

/// MDI-related globals
pub static mut MDIFrame: object = ptr::null_mut();
pub static mut MDIToolbar: object = ptr::null_mut();
pub static mut MDIStatus: *mut c_void = ptr::null_mut();
pub static mut hwndMain: *mut c_void = ptr::null_mut();
pub static mut hwndFrame: *mut c_void = ptr::null_mut();
pub static mut hwndClient: *mut c_void = ptr::null_mut();

pub static mut this_instance: *mut c_void = ptr::null_mut();
pub static mut prev_instance: *mut c_void = ptr::null_mut();
pub static mut menus_active: c_int = 1;
pub static mut localeCP: std::os::raw::c_uint = 0;
pub static mut is_NT: c_int = 1;
