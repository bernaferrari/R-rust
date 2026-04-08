#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Status bar functions for GraphApp.

use std::os::raw::c_int;

pub unsafe fn addstatusbar() -> c_int {
    0
}
pub unsafe fn delstatusbar() -> c_int {
    0
}
pub unsafe fn setstatus(_text: *const std::os::raw::c_char) { /* TODO */
}

pub unsafe fn updatestatus(_text: *const std::os::raw::c_char) { /* TODO */
}
