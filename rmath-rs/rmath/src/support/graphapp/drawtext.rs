#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Text drawing functions for GraphApp.
//!
//! Ported from drawtext.c - word-wrapping text rendering.

use std::os::raw::c_int;
use std::ptr;

use super::types::*;

pub unsafe fn textheight(_width: c_int, _text: *const std::os::raw::c_char) -> c_int {
    0
}

pub unsafe fn drawtext(
    _r: rect,
    _alignment: c_int,
    text: *const std::os::raw::c_char,
) -> *const std::os::raw::c_char {
    if text.is_null() {
        return ptr::null();
    }
    text
}

pub unsafe fn gprintf(
    _fmt: *const std::os::raw::c_char,
    _args: *const std::os::raw::c_void,
) -> c_int {
    // Variadic functions not supported; stub
    0
}
