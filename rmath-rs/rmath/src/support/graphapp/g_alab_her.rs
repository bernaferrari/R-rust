#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

use std::os::raw::{c_double, c_int, c_void};
use super::g_cntrlify::*;
use super::g_fontdb::*;
use super::g_her_glyph::*;

pub const HERSHEY_STROKE_WIDTH: c_double = 1.42;
pub const HERSHEY_ORIENTAL_STROKE_WIDTH: c_double = 1.175;
pub const HERSHEY_LARGE_HEIGHT: c_double = 33.0;
pub const HERSHEY_LARGE_CAPHEIGHT: c_double = 22.0;
pub const HERSHEY_EM: c_double = 33.0;
pub const HERSHEY_BASELINE: c_double = -9.5;
pub const SHEAR: c_double = 2.0 / 7.0;
pub const SCRIPTSIZE: c_double = 0.6;
pub const SUBSCRIPT_DX: c_double = 0.0;
pub const SUBSCRIPT_DY: c_double = -0.25;
pub const SUPERSCRIPT_DX: c_double = 0.0;
pub const SUPERSCRIPT_DY: c_double = 0.4;
pub const ACCENT_UP_SHIFT: c_double = 7.0;
pub const ACCENT_RIGHT_SHIFT: c_double = 2.0;
pub const SMALL_KANA_SIZE: c_double = 0.725;
pub const M_PI: c_double = 3.14159265358979323846;

