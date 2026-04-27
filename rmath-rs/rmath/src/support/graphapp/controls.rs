#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Control management for GraphApp.
//!
//! Ported from controls.c - manipulating buttons, text fields,
//! scrollbars, checkboxes, and other controls.

use std::ffi::CStr;
use std::fs::File;
use std::io::Write;
use std::os::raw::{c_char, c_int, c_long, c_void};
use std::ptr;

use super::{image as image_api, memory, objects, strings, windows};
use super::types::*;

fn first_list_item(list: *const *const c_char) -> *const c_char {
    if list.is_null() {
        ptr::null()
    } else {
        unsafe { *list }
    }
}

fn list_length(list: *const *const c_char) -> c_int {
    if list.is_null() {
        return 0;
    }
    let mut len = 0;
    loop {
        let item = unsafe { *list.add(len as usize) };
        if item.is_null() {
            break;
        }
        len += 1;
    }
    len
}

unsafe fn alloc_drawstate(dest: drawing) -> drawstate {
    let state = unsafe { memory::memalloc(std::mem::size_of::<drawstruct>() as i64) as drawstate };
    if state.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::write_bytes(state as *mut u8, 0, std::mem::size_of::<drawstruct>());
        (*state).dest = dest;
        (*state).hue = Black;
    }
    state
}

unsafe fn new_control_object(kind: c_int, text: *const c_char, r: rect, with_drawstate: bool) -> control {
    unsafe {
        objects::init_objects();
        let parent = windows::get_current_window();
        let obj = objects::new_object(kind, ptr::null_mut(), parent);
        if obj.is_null() {
            return ptr::null_mut();
        }
        (*obj).rect = r;
        (*obj).state = GA_Visible | GA_Enabled;
        (*obj).fg = Black;
        (*obj).bg = White;
        (*obj).text = strings::new_string(text);
        if with_drawstate {
            (*obj).drawstate = alloc_drawstate(obj);
        }
        obj
    }
}

unsafe fn new_bitmap_object(width: c_int, height: c_int, depth: c_int) -> bitmap {
    unsafe {
        let bitmap = new_control_object(
            BitmapObject,
            ptr::null(),
            rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            false,
        );
        if bitmap.is_null() {
            return ptr::null_mut();
        }
        let normalized_depth = if depth <= 8 { 8 } else { 32 };
        (*bitmap).depth = normalized_depth;
        (*bitmap).img = image_api::newimage(width.max(0), height.max(0), normalized_depth);
        bitmap
    }
}

unsafe fn set_bitmap_pixels(bitmap: bitmap, data: *mut GAbyte) {
    unsafe {
        if bitmap.is_null() || (*bitmap).img.is_null() || data.is_null() {
            return;
        }
        image_api::setpixels((*bitmap).img, data);
    }
}

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

