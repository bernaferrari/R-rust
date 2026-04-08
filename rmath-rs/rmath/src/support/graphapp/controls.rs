#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Control management for GraphApp.
//!
//! Ported from controls.c - manipulating buttons, text fields,
//! scrollbars, checkboxes, and other controls.

use std::os::raw::{c_char, c_int, c_long, c_void};
use std::ptr;

use super::types::*;

pub unsafe fn setaction(c: control, fn_: actionfn) {
    unsafe {
        if !c.is_null() {
            (*c).action = fn_;
        }
    }
}
pub unsafe fn sethit(c: control, fn_: intfn) {
    unsafe {
        if !c.is_null() {
            (*c).hit = fn_;
        }
    }
}
pub unsafe fn setdel(c: control, fn_: actionfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).die = fn_;
        }
    }
}
pub unsafe fn setclose(c: control, fn_: actionfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).close = fn_;
        }
    }
}
pub unsafe fn setredraw(c: control, fn_: drawfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).redraw = fn_;
        }
    }
}
pub unsafe fn setresize(c: control, fn_: drawfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).resize = fn_;
        }
    }
}
pub unsafe fn setkeydown(c: control, fn_: keyfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).keydown = fn_;
        }
    }
}
pub unsafe fn setkeyaction(c: control, fn_: keyfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).keyaction = fn_;
        }
    }
}
pub unsafe fn setmousedown(c: control, fn_: mousefn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).mousedown = fn_;
        }
    }
}
pub unsafe fn setmousedrag(c: control, fn_: mousefn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).mousedrag = fn_;
        }
    }
}
pub unsafe fn setmouseup(c: control, fn_: mousefn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).mouseup = fn_;
        }
    }
}
pub unsafe fn setmousemove(c: control, fn_: mousefn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).mousemove = fn_;
        }
    }
}
pub unsafe fn setmouserepeat(c: control, fn_: mousefn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).mouserepeat = fn_;
        }
    }
}
pub unsafe fn setdrop(c: control, fn_: dropfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).drop = fn_;
        }
    }
}
pub unsafe fn setonfocus(c: control, fn_: actionfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).focus = fn_;
        }
    }
}
pub unsafe fn setim(c: control, fn_: imfn) {
    unsafe {
        if !c.is_null() && !(*c).call.is_null() {
            (*(*c).call).im = fn_;
        }
    }
}

