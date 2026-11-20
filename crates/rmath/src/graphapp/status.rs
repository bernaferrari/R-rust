#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

//! Status bar functions for GraphApp.

use std::os::raw::{c_char, c_int};

use super::types::{cstr, obj_mut};
use super::windows;

fn write_status_bytes(text: *const c_char) {
    let window = windows::get_current_window();
    let Some(data) = obj_mut(window) else {
        return;
    };
    data.status.fill(0);
    let Some(text) = cstr(text) else {
        return;
    };
    for (idx, &byte) in text
        .to_bytes()
        .iter()
        .take(data.status.len().saturating_sub(1))
        .enumerate()
    {
        data.status[idx] = byte as c_char;
    }
}

pub fn addstatusbar() -> c_int {
    0
}
pub fn delstatusbar() -> c_int {
    0
}
pub fn setstatus(text: *const c_char) {
    write_status_bytes(text);
}

pub fn updatestatus(text: *const c_char) {
    write_status_bytes(text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphapp::types::ObjInfo;
    use std::ffi::CString;
    use std::mem;
    use std::ptr;

    #[test]
    fn status_updates_current_window_buffer() {
        let _session = crate::sexp::session::RSession::new_for_gc_tests();
        unsafe {
            let mut window = Box::new(mem::zeroed::<ObjInfo>());
            let window_ptr = &mut *window as *mut ObjInfo;
            let old = windows::get_current_window();
            windows::set_current_window(window_ptr);

            let text = CString::new("ready").unwrap();
            setstatus(text.as_ptr());

            assert_eq!(window.status[0], b'r' as i8);
            assert_eq!(window.status[4], b'y' as i8);
            assert_eq!(window.status[5], 0);

            windows::set_current_window(old);
            if old.is_null() {
                windows::set_current_window(ptr::null_mut());
            }
        }
    }
}
