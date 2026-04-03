#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Menu management for GraphApp.

use std::os::raw::c_int;
use std::ptr;

use super::types::*;

pub unsafe fn init_menus() { /* TODO */
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newmdimenu() -> menu {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newpopup(_fn_: actionfn) -> menu {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gmenubar(_fn_: actionfn, _items: *mut MenuItem) -> menubar {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpopup(_fn_: actionfn, _items: *mut MenuItem) -> menu {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gchangepopup(_w: window, _p: menu) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gchangemenubar(_mb: menubar) { /* TODO */
}

pub unsafe fn adjust_menu(_wparam: usize) { /* TODO */
}
pub unsafe fn handle_menu_id(_wparam: usize) { /* TODO */
}
pub unsafe fn handle_menu_key(_wparam: usize) -> c_int {
    0
}
