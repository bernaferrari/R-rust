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

fn first_list_item(list: *const *const c_char) -> *const c_char {
    ptr_ref(list).copied().unwrap_or(ptr::null())
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

fn alloc_drawstate(dest: drawing) -> drawstate {
    let state = memory::memalloc_bytes(std::mem::size_of::<drawstruct>()) as drawstate;
    if let Some(state) = ptr_mut(state) {
        *state = drawstruct {
            dest,
            hue: Black,
            ..Default::default()
        };
    }
    state
}

fn new_control_object(kind: c_int, text: *const c_char, r: rect, with_drawstate: bool) -> control {
    objects::init_objects();
    let parent = windows::get_current_window();
    let obj = objects::new_object(kind, ptr::null_mut(), parent);
    let Some(obj) = obj_mut(obj) else {
        return ptr::null_mut();
    };
    obj.rect = r;
    obj.state = GA_Visible | GA_Enabled;
    obj.fg = Black;
    obj.bg = White;
    obj.text = strings::new_string(text);
    if with_drawstate {
        obj.drawstate = alloc_drawstate(obj);
    }
    obj
}

fn new_bitmap_object(width: c_int, height: c_int, depth: c_int) -> bitmap {
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
    let Some(bitmap) = obj_mut(bitmap) else {
        return ptr::null_mut();
    };
    let normalized_depth = if depth <= 8 { 8 } else { 32 };
    bitmap.depth = normalized_depth;
    bitmap.img = image_api::newimage(width.max(0), height.max(0), normalized_depth);
    bitmap
}

fn set_bitmap_pixels(bitmap: bitmap, data: *mut GAbyte) {
    let Some(img) = obj_ref(bitmap).map(|b| b.img) else {
        return;
    };
    if img.is_null() || data.is_null() {
        return;
    }
    image_api::setpixels(img, data);
}

pub fn setaction(c: control, fn_: actionfn) {
    if let Some(c) = obj_mut(c) {
        c.action = fn_;
    }
}
pub fn sethit(c: control, fn_: intfn) {
    if let Some(c) = obj_mut(c) {
        c.hit = fn_;
    }
}
pub fn setdel(c: control, fn_: actionfn) {
    if let Some(call) = obj_ref(c).and_then(|c| ptr_mut(c.call)) {
        call.die = fn_;
    }
}
pub fn setclose(c: control, fn_: actionfn) {
    if let Some(call) = obj_ref(c).and_then(|c| ptr_mut(c.call)) {
        call.close = fn_;
    }
}
pub fn setredraw(c: control, fn_: drawfn) {
    if let Some(call) = obj_ref(c).and_then(|c| ptr_mut(c.call)) {
        call.redraw = fn_;
    }
}
pub fn setresize(c: control, fn_: drawfn) {
    if let Some(call) = obj_ref(c).and_then(|c| ptr_mut(c.call)) {
        call.resize = fn_;
    }
}
pub fn setkeydown(c: control, fn_: keyfn) {
    if let Some(call) = obj_ref(c).and_then(|c| ptr_mut(c.call)) {
        call.keydown = fn_;
    }
}
pub fn setkeyaction(c: control, fn_: keyfn) {
    if let Some(call) = obj_ref(c).and_then(|c| ptr_mut(c.call)) {
        call.keyaction = fn_;
    }
}
pub fn setmousedown(c: control, fn_: mousefn) {
    if let Some(call) = obj_ref(c).and_then(|c| ptr_mut(c.call)) {
        call.mousedown = fn_;
    }
}
pub fn setmousedrag(c: control, fn_: mousefn) {
    if let Some(call) = obj_ref(c).and_then(|c| ptr_mut(c.call)) {
        call.mousedrag = fn_;
    }
}
pub fn setmouseup(c: control, fn_: mousefn) {
    if let Some(call) = obj_ref(c).and_then(|c| ptr_mut(c.call)) {
        call.mouseup = fn_;
    }
}
pub fn setmousemove(c: control, fn_: mousefn) {
    if let Some(call) = obj_ref(c).and_then(|c| ptr_mut(c.call)) {
        call.mousemove = fn_;
    }
}
pub fn setmouserepeat(c: control, fn_: mousefn) {
    if let Some(call) = obj_ref(c).and_then(|c| ptr_mut(c.call)) {
        call.mouserepeat = fn_;
    }
}
pub fn setdrop(c: control, fn_: dropfn) {
    if let Some(call) = obj_ref(c).and_then(|c| ptr_mut(c.call)) {
        call.drop = fn_;
    }
}
pub fn setonfocus(c: control, fn_: actionfn) {
    if let Some(call) = obj_ref(c).and_then(|c| ptr_mut(c.call)) {
        call.focus = fn_;
    }
}
pub fn setim(c: control, fn_: imfn) {
    if let Some(call) = obj_ref(c).and_then(|c| ptr_mut(c.call)) {
        call.im = fn_;
    }
}

pub fn clear(c: control) {
    draw(c);
}
pub fn draw(c: control) {
    let Some(obj) = obj_ref(c) else { return };
    if (obj.state & GA_Visible) == 0 {
        return;
    }
    if let Some(redraw) = ptr_ref(obj.call).and_then(|call| call.redraw) {
        redraw(c, obj.rect);
    }
}
pub fn redraw(c: control) {
    draw(c);
}
pub fn resize(c: control, r: rect) {
    if let Some(c) = obj_mut(c) {
        c.rect = r;
    }
}
pub fn show_control(c: control) {
    if let Some(c) = obj_mut(c) {
        c.state |= GA_Visible;
    }
}
pub fn hide_control(c: control) {
    if let Some(c) = obj_mut(c) {
        c.state &= !GA_Visible;
    }
}
pub fn isvisible(c: control) -> c_int {
    obj_ref(c).map_or(0, |c| if (c.state & GA_Visible) != 0 { 1 } else { 0 })
}
pub fn enable(c: control) {
    if let Some(c) = obj_mut(c) {
        c.state |= GA_Enabled;
    }
}
pub fn disable(c: control) {
    if let Some(c) = obj_mut(c) {
        c.state &= !GA_Enabled;
    }
}
pub fn isenabled(c: control) -> c_int {
    obj_ref(c).map_or(0, |c| if (c.state & GA_Enabled) != 0 { 1 } else { 0 })
}
pub fn check(c: control) {
    if let Some(c) = obj_mut(c) {
        c.state |= GA_Checked;
    }
}
pub fn uncheck(c: control) {
    if let Some(c) = obj_mut(c) {
        c.state &= !GA_Checked;
    }
}
pub fn ischecked(c: control) -> c_int {
    obj_ref(c).map_or(0, |c| if (c.state & GA_Checked) != 0 { 1 } else { 0 })
}
pub fn highlight(c: control) {
    if let Some(c) = obj_mut(c) {
        c.state |= GA_Highlighted;
    }
}
pub fn unhighlight(c: control) {
    if let Some(c) = obj_mut(c) {
        c.state &= !GA_Highlighted;
    }
}
pub fn ishighlighted(c: control) -> c_int {
    obj_ref(c).map_or(0, |c| if (c.state & GA_Highlighted) != 0 { 1 } else { 0 })
}
pub fn flashcontrol(c: control) {
    let Some(state) = obj_ref(c).map(|obj| obj.state) else {
        return;
    };
    if (state & GA_Enabled) == 0 {
        return;
    }

    let was_armed = (state & GA_Armed) != 0;
    if let Some(obj) = obj_mut(c) {
        obj.state |= GA_Armed;
    }
    redraw(c);
    if !was_armed {
        if let Some(obj) = obj_mut(c) {
            obj.state &= !GA_Armed;
        }
    }
    redraw(c);
}
pub fn activatecontrol(c: control) {
    let Some(state) = obj_ref(c).map(|obj| obj.state) else {
        return;
    };
    if (state & GA_Enabled) == 0 {
        return;
    }

    flashcontrol(c);

    let focus = obj_ref(c)
        .and_then(|obj| ptr_ref(obj.call))
        .and_then(|call| call.focus);
    if let Some(focus) = focus {
        focus(c);
    }
    if let Some(action) = obj_ref(c).and_then(|obj| obj.action) {
        action(c);
    }
}

pub fn settext(c: control, text: *const c_char) {
    let Some(c) = obj_mut(c) else { return };
    if !c.text.is_null() {
        super::memory::memfree(c.text as *mut u8);
    }
    c.text = super::strings::new_string(text);
}
pub fn GA_gettext(c: control) -> *mut c_char {
    obj_ref(c).map_or(ptr::null_mut(), |c| c.text)
}
#[allow(clippy::if_same_then_else)]
pub fn gettextfont(c: control) -> font {
    obj_ref(c)
        .and_then(|c| ptr_ref(c.drawstate))
        .map_or(ptr::null_mut(), |ds| ds.fnt)
}
pub fn settextfont(c: control, f: font) {
    if let Some(ds) = obj_ref(c).and_then(|c| ptr_mut(c.drawstate)) {
        ds.fnt = f;
    }
}
pub fn setforeground(c: control, fg: rgb) {
    if let Some(c) = obj_mut(c) {
        c.fg = fg;
    }
}
pub fn getforeground(c: control) -> rgb {
    obj_ref(c).map_or(Black, |c| c.fg)
}
pub fn setbackground(c: control, bg: rgb) {
    if let Some(c) = obj_mut(c) {
        c.bg = bg;
    }
}
pub fn getbackground(c: control) -> rgb {
    obj_ref(c).map_or(Transparent, |c| c.bg)
}
pub fn setvalue(c: control, value: c_int) {
    if let Some(c) = obj_mut(c) {
        c.value = value;
    }
}
pub fn getvalue(c: control) -> c_int {
    obj_ref(c).map_or(0, |c| c.value)
}
pub fn setdata(c: control, data: *mut c_void) {
    if let Some(c) = obj_mut(c) {
        c.data = data;
    }
}
pub fn getdata(c: control) -> *mut c_void {
    obj_ref(c).map_or(ptr::null_mut(), |c| c.data)
}
pub fn parentwindow(c: control) -> window {
    obj_ref(c).map_or(ptr::null_mut(), |c| c.parent)
}

pub fn newcontrol(text: *const c_char, r: rect) -> control {
    new_control_object(ControlObject, text, r, false)
}
pub fn newdrawing(r: rect, fn_: drawfn) -> drawing {
    let drawing = new_control_object(UserObject, ptr::null(), r, true);
    if let Some(call) = obj_ref(drawing).and_then(|d| ptr_mut(d.call)) {
        call.redraw = fn_;
    }
    drawing
}
pub fn newpicture(img: image, r: rect) -> drawing {
    let picture = newdrawing(r, None);
    if let Some(picture) = obj_mut(picture) {
        picture.img = img;
    }
    picture
}
pub fn newbutton(text: *const c_char, r: rect, fn_: actionfn) -> button {
    let button = new_control_object(ButtonObject, text, r, false);
    if let Some(button) = obj_mut(button) {
        button.action = fn_;
    }
    button
}
pub fn newimagebutton(img: image, r: rect, fn_: actionfn) -> button {
    let button = newbutton(ptr::null(), r, fn_);
    if let Some(button) = obj_mut(button) {
        button.img = img;
    }
    button
}
pub fn setimage(c: control, img: image) {
    if let Some(c) = obj_mut(c) {
        c.img = img;
    }
}
pub fn newcheckbox(text: *const c_char, r: rect, fn_: actionfn) -> checkbox {
    let checkbox = new_control_object(CheckboxObject, text, r, false);
    if let Some(checkbox) = obj_mut(checkbox) {
        checkbox.action = fn_;
    }
    checkbox
}
pub fn newimagecheckbox(img: image, r: rect, fn_: actionfn) -> checkbox {
    let checkbox = newcheckbox(ptr::null(), r, fn_);
    if let Some(checkbox) = obj_mut(checkbox) {
        checkbox.img = img;
    }
    checkbox
}
pub fn newradiobutton(text: *const c_char, r: rect, fn_: actionfn) -> radiobutton {
    let button = new_control_object(RadioObject, text, r, false);
    if let Some(button) = obj_mut(button) {
        button.action = fn_;
    }
    button
}
pub fn newradiogroup() -> radiogroup {
    new_control_object(RadiogroupObject, ptr::null(), rect::default(), false)
}
pub fn newscrollbar(r: rect, max: c_int, pagesize: c_int, _fn_: scrollfn) -> scrollbar {
    let scrollbar = new_control_object(ScrollbarObject, ptr::null(), r, false);
    if !scrollbar.is_null() {
        changescrollbar(scrollbar, 0, max, pagesize);
    }
    scrollbar
}
pub fn changescrollbar(s: scrollbar, where_: c_int, max: c_int, size: c_int) {
    let Some(s) = obj_mut(s) else { return };

    let max = max.max(0);
    let size = size.clamp(0, max);
    let limit = max.saturating_sub(size);

    s.max = max;
    s.size = size;
    s.value = where_.clamp(0, limit);
}
pub fn newlabel(text: *const c_char, r: rect, alignment: c_int) -> label {
    let label = new_control_object(LabelObject, text, r, false);
    if let Some(label) = obj_mut(label) {
        label.value = alignment;
    }
    label
}
pub fn newfield(text: *const c_char, r: rect) -> field {
    new_control_object(FieldObject, text, r, true)
}
pub fn newpassword(text: *const c_char, r: rect) -> field {
    let field = newfield(text, r);
    if let Some(field) = obj_mut(field) {
        field.flags |= 1;
    }
    field
}
pub fn newtextbox(text: *const c_char, r: rect) -> textbox {
    new_control_object(TextboxObject, text, r, true)
}
pub fn newtextarea(text: *const c_char, r: rect) -> textbox {
    newtextbox(text, r)
}
pub fn newrichtextarea(text: *const c_char, r: rect) -> textbox {
    newtextbox(text, r)
}
pub fn newlistbox(list: *const *const c_char, r: rect, _fn_: scrollfn, dble: actionfn) -> listbox {
    let listbox = new_control_object(ListboxObject, first_list_item(list), r, false);
    if let Some(listbox) = obj_mut(listbox) {
        listbox.max = list_length(list);
        listbox.value = -1;
        listbox.dble = dble;
    }
    listbox
}
pub fn newdroplist(list: *const *const c_char, r: rect, fn_: scrollfn) -> listbox {
    let listbox = newlistbox(list, r, fn_, None);
    if let Some(listbox) = obj_mut(listbox) {
        listbox.kind = DroplistObject;
    }
    listbox
}
pub fn newdropfield(list: *const *const c_char, r: rect, fn_: scrollfn) -> listbox {
    let listbox = newlistbox(list, r, fn_, None);
    if let Some(listbox) = obj_mut(listbox) {
        listbox.kind = DropfieldObject;
    }
    listbox
}
pub fn newmultilist(list: *const *const c_char, r: rect, fn_: scrollfn, dble: actionfn) -> listbox {
    let listbox = newlistbox(list, r, fn_, dble);
    if let Some(listbox) = obj_mut(listbox) {
        listbox.kind = MultilistObject;
    }
    listbox
}
pub fn isselected(b: listbox, index: c_int) -> c_int {
    obj_ref(b).map_or(0, |b| if b.value == index { 1 } else { 0 })
}
pub fn setlistitem(b: listbox, index: c_int) {
    if let Some(b) = obj_mut(b) {
        b.value = index.max(-1);
    }
}
pub fn getlistitem(b: listbox) -> c_int {
    obj_ref(b).map_or(-1, |b| b.value)
}
pub fn changelistbox(b: listbox, list: *const *const c_char) {
    if b.is_null() {
        return;
    }
    settext(b, first_list_item(list));
    if let Some(b) = obj_mut(b) {
        b.max = list_length(list);
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
    let progress = new_control_object(ProgressbarObject, ptr::null(), r, false);
    if !progress.is_null() {
        setprogressbarrange(progress, pmin, pmax);
        if let Some(progress) = obj_mut(progress) {
            progress.data = incr as usize as *mut c_void;
            progress.flags = smooth as c_long;
        }
    }
    progress
}
pub fn setprogressbar(obj: progressbar, n: c_int) {
    let Some(obj) = obj_mut(obj) else { return };

    let min = obj.size.min(obj.max);
    let max = obj.max.max(obj.size);
    obj.value = n.clamp(min, max);
}
pub fn stepprogressbar(obj: progressbar, n: c_int) {
    if let Some(value) = obj_ref(obj).map(|obj| obj.value) {
        setprogressbar(obj, value.saturating_add(n));
    }
}
pub fn setprogressbarrange(obj: progressbar, pbmin: c_int, pbmax: c_int) {
    let Some(obj) = obj_mut(obj) else { return };

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
pub fn cleartext(t: textbox) {
    if !t.is_null() {
        settext(t, ptr::null());
        selecttext(t, 0, 0);
    }
}
pub fn pastetext(_t: textbox) {}
pub fn inserttext(t: textbox, text: *const c_char) {
    let Some(obj) = obj_mut(t) else { return };
    if text.is_null() {
        return;
    }

    let old_text = obj.text;
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
    obj.text = combined;

    let end = super::strings::string_length(obj.text);
    selecttext(t, end, end);
}
pub fn selecttext(t: textbox, start: c_long, end: c_long) {
    let Some(t) = obj_mut(t) else { return };

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
pub fn textselection(t: textbox, start: *mut c_long, end: *mut c_long) {
    let selection = obj_ref(t).map_or((0, 0), |t| (t.caretx as c_long, t.carety as c_long));

    if let Some(start) = ptr_mut(start) {
        *start = selection.0;
    }
    if let Some(end) = ptr_mut(end) {
        *end = selection.1;
    }
}

// Font functions
pub fn newfont(name: *const c_char, style: c_int, size: c_int) -> font {
    objects::init_objects();
    let font = objects::new_object(FontObject, ptr::null_mut(), ptr::null_mut());
    let Some(font) = obj_mut(font) else {
        return ptr::null_mut();
    };
    font.text = strings::new_string(name);
    font.value = size.max(1);
    font.flags = style as c_long;
    font
}
pub fn fontwidth(f: font) -> c_int {
    obj_ref(f).map_or(0, |f| {
        if (f.flags & FixedWidth as c_long) != 0 {
            (f.value / 2).max(1)
        } else {
            (f.value * 3 / 5).max(1)
        }
    })
}
pub fn fontheight(f: font) -> c_int {
    obj_ref(f).map_or(0, |f| f.value.max(1))
}
pub fn fontascent(f: font) -> c_int {
    let height = fontheight(f);
    (height * 3) / 4
}
pub fn fontdescent(f: font) -> c_int {
    fontheight(f).saturating_sub(fontascent(f))
}

// Height alias
pub fn getheight(f: font) -> c_int {
    fontheight(f)
}
pub fn getdescent(f: font) -> c_int {
    fontdescent(f)
}

// Image control
pub fn newbitmap(width: c_int, height: c_int, depth: c_int) -> bitmap {
    new_bitmap_object(width, height, depth)
}
pub fn loadbitmap(name: *const c_char) -> bitmap {
    let img = loadimage(name);
    imagetobitmap(img)
}
pub fn imagetobitmap(img: image) -> bitmap {
    let Some((width, height, depth)) = ptr_ref(img).map(|img| (img.width, img.height, img.depth))
    else {
        return ptr::null_mut();
    };
    let bitmap = new_bitmap_object(width, height, depth);
    if let Some(bitmap) = obj_mut(bitmap) {
        image_api::delimage(bitmap.img);
        bitmap.img = image_api::copyimage(img);
    }
    bitmap
}
pub fn createbitmap(width: c_int, height: c_int, depth: c_int, data: *mut GAbyte) -> bitmap {
    let bitmap = new_bitmap_object(width, height, depth);
    set_bitmap_pixels(bitmap, data);
    bitmap
}
pub fn setbitmapdata(b: bitmap, data: *mut GAbyte) {
    set_bitmap_pixels(b, data)
}
pub fn getbitmapdata(b: bitmap, data: *mut GAbyte) {
    let Some(img) = obj_ref(b).map(|b| b.img) else { return };
    if img.is_null() || data.is_null() {
        return;
    }
    let pixels = image_api::getpixels(img);
    if pixels.is_null() {
        return;
    }
    let Some((width, height, depth)) = ptr_ref(img).map(|img| (img.width, img.height, img.depth))
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
    if let (Some(dest), Some(src)) = (
        slice_mut(data, byte_len),
        slice_ref(pixels, byte_len),
    ) {
        dest.copy_from_slice(src);
    }
}
pub fn getbitmapdata2(b: bitmap, data: *mut *mut GAbyte) {
    let Some(img) = obj_ref(b).map(|b| b.img) else { return };
    let Some(data) = ptr_mut(data) else { return };
    if img.is_null() {
        return;
    }
    *data = image_api::getpixels(img);
}

// Cursor functions
pub fn newcursor(hotspot: point, img: image) -> cursor {
    let cursor = new_control_object(CursorObject, ptr::null(), rect::default(), false);
    if let Some(cursor) = obj_mut(cursor) {
        cursor.rect.x = hotspot.x;
        cursor.rect.y = hotspot.y;
        cursor.img = img;
    }
    cursor
}
pub fn createcursor(offset: point, _white_mask: *mut GAbyte, _black_shape: *mut GAbyte) -> cursor {
    newcursor(offset, ptr::null_mut())
}
pub fn loadcursor(name: *const c_char) -> cursor {
    let cursor = newcursor(point::default(), ptr::null_mut());
    if !cursor.is_null() {
        settext(cursor, name);
    }
    cursor
}

// Image load/save
pub fn loadimage(_filename: *const c_char) -> image {
    ptr::null_mut()
}
pub fn saveimage(img: image, filename: *const c_char) {
    let Some(data) = ptr_ref(img) else {
        return;
    };
    let Some(path) = cstr(filename).and_then(|f| f.to_str().ok()) else {
        return;
    };
    let Ok(mut file) = File::create(path) else {
        return;
    };
    let width = data.width.max(0);
    let height = data.height.max(0);

    if data.depth <= 8 {
        let _ = writeln!(file, "P5\n{} {}\n255", width, height);
        let pixels = image_api::getpixels(img);
        // Read `width * height` bytes from the owned pixel buffer.
        if let Some(bytes) = slice_ref(pixels, (width * height) as usize) {
            let _ = file.write_all(bytes);
        }
        return;
    }

    let _ = writeln!(file, "P6\n{} {}\n255", width, height);
    for y in 0..height {
        for x in 0..width {
            let pixel = image_api::get_image_pixel(img, x, y);
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

        draw(control);
        REDRAW_CALLS.with(|calls| assert_eq!(calls.get(), 0));

        show_control(control);
        draw(control);
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
