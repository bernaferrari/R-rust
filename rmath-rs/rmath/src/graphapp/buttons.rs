#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Button-related functions for GraphApp.

use super::types::*;
use std::os::raw::c_int;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clickbutton(w: window, b: button) { /* TODO */
}
