#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

//! Printer support for GraphApp.

use std::ptr;

use super::objects;
use super::types::*;

pub fn newprinter(w: f64, h: f64, name: *const std::os::raw::c_char) -> printer {
    let printer = objects::new_object(PrinterObject, ptr::null_mut(), ptr::null_mut());
    if printer.is_null() {
        return ptr::null_mut();
    }

    if let Some(data) = obj_mut(printer) {
        data.rect.width = w.round() as i32;
        data.rect.height = h.round() as i32;
        data.text = super::strings::new_string(name);
    }
    printer
}
pub fn nextpage(p: printer) {
    if let Some(data) = obj_mut(p) {
        data.value += 1;
    }
}