const OCCIDENTAL: c_int = 0;
const ORIENTAL: c_int = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct vfontContext {
    pub currX: c_double,
    pub currY: c_double,
    pub angle: c_double,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct pGEcontextStub {
    pub ps: c_double,
    pub cex: c_double,
    pub lwd: c_double,
    pub lty: c_int,
    pub lend: c_int,
    pub ljoin: c_int,
    pub fontface: c_int,
    pub fontfamily: [u8; 201],
}

#[repr(C)]
pub struct pGEDevDescStub {
    pub ipr: [c_double; 2],
    pub _pad: [u8; 0],
}

pub type pGEcontext = *mut pGEcontextStub;
pub type pGEDevDesc = *mut pGEDevDescStub;

pub const GE_INCHES: c_int = 2;
pub const LTY_SOLID: c_int = 1;
pub const GE_ROUND_CAP: c_int = 1;
pub const GE_ROUND_JOIN: c_int = 1;

extern "C" {
    fn strlen(s: *const u8) -> usize;
}

pub fn hershey_x_units_to_user_units(size: c_double, gc: pGEcontext, dd: pGEDevDesc) -> c_double {
    unsafe {
        let gc_ref = &*gc;
        let dd_ref = &*dd;
        size * ((gc_ref.ps * gc_ref.cex / 72.27) / dd_ref.ipr[0]) / HERSHEY_LARGE_HEIGHT
    }
}

pub fn hershey_y_units_to_user_units(size: c_double, gc: pGEcontext, dd: pGEDevDesc) -> c_double {
    unsafe {
        let gc_ref = &*gc;
        let dd_ref = &*dd;
        size * ((gc_ref.ps * gc_ref.cex / 72.27) / dd_ref.ipr[1]) / HERSHEY_LARGE_HEIGHT
    }
}

pub fn hershey_line_width_to_lwd(width: c_double, gc: pGEcontext) -> c_double {
    unsafe {
        let gc_ref = &*gc;
        width * ((4.0 / 3.0) * gc_ref.ps * gc_ref.cex) / HERSHEY_LARGE_HEIGHT
    }
}

#[inline]
pub unsafe fn from_device_width(value: c_double, _to: c_int, _dd: pGEDevDesc) -> c_double {
    value
}

#[inline]
pub unsafe fn to_device_width(value: c_double, _from: c_int, _dd: pGEDevDesc) -> c_double {
    value
}

#[inline]
pub unsafe fn from_device_height(value: c_double, _to: c_int, _dd: pGEDevDesc) -> c_double {
    value
}

#[inline]
pub unsafe fn to_device_height(value: c_double, _from: c_int, _dd: pGEDevDesc) -> c_double {
    value
}

#[inline]
pub unsafe fn from_device_x(value: c_double, _to: c_int, _dd: pGEDevDesc) -> c_double {
    value
}

#[inline]
pub unsafe fn to_device_x(value: c_double, _from: c_int, _dd: pGEDevDesc) -> c_double {
    value
}

#[inline]
pub unsafe fn from_device_y(value: c_double, _to: c_int, _dd: pGEDevDesc) -> c_double {
    value
}

#[inline]
pub unsafe fn to_device_y(value: c_double, _from: c_int, _dd: pGEDevDesc) -> c_double {
    value
}

pub unsafe fn ge_line(
    _x1: c_double,
    _y1: c_double,
    _x2: c_double,
    _y2: c_double,
    _gc: pGEcontext,
    _dd: pGEDevDesc,
) {
}

pub unsafe fn vmaxget() -> *mut c_void {
    std::ptr::null_mut()
}

pub unsafe fn vmaxset(_vmax: *const c_void) {}

pub unsafe fn r_alloc(_n: usize, _size: usize) -> *mut c_void {
    std::ptr::null_mut()
}

pub unsafe fn _draw_hershey_stroke(
    vc: &mut vfontContext,
    gc: pGEcontext,
    dd: pGEDevDesc,
    pendown: c_int,
    deltax: c_double,
    deltay: c_double,
) {
    _draw_stroke(
        vc,
        gc,
        dd,
        pendown,
        from_device_width(hershey_x_units_to_user_units(deltax, gc, dd), GE_INCHES, dd),
        from_device_height(hershey_y_units_to_user_units(deltay, gc, dd), GE_INCHES, dd),
    );
}

pub unsafe fn moverel(dx: c_double, dy: c_double, vc: &mut vfontContext) {
    vc.currX += dx;
    vc.currY += dy;
}

pub unsafe fn linerel(
    dx: c_double,
    dy: c_double,
    vc: &mut vfontContext,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    ge_line(
        to_device_x(vc.currX, GE_INCHES, dd),
        to_device_y(vc.currY, GE_INCHES, dd),
        to_device_x(vc.currX + dx, GE_INCHES, dd),
        to_device_y(vc.currY + dy, GE_INCHES, dd),
        gc,
        dd,
    );
    vc.currX += dx;
    vc.currY += dy;
}

pub unsafe fn _draw_stroke(
    vc: &mut vfontContext,
    gc: pGEcontext,
    dd: pGEDevDesc,
    pendown: c_int,
    deltax: c_double,
    deltay: c_double,
) {
    let theta = M_PI * vc.angle / 180.0;
    let dx = theta.cos() * deltax - theta.sin() * deltay;
    let dy = theta.sin() * deltax + theta.cos() * deltay;

    if pendown != 0 {
        linerel(dx, dy, vc, gc, dd);
    } else {
        moverel(dx, dy, vc);
    }
}

pub unsafe fn r_ge_vstr_width(s: *const u8, _enc: c_int, gc: pGEcontext, dd: pGEDevDesc) -> c_double {
    let vmax = vmaxget();
    let typeface = ((*gc).fontfamily[7] as c_int) - 1;
    let codestring = _controlify(dd, s, typeface, (*gc).fontface);
    let label_width = _label_width_hershey(gc, dd, codestring as *const u16);
    vmaxset(vmax);
    label_width
}

pub unsafe fn r_ge_vstr_height(s: *const u8, _enc: c_int, gc: pGEcontext, dd: pGEDevDesc) -> c_double {
    hershey_y_units_to_user_units(HERSHEY_LARGE_CAPHEIGHT, gc, dd)
}

pub unsafe fn r_ge_vtext(
    x: c_double,
    y: c_double,
    s: *const u8,
    _enc: c_int,
    x_justify: c_double,
    y_justify: c_double,
    rotation: c_double,
    gc: pGEcontext,
    dd: pGEDevDesc,
) {
    let vmax = vmaxget();
    let mut vc = vfontContext {
        currX: from_device_x(x, GE_INCHES, dd),
        currY: from_device_y(y, GE_INCHES, dd),
        angle: rotation,
    };
    (*gc).lty = LTY_SOLID;
    (*gc).lwd = hershey_line_width_to_lwd(HERSHEY_STROKE_WIDTH, gc);
    (*gc).lend = GE_ROUND_CAP;
    (*gc).ljoin = GE_ROUND_JOIN;
    let typeface = ((*gc).fontfamily[7] as c_int) - 1;
    let codestring = _controlify(dd, s, typeface, (*gc).fontface);
    let label_width = _label_width_hershey(gc, dd, codestring as *const u16);
    let label_height = hershey_y_units_to_user_units(HERSHEY_LARGE_CAPHEIGHT, gc, dd);
    let x_justify = if x_justify.is_finite() { x_justify } else { 0.5 };
    let y_justify = if y_justify.is_finite() { y_justify } else { 0.5 };
    let x_offset = 0.0 - x_justify;
    let y_offset = 0.0 - y_justify * 1.0;
    _draw_stroke(
        &mut vc,
        gc,
        dd,
        0,
        from_device_width(x_offset * label_width, GE_INCHES, dd),
        from_device_height(y_offset * label_height, GE_INCHES, dd),
    );
    _draw_hershey_string(&mut vc, gc, dd, codestring as *const u16);
    vmaxset(vmax);
}

pub unsafe fn _label_width_hershey(
    gc: pGEcontext,
    dd: pGEDevDesc,
    label: *const u16,
) -> c_double {
    let mut ptr = label;
    let mut charsize: c_double = 1.0;
    let mut saved_charsize: c_double = 1.0;
    let mut width: c_double = 0.0;
    let mut saved_width: c_double = 0.0;

    loop {
        let c = *ptr;
        if c == 0 {
            break;
        }
    }

    hershey_x_units_to_user_units(width, gc, dd)
}

pub unsafe fn _draw_hershey_penup_stroke(
    vc: &mut vfontContext,
    gc: pGEcontext,
    dd: pGEDevDesc,
    dx: c_double,
    dy: c_double,
    charsize: c_double,
    oblique: c_int,
) {
    let shear = if oblique != 0 { SHEAR } else { 0.0 };
    _draw_hershey_stroke(vc, gc, dd, 0, charsize * (dx + shear * dy), charsize * dy);
}

pub unsafe fn _draw_hershey_glyph(
    vc: &mut vfontContext,
    gc: pGEcontext,
    dd: pGEDevDesc,
    glyphnum: c_int,
    charsize: c_double,
    glyph_type: c_int,
    oblique: c_int,
) {
    let shear = if oblique != 0 { SHEAR } else { 0.0 };
    let glyph_str = if glyph_type == ORIENTAL {
        _oriental_hershey_glyphs[glyphnum as usize].as_bytes()
    } else {
        _occidental_hershey_glyphs[glyphnum as usize].as_bytes()
    };

    if glyph_str.is_empty() {
        return;
    }

    let mut xcurr = charsize * glyph_str[0] as c_double;
    let xfinal = charsize * glyph_str[1] as c_double;
    let mut ycurr: c_double = 0.0;
    let mut pendown: c_int = 0;
    let mut i = 2;
    while i + 1 < glyph_str.len() {
        let xnewint = glyph_str[i];
        if xnewint == b' ' {
            pendown = 0;
        } else {
            let xnew = charsize * xnewint as c_double;
            let ynew = charsize * (b'R' as c_double - (glyph_str[i + 1] as c_double + HERSHEY_BASELINE));
            let dx = xnew - xcurr;
            let dy = ynew - ycurr;
            _draw_hershey_stroke(vc, gc, dd, pendown, dx + shear * dy, dy);
            xcurr = xnew;
            ycurr = ynew;
            pendown = 1;
        }
    }

    let dx = xfinal - xcurr;
    let dy = 0.0 - ycurr;
    _draw_hershey_stroke(vc, gc, dd, 0, dx + shear * dy, dy);
}

pub unsafe fn _draw_hershey_string(
    vc: &mut vfontContext,
    gc: pGEcontext,
    dd: pGEDevDesc,
    string: *const u16,
) {
    let mut ptr = string;
    let mut charsize: c_double = 1.0;
    let mut line_width_type: c_int = 0;

    loop {
        let c = *ptr;
        ptr = ptr.add(1);
        if c == 0 {
            break;
        }
    }
}

pub unsafe _composite_char(composite: &mut u8) -> bool {
    unsafe {

    let given = *composite;
    let mut i = 0;
    while i < _hershey_accented_char_info.len() {
        if _hershey_accented_char_info[i].composite == given {
            let character = _hershey_accented_char_info[i].character;
            *composite = character;
            return true;
        }
    }
    false

    }
}
    let given = *composite;
    let mut i = 0;
    while i < _hershey_accented_char_info.len() {
        if _hershey_accented_char_info[i].composite == given {
            let character = _hershey_accented_char_info[i].character;
            *composite = character;
            return true;
        }
    }
    false
}