pub unsafe fn clear(_c: control) { /* TODO */
}
pub unsafe fn draw(_c: control) { /* TODO */
}
pub unsafe fn redraw(_c: control) { /* TODO */
}
pub unsafe fn resize(c: control, r: rect) {
    unsafe {
        if !c.is_null() {
            (*c).rect = r;
        }
    }
}
pub unsafe fn show_control(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state |= GA_Visible;
        }
    }
}
pub unsafe fn hide_control(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state &= !GA_Visible;
        }
    }
}
pub unsafe fn isvisible(c: control) -> c_int {
    unsafe {
        if c.is_null() {
            0
        } else {
            if ((*c).state & GA_Visible) != 0 { 1 } else { 0 }
        }
    }
}
pub unsafe fn enable(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state |= GA_Enabled;
        }
    }
}
pub unsafe fn disable(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state &= !GA_Enabled;
        }
    }
}
pub unsafe fn isenabled(c: control) -> c_int {
    unsafe {
        if c.is_null() {
            0
        } else {
            if ((*c).state & GA_Enabled) != 0 { 1 } else { 0 }
        }
    }
}
pub unsafe fn check(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state |= GA_Checked;
        }
    }
}
pub unsafe fn uncheck(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state &= !GA_Checked;
        }
    }
}
pub unsafe fn ischecked(c: control) -> c_int {
    unsafe {
        if c.is_null() {
            0
        } else {
            if ((*c).state & GA_Checked) != 0 { 1 } else { 0 }
        }
    }
}
pub unsafe fn highlight(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state |= GA_Highlighted;
        }
    }
}
pub unsafe fn unhighlight(c: control) {
    unsafe {
        if !c.is_null() {
            (*c).state &= !GA_Highlighted;
        }
    }
}
pub unsafe fn ishighlighted(c: control) -> c_int {
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
pub unsafe fn flashcontrol(_c: control) { /* TODO */
}
pub unsafe fn activatecontrol(_c: control) { /* TODO */
}

pub unsafe fn settext(c: control, text: *const c_char) {
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
pub unsafe fn GA_gettext(c: control) -> *mut c_char {
    unsafe {
        if c.is_null() {
            ptr::null_mut()
        } else {
            (*c).text
        }
    }
}
pub unsafe fn gettextfont(c: control) -> font {
    if c.is_null() {
        ptr::null_mut()
    } else {
        ptr::null_mut()
    }
}
pub unsafe fn settextfont(_c: control, _f: font) { /* TODO */
}
pub unsafe fn setforeground(c: control, fg: rgb) {
    unsafe {
        if !c.is_null() {
            (*c).fg = fg;
        }
    }
}
pub unsafe fn getforeground(c: control) -> rgb {
    unsafe { if c.is_null() { Black } else { (*c).fg } }
}
pub unsafe fn setbackground(c: control, bg: rgb) {
    unsafe {
        if !c.is_null() {
            (*c).bg = bg;
        }
    }
}
pub unsafe fn getbackground(c: control) -> rgb {
    unsafe { if c.is_null() { Transparent } else { (*c).bg } }
}
pub unsafe fn setvalue(c: control, value: c_int) {
    unsafe {
        if !c.is_null() {
            (*c).value = value;
        }
    }
}
pub unsafe fn getvalue(c: control) -> c_int {
    unsafe { if c.is_null() { 0 } else { (*c).value } }
}
pub unsafe fn setdata(c: control, data: *mut c_void) {
    unsafe {
        if !c.is_null() {
            (*c).data = data;
        }
    }
}
pub unsafe fn getdata(c: control) -> *mut c_void {
    unsafe {
        if c.is_null() {
            ptr::null_mut()
        } else {
            (*c).data
        }
    }
}
pub unsafe fn parentwindow(c: control) -> window {
    unsafe {
        if c.is_null() {
            ptr::null_mut()
        } else {
            (*c).parent
        }
    }
}

pub unsafe fn newcontrol(_text: *const c_char, _r: rect) -> control {
    ptr::null_mut() // TODO: Platform-specific
}
pub unsafe fn newdrawing(_r: rect, _fn_: drawfn) -> drawing {
    ptr::null_mut() // TODO: Platform-specific
}
pub unsafe fn newpicture(_img: image, _r: rect) -> drawing {
    ptr::null_mut() // TODO: Platform-specific
}
pub unsafe fn newbutton(_text: *const c_char, _r: rect, _fn_: actionfn) -> button {
    ptr::null_mut() // TODO
}
pub unsafe fn newimagebutton(_img: image, _r: rect, _fn_: actionfn) -> button {
    ptr::null_mut() // TODO
}
pub unsafe fn setimage(_c: control, _img: image) { /* TODO */
}
pub unsafe fn newcheckbox(_text: *const c_char, _r: rect, _fn_: actionfn) -> checkbox {
    ptr::null_mut() // TODO
}
pub unsafe fn newimagecheckbox(_img: image, _r: rect, _fn_: actionfn) -> checkbox {
    ptr::null_mut() // TODO
}
pub unsafe fn newradiobutton(
    _text: *const c_char,
    _r: rect,
    _fn_: actionfn,
) -> radiobutton {
    ptr::null_mut() // TODO
}
pub unsafe fn newradiogroup() -> radiogroup {
    ptr::null_mut() // TODO
}
pub unsafe fn newscrollbar(
    _r: rect,
    _max: c_int,
    _pagesize: c_int,
    _fn_: scrollfn,
) -> scrollbar {
    ptr::null_mut() // TODO
}
pub unsafe fn changescrollbar(_s: scrollbar, _where_: c_int, _max: c_int, _size: c_int) {
    /* TODO */
}
pub unsafe fn newlabel(_text: *const c_char, _r: rect, _alignment: c_int) -> label {
    ptr::null_mut() // TODO
}
pub unsafe fn newfield(_text: *const c_char, _r: rect) -> field {
    ptr::null_mut() // TODO
}
pub unsafe fn newpassword(_text: *const c_char, _r: rect) -> field {
    ptr::null_mut() // TODO
}
pub unsafe fn newtextbox(_text: *const c_char, _r: rect) -> textbox {
    ptr::null_mut() // TODO
}
pub unsafe fn newtextarea(_text: *const c_char, _r: rect) -> textbox {
    ptr::null_mut() // TODO
}
pub unsafe fn newrichtextarea(_text: *const c_char, _r: rect) -> textbox {
    ptr::null_mut() // TODO
}
pub unsafe fn newlistbox(
    _list: *const *const c_char,
    _r: rect,
    _fn_: scrollfn,
    _dble: actionfn,
) -> listbox {
    ptr::null_mut() // TODO
}
pub unsafe fn newdroplist(
    _list: *const *const c_char,
    _r: rect,
    _fn_: scrollfn,
) -> listbox {
    ptr::null_mut() // TODO
}
pub unsafe fn newdropfield(
    _list: *const *const c_char,
    _r: rect,
    _fn_: scrollfn,
) -> listbox {
    ptr::null_mut() // TODO
}
pub unsafe fn newmultilist(
    _list: *const *const c_char,
    _r: rect,
    _fn_: scrollfn,
    _dble: actionfn,
) -> listbox {
    ptr::null_mut() // TODO
}
pub unsafe fn isselected(_b: listbox, _index: c_int) -> c_int {
    0
}
pub unsafe fn setlistitem(_b: listbox, _index: c_int) { /* TODO */
}
pub unsafe fn getlistitem(_b: listbox) -> c_int {
    0
}
pub unsafe fn changelistbox(_b: listbox, _list: *const *const c_char) {
    /* TODO */
}
pub unsafe fn newprogressbar(
    _r: rect,
    _pmin: c_int,
    _pmax: c_int,
    _incr: c_int,
    _smooth: c_int,
) -> progressbar {
    ptr::null_mut() // TODO
}
pub unsafe fn setprogressbar(_obj: progressbar, _n: c_int) { /* TODO */
}
pub unsafe fn stepprogressbar(_obj: progressbar, _n: c_int) { /* TODO */
}
pub unsafe fn setprogressbarrange(_obj: progressbar, _pbmin: c_int, _pbmax: c_int) {
    /* TODO */
}
pub unsafe fn newmenubar(_fn_: actionfn) -> menubar {
    ptr::null_mut()
}
pub unsafe fn newsubmenu(_parent: menu, _name: *const c_char) -> menu {
    ptr::null_mut()
}
pub unsafe fn newmenu(_name: *const c_char) -> menu {
    ptr::null_mut()
}
pub unsafe fn newmenuitem(_name: *const c_char, _key: c_int, _fn_: menufn) -> menuitem {
    ptr::null_mut()
}

// Text editing
pub unsafe fn undotext(_t: textbox) { /* TODO */
}
pub unsafe fn cuttext(_t: textbox) { /* TODO */
}
pub unsafe fn copytext(_t: textbox) { /* TODO */
}
pub unsafe fn cleartext(_t: textbox) { /* TODO */
}
pub unsafe fn pastetext(_t: textbox) { /* TODO */
}
pub unsafe fn inserttext(_t: textbox, _text: *const c_char) { /* TODO */
}
pub unsafe fn selecttext(_t: textbox, _start: c_long, _end: c_long) {
    /* TODO */
}
pub unsafe fn textselection(_t: textbox, _start: *mut c_long, _end: *mut c_long) {
    /* TODO */
}

// Font functions
pub unsafe fn newfont(_name: *const c_char, _style: c_int, _size: c_int) -> font {
    ptr::null_mut() // TODO: Platform-specific
}
pub unsafe fn fontwidth(_f: font) -> c_int {
    0
}
pub unsafe fn fontheight(_f: font) -> c_int {
    0
}
pub unsafe fn fontascent(_f: font) -> c_int {
    0
}
pub unsafe fn fontdescent(_f: font) -> c_int {
    0
}

// Height alias
pub unsafe fn getheight(f: font) -> c_int {
    unsafe { fontheight(f) }
}
pub unsafe fn getdescent(f: font) -> c_int {
    unsafe { fontdescent(f) }
}

// Image control
pub unsafe fn newbitmap(_width: c_int, _height: c_int, _depth: c_int) -> bitmap {
    ptr::null_mut() // TODO: Platform-specific
}
pub unsafe fn loadbitmap(_name: *const c_char) -> bitmap {
    ptr::null_mut() // TODO
}
pub unsafe fn imagetobitmap(_img: image) -> bitmap {
    ptr::null_mut() // TODO
}
pub unsafe fn createbitmap(
    _width: c_int,
    _height: c_int,
    _depth: c_int,
    _data: *mut GAbyte,
) -> bitmap {
    ptr::null_mut() // TODO
}
pub unsafe fn setbitmapdata(_b: bitmap, _data: *mut GAbyte) { /* TODO */
}
pub unsafe fn getbitmapdata(_b: bitmap, _data: *mut GAbyte) { /* TODO */
}
pub unsafe fn getbitmapdata2(_b: bitmap, _data: *mut *mut GAbyte) { /* TODO */
}

// Cursor functions
pub unsafe fn newcursor(_hotspot: point, _img: image) -> cursor {
    ptr::null_mut() // TODO
}
pub unsafe fn createcursor(
    _offset: point,
    _white_mask: *mut GAbyte,
    _black_shape: *mut GAbyte,
) -> cursor {
    ptr::null_mut() // TODO
}
pub unsafe fn loadcursor(_name: *const c_char) -> cursor {
    ptr::null_mut() // TODO
}

// Image load/save
pub unsafe fn loadimage(_filename: *const c_char) -> image {
    ptr::null_mut()
}
pub unsafe fn saveimage(_img: image, _filename: *const c_char) { /* TODO */
}
