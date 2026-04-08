#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Printer support for GraphApp.

use super::types::*;

pub unsafe fn newprinter(_w: f64, _h: f64, _name: *const std::os::raw::c_char) -> printer {
    std::ptr::null_mut()
}
pub unsafe fn nextpage(_p: printer) { /* TODO */
}
