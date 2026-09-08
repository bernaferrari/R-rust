#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Metafile support for GraphApp.

use super::types::*;

pub unsafe fn newmetafile(
    _name: *const std::os::raw::c_char,
    _width: f64,
    _height: f64,
    _xpinch: f64,
    _ypinch: f64,
) -> metafile {
    std::ptr::null_mut()
}
