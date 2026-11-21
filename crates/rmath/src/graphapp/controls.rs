#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

//! Control management for GraphApp.
//!
//! Ported from controls.c - manipulating buttons, text fields,
//! scrollbars, checkboxes, and other controls.

use std::fs::File;
use std::io::Write;
use std::os::raw::{c_char, c_int, c_long, c_void};
use std::ptr;

use super::types::*;
use super::{image as image_api, memory, objects, strings, windows};

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn first_list_item(list: *const *const c_char) -> *const c_char {
    // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { ptr_ref(list) }).copied().unwrap_or(ptr::null())
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn list_length(list: *const *const c_char) -> c_int {
    if list.is_null() {
        return 0;
    }
    let mut len = 0;
    loop {
        // Walk the caller's NULL-terminated pointer array.
        // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
        let item = unsafe { ptr_ref(list.wrapping_add(len as usize)) }
            .copied()
            .unwrap_or(ptr::null());
        if item.is_null() {
            break;
        }
        len += 1;
    }
    len
}

fn alloc_drawstate(dest: drawing) -> drawstate {
    let state = memory::memalloc_bytes(std::mem::size_of::<drawstruct>()) as drawstate;
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    if let Some(state) = unsafe { ptr_mut(state) } {
        *state = drawstruct {
            dest,
            hue: Black,
            ..Default::default()
        };
    }
    state
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn new_control_object(
    kind: c_int,
    text: *const c_char,
    r: rect,
    with_drawstate: bool,
) -> control {
    objects::init_objects();
    let parent = windows::get_current_window();
    // SAFETY: forwarded raw pointers satisfy `new_object`'s contract per this function's own `# Safety` requirements.
    let obj = unsafe { objects::new_object(kind, ptr::null_mut(), parent) };
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    let Some(obj) = (unsafe { obj_mut(obj) }) else {
        return ptr::null_mut();
    };
    obj.rect = r;
    obj.state = GA_Visible | GA_Enabled;
    obj.fg = Black;
    obj.bg = White;
    // SAFETY: forwarded raw pointers satisfy `new_string`'s contract per this function's own `# Safety` requirements.
    obj.text = unsafe { strings::new_string(text) };
    if with_drawstate {
        obj.drawstate = alloc_drawstate(obj);
    }
    obj
}

fn new_bitmap_object(width: c_int, height: c_int, depth: c_int) -> bitmap {
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let bitmap = unsafe {
        new_control_object(
            BitmapObject,
            ptr::null(),
            rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            false,
        )
    };
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let Some(bitmap) = (unsafe { obj_mut(bitmap) }) else {
        return ptr::null_mut();
    };
    let normalized_depth = if depth <= 8 { 8 } else { 32 };
    bitmap.depth = normalized_depth;
    bitmap.img = image_api::newimage(width.max(0), height.max(0), normalized_depth);
    bitmap
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn set_bitmap_pixels(bitmap: bitmap, data: *mut GAbyte) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some(img) = unsafe { obj_ref(bitmap) }.map(|b| b.img) else {
        return;
    };
    if img.is_null() || data.is_null() {
        return;
    }
    // SAFETY: forwarded raw pointers satisfy `setpixels`'s contract per this function's own `# Safety` requirements.
    unsafe { image_api::setpixels(img, data) };
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setaction(c: control, fn_: actionfn) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.action = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn sethit(c: control, fn_: intfn) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.hit = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setdel(c: control, fn_: actionfn) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(call) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.call) }) {
        call.die = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setclose(c: control, fn_: actionfn) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(call) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.call) }) {
        call.close = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setredraw(c: control, fn_: drawfn) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(call) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.call) }) {
        call.redraw = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setresize(c: control, fn_: drawfn) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(call) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.call) }) {
        call.resize = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setkeydown(c: control, fn_: keyfn) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(call) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.call) }) {
        call.keydown = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setkeyaction(c: control, fn_: keyfn) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(call) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.call) }) {
        call.keyaction = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setmousedown(c: control, fn_: mousefn) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(call) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.call) }) {
        call.mousedown = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setmousedrag(c: control, fn_: mousefn) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(call) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.call) }) {
        call.mousedrag = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setmouseup(c: control, fn_: mousefn) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(call) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.call) }) {
        call.mouseup = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setmousemove(c: control, fn_: mousefn) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(call) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.call) }) {
        call.mousemove = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setmouserepeat(c: control, fn_: mousefn) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(call) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.call) }) {
        call.mouserepeat = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setdrop(c: control, fn_: dropfn) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(call) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.call) }) {
        call.drop = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setonfocus(c: control, fn_: actionfn) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(call) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.call) }) {
        call.focus = fn_;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setim(c: control, fn_: imfn) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(call) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.call) }) {
        call.im = fn_;
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn clear(c: control) {
    // SAFETY: forwarded raw pointers satisfy `draw`'s contract per this function's own `# Safety` requirements.
    unsafe { draw(c) };
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn draw(c: control) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some(obj) = (unsafe { obj_ref(c) }) else {
        return;
    };
    if (obj.state & GA_Visible) == 0 {
        return;
    }
    // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(redraw) = unsafe { ptr_ref(obj.call) }.and_then(|call| call.redraw) {
        redraw(c, obj.rect);
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn redraw(c: control) {
    // SAFETY: forwarded raw pointers satisfy `draw`'s contract per this function's own `# Safety` requirements.
    unsafe { draw(c) };
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn resize(c: control, r: rect) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.rect = r;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn show_control(c: control) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.state |= GA_Visible;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn hide_control(c: control) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.state &= !GA_Visible;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn isvisible(c: control) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(c) }).map_or(0, |c| if (c.state & GA_Visible) != 0 { 1 } else { 0 })
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn enable(c: control) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.state |= GA_Enabled;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn disable(c: control) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.state &= !GA_Enabled;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn isenabled(c: control) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(c) }).map_or(0, |c| if (c.state & GA_Enabled) != 0 { 1 } else { 0 })
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn check(c: control) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.state |= GA_Checked;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn uncheck(c: control) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.state &= !GA_Checked;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn ischecked(c: control) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(c) }).map_or(0, |c| if (c.state & GA_Checked) != 0 { 1 } else { 0 })
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn highlight(c: control) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.state |= GA_Highlighted;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn unhighlight(c: control) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.state &= !GA_Highlighted;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn ishighlighted(c: control) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(c) }).map_or(0, |c| {
        if (c.state & GA_Highlighted) != 0 {
            1
        } else {
            0
        }
    })
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn flashcontrol(c: control) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some(state) = unsafe { obj_ref(c) }.map(|obj| obj.state) else {
        return;
    };
    if (state & GA_Enabled) == 0 {
        return;
    }

    let was_armed = (state & GA_Armed) != 0;
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(obj) = unsafe { obj_mut(c) } {
        obj.state |= GA_Armed;
    }
    // SAFETY: forwarded raw pointers satisfy `redraw`'s contract per this function's own `# Safety` requirements.
    unsafe { redraw(c) };
    if !was_armed {
        // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(obj) = unsafe { obj_mut(c) } {
            obj.state &= !GA_Armed;
        }
    }
    // SAFETY: forwarded raw pointers satisfy `redraw`'s contract per this function's own `# Safety` requirements.
    unsafe { redraw(c) };
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn activatecontrol(c: control) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some(state) = unsafe { obj_ref(c) }.map(|obj| obj.state) else {
        return;
    };
    if (state & GA_Enabled) == 0 {
        return;
    }

    // SAFETY: forwarded raw pointers satisfy `flashcontrol`'s contract per this function's own `# Safety` requirements.
    unsafe { flashcontrol(c) };

    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let focus = unsafe { obj_ref(c) }
        // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
        .and_then(|obj| unsafe { ptr_ref(obj.call) })
        .and_then(|call| call.focus);
    if let Some(focus) = focus {
        focus(c);
    }
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(action) = unsafe { obj_ref(c) }.and_then(|obj| obj.action) {
        action(c);
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn settext(c: control, text: *const c_char) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    let Some(c) = (unsafe { obj_mut(c) }) else {
        return;
    };
    if !c.text.is_null() {
        // SAFETY: forwarded raw pointers satisfy `memfree`'s contract per this function's own `# Safety` requirements.
        unsafe { super::memory::memfree(c.text as *mut u8) };
    }
    // SAFETY: forwarded raw pointers satisfy `new_string`'s contract per this function's own `# Safety` requirements.
    c.text = unsafe { super::strings::new_string(text) };
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn GA_gettext(c: control) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(c) }).map_or(ptr::null_mut(), |c| c.text)
}
#[allow(clippy::if_same_then_else)]
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn gettextfont(c: control) -> font {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(c) })
        // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
        .and_then(|c| unsafe { ptr_ref(c.drawstate) })
        .map_or(ptr::null_mut(), |ds| ds.fnt)
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn settextfont(c: control, f: font) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(ds) = unsafe { obj_ref(c) }.and_then(|c| unsafe { ptr_mut(c.drawstate) }) {
        ds.fnt = f;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setforeground(c: control, fg: rgb) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.fg = fg;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getforeground(c: control) -> rgb {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(c) }).map_or(Black, |c| c.fg)
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setbackground(c: control, bg: rgb) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.bg = bg;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getbackground(c: control) -> rgb {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(c) }).map_or(Transparent, |c| c.bg)
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setvalue(c: control, value: c_int) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.value = value;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getvalue(c: control) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(c) }).map_or(0, |c| c.value)
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setdata(c: control, data: *mut c_void) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.data = data;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getdata(c: control) -> *mut c_void {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(c) }).map_or(ptr::null_mut(), |c| c.data)
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn parentwindow(c: control) -> window {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(c) }).map_or(ptr::null_mut(), |c| c.parent)
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newcontrol(text: *const c_char, r: rect) -> control {
    // SAFETY: forwarded raw pointers satisfy `new_control_object`'s contract per this function's own `# Safety` requirements.
    unsafe { new_control_object(ControlObject, text, r, false) }
}
pub fn newdrawing(r: rect, fn_: drawfn) -> drawing {
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let drawing = unsafe { new_control_object(UserObject, ptr::null(), r, true) };
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    if let Some(call) = unsafe { obj_ref(drawing) }.and_then(|d| unsafe { ptr_mut(d.call) }) {
        call.redraw = fn_;
    }
    drawing
}
pub fn newpicture(img: image, r: rect) -> drawing {
    let picture = newdrawing(r, None);
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    if let Some(picture) = unsafe { obj_mut(picture) } {
        picture.img = img;
    }
    picture
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newbutton(text: *const c_char, r: rect, fn_: actionfn) -> button {
    // SAFETY: forwarded raw pointers satisfy `new_control_object`'s contract per this function's own `# Safety` requirements.
    let button = unsafe { new_control_object(ButtonObject, text, r, false) };
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(button) = unsafe { obj_mut(button) } {
        button.action = fn_;
    }
    button
}
pub fn newimagebutton(img: image, r: rect, fn_: actionfn) -> button {
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let button = unsafe { newbutton(ptr::null(), r, fn_) };
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    if let Some(button) = unsafe { obj_mut(button) } {
        button.img = img;
    }
    button
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setimage(c: control, img: image) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(c) = unsafe { obj_mut(c) } {
        c.img = img;
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newcheckbox(text: *const c_char, r: rect, fn_: actionfn) -> checkbox {
    // SAFETY: forwarded raw pointers satisfy `new_control_object`'s contract per this function's own `# Safety` requirements.
    let checkbox = unsafe { new_control_object(CheckboxObject, text, r, false) };
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(checkbox) = unsafe { obj_mut(checkbox) } {
        checkbox.action = fn_;
    }
    checkbox
}
pub fn newimagecheckbox(img: image, r: rect, fn_: actionfn) -> checkbox {
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let checkbox = unsafe { newcheckbox(ptr::null(), r, fn_) };
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    if let Some(checkbox) = unsafe { obj_mut(checkbox) } {
        checkbox.img = img;
    }
    checkbox
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newradiobutton(text: *const c_char, r: rect, fn_: actionfn) -> radiobutton {
    // SAFETY: forwarded raw pointers satisfy `new_control_object`'s contract per this function's own `# Safety` requirements.
    let button = unsafe { new_control_object(RadioObject, text, r, false) };
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(button) = unsafe { obj_mut(button) } {
        button.action = fn_;
    }
    button
}
pub fn newradiogroup() -> radiogroup {
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    unsafe { new_control_object(RadiogroupObject, ptr::null(), rect::default(), false) }
}
pub fn newscrollbar(r: rect, max: c_int, pagesize: c_int, _fn_: scrollfn) -> scrollbar {
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let scrollbar = unsafe { new_control_object(ScrollbarObject, ptr::null(), r, false) };
    if !scrollbar.is_null() {
        // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
        unsafe { changescrollbar(scrollbar, 0, max, pagesize) };
    }
    scrollbar
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn changescrollbar(s: scrollbar, where_: c_int, max: c_int, size: c_int) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    let Some(s) = (unsafe { obj_mut(s) }) else {
        return;
    };

    let max = max.max(0);
    let size = size.clamp(0, max);
    let limit = max.saturating_sub(size);

    s.max = max;
    s.size = size;
    s.value = where_.clamp(0, limit);
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newlabel(text: *const c_char, r: rect, alignment: c_int) -> label {
    // SAFETY: forwarded raw pointers satisfy `new_control_object`'s contract per this function's own `# Safety` requirements.
    let label = unsafe { new_control_object(LabelObject, text, r, false) };
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(label) = unsafe { obj_mut(label) } {
        label.value = alignment;
    }
    label
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newfield(text: *const c_char, r: rect) -> field {
    // SAFETY: forwarded raw pointers satisfy `new_control_object`'s contract per this function's own `# Safety` requirements.
    unsafe { new_control_object(FieldObject, text, r, true) }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newpassword(text: *const c_char, r: rect) -> field {
    // SAFETY: forwarded raw pointers satisfy `newfield`'s contract per this function's own `# Safety` requirements.
    let field = unsafe { newfield(text, r) };
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(field) = unsafe { obj_mut(field) } {
        field.flags |= 1;
    }
    field
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newtextbox(text: *const c_char, r: rect) -> textbox {
    // SAFETY: forwarded raw pointers satisfy `new_control_object`'s contract per this function's own `# Safety` requirements.
    unsafe { new_control_object(TextboxObject, text, r, true) }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newtextarea(text: *const c_char, r: rect) -> textbox {
    // SAFETY: forwarded raw pointers satisfy `newtextbox`'s contract per this function's own `# Safety` requirements.
    unsafe { newtextbox(text, r) }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newrichtextarea(text: *const c_char, r: rect) -> textbox {
    // SAFETY: forwarded raw pointers satisfy `newtextbox`'s contract per this function's own `# Safety` requirements.
    unsafe { newtextbox(text, r) }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newlistbox(
    list: *const *const c_char,
    r: rect,
    _fn_: scrollfn,
    dble: actionfn,
) -> listbox {
    // SAFETY: forwarded raw pointers satisfy `new_control_object`'s contract per this function's own `# Safety` requirements.
    let listbox = unsafe { new_control_object(ListboxObject, first_list_item(list), r, false) };
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(listbox) = unsafe { obj_mut(listbox) } {
        // SAFETY: forwarded raw pointers satisfy `list_length`'s contract per this function's own `# Safety` requirements.
        listbox.max = unsafe { list_length(list) };
        listbox.value = -1;
        listbox.dble = dble;
    }
    listbox
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newdroplist(list: *const *const c_char, r: rect, fn_: scrollfn) -> listbox {
    // SAFETY: forwarded raw pointers satisfy `newlistbox`'s contract per this function's own `# Safety` requirements.
    let listbox = unsafe { newlistbox(list, r, fn_, None) };
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(listbox) = unsafe { obj_mut(listbox) } {
        listbox.kind = DroplistObject;
    }
    listbox
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newdropfield(list: *const *const c_char, r: rect, fn_: scrollfn) -> listbox {
    // SAFETY: forwarded raw pointers satisfy `newlistbox`'s contract per this function's own `# Safety` requirements.
    let listbox = unsafe { newlistbox(list, r, fn_, None) };
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(listbox) = unsafe { obj_mut(listbox) } {
        listbox.kind = DropfieldObject;
    }
    listbox
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newmultilist(
    list: *const *const c_char,
    r: rect,
    fn_: scrollfn,
    dble: actionfn,
) -> listbox {
    // SAFETY: forwarded raw pointers satisfy `newlistbox`'s contract per this function's own `# Safety` requirements.
    let listbox = unsafe { newlistbox(list, r, fn_, dble) };
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(listbox) = unsafe { obj_mut(listbox) } {
        listbox.kind = MultilistObject;
    }
    listbox
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn isselected(b: listbox, index: c_int) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(b) }).map_or(0, |b| if b.value == index { 1 } else { 0 })
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setlistitem(b: listbox, index: c_int) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(b) = unsafe { obj_mut(b) } {
        b.value = index.max(-1);
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getlistitem(b: listbox) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(b) }).map_or(-1, |b| b.value)
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn changelistbox(b: listbox, list: *const *const c_char) {
    if b.is_null() {
        return;
    }
    // SAFETY: forwarded raw pointers satisfy `settext`'s contract per this function's own `# Safety` requirements.
    unsafe { settext(b, first_list_item(list)) };
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(b) = unsafe { obj_mut(b) } {
        // SAFETY: forwarded raw pointers satisfy `list_length`'s contract per this function's own `# Safety` requirements.
        b.max = unsafe { list_length(list) };
        b.value = -1;
    }
}
pub fn newprogressbar(
    r: rect,
    pmin: c_int,
    pmax: c_int,
    incr: c_int,
    smooth: c_int,
) -> progressbar {
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let progress = unsafe { new_control_object(ProgressbarObject, ptr::null(), r, false) };
    if !progress.is_null() {
        // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
        unsafe { setprogressbarrange(progress, pmin, pmax) };
        // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
        if let Some(progress) = unsafe { obj_mut(progress) } {
            progress.data = incr as usize as *mut c_void;
            progress.flags = smooth as c_long;
        }
    }
    progress
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setprogressbar(obj: progressbar, n: c_int) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    let Some(obj) = (unsafe { obj_mut(obj) }) else {
        return;
    };

    let min = obj.size.min(obj.max);
    let max = obj.max.max(obj.size);
    obj.value = n.clamp(min, max);
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn stepprogressbar(obj: progressbar, n: c_int) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    if let Some(value) = unsafe { obj_ref(obj) }.map(|obj| obj.value) {
        // SAFETY: forwarded raw pointers satisfy `setprogressbar`'s contract per this function's own `# Safety` requirements.
        unsafe { setprogressbar(obj, value.saturating_add(n)) };
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setprogressbarrange(obj: progressbar, pbmin: c_int, pbmax: c_int) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    let Some(obj) = (unsafe { obj_mut(obj) }) else {
        return;
    };

    let (min, max) = if pbmin <= pbmax {
        (pbmin, pbmax)
    } else {
        (pbmax, pbmin)
    };

    obj.size = min;
    obj.max = max;
    obj.value = obj.value.clamp(min, max);
}
pub fn newmenubar(_fn_: actionfn) -> menubar {
    ptr::null_mut()
}
pub fn newsubmenu(_parent: menu, _name: *const c_char) -> menu {
    ptr::null_mut()
}
pub fn newmenu(_name: *const c_char) -> menu {
    ptr::null_mut()
}
pub fn newmenuitem(_name: *const c_char, _key: c_int, _fn_: menufn) -> menuitem {
    ptr::null_mut()
}

// Text editing
pub fn undotext(_t: textbox) {}
pub fn cuttext(_t: textbox) {}
pub fn copytext(_t: textbox) {}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn cleartext(t: textbox) {
    if !t.is_null() {
        // SAFETY: forwarded raw pointers satisfy `settext`'s contract per this function's own `# Safety` requirements.
        unsafe { settext(t, ptr::null()) };
        // SAFETY: forwarded raw pointers satisfy `selecttext`'s contract per this function's own `# Safety` requirements.
        unsafe { selecttext(t, 0, 0) };
    }
}
pub fn pastetext(_t: textbox) {}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn inserttext(t: textbox, text: *const c_char) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    let Some(obj) = (unsafe { obj_mut(t) }) else {
        return;
    };
    if text.is_null() {
        return;
    }

    let old_text = obj.text;
    let combined = if old_text.is_null() {
        // SAFETY: forwarded raw pointers satisfy `new_string`'s contract per this function's own `# Safety` requirements.
        unsafe { super::strings::new_string(text) }
    } else {
        // SAFETY: forwarded raw pointers satisfy `add_strings`'s contract per this function's own `# Safety` requirements.
        unsafe { super::strings::add_strings(old_text, text) }
    };
    if combined.is_null() {
        return;
    }
    if !old_text.is_null() {
        // SAFETY: forwarded raw pointers satisfy `del_string`'s contract per this function's own `# Safety` requirements.
        unsafe { super::strings::del_string(old_text) };
    }
    obj.text = combined;

    // SAFETY: forwarded raw pointers satisfy `string_length`'s contract per this function's own `# Safety` requirements.
    let end = unsafe { super::strings::string_length(obj.text) };
    // SAFETY: forwarded raw pointers satisfy `selecttext`'s contract per this function's own `# Safety` requirements.
    unsafe { selecttext(t, end, end) };
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn selecttext(t: textbox, start: c_long, end: c_long) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    let Some(t) = (unsafe { obj_mut(t) }) else {
        return;
    };

    let start = start.max(0);
    let end = end.max(0);
    let (start, end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };

    t.caretx = start as c_int;
    t.carety = end as c_int;
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn textselection(t: textbox, start: *mut c_long, end: *mut c_long) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let selection =
        unsafe { obj_ref(t) }.map_or((0, 0), |t| (t.caretx as c_long, t.carety as c_long));

    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(start) = unsafe { ptr_mut(start) } {
        *start = selection.0;
    }
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(end) = unsafe { ptr_mut(end) } {
        *end = selection.1;
    }
}

// Font functions
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn newfont(name: *const c_char, style: c_int, size: c_int) -> font {
    objects::init_objects();
    // SAFETY: forwarded raw pointers satisfy `new_object`'s contract per this function's own `# Safety` requirements.
    let font = unsafe { objects::new_object(FontObject, ptr::null_mut(), ptr::null_mut()) };
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    let Some(font) = (unsafe { obj_mut(font) }) else {
        return ptr::null_mut();
    };
    // SAFETY: forwarded raw pointers satisfy `new_string`'s contract per this function's own `# Safety` requirements.
    font.text = unsafe { strings::new_string(name) };
    font.value = size.max(1);
    font.flags = style as c_long;
    font
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn fontwidth(f: font) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(f) }).map_or(0, |f| {
        if (f.flags & FixedWidth as c_long) != 0 {
            (f.value / 2).max(1)
        } else {
            (f.value * 3 / 5).max(1)
        }
    })
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn fontheight(f: font) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    (unsafe { obj_ref(f) }).map_or(0, |f| f.value.max(1))
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn fontascent(f: font) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `fontheight`'s contract per this function's own `# Safety` requirements.
    let height = unsafe { fontheight(f) };
    (height * 3) / 4
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn fontdescent(f: font) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `fontheight`'s contract per this function's own `# Safety` requirements.
    (unsafe { fontheight(f) }).saturating_sub(unsafe { fontascent(f) })
}

// Height alias
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getheight(f: font) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `fontheight`'s contract per this function's own `# Safety` requirements.
    unsafe { fontheight(f) }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getdescent(f: font) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `fontdescent`'s contract per this function's own `# Safety` requirements.
    unsafe { fontdescent(f) }
}

// Image control
pub fn newbitmap(width: c_int, height: c_int, depth: c_int) -> bitmap {
    new_bitmap_object(width, height, depth)
}
pub fn loadbitmap(name: *const c_char) -> bitmap {
    let img = loadimage(name);
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    unsafe { imagetobitmap(img) }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn imagetobitmap(img: image) -> bitmap {
    // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
    let Some((width, height, depth)) =
        unsafe { ptr_ref(img) }.map(|img| (img.width, img.height, img.depth))
    else {
        return ptr::null_mut();
    };
    let bitmap = new_bitmap_object(width, height, depth);
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(bitmap) = unsafe { obj_mut(bitmap) } {
        // SAFETY: forwarded raw pointers satisfy `delimage`'s contract per this function's own `# Safety` requirements.
        unsafe { image_api::delimage(bitmap.img) };
        // SAFETY: forwarded raw pointers satisfy `copyimage`'s contract per this function's own `# Safety` requirements.
        bitmap.img = unsafe { image_api::copyimage(img) };
    }
    bitmap
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn createbitmap(width: c_int, height: c_int, depth: c_int, data: *mut GAbyte) -> bitmap {
    let bitmap = new_bitmap_object(width, height, depth);
    // SAFETY: forwarded raw pointers satisfy `set_bitmap_pixels`'s contract per this function's own `# Safety` requirements.
    unsafe { set_bitmap_pixels(bitmap, data) };
    bitmap
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setbitmapdata(b: bitmap, data: *mut GAbyte) {
    // SAFETY: forwarded raw pointers satisfy `set_bitmap_pixels`'s contract per this function's own `# Safety` requirements.
    unsafe { set_bitmap_pixels(b, data) }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getbitmapdata(b: bitmap, data: *mut GAbyte) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some(img) = unsafe { obj_ref(b) }.map(|b| b.img) else {
        return;
    };
    if img.is_null() || data.is_null() {
        return;
    }
    // SAFETY: forwarded raw pointers satisfy `getpixels`'s contract per this function's own `# Safety` requirements.
    let pixels = unsafe { image_api::getpixels(img) };
    if pixels.is_null() {
        return;
    }
    // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
    let Some((width, height, depth)) =
        unsafe { ptr_ref(img) }.map(|img| (img.width, img.height, img.depth))
    else {
        return;
    };
    let pixel_count = (width.max(0) * height.max(0)) as usize;
    let byte_len = if depth <= 8 {
        pixel_count
    } else {
        pixel_count * std::mem::size_of::<rgb>()
    };
    // Copy `byte_len` bytes into the caller-provided buffer; the length
    // is derived from the image dimensions recorded above.
    // SAFETY: forwarded raw pointers satisfy `slice_mut`'s contract per this function's own `# Safety` requirements.
    if let (Some(dest), Some(src)) = (unsafe { slice_mut(data, byte_len) }, unsafe {
        slice_ref(pixels, byte_len)
    }) {
        dest.copy_from_slice(src);
    }
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn getbitmapdata2(b: bitmap, data: *mut *mut GAbyte) {
    // SAFETY: forwarded raw pointers satisfy `obj_ref`'s contract per this function's own `# Safety` requirements.
    let Some(img) = unsafe { obj_ref(b) }.map(|b| b.img) else {
        return;
    };
    // SAFETY: forwarded raw pointers satisfy `ptr_mut`'s contract per this function's own `# Safety` requirements.
    let Some(data) = (unsafe { ptr_mut(data) }) else {
        return;
    };
    if img.is_null() {
        return;
    }
    // SAFETY: forwarded raw pointers satisfy `getpixels`'s contract per this function's own `# Safety` requirements.
    *data = unsafe { image_api::getpixels(img) };
}

// Cursor functions
pub fn newcursor(hotspot: point, img: image) -> cursor {
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let cursor = unsafe { new_control_object(CursorObject, ptr::null(), rect::default(), false) };
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    if let Some(cursor) = unsafe { obj_mut(cursor) } {
        cursor.rect.x = hotspot.x;
        cursor.rect.y = hotspot.y;
        cursor.img = img;
    }
    cursor
}
pub fn createcursor(offset: point, _white_mask: *mut GAbyte, _black_shape: *mut GAbyte) -> cursor {
    newcursor(offset, ptr::null_mut())
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn loadcursor(name: *const c_char) -> cursor {
    let cursor = newcursor(point::default(), ptr::null_mut());
    if !cursor.is_null() {
        // SAFETY: forwarded raw pointers satisfy `settext`'s contract per this function's own `# Safety` requirements.
        unsafe { settext(cursor, name) };
    }
    cursor
}

// Image load/save
pub fn loadimage(_filename: *const c_char) -> image {
    ptr::null_mut()
}
/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn saveimage(img: image, filename: *const c_char) {
    // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
    let Some(data) = (unsafe { ptr_ref(img) }) else {
        return;
    };
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    let Some(path) = unsafe { cstr(filename) }.and_then(|f| f.to_str().ok()) else {
        return;
    };
    let Ok(mut file) = File::create(path) else {
        return;
    };
    let width = data.width.max(0);
    let height = data.height.max(0);

    if data.depth <= 8 {
        let _ = writeln!(file, "P5\n{} {}\n255", width, height);
        // SAFETY: forwarded raw pointers satisfy `getpixels`'s contract per this function's own `# Safety` requirements.
        let pixels = unsafe { image_api::getpixels(img) };
        // Read `width * height` bytes from the owned pixel buffer.
        // SAFETY: forwarded raw pointers satisfy `slice_ref`'s contract per this function's own `# Safety` requirements.
        if let Some(bytes) = unsafe { slice_ref(pixels, (width * height) as usize) } {
            let _ = file.write_all(bytes);
        }
        return;
    }

    let _ = writeln!(file, "P6\n{} {}\n255", width, height);
    for y in 0..height {
        for x in 0..width {
            // SAFETY: forwarded raw pointers satisfy `get_image_pixel`'s contract per this function's own `# Safety` requirements.
            let pixel = unsafe { image_api::get_image_pixel(img, x, y) };
            let bytes = [
                getred(pixel) as u8,
                getgreen(pixel) as u8,
                getblue(pixel) as u8,
            ];
            let _ = file.write_all(&bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::ffi::{CStr, CString};
    use std::mem;

    thread_local! {
        static ACTION_CALLS: Cell<c_int> = Cell::new(0);
        static FOCUS_CALLS: Cell<c_int> = Cell::new(0);
        static REDRAW_CALLS: Cell<c_int> = Cell::new(0);
    }

    extern "C" fn record_action(_c: control) {
        ACTION_CALLS.with(|calls| calls.set(calls.get() + 1));
    }

    extern "C" fn record_focus(_c: control) {
        FOCUS_CALLS.with(|calls| calls.set(calls.get() + 1));
    }

    extern "C" fn record_redraw(_c: control, _r: rect) {
        REDRAW_CALLS.with(|calls| calls.set(calls.get() + 1));
    }

    fn make_control() -> (Box<ObjInfo>, Box<callinfo>) {
        let mut call = Box::new(unsafe { mem::zeroed::<callinfo>() });
        let mut obj = Box::new(unsafe { mem::zeroed::<ObjInfo>() });
        obj.kind = ButtonObject;
        obj.call = &mut *call;
        (obj, call)
    }

    fn make_text_control() -> (Box<ObjInfo>, Box<callinfo>, Box<drawstruct>) {
        let (mut obj, call) = make_control();
        let mut drawstate = Box::new(unsafe { mem::zeroed::<drawstruct>() });
        obj.kind = TextboxObject;
        obj.drawstate = &mut *drawstate;
        (obj, call, drawstate)
    }

    #[test]
    fn activatecontrol_requires_enabled_control_and_runs_callbacks() {
        unsafe {
            let (mut control, mut call) = make_control();
            let control = &mut *control as control;

            ACTION_CALLS.with(|calls| calls.set(0));
            FOCUS_CALLS.with(|calls| calls.set(0));
            REDRAW_CALLS.with(|calls| calls.set(0));

            (*control).state = GA_Visible | GA_Enabled;
            (*control).action = Some(record_action);
            call.focus = Some(record_focus);
            call.redraw = Some(record_redraw);

            activatecontrol(control);

            ACTION_CALLS.with(|calls| assert_eq!(calls.get(), 1));
            FOCUS_CALLS.with(|calls| assert_eq!(calls.get(), 1));
            REDRAW_CALLS.with(|calls| assert_eq!(calls.get(), 2));
            assert_eq!((*control).state & GA_Armed, 0);

            disable(control);
            activatecontrol(control);
            ACTION_CALLS.with(|calls| assert_eq!(calls.get(), 1));
        }
    }

    #[test]
    fn draw_ignores_hidden_controls() {
        let (mut control, mut call) = make_control();
        let control = &mut *control as control;

        REDRAW_CALLS.with(|calls| calls.set(0));
        call.redraw = Some(record_redraw);

        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { draw(control) };
        REDRAW_CALLS.with(|calls| assert_eq!(calls.get(), 0));

        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { show_control(control) };
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { draw(control) };
        REDRAW_CALLS.with(|calls| assert_eq!(calls.get(), 1));
    }

    #[test]
    fn stateful_helpers_store_font_image_and_selection() {
        unsafe {
            let (mut control, _call, mut drawstate) = make_text_control();
            let control = &mut *control as control;

            let font_obj = Box::new(mem::zeroed::<ObjInfo>());
            let image_data = Box::new(mem::zeroed::<imagedata>());
            let font = Box::into_raw(font_obj) as font;
            let image = Box::into_raw(image_data);

            settextfont(control, font);
            setimage(control, image);
            selecttext(control, 9, 4);

            let mut start = -1;
            let mut end = -1;
            textselection(control, &mut start, &mut end);

            assert_eq!(gettextfont(control), font);
            assert_eq!((*control).img, image);
            assert_eq!(start, 4);
            assert_eq!(end, 9);

            drawstate.fnt = ptr::null_mut();
        }
    }

    #[test]
    fn list_scroll_and_progress_helpers_clamp_state() {
        unsafe {
            let (mut control, _call) = make_control();
            let control = &mut *control as control;

            changescrollbar(control, 15, 10, 4);
            assert_eq!((*control).max, 10);
            assert_eq!((*control).size, 4);
            assert_eq!((*control).value, 6);

            setlistitem(control, 3);
            assert_eq!(getlistitem(control), 3);
            setlistitem(control, -8);
            assert_eq!(getlistitem(control), -1);

            setprogressbarrange(control, 10, 4);
            assert_eq!((*control).size, 4);
            assert_eq!((*control).max, 10);

            setprogressbar(control, 99);
            assert_eq!((*control).value, 10);
            stepprogressbar(control, -20);
            assert_eq!((*control).value, 4);
        }
    }

    #[test]
    fn text_edit_helpers_update_contents() {
        unsafe {
            let (mut control, _call, _drawstate) = make_text_control();
            let control = &mut *control as control;
            let prefix = CString::new("hello").unwrap();
            let suffix = CString::new(" world").unwrap();

            settext(control, prefix.as_ptr());
            inserttext(control, suffix.as_ptr());

            let mut start = -1;
            let mut end = -1;
            textselection(control, &mut start, &mut end);

            assert_eq!(
                CStr::from_ptr(GA_gettext(control)).to_bytes(),
                b"hello world"
            );
            assert_eq!(start, 11);
            assert_eq!(end, 11);

            cleartext(control);

            textselection(control, &mut start, &mut end);
            assert_eq!(CStr::from_ptr(GA_gettext(control)).to_bytes(), b"");
            assert_eq!(start, 0);
            assert_eq!(end, 0);
        }
    }
}
