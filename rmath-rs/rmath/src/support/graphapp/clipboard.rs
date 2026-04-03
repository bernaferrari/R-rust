#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Clipboard functions for GraphApp.

use super::types::*;
use std::os::raw::c_int;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn copytoclipboard(_src: drawing) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn copystringtoclipboard(_str: *const std::os::raw::c_char) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getstringfromclipboard(
    _str: *mut std::os::raw::c_char,
    _n: c_int,
) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn clipboardhastext() -> c_int {
    0
}
