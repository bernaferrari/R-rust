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
pub unsafe extern "C" fn clear(c: control) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn draw(c: control) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn redraw(c: control) { /* TODO */
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
pub unsafe extern "C" fn flashcontrol(c: control) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn activatecontrol(c: control) { /* TODO */
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
pub unsafe extern "C" fn settextfont(c: control, f: font) { /* TODO */
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
pub unsafe extern "C" fn newcontrol(text: *const c_char, r: rect) -> control {
    ptr::null_mut() // TODO: Platform-specific
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newdrawing(r: rect, fn_: drawfn) -> drawing {
    ptr::null_mut() // TODO: Platform-specific
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newpicture(img: image, r: rect) -> drawing {
    ptr::null_mut() // TODO: Platform-specific
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newbutton(text: *const c_char, r: rect, fn_: actionfn) -> button {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newimagebutton(img: image, r: rect, fn_: actionfn) -> button {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setimage(c: control, img: image) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newcheckbox(text: *const c_char, r: rect, fn_: actionfn) -> checkbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newimagecheckbox(img: image, r: rect, fn_: actionfn) -> checkbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newradiobutton(
    text: *const c_char,
    r: rect,
    fn_: actionfn,
) -> radiobutton {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newradiogroup() -> radiogroup {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newscrollbar(
    r: rect,
    max: c_int,
    pagesize: c_int,
    fn_: scrollfn,
) -> scrollbar {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn changescrollbar(s: scrollbar, where_: c_int, max: c_int, size: c_int) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newlabel(text: *const c_char, r: rect, alignment: c_int) -> label {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newfield(text: *const c_char, r: rect) -> field {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newpassword(text: *const c_char, r: rect) -> field {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newtextbox(text: *const c_char, r: rect) -> textbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newtextarea(text: *const c_char, r: rect) -> textbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newrichtextarea(text: *const c_char, r: rect) -> textbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newlistbox(
    list: *const *const c_char,
    r: rect,
    fn_: scrollfn,
    dble: actionfn,
) -> listbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newdroplist(
    list: *const *const c_char,
    r: rect,
    fn_: scrollfn,
) -> listbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newdropfield(
    list: *const *const c_char,
    r: rect,
    fn_: scrollfn,
) -> listbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newmultilist(
    list: *const *const c_char,
    r: rect,
    fn_: scrollfn,
    dble: actionfn,
) -> listbox {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn isselected(b: listbox, index: c_int) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setlistitem(b: listbox, index: c_int) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getlistitem(b: listbox) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn changelistbox(b: listbox, list: *const *const c_char) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newprogressbar(
    r: rect,
    pmin: c_int,
    pmax: c_int,
    incr: c_int,
    smooth: c_int,
) -> progressbar {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setprogressbar(obj: progressbar, n: c_int) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn stepprogressbar(obj: progressbar, n: c_int) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setprogressbarrange(obj: progressbar, pbmin: c_int, pbmax: c_int) {
    /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newmenubar(fn_: actionfn) -> menubar {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newsubmenu(parent: menu, name: *const c_char) -> menu {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newmenu(name: *const c_char) -> menu {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newmenuitem(name: *const c_char, key: c_int, fn_: menufn) -> menuitem {
    ptr::null_mut()
}

// Text editing
#[unsafe(no_mangle)]
pub unsafe extern "C" fn undotext(t: textbox) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cuttext(t: textbox) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn copytext(t: textbox) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cleartext(t: textbox) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pastetext(t: textbox) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn inserttext(t: textbox, text: *const c_char) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn selecttext(t: textbox, start: c_long, end: c_long) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn textselection(t: textbox, start: *mut c_long, end: *mut c_long) {
    /* TODO */
}

// Font functions
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newfont(name: *const c_char, style: c_int, size: c_int) -> font {
    ptr::null_mut() // TODO: Platform-specific
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fontwidth(f: font) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fontheight(f: font) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fontascent(f: font) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fontdescent(f: font) -> c_int {
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
pub unsafe extern "C" fn newbitmap(width: c_int, height: c_int, depth: c_int) -> bitmap {
    ptr::null_mut() // TODO: Platform-specific
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loadbitmap(name: *const c_char) -> bitmap {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imagetobitmap(img: image) -> bitmap {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createbitmap(
    width: c_int,
    height: c_int,
    depth: c_int,
    data: *mut GAbyte,
) -> bitmap {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setbitmapdata(b: bitmap, data: *mut GAbyte) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getbitmapdata(b: bitmap, data: *mut GAbyte) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getbitmapdata2(b: bitmap, data: *mut *mut GAbyte) { /* TODO */
}

// Cursor functions
#[unsafe(no_mangle)]
pub unsafe extern "C" fn newcursor(hotspot: point, img: image) -> cursor {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createcursor(
    offset: point,
    white_mask: *mut GAbyte,
    black_shape: *mut GAbyte,
) -> cursor {
    ptr::null_mut() // TODO
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loadcursor(name: *const c_char) -> cursor {
    ptr::null_mut() // TODO
}

// Image load/save
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loadimage(filename: *const c_char) -> image {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn saveimage(img: image, filename: *const c_char) { /* TODO */
}
