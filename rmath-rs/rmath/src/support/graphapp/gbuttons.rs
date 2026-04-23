#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Extended button/scrollbar functions for GraphApp.
//!
//! Ported from gbuttons.c.

use std::cmp::{max, min};
use std::os::raw::{c_char, c_int, c_long};
use std::ptr;

use super::drawing as drawing_state;
use super::events;
use super::types::*;

unsafe fn text_len(text: *const c_char) -> usize {
    let mut len = 0usize;
    unsafe {
        while !text.is_null() && *text.add(len) != 0 {
            len += 1;
        }
    }
    len
}

unsafe fn selection_bounds(obj: control, len: usize) -> (usize, usize) {
    unsafe {
        let start = (*obj).size.max(0) as usize;
        let end = (*obj).xsize.max(0) as usize;
        (min(start, len), min(max(start, end), len))
    }
}

pub unsafe fn gchangescrollbar(
    sb: scrollbar,
    _which: c_int,
    where_: c_int,
    max_value: c_int,
    pagesize: c_int,
    _disablenoscroll: c_int,
) {
    unsafe {
        if !sb.is_null() {
            (*sb).value = where_;
            (*sb).max = max_value;
            (*sb).size = pagesize;
        }
    }
}

pub unsafe fn gsetcursor(d: drawing, c: cursor) {
    if d.is_null() {
        return;
    }
    drawing_state::setcursor(c);
}

pub unsafe fn newtoolbar(_height: c_int) -> control {
    ptr::null_mut()
}

pub unsafe fn newtoolbutton(_img: image, _r: rect, _fn_: actionfn) -> button {
    ptr::null_mut()
}

pub unsafe fn scrolltext(c: textbox, lines: c_int) {
    unsafe {
        if !c.is_null() {
            (*c).carety = ((*c).carety + lines).max(0);
        }
    }
}

pub fn ggetkeystate() -> c_int {
    events::getkeystate()
}

pub unsafe fn scrollcaret(c: textbox, lines: c_int) {
    unsafe {
        if !c.is_null() {
            (*c).carety = ((*c).carety + lines).max(0);
        }
    }
}

pub unsafe fn gsetmodified(c: textbox, modified: c_int) {
    unsafe {
        if !c.is_null() {
            (*c).value = i32::from(modified != 0);
        }
    }
}

pub unsafe fn ggetmodified(c: textbox) -> c_int {
    unsafe { if c.is_null() { 0 } else { i32::from((*c).value != 0) } }
}

pub unsafe fn getlinelength(c: textbox) -> c_int {
    unsafe {
        if c.is_null() || (*c).text.is_null() {
            return 0;
        }

        let mut len = 0usize;
        while *(*c).text.add(len) != 0 && *(*c).text.add(len) != b'\n' as libc::c_char {
            len += 1;
        }
        len as c_int
    }
}

pub unsafe fn getcurrentline(c: textbox, line: *mut c_char, length: c_int) {
    unsafe {
        if c.is_null() || line.is_null() || length <= 0 {
            return;
        }

        let count = getlinelength(c).min(length - 1).max(0) as usize;
        if !(*c).text.is_null() && count > 0 {
            ptr::copy_nonoverlapping((*c).text, line, count);
        }
        *line.add(count) = 0;
    }
}

pub unsafe fn getseltext(c: textbox, text: *mut c_char) {
    unsafe {
        if c.is_null() || text.is_null() {
            return;
        }

        let source = (*c).text;
        if source.is_null() {
            *text = 0;
            return;
        }

        let len = text_len(source);
        let (start, end) = selection_bounds(c, len);
        let count = end.saturating_sub(start);
        if count > 0 {
            ptr::copy_nonoverlapping(source.add(start), text, count);
        }
        *text.add(count) = 0;
    }
}

pub unsafe fn setlimittext(t: textbox, limit: c_long) {
    unsafe {
        if !t.is_null() {
            (*t).max = limit.max(0) as c_int;
            checklimittext(t, limit);
        }
    }
}

pub unsafe fn getlimittext(t: textbox) -> c_long {
    unsafe { if t.is_null() { 0 } else { (*t).max as c_long } }
}

pub unsafe fn checklimittext(t: textbox, n: c_long) {
    unsafe {
        if t.is_null() || (*t).text.is_null() {
            return;
        }

        let limit = if n > 0 { n as usize } else { (*t).max.max(0) as usize };
        if limit == 0 {
            return;
        }

        if text_len((*t).text) > limit {
            *(*t).text.add(limit) = 0;
        }
    }
}

pub unsafe fn getpastelength() -> c_long {
    0
}

pub unsafe fn textselectionex(obj: control, start: *mut c_long, end: *mut c_long) {
    unsafe {
        if obj.is_null() {
            return;
        }
        if !start.is_null() {
            *start = (*obj).size as c_long;
        }
        if !end.is_null() {
            *end = (*obj).xsize as c_long;
        }
    }
}

pub unsafe fn selecttextex(obj: control, start: c_long, end: c_long) {
    unsafe {
        if !obj.is_null() {
            (*obj).size = start.max(0) as c_int;
            (*obj).xsize = end.max(start).max(0) as c_int;
        }
    }
}

pub unsafe fn finddialog(_t: textbox) {}

pub unsafe fn replacedialog(_t: textbox) {}

pub unsafe fn modeless_active() -> c_int {
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
            textbox.text = text.as_mut_ptr() as *mut libc::c_char;
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