pub unsafe fn _controlify(
    _dd: pGEDevDesc,
    src: *const u8,
    typeface: c_int,
    fontindex: c_int,
) -> *mut u16 {
    let len = strlen(src);
    let dest = r_alloc(6 * len + 1, std::mem::size_of::<u16>()) as *mut u16;
    let mut j: usize = 0;
    let raw_fontnum = _hershey_typeface_info[typeface as usize].fonts[fontindex as usize];
    let raw_symbol_fontnum = _hershey_typeface_info[typeface as usize].fonts[0];
    let fontword = (raw_fontnum as u16) << FONT_SHIFT;
    let symbol_fontword = (raw_symbol_fontnum as u16) << FONT_SHIFT;
    let mut src_ptr = src;

    unsafe {
        while *src_ptr != 0 {
            if (raw_fontnum as c_int) == HERSHEY_EUC && (*src_ptr & 0x80) != 0 && (*src_ptr.add(1) & 0x80) != 0 {
                let jis_row = *src_ptr & !0x80u8;
                let jis_col = *src_ptr.add(1) & !0x80u8;
                let jis_glyphindex = 256 * jis_row as c_int + jis_col as c_int;
                let good_jis = jis_row > 0x20 && jis_row < 0x7f && jis_col > 0x20 && jis_col < 0x7f;
                if good_jis {
                    if jis_glyphindex >= BEGINNING_OF_KANJI {
                        let mut matched = false;
                        let mut nelson = 0;
                        for i in 0.._builtin_kanji_glyphs.len() {
                            if _builtin_kanji_glyphs[i].jis == jis_glyphindex {
                                matched = true;
                                nelson = _builtin_kanji_glyphs[i].nelson;
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    dest
}
