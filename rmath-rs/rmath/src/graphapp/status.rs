#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Status bar functions for GraphApp.

use super::types::*;
use std::os::raw::c_int;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn addstatusbar() -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn delstatusbar() -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setstatus(text: *const std::os::raw::c_char) { /* TODO */
}

pub unsafe fn updatestatus(text: *const std::os::raw::c_char) { /* TODO */
}
