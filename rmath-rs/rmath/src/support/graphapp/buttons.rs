#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Button-related functions for GraphApp.

use super::controls;
use super::types::*;
use super::windows;

pub unsafe fn clickbutton(w: window, b: button) {
    if !w.is_null() {
        windows::set_current_window(w);
    }
    unsafe {
        controls::activatecontrol(b);
    }
}
