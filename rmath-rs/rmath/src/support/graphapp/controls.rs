#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Control management for GraphApp.
//!
//! Ported from controls.c - manipulating buttons, text fields,
//! scrollbars, checkboxes, and other controls.

use std::os::raw::{c_char, c_int, c_long, c_void};
use std::ptr;

use super::types::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn setaction(c: control, fn_: actionfn) {
    unsafe {
        if !c.is_null() {
            (*c).action = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sethit(c: control, fn_: intfn) {
    unsafe {
        if !c.is_null() {
            (*c).hit = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setdel(c: control, fn_: actionfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).die = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setclose(c: control, fn_: actionfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).close = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setredraw(c: control, fn_: drawfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).redraw = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setresize(c: control, fn_: drawfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).resize = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setkeydown(c: control, fn_: keyfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).keydown = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setkeyaction(c: control, fn_: keyfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).keyaction = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setmousedown(c: control, fn_: mousefn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).mousedown = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setmousedrag(c: control, fn_: mousefn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).mousedrag = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setmouseup(c: control, fn_: mousefn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).mouseup = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setmousemove(c: control, fn_: mousefn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).mousemove = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setmouserepeat(c: control, fn_: mousefn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).mouserepeat = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setdrop(c: control, fn_: dropfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).drop = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setonfocus(c: control, fn_: actionfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).focus = fn_;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setim(c: control, fn_: imfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).im = fn_;
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clear(_c: control) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn draw(_c: control) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn redraw(_c: control) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn resize(c: control, r: rect) {
    unsafe {
        if !c.is_null() {
            (*c).rect = r;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn show_control(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state |= GA_Visible;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hide_control(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state &= !GA_Visible;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn isvisible(c: control) -> c_int {
    unsafe {
        if c.is_null() {
            0
        } else {
            if ((*c).state & GA_Visible) != 0 { 1 } else { 0 }
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn enable(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state |= GA_Enabled;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn disable(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state &= !GA_Enabled;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn isenabled(c: control) -> c_int {
    unsafe {
        if c.is_null() {
            0
        } else {
            if ((*c).state & GA_Enabled) != 0 { 1 } else { 0 }
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn check(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state |= GA_Checked;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn uncheck(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state &= !GA_Checked;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ischecked(c: control) -> c_int {
    unsafe {
        if c.is_null() {
            0
        } else {
            if ((*c).state & GA_Checked) != 0 { 1 } else { 0 }
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn highlight(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state |= GA_Highlighted;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn unhighlight(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state &= !GA_Highlighted;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ishighlighted(c: control) -> c_int {
    unsafe {
        if c.is_null() {
            0
        } else {
            if ((*c).state & GA_Highlighted) != 0 {
                1
            } else {
                0
            }
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flashcontrol(_c: control) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn activatecontrol(_c: control) { /* TODO */
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn settext(c: control, text: *const c_char) {
    unsafe {
        if c.is_null() {
            return;
        }
        if !(*c).text.is_null() {
            super::memory::memfree((*c).text as *mut u8);
        }
        (*c).text = super::strings::new_string(text);
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn GA_gettext(c: control) -> *mut c_char {
    unsafe {
        if c.is_null() {
            ptr::null_mut()
        } else {
            (*c).text
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gettextfont(c: control) -> font {
    if c.is_null() {
        ptr::null_mut()
    } else {
        ptr::null_mut()
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn settextfont(_c: control, _f: font) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setforeground(c: control, fg: rgb) {
    unsafe {
        if !c.is_null() {
            (*c).fg = fg;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getforeground(c: control) -> rgb {
    unsafe { if c.is_null() { Black } else { (*c).fg } }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setbackground(c: control, bg: rgb) {
    unsafe {
        if !c.is_null() {
            (*c).bg = bg;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getbackground(c: control) -> rgb {
    unsafe { if c.is_null() { Transparent } else { (*c).bg } }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setvalue(c: control, value: c_int) {
    unsafe {
        if !c.is_null() {
            (*c).value = value;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getvalue(c: control) -> c_int {
    unsafe { if c.is_null() { 0 } else { (*c).value } }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setdata(c: control, data: *mut c_void) {
    unsafe {
        if !c.is_null() {
            (*c).data = data;
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getdata(c: control) -> *mut c_void {
    unsafe {
        if c.is_null() {
            ptr::null_mut()
        } else {
            (*c).data
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn parentwindow(c: control) -> window {
    unsafe {
        if c.is_null() {
            ptr::null_mut()
        } else {
            (*c).parent
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn newcontrol(_text: *const c_char, _r: rect) -> control {
    ptr::null_mut() // TODO: Platform-specific
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newdrawing(_r: rect, _fn_: drawfn) -> drawing {
    ptr::null_mut() // TODO: Platform-specific
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newpicture(_img: image, _r: rect) -> drawing {
    ptr::null_mut() // TODO: Platform-specific
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newbutton(_text: *const c_char, _r: rect, _fn_: actionfn) -> button {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newimagebutton(_img: image, _r: rect, _fn_: actionfn) -> button {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setimage(_c: control, _img: image) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newcheckbox(_text: *const c_char, _r: rect, _fn_: actionfn) -> checkbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newimagecheckbox(_img: image, _r: rect, _fn_: actionfn) -> checkbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newradiobutton(
    _text: *const c_char,
    _r: rect,
    _fn_: actionfn,
) -> radiobutton {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newradiogroup() -> radiogroup {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newscrollbar(
    _r: rect,
    _max: c_int,
    _pagesize: c_int,
    _fn_: scrollfn,
) -> scrollbar {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn changescrollbar(_s: scrollbar, _where_: c_int, _max: c_int, _size: c_int) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newlabel(_text: *const c_char, _r: rect, _alignment: c_int) -> label {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newfield(_text: *const c_char, _r: rect) -> field {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newpassword(_text: *const c_char, _r: rect) -> field {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newtextbox(_text: *const c_char, _r: rect) -> textbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newtextarea(_text: *const c_char, _r: rect) -> textbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newrichtextarea(_text: *const c_char, _r: rect) -> textbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newlistbox(
    _list: *const *const c_char,
    _r: rect,
    _fn_: scrollfn,
    _dble: actionfn,
) -> listbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newdroplist(
    _list: *const *const c_char,
    _r: rect,
    _fn_: scrollfn,
) -> listbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newdropfield(
    _list: *const *const c_char,
    _r: rect,
    _fn_: scrollfn,
) -> listbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newmultilist(
    _list: *const *const c_char,
    _r: rect,
    _fn_: scrollfn,
    _dble: actionfn,
) -> listbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn isselected(_b: listbox, _index: c_int) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setlistitem(_b: listbox, _index: c_int) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getlistitem(_b: listbox) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn changelistbox(_b: listbox, _list: *const *const c_char) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newprogressbar(
    _r: rect,
    _pmin: c_int,
    _pmax: c_int,
    _incr: c_int,
    _smooth: c_int,
) -> progressbar {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setprogressbar(_obj: progressbar, _n: c_int) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stepprogressbar(_obj: progressbar, _n: c_int) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setprogressbarrange(_obj: progressbar, _pbmin: c_int, _pbmax: c_int) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newmenubar(_fn_: actionfn) -> menubar {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newsubmenu(_parent: menu, _name: *const c_char) -> menu {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newmenu(_name: *const c_char) -> menu {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newmenuitem(_name: *const c_char, _key: c_int, _fn_: menufn) -> menuitem {
    ptr::null_mut()
}

// Text editing
#[unsafe(no_mangle)]
pub unsafe extern "C" fn undotext(_t: textbox) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cuttext(_t: textbox) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn copytext(_t: textbox) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cleartext(_t: textbox) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pastetext(_t: textbox) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn inserttext(_t: textbox, _text: *const c_char) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn selecttext(_t: textbox, _start: c_long, _end: c_long) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn textselection(_t: textbox, _start: *mut c_long, _end: *mut c_long) {
    /* TODO */
}

// Font functions
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newfont(_name: *const c_char, _style: c_int, _size: c_int) -> font {
    ptr::null_mut() // TODO: Platform-specific
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fontwidth(_f: font) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fontheight(_f: font) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fontascent(_f: font) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fontdescent(_f: font) -> c_int {
    0
}

// Height alias
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getheight(f: font) -> c_int {
    unsafe { fontheight(f) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getdescent(f: font) -> c_int {
    unsafe { fontdescent(f) }
}

// Image control
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newbitmap(_width: c_int, _height: c_int, _depth: c_int) -> bitmap {
    ptr::null_mut() // TODO: Platform-specific
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loadbitmap(_name: *const c_char) -> bitmap {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imagetobitmap(_img: image) -> bitmap {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createbitmap(
    _width: c_int,
    _height: c_int,
    _depth: c_int,
    _data: *mut GAbyte,
) -> bitmap {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setbitmapdata(_b: bitmap, _data: *mut GAbyte) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getbitmapdata(_b: bitmap, _data: *mut GAbyte) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getbitmapdata2(_b: bitmap, _data: *mut *mut GAbyte) { /* TODO */
}

// Cursor functions
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newcursor(_hotspot: point, _img: image) -> cursor {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createcursor(
    _offset: point,
    _white_mask: *mut GAbyte,
    _black_shape: *mut GAbyte,
) -> cursor {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loadcursor(_name: *const c_char) -> cursor {
    ptr::null_mut() // TODO
}

// Image load/save
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loadimage(_filename: *const c_char) -> image {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn saveimage(_img: image, _filename: *const c_char) { /* TODO */
}
