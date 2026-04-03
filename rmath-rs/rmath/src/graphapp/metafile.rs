#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Metafile support for GraphApp.

use super::types::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newmetafile(
    name: *const std::os::raw::c_char,
    width: f64,
    height: f64,
    xpinch: f64,
    ypinch: f64,
) -> metafile {
    std::ptr::null_mut()
}
