#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Font management for GraphApp.
//!
//! Ported from fonts.c - font creation and measurement.

use std::os::raw::c_int;
use std::ptr;

use super::objects;
use super::runtime::with_graphapp_runtime;
use super::strings;
use super::types::*;

unsafe fn alloc_font(name: &str, style: c_int, size: c_int) -> font {
    let obj = unsafe { objects::new_object(FontObject, ptr::null_mut(), ptr::null_mut()) };
    if obj.is_null() {
        return ptr::null_mut();
    }

    let mut bytes = Vec::with_capacity(name.len() + 1);
    bytes.extend_from_slice(name.as_bytes());
    bytes.push(0);

    unsafe {
        (*obj).text = strings::new_string(bytes.as_ptr() as *const libc::c_char);
        (*obj).state = style as _;
        (*obj).value = size;
        (*obj).size = size;
        (*obj).xsize = (size / 2).max(1);
        (*obj).max = (size * 4) / 5;
    }
    obj
}

macro_rules! ensure_font {
    ($field:ident, $name:expr, $style:expr, $size:expr) => {
        if with_graphapp_runtime(|runtime| runtime.fonts.$field.is_null()) {
            let font = unsafe { alloc_font($name, $style, $size) };
            with_graphapp_runtime(|runtime| {
                if runtime.fonts.$field.is_null() {
                    runtime.fonts.$field = font;
                }
            });
        }
    };
}

pub fn init_fonts() {
    ensure_font!(fixed, "Fixed", FixedWidth, 10);
    ensure_font!(system, "System", Plain, 10);
    ensure_font!(times, "Times", Plain, 12);
    ensure_font!(helvetica, "Helvetica", SansSerif, 12);
    ensure_font!(courier, "Courier", FixedWidth, 10);
}

pub fn getSysFontSize() -> c_int {
    10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_fonts_populates_default_handles() {
        unsafe {
            objects::init_objects();
        }
        init_fonts();
        with_graphapp_runtime(|runtime| {
            assert!(!runtime.fonts.system.is_null());
            assert!(!runtime.fonts.fixed.is_null());
        });
    }
}
