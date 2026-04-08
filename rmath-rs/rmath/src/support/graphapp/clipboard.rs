#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Clipboard functions for GraphApp.

use super::types::*;
use std::os::raw::c_int;

pub unsafe fn copytoclipboard(_src: drawing) { /* TODO */
}
pub unsafe fn copystringtoclipboard(_str: *const std::os::raw::c_char) -> c_int {
    0
}
pub unsafe fn getstringfromclipboard(
    _str: *mut std::os::raw::c_char,
    _n: c_int,
) -> c_int {
    0
}
pub unsafe fn clipboardhastext() -> c_int {
    0
}
