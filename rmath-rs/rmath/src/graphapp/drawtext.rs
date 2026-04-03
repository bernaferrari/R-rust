#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Text drawing functions for GraphApp.
//!
//! Ported from drawtext.c - word-wrapping text rendering.

use std::os::raw::c_int;
use std::ptr;

use super::types::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn textheight(width: c_int, text: *const std::os::raw::c_char) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn drawtext(
    r: rect,
    alignment: c_int,
    text: *const std::os::raw::c_char,
) -> *const std::os::raw::c_char {
    if text.is_null() {
        return ptr::null();
    }
    text
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gprintf(
    fmt: *const std::os::raw::c_char,
    _args: *const std::os::raw::c_void,
) -> c_int {
    // Variadic functions not supported; stub
    0
}