pub unsafe fn clear(c: control) {
    unsafe {
        draw(c);
    }
}
pub unsafe fn draw(c: control) {
    unsafe {
        if c.is_null() || ((*c).state & GA_Visible) == 0 || (*c).call.is_null() {
            return;
        }
        if let Some(redraw) = (*(*c).call).redraw {
            redraw(c, (*c).rect);
        }
    }
}
pub unsafe fn redraw(c: control) {
    unsafe {
        draw(c);
    }
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
pub unsafe fn flashcontrol(c: control) {
    unsafe {
        if c.is_null() || ((*c).state & GA_Enabled) == 0 {
            return;
        }

        let was_armed = ((*c).state & GA_Armed) != 0;
        (*c).state |= GA_Armed;
        redraw(c);
        if !was_armed {
            (*c).state &= !GA_Armed;
        }
        redraw(c);
    }
}
pub unsafe fn activatecontrol(c: control) {
    unsafe {
        if c.is_null() || ((*c).state & GA_Enabled) == 0 {
            return;
        }

        flashcontrol(c);

        if !(*c).call.is_null()
            && let Some(focus) = (*(*c).call).focus
        {
            focus(c);
        }
        if let Some(action) = (*c).action {
            action(c);
        }
    }
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
    unsafe {
        if c.is_null() || (*c).drawstate.is_null() {
            ptr::null_mut()
        } else {
            (*(*c).drawstate).fnt
        }
    }
}
pub unsafe fn settextfont(c: control, f: font) {
    unsafe {
        if !c.is_null() && !(*c).drawstate.is_null() {
            (*(*c).drawstate).fnt = f;
        }
    }
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

pub unsafe fn newcontrol(text: *const c_char, r: rect) -> control {
    unsafe { new_control_object(ControlObject, text, r, false) }
}
pub unsafe fn newdrawing(r: rect, fn_: drawfn) -> drawing {
    unsafe {
        let drawing = new_control_object(UserObject, ptr::null(), r, true);
        if !drawing.is_null() && !(*drawing).call.is_null() {
            (*(*drawing).call).redraw = fn_;
        }
        drawing
    }
}
pub unsafe fn newpicture(img: image, r: rect) -> drawing {
    unsafe {
        let picture = newdrawing(r, None);
        if !picture.is_null() {
            (*picture).img = img;
        }
        picture
    }
}
pub unsafe fn newbutton(text: *const c_char, r: rect, fn_: actionfn) -> button {
    unsafe {
        let button = new_control_object(ButtonObject, text, r, false);
        if !button.is_null() {
            (*button).action = fn_;
        }
        button
    }
}
pub unsafe fn newimagebutton(img: image, r: rect, fn_: actionfn) -> button {
    unsafe {
        let button = newbutton(ptr::null(), r, fn_);
        if !button.is_null() {
            (*button).img = img;
        }
        button
    }
}
pub unsafe fn setimage(c: control, img: image) {
    unsafe {
        if !c.is_null() {
            (*c).img = img;
        }
    }
}
pub unsafe fn newcheckbox(text: *const c_char, r: rect, fn_: actionfn) -> checkbox {
    unsafe {
        let checkbox = new_control_object(CheckboxObject, text, r, false);
        if !checkbox.is_null() {
            (*checkbox).action = fn_;
        }
        checkbox
    }
}
pub unsafe fn newimagecheckbox(img: image, r: rect, fn_: actionfn) -> checkbox {
    unsafe {
        let checkbox = newcheckbox(ptr::null(), r, fn_);
        if !checkbox.is_null() {
            (*checkbox).img = img;
        }
        checkbox
    }
}
pub unsafe fn newradiobutton(
    text: *const c_char,
    r: rect,
    fn_: actionfn,
) -> radiobutton {
    unsafe {
        let button = new_control_object(RadioObject, text, r, false);
        if !button.is_null() {
            (*button).action = fn_;
        }
        button
    }
}
pub unsafe fn newradiogroup() -> radiogroup {
    unsafe { new_control_object(RadiogroupObject, ptr::null(), rect::default(), false) }
}
pub unsafe fn newscrollbar(
    r: rect,
    max: c_int,
    pagesize: c_int,
    _fn_: scrollfn,
) -> scrollbar {
    unsafe {
        let scrollbar = new_control_object(ScrollbarObject, ptr::null(), r, false);
        if !scrollbar.is_null() {
            changescrollbar(scrollbar, 0, max, pagesize);
        }
        scrollbar
    }
}
pub unsafe fn changescrollbar(s: scrollbar, where_: c_int, max: c_int, size: c_int) {
    unsafe {
        if s.is_null() {
            return;
        }

        let max = max.max(0);
        let size = size.clamp(0, max);
        let limit = max.saturating_sub(size);

        (*s).max = max;
        (*s).size = size;
        (*s).value = where_.clamp(0, limit);
    }
}
pub unsafe fn newlabel(text: *const c_char, r: rect, alignment: c_int) -> label {
    unsafe {
        let label = new_control_object(LabelObject, text, r, false);
        if !label.is_null() {
            (*label).value = alignment;
        }
        label
    }
}
pub unsafe fn newfield(text: *const c_char, r: rect) -> field {
    unsafe { new_control_object(FieldObject, text, r, true) }
}
pub unsafe fn newpassword(text: *const c_char, r: rect) -> field {
    unsafe {
        let field = newfield(text, r);
        if !field.is_null() {
            (*field).flags |= 1;
        }
        field
    }
}
pub unsafe fn newtextbox(text: *const c_char, r: rect) -> textbox {
    unsafe { new_control_object(TextboxObject, text, r, true) }
}
pub unsafe fn newtextarea(text: *const c_char, r: rect) -> textbox {
    unsafe { newtextbox(text, r) }
}
pub unsafe fn newrichtextarea(text: *const c_char, r: rect) -> textbox {
    unsafe { newtextbox(text, r) }
}
pub unsafe fn newlistbox(
    list: *const *const c_char,
    r: rect,
    _fn_: scrollfn,
    dble: actionfn,
) -> listbox {
    unsafe {
        let listbox = new_control_object(ListboxObject, first_list_item(list), r, false);
        if !listbox.is_null() {
            (*listbox).max = list_length(list);
            (*listbox).value = -1;
            (*listbox).dble = dble;
        }
        listbox
    }
}
pub unsafe fn newdroplist(
    list: *const *const c_char,
    r: rect,
    fn_: scrollfn,
) -> listbox {
    unsafe {
        let listbox = newlistbox(list, r, fn_, None);
        if !listbox.is_null() {
            (*listbox).kind = DroplistObject;
        }
        listbox
    }
}
pub unsafe fn newdropfield(
    list: *const *const c_char,
    r: rect,
    fn_: scrollfn,
) -> listbox {
    unsafe {
        let listbox = newlistbox(list, r, fn_, None);
        if !listbox.is_null() {
            (*listbox).kind = DropfieldObject;
        }
        listbox
    }
}
pub unsafe fn newmultilist(
    list: *const *const c_char,
    r: rect,
    fn_: scrollfn,
    dble: actionfn,
) -> listbox {
    unsafe {
        let listbox = newlistbox(list, r, fn_, dble);
        if !listbox.is_null() {
            (*listbox).kind = MultilistObject;
        }
        listbox
    }
}
pub unsafe fn isselected(b: listbox, index: c_int) -> c_int {
    unsafe { if !b.is_null() && (*b).value == index { 1 } else { 0 } }
}
pub unsafe fn setlistitem(b: listbox, index: c_int) {
    unsafe {
        if !b.is_null() {
            (*b).value = index.max(-1);
        }
    }
}
pub unsafe fn getlistitem(b: listbox) -> c_int {
    unsafe {
        if b.is_null() {
            -1
        } else {
            (*b).value
        }
    }
}
pub unsafe fn changelistbox(b: listbox, list: *const *const c_char) {
    unsafe {
        if b.is_null() {
            return;
        }
        settext(b, first_list_item(list));
        (*b).max = list_length(list);
        (*b).value = -1;
    }
}
pub unsafe fn newprogressbar(
    r: rect,
    pmin: c_int,
    pmax: c_int,
    incr: c_int,
    smooth: c_int,
) -> progressbar {
    unsafe {
        let progress = new_control_object(ProgressbarObject, ptr::null(), r, false);
        if !progress.is_null() {
            setprogressbarrange(progress, pmin, pmax);
            (*progress).data = incr as usize as *mut c_void;
            (*progress).flags = smooth as c_long;
        }
        progress
    }
}
pub unsafe fn setprogressbar(obj: progressbar, n: c_int) {
    unsafe {
        if obj.is_null() {
            return;
        }

        let min = (*obj).size.min((*obj).max);
        let max = (*obj).max.max((*obj).size);
        (*obj).value = n.clamp(min, max);
    }
}
pub unsafe fn stepprogressbar(obj: progressbar, n: c_int) {
    unsafe {
        if !obj.is_null() {
            setprogressbar(obj, (*obj).value.saturating_add(n));
        }
    }
}
pub unsafe fn setprogressbarrange(obj: progressbar, pbmin: c_int, pbmax: c_int) {
    unsafe {
        if obj.is_null() {
            return;
        }

        let (min, max) = if pbmin <= pbmax {
            (pbmin, pbmax)
        } else {
            (pbmax, pbmin)
        };

        (*obj).size = min;
        (*obj).max = max;
        (*obj).value = (*obj).value.clamp(min, max);
    }
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
pub unsafe fn undotext(_t: textbox) {}
pub unsafe fn cuttext(_t: textbox) {}
pub unsafe fn copytext(_t: textbox) {}
pub unsafe fn cleartext(t: textbox) {
    unsafe {
        if !t.is_null() {
            settext(t, ptr::null());
            selecttext(t, 0, 0);
        }
    }
}
pub unsafe fn pastetext(_t: textbox) {}
pub unsafe fn inserttext(t: textbox, text: *const c_char) {
    unsafe {
        if t.is_null() || text.is_null() {
            return;
        }

        let old_text = (*t).text;
        let combined = if old_text.is_null() {
            super::strings::new_string(text)
        } else {
            super::strings::add_strings(old_text, text)
        };
        if combined.is_null() {
            return;
        }
        if !old_text.is_null() {
            super::strings::del_string(old_text);
        }
        (*t).text = combined;

        let end = super::strings::string_length((*t).text);
        selecttext(t, end, end);
    }
}
pub unsafe fn selecttext(_t: textbox, _start: c_long, _end: c_long) {
    unsafe {
        if _t.is_null() {
            return;
        }

        let start = _start.max(0);
        let end = _end.max(0);
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };

        (*_t).caretx = start as c_int;
        (*_t).carety = end as c_int;
    }
}
pub unsafe fn textselection(_t: textbox, _start: *mut c_long, _end: *mut c_long) {
    unsafe {
        let selection = if _t.is_null() {
            (0, 0)
        } else {
            ((*_t).caretx as c_long, (*_t).carety as c_long)
        };

        if !_start.is_null() {
            *_start = selection.0;
        }
        if !_end.is_null() {
            *_end = selection.1;
        }
    }
}

// Font functions
pub unsafe fn newfont(name: *const c_char, style: c_int, size: c_int) -> font {
    unsafe {
        objects::init_objects();
        let font = objects::new_object(FontObject, ptr::null_mut(), ptr::null_mut());
        if font.is_null() {
            return ptr::null_mut();
        }
        (*font).text = strings::new_string(name);
        (*font).value = size.max(1);
        (*font).flags = style as c_long;
        font
    }
}
pub unsafe fn fontwidth(f: font) -> c_int {
    unsafe {
        if f.is_null() {
            0
        } else if ((*f).flags & FixedWidth as c_long) != 0 {
            ((*f).value / 2).max(1)
        } else {
            ((*f).value * 3 / 5).max(1)
        }
    }
}
pub unsafe fn fontheight(f: font) -> c_int {
    unsafe { if f.is_null() { 0 } else { (*f).value.max(1) } }
}
pub unsafe fn fontascent(f: font) -> c_int {
    let height = unsafe { fontheight(f) };
    (height * 3) / 4
}
pub unsafe fn fontdescent(f: font) -> c_int {
    unsafe { fontheight(f).saturating_sub(fontascent(f)) }
}

// Height alias
pub unsafe fn getheight(f: font) -> c_int {
    unsafe { fontheight(f) }
}
pub unsafe fn getdescent(f: font) -> c_int {
    unsafe { fontdescent(f) }
}

// Image control
pub unsafe fn newbitmap(width: c_int, height: c_int, depth: c_int) -> bitmap {
    unsafe { new_bitmap_object(width, height, depth) }
}
pub unsafe fn loadbitmap(name: *const c_char) -> bitmap {
    unsafe {
        let img = loadimage(name);
        imagetobitmap(img)
    }
}
pub unsafe fn imagetobitmap(img: image) -> bitmap {
    unsafe {
        if img.is_null() {
            return ptr::null_mut();
        }
        let bitmap = new_bitmap_object((*img).width, (*img).height, (*img).depth);
        if !bitmap.is_null() {
            image_api::delimage((*bitmap).img);
            (*bitmap).img = image_api::copyimage(img);
        }
        bitmap
    }
}
pub unsafe fn createbitmap(
    width: c_int,
    height: c_int,
    depth: c_int,
    data: *mut GAbyte,
) -> bitmap {
    unsafe {
        let bitmap = new_bitmap_object(width, height, depth);
        set_bitmap_pixels(bitmap, data);
        bitmap
    }
}
pub unsafe fn setbitmapdata(b: bitmap, data: *mut GAbyte) {
    unsafe { set_bitmap_pixels(b, data) }
}
pub unsafe fn getbitmapdata(b: bitmap, data: *mut GAbyte) {
    unsafe {
        if b.is_null() || (*b).img.is_null() || data.is_null() {
            return;
        }
        let pixels = image_api::getpixels((*b).img);
        if pixels.is_null() {
            return;
        }
        let pixel_count = ((*(*b).img).width.max(0) * (*(*b).img).height.max(0)) as usize;
        let byte_len = if (*(*b).img).depth <= 8 {
            pixel_count
        } else {
            pixel_count * std::mem::size_of::<rgb>()
        };
        ptr::copy_nonoverlapping(pixels, data, byte_len);
    }
}
pub unsafe fn getbitmapdata2(b: bitmap, data: *mut *mut GAbyte) {
    unsafe {
        if b.is_null() || (*b).img.is_null() || data.is_null() {
            return;
        }
        *data = image_api::getpixels((*b).img);
    }
}

// Cursor functions
pub unsafe fn newcursor(hotspot: point, img: image) -> cursor {
    unsafe {
        let cursor = new_control_object(CursorObject, ptr::null(), rect::default(), false);
        if !cursor.is_null() {
            (*cursor).rect.x = hotspot.x;
            (*cursor).rect.y = hotspot.y;
            (*cursor).img = img;
        }
        cursor
    }
}
pub unsafe fn createcursor(
    offset: point,
    _white_mask: *mut GAbyte,
    _black_shape: *mut GAbyte,
) -> cursor {
    unsafe { newcursor(offset, ptr::null_mut()) }
}
pub unsafe fn loadcursor(name: *const c_char) -> cursor {
    unsafe {
        let cursor = newcursor(point::default(), ptr::null_mut());
        if !cursor.is_null() {
            settext(cursor, name);
        }
        cursor
    }
}

// Image load/save
pub unsafe fn loadimage(_filename: *const c_char) -> image {
    ptr::null_mut()
}
pub unsafe fn saveimage(img: image, filename: *const c_char) {
    unsafe {
        if img.is_null() || filename.is_null() {
            return;
        }
        let Ok(path) = CStr::from_ptr(filename).to_str() else {
            return;
        };
        let Ok(mut file) = File::create(path) else {
            return;
        };
        let width = (*img).width.max(0);
        let height = (*img).height.max(0);

        if (*img).depth <= 8 {
            let _ = writeln!(file, "P5\n{} {}\n255", width, height);
            let pixels = image_api::getpixels(img);
            if !pixels.is_null() {
                let bytes = std::slice::from_raw_parts(pixels, (width * height) as usize);
                let _ = file.write_all(bytes);
            }
            return;
        }

        let _ = writeln!(file, "P6\n{} {}\n255", width, height);
        for y in 0..height {
            for x in 0..width {
                let pixel = image_api::get_image_pixel(img, x, y);
                let bytes = [getred(pixel) as u8, getgreen(pixel) as u8, getblue(pixel) as u8];
                let _ = file.write_all(&bytes);
            }
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

    unsafe extern "C" fn record_action(_c: control) {
        ACTION_CALLS.with(|calls| calls.set(calls.get() + 1));
    }

    unsafe extern "C" fn record_focus(_c: control) {
        FOCUS_CALLS.with(|calls| calls.set(calls.get() + 1));
    }

    unsafe extern "C" fn record_redraw(_c: control, _r: rect) {
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
        unsafe {
            let (mut control, mut call) = make_control();
            let control = &mut *control as control;

            REDRAW_CALLS.with(|calls| calls.set(0));
            call.redraw = Some(record_redraw);

            draw(control);
            REDRAW_CALLS.with(|calls| assert_eq!(calls.get(), 0));

            show_control(control);
            draw(control);
            REDRAW_CALLS.with(|calls| assert_eq!(calls.get(), 1));
        }
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

            assert_eq!(CStr::from_ptr(GA_gettext(control)).to_bytes(), b"hello world");
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
