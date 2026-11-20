#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

//! Button-related functions for GraphApp.

use super::controls;
use super::types::*;
use super::windows;

pub fn clickbutton(w: window, b: button) {
    if !w.is_null() {
        windows::set_current_window(w);
    }

    controls::activatecontrol(b);
}
