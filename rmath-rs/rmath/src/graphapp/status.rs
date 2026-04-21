#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Status bar functions for GraphApp.

use std::os::raw::{c_char, c_int};

use super::windows;

unsafe fn write_status_bytes(text: *const c_char) {
    let window = windows::get_current_window();
    if window.is_null() {
        return;
    }

    unsafe {
        let status = &mut (*window).status;
        status.fill(0);
        if text.is_null() {
            return;
        }

        let mut i = 0usize;
        while i + 1 < status.len() && *text.add(i) != 0 {
            status[i] = *text.add(i);
            i += 1;
        }
    }
}

pub unsafe fn addstatusbar() -> c_int {
    0
}
pub unsafe fn delstatusbar() -> c_int {
    0
}
pub unsafe fn setstatus(text: *const c_char) {
    unsafe { write_status_bytes(text) };
}

pub unsafe fn updatestatus(text: *const c_char) {
    unsafe { write_status_bytes(text) };
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
