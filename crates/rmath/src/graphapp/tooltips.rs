#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

//! Tooltip functions for GraphApp.

use super::types::*;
use std::os::raw::c_int;

pub fn addtooltip(_c: control, _tp: *const std::os::raw::c_char) -> c_int {
    0
}
