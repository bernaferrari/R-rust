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

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn text_len(text: *const c_char) -> usize {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    (unsafe { cstr(text) }).map_or(0, |s| s.to_bytes().len())
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn selection_bounds(obj: control, len: usize) -> (usize, usize) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some(data) = (unsafe { obj_ref(obj) }) else {
        return (0, 0);
    };
    let start = data.size.max(0) as usize;
    let end = data.xsize.max(0) as usize;
    (min(start, len), min(max(start, end), len))
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn gchangescrollbar(
    sb: scrollbar,
    _which: c_int,
    where_: c_int,
    max_value: c_int,
    pagesize: c_int,
    _disablenoscroll: c_int,
) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(data) = unsafe { obj_mut(sb) } {
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

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn scrolltext(c: textbox, lines: c_int) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(data) = unsafe { obj_mut(c) } {
        data.carety = (data.carety + lines).max(0);
    }
}

pub fn ggetkeystate() -> c_int {
    events::getkeystate()
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn scrollcaret(c: textbox, lines: c_int) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(data) = unsafe { obj_mut(c) } {
        data.carety = (data.carety + lines).max(0);
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn gsetmodified(c: textbox, modified: c_int) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(data) = unsafe { obj_mut(c) } {
        data.value = i32::from(modified != 0);
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn ggetmodified(c: textbox) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(c) }).map_or(0, |data| i32::from(data.value != 0))
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getlinelength(c: textbox) -> c_int {
    // SAFETY: `c` is a live textbox per this function's contract, and its
    // `text` field is null or a NUL-terminated string owned by the object.
    let Some(text) = (unsafe { obj_ref(c) })
        .map(|data| data.text as *const c_char)
        .and_then(|text| unsafe { cstr(text) })
    else {
        return 0;
    };
    let bytes = text.to_bytes();
    bytes
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(bytes.len()) as c_int
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getcurrentline(c: textbox, line: *mut c_char, length: c_int) {
    if c.is_null() || line.is_null() || length <= 0 {
        return;
    }
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let source = unsafe { obj_ref(c) }.map_or(ptr::null(), |data| data.text as *const c_char);
    // SAFETY: forwarded raw pointers satisfy `getlinelength`'s contract per this function's own `# Safety` requirements.
    let count = unsafe { getlinelength(c) }.min(length - 1).max(0) as usize;
    // Copy `count` bytes into the caller's `length`-byte buffer and
    // NUL-terminate it.
    // SAFETY: forwarded raw pointers satisfy `slice_mut`'s contract per this function's own `# Safety` requirements.
    let Some(out) = (unsafe { slice_mut(line as *mut u8, length as usize) }) else {
        return;
    };
    if count > 0
        // SAFETY: forwarded raw pointers satisfy `slice_ref`'s contract per this function's own `# Safety` requirements.
        && let Some(src) = unsafe { slice_ref(source as *const u8, count) }
    {
        out[..count].copy_from_slice(src);
    }
    out[count] = 0;
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getseltext(c: textbox, text: *mut c_char) {
    if c.is_null() || text.is_null() {
        return;
    }
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let source = unsafe { obj_ref(c) }.map_or(ptr::null_mut(), |data| data.text);
    if source.is_null() {
        // NUL-terminate the caller's buffer.
        // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(t) = unsafe { ptr_mut(text) } {
            *t = 0;
        }
        return;
    }

    // SAFETY: forwarded raw pointers satisfy `text_len`'s contract per this function's own `# Safety` requirements.
    let len = unsafe { text_len(source) };
    // SAFETY: forwarded raw pointers satisfy `selection_bounds`'s contract per this function's own `# Safety` requirements.
    let (start, end) = unsafe { selection_bounds(c, len) };
    let count = end.saturating_sub(start);
    // Copy the selected range into the caller's buffer, sized for the
    // full text plus NUL, and terminate it.
    // SAFETY: forwarded raw pointers satisfy `slice_mut`'s contract per this function's own `# Safety` requirements.
    let Some(out) = (unsafe { slice_mut(text as *mut u8, count + 1) }) else {
        return;
    };
    if count > 0
        // SAFETY: forwarded raw pointers satisfy `slice_ref`'s contract per this function's own `# Safety` requirements.
        && let Some(src) = unsafe { slice_ref(source as *const u8, len) }.and_then(|s| s.get(start..end))
    {
        out[..count].copy_from_slice(src);
    }
    out[count] = 0;
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setlimittext(t: textbox, limit: c_long) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    let Some(data) = (unsafe { obj_mut(t) }) else {
        return;
    };
    data.max = limit.max(0) as c_int;
    // SAFETY: forwarded raw pointers satisfy `checklimittext`'s contract per this function's own `# Safety` requirements.
    unsafe { checklimittext(t, limit) };
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getlimittext(t: textbox) -> c_long {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(t) }).map_or(0, |data| data.max as c_long)
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn checklimittext(t: textbox, n: c_long) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    let Some(data) = (unsafe { obj_mut(t) }) else {
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
    // SAFETY: forwarded raw pointers satisfy `text_len`'s contract per this function's own `# Safety` requirements.
    let len = unsafe { text_len(text) };
    if len > limit {
        // Truncate the NUL-terminated text buffer at `limit` bytes.
        // SAFETY: forwarded raw pointers satisfy `slice_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(b) =
            unsafe { slice_mut(text as *mut u8, len + 1) }.and_then(|s| s.get_mut(limit))
        {
            *b = 0;
        }
    }
}

pub fn getpastelength() -> c_long {
    0
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn textselectionex(obj: control, start: *mut c_long, end: *mut c_long) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some(data) = (unsafe { obj_ref(obj) }) else {
        return;
    };
    // Write through the caller's out-pointers after null checks.
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(p) = unsafe { ptr_mut(start) } {
        *p = data.size as c_long;
    }
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(p) = unsafe { ptr_mut(end) } {
        *p = data.xsize as c_long;
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn selecttextex(obj: control, start: c_long, end: c_long) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(data) = unsafe { obj_mut(obj) } {
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
