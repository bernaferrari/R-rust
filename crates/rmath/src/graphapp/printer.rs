#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

//! Printer support for GraphApp.

use std::ptr;

use super::objects;
use super::types::*;

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newprinter(w: f64, h: f64, name: *const std::os::raw::c_char) -> printer {
    // SAFETY: forwarded raw pointers satisfy `new_object`'s contract per this function's own `# Safety` requirements.
    let printer = unsafe { objects::new_object(PrinterObject, ptr::null_mut(), ptr::null_mut()) };
    if printer.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(data) = unsafe { obj_mut(printer) } {
        data.rect.width = w.round() as i32;
        data.rect.height = h.round() as i32;
        // SAFETY: forwarded raw pointers satisfy `new_string`'s contract per this function's own `# Safety` requirements.
        data.text = unsafe { super::strings::new_string(name) };
    }
    printer
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn nextpage(p: printer) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(data) = unsafe { obj_mut(p) } {
        data.value += 1;
    }
}
