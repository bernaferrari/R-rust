#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Printer support for GraphApp.

use std::ptr;

use super::objects;
use super::types::*;

pub unsafe fn newprinter(
    w: f64,
    h: f64,
    name: *const std::os::raw::c_char,
) -> printer {
    let printer = unsafe { objects::new_object(PrinterObject, ptr::null_mut(), ptr::null_mut()) };
    if printer.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*printer).rect.width = w.round() as i32;
        (*printer).rect.height = h.round() as i32;
        (*printer).text = super::strings::new_string(name);
    }
    printer
}
pub unsafe fn nextpage(p: printer) {
    unsafe {
        if !p.is_null() {
            (*p).value += 1;
        }
    }
}
