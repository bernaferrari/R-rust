#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Printer support for GraphApp.

use super::types::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newprinter(w: f64, h: f64, name: *const std::os::raw::c_char) -> printer {
    std::ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nextpage(p: printer) { /* TODO */
}
