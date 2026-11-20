#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

//! Extended button/scrollbar functions for GraphApp.
//!
//! Ported from gbuttons.c.

use std::cmp::{max, min};
use std::os::raw::{c_char, c_int, c_long};
use std::ptr;

use super::drawing as drawing_state;
use super::events;
use super::types::*;

fn text_len(text: *const c_char) -> usize {
    cstr(text).map_or(0, |s| s.to_bytes().len())
}

fn selection_bounds(obj: control, len: usize) -> (usize, usize) {
    let Some(data) = obj_ref(obj) else {
        return (0, 0);
    };
    let start = data.size.max(0) as usize;
    let end = data.xsize.max(0) as usize;
    (min(start, len), min(max(start, end), len))
}

pub fn gchangescrollbar(
    sb: scrollbar,
    _which: c_int,
    where_: c_int,
    max_value: c_int,
    pagesize: c_int,
    _disablenoscroll: c_int,
) {
    if let Some(data) = obj_mut(sb) {
        data.value = where_;
        data.max = max_value;
        data.size = pagesize;
    }
}

pub fn gsetcursor(d: drawing, c: cursor) {
    if d.is_null() {
        return;
    }
    drawing_state::setcursor(c);
}

pub fn newtoolbar(_height: c_int) -> control {
    ptr::null_mut()
}

pub fn newtoolbutton(_img: image, _r: rect, _fn_: actionfn) -> button {
    ptr::null_mut()
}

pub fn scrolltext(c: textbox, lines: c_int) {
    if let Some(data) = obj_mut(c) {
        data.carety = (data.carety + lines).max(0);
    }
}

pub fn ggetkeystate() -> c_int {
    events::getkeystate()
}

pub fn scrollcaret(c: textbox, lines: c_int) {
    if let Some(data) = obj_mut(c) {
        data.carety = (data.carety + lines).max(0);
    }
}

pub fn gsetmodified(c: textbox, modified: c_int) {
    if let Some(data) = obj_mut(c) {
        data.value = i32::from(modified != 0);
    }
}

pub fn ggetmodified(c: textbox) -> c_int {
    obj_ref(c).map_or(0, |data| i32::from(data.value != 0))
}

pub fn getlinelength(c: textbox) -> c_int {
    let Some(text) = obj_ref(c)
        .map(|data| data.text as *const c_char)
        .and_then(cstr)
    else {
        return 0;
    };
    let bytes = text.to_bytes();
    bytes
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(bytes.len()) as c_int
}

pub fn getcurrentline(c: textbox, line: *mut c_char, length: c_int) {
    if c.is_null() || line.is_null() || length <= 0 {
        return;
    }
    let source = obj_ref(c).map_or(ptr::null(), |data| data.text as *const c_char);
    let count = getlinelength(c).min(length - 1).max(0) as usize;
    // Copy `count` bytes into the caller's `length`-byte buffer and
    // NUL-terminate it.
    unsafe {
        if !source.is_null() && count > 0 {
            ptr::copy_nonoverlapping(source, line, count);
        }
        *line.add(count) = 0;
    }
}

pub fn getseltext(c: textbox, text: *mut c_char) {
    if c.is_null() || text.is_null() {
        return;
    }
    let source = obj_ref(c).map_or(ptr::null_mut(), |data| data.text);
    if source.is_null() {
        // NUL-terminate the caller's buffer.
        unsafe { *text = 0 };
        return;
    }

    let len = text_len(source);
    let (start, end) = selection_bounds(c, len);
    let count = end.saturating_sub(start);
    // Copy the selected range into the caller's buffer, sized for the
    // full text plus NUL, and terminate it.
    unsafe {
        if count > 0 {
            ptr::copy_nonoverlapping(source.add(start), text, count);
        }
        *text.add(count) = 0;
    }
}

pub fn setlimittext(t: textbox, limit: c_long) {
    let Some(data) = obj_mut(t) else {
        return;
    };
    data.max = limit.max(0) as c_int;
    checklimittext(t, limit);
}

pub fn getlimittext(t: textbox) -> c_long {
    obj_ref(t).map_or(0, |data| data.max as c_long)
}

pub fn checklimittext(t: textbox, n: c_long) {
    let Some(data) = obj_mut(t) else {
        return;
    };
    if data.text.is_null() {
        return;
    }

    let limit = if n > 0 {
        n as usize
    } else {
        data.max.max(0) as usize
    };
    if limit == 0 {
        return;
    }

    let text = data.text;
    if text_len(text) > limit {
        // Truncate the NUL-terminated text buffer at `limit` bytes.
        unsafe { *text.add(limit) = 0 };
    }
}

pub fn getpastelength() -> c_long {
    0
}

pub fn textselectionex(obj: control, start: *mut c_long, end: *mut c_long) {
    let Some(data) = obj_ref(obj) else {
        return;
    };
    // Write through the caller's out-pointers after null checks.
    unsafe {
        if !start.is_null() {
            *start = data.size as c_long;
        }
        if !end.is_null() {
            *end = data.xsize as c_long;
        }
    }
}

pub fn selecttextex(obj: control, start: c_long, end: c_long) {
    if let Some(data) = obj_mut(obj) {
        data.size = start.max(0) as c_int;
        data.xsize = end.max(start).max(0) as c_int;
    }
}

pub fn finddialog(_t: textbox) {}

pub fn replacedialog(_t: textbox) {}

pub fn modeless_active() -> c_int {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    use std::mem;

    #[test]
    fn textbox_helpers_track_selection_and_limit() {
        unsafe {
            let mut textbox = Box::new(mem::zeroed::<ObjInfo>());
            let mut text = *b"hello world\0";
            textbox.text = text.as_mut_ptr() as *mut core::ffi::c_char;
            textbox.max = 64;

            let textbox_ptr = &mut *textbox as textbox;
            selecttextex(textbox_ptr, 1, 5);
            gsetmodified(textbox_ptr, 1);

            let mut start = 0;
            let mut end = 0;
            textselectionex(textbox_ptr, &mut start, &mut end);
            assert_eq!((start, end), (1, 5));
            assert_eq!(ggetmodified(textbox_ptr), 1);

            let mut buf = [0i8; 16];
            getseltext(textbox_ptr, buf.as_mut_ptr());
            assert_eq!(CStr::from_ptr(buf.as_ptr()).to_str().unwrap(), "ello");

            setlimittext(textbox_ptr, 5);
            assert_eq!(getlimittext(textbox_ptr), 5);
            assert_eq!(getlinelength(textbox_ptr), 5);
            assert_eq!(CStr::from_ptr(textbox.text).to_str().unwrap(), "hello");
        }
    }
}
