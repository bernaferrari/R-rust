#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/main/g_alab_her.c
 *
 *  This is from the GNU plotutils libplot-2.3 distribution
 *  Several modifications have been made to use the R graphics engine
 *  for output.
 */

use std::os::raw::{c_char, c_double, c_int};
use std::cell::Cell;
use std::ptr;

use crate::main::engine::{
    GE_INCHES, GEDevDesc, GELine, R_GE_gcontext, fromDeviceHeight, fromDeviceWidth, fromDeviceX,
    fromDeviceY,
};
use crate::sexp::memory_ext::{vmaxget, vmaxset};

// ---------------------------------------------------------------------------
// Hershey font metric constants (from g_her_metr.h)
// ---------------------------------------------------------------------------

const HERSHEY_STROKE_WIDTH: c_double = 1.42;
const HERSHEY_ORIENTAL_STROKE_WIDTH: c_double = 1.175;

const HERSHEY_LARGE_BASELINE: c_double = -9.5;
const HERSHEY_LARGE_CAPLINE: c_double = 12.5;
const HERSHEY_LARGE_TOPLINE: c_double = 16.5;
const HERSHEY_LARGE_BOTTOMLINE: c_double = -16.5;
const HERSHEY_LARGE_CAPHEIGHT: c_double = 22.0;
const HERSHEY_LARGE_ASCENT: c_double = 26.0;
const HERSHEY_LARGE_DESCENT: c_double = 7.0;
const HERSHEY_LARGE_HEIGHT: c_double = HERSHEY_LARGE_ASCENT + HERSHEY_LARGE_DESCENT;
const HERSHEY_LARGE_EM: c_double = 33.0;

const HERSHEY_BASELINE: c_double = HERSHEY_LARGE_BASELINE;
const HERSHEY_ASCENT: c_double = HERSHEY_LARGE_ASCENT;
const HERSHEY_DESCENT: c_double = HERSHEY_LARGE_DESCENT;
const HERSHEY_HEIGHT: c_double = HERSHEY_LARGE_HEIGHT;
const HERSHEY_EM: c_double = HERSHEY_LARGE_EM;

// ---------------------------------------------------------------------------
// Control codes (from g_control.h)
// ---------------------------------------------------------------------------

const C_BEGIN_SUPERSCRIPT: c_int = 0;
const C_END_SUPERSCRIPT: c_int = 1;
const C_BEGIN_SUBSCRIPT: c_int = 2;
const C_END_SUBSCRIPT: c_int = 3;
const C_PUSH_LOCATION: c_int = 4;
const C_POP_LOCATION: c_int = 5;
const C_RIGHT_ONE_EM: c_int = 6;
const C_RIGHT_HALF_EM: c_int = 7;
const C_RIGHT_QUARTER_EM: c_int = 8;
const C_RIGHT_SIXTH_EM: c_int = 9;
const C_RIGHT_EIGHTH_EM: c_int = 10;
const C_RIGHT_TWELFTH_EM: c_int = 11;
const C_LEFT_ONE_EM: c_int = 12;
const C_LEFT_HALF_EM: c_int = 13;
const C_LEFT_QUARTER_EM: c_int = 14;
const C_LEFT_SIXTH_EM: c_int = 15;
const C_LEFT_EIGHTH_EM: c_int = 16;
const C_LEFT_TWELFTH_EM: c_int = 17;

const CONTROL_CODE: c_int = 0x8000;
const RAW_HERSHEY_GLYPH: c_int = 0x4000;
const RAW_ORIENTAL_HERSHEY_GLYPH: c_int = 0x2000;

const ONE_BYTE: c_int = 0xff;
const FONT_SHIFT: c_int = 8;
const FONT_SPEC: c_int = ONE_BYTE << FONT_SHIFT;
const GLYPH_SPEC: c_int = 0x1fff;

// ---------------------------------------------------------------------------
// Font database constants (from g_extern.h)
// ---------------------------------------------------------------------------

const ACC0: c_int = 16384 + 0;
const ACC1: c_int = 16384 + 1;
const ACC2: c_int = 16384 + 2;
const KS: c_int = 8192;
const UNDE: c_int = 4023;

// Glyph array types
const OCCIDENTAL: c_int = 0;
const ORIENTAL: c_int = 1;

const BEGINNING_OF_KANA: c_int = 4195;

// ---------------------------------------------------------------------------
// Shearing and positioning constants
// ---------------------------------------------------------------------------

const SHEAR: c_double = 2.0 / 7.0;
const SCRIPTSIZE: c_double = 0.6;
const SUBSCRIPT_DX: c_double = 0.0;
const SUBSCRIPT_DY: c_double = -0.25;
const SUPERSCRIPT_DX: c_double = 0.0;
const SUPERSCRIPT_DY: c_double = 0.4;
const ACCENT_UP_SHIFT: c_double = 7.0;
const ACCENT_RIGHT_SHIFT: c_double = 2.0;
const SMALL_KANA_SIZE: c_double = 0.725;

const M_PI: c_double = 3.14159265358979323846;

// ---------------------------------------------------------------------------
// Helper macros converted to functions
// ---------------------------------------------------------------------------

/// Convert Hershey X units to user units (inches)
unsafe fn hershey_x_units_to_user_units(
    size: c_double,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> c_double {
    unsafe {
        let gc_ref = &*gc;
        let dd_ref = &mut *dd;
        let dev = dd_ref.dev;
        let dev_ref = &*dev;
        (size * ((gc_ref.ps * gc_ref.cex / 72.27) / dev_ref.ipr[0])) / HERSHEY_LARGE_HEIGHT
    }
}

/// Convert Hershey Y units to user units (inches)
unsafe fn hershey_y_units_to_user_units(
    size: c_double,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> c_double {
    unsafe {
        let gc_ref = &*gc;
        let dd_ref = &mut *dd;
        let dev = dd_ref.dev;
        let dev_ref = &*dev;
        (size * ((gc_ref.ps * gc_ref.cex / 72.27) / dev_ref.ipr[1])) / HERSHEY_LARGE_HEIGHT
    }
}

/// Convert Hershey line width to R Graphics Engine lwd units
unsafe fn hershey_line_width_to_lwd(width: c_double, gc: *const R_GE_gcontext) -> c_double {
    unsafe {
        let gc_ref = &*gc;
        (width * ((4.0 / 3.0) * (gc_ref.ps * gc_ref.cex))) / HERSHEY_LARGE_HEIGHT
    }
}

// ---------------------------------------------------------------------------
// Stub types and data for Hershey font database
// TODO: Port g_fontdb.c, g_her_glyph.c, g_cntrlify.c for full support
// ---------------------------------------------------------------------------

/// Stub: Hershey font info structure
#[derive(Clone, Copy)]
struct plHersheyFontInfoStruct {
    name: *const c_char,
    othername: *const c_char,
    orig_name: *const c_char,
    chars: [i16; 256],
    typeface_index: c_int,
    font_index: c_int,
    obliquing: c_int,
    iso8859_1: c_int,
    visible: c_int,
}

/// Stub: Hershey accented char info structure
#[derive(Clone, Copy)]
struct plHersheyAccentedCharInfoStruct {
    composite: u8,
    character: u8,
    accent: u8,
}

thread_local! {
    static _hershey_font_info: Cell<[plHersheyFontInfoStruct; 22]> = Cell::new([plHersheyFontInfoStruct {
        name: ptr::null(),
        othername: ptr::null(),
        orig_name: ptr::null(),
        chars: [0; 256],
        typeface_index: 0,
        font_index: 0,
        obliquing: 0,
        iso8859_1: 0,
        visible: 0,
    }; 22]);
    static _hershey_accented_char_info: Cell<[plHersheyAccentedCharInfoStruct; 1]> =
        Cell::new([plHersheyAccentedCharInfoStruct {
            composite: 0,
            character: 0,
            accent: 0,
        }]);
    static _occidental_hershey_glyphs: Cell<[*const u8; 4200]> = Cell::new([ptr::null(); 4200]);
    static _oriental_hershey_glyphs: Cell<[*const u8; 1]> = Cell::new([ptr::null(); 1]);
}

/// Stub: _controlify function -- converts string to codestring with annotations
/// Real implementation in g_cntrlify.c
unsafe fn _controlify(
    _dd: *mut GEDevDesc,
    _s: *const u8,
    _fontindex: c_int,
    _fontface: c_int,
) -> *mut u16 {
    unsafe {
        // TODO: Port g_cntrlify.c for real implementation
        // For now, return a minimal allocation
        let result = crate::sexp::memory_ext::R_alloc(1, 2) as *mut u16;
        *result = 0; // null-terminated
        result
    }
}

// ---------------------------------------------------------------------------
// vfontContext structure
// ---------------------------------------------------------------------------

struct vfontContext {
    currX: c_double,
    currY: c_double,
    angle: c_double,
}

// ---------------------------------------------------------------------------
// Internal drawing functions
// ---------------------------------------------------------------------------

unsafe fn moverel(dx: c_double, dy: c_double, vc: &mut vfontContext) {
    vc.currX += dx;
    vc.currY += dy;
}

unsafe fn linerel(
    dx: c_double,
    dy: c_double,
    vc: &mut vfontContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) {
    unsafe {
        GELine(
            fromDeviceX(vc.currX, GE_INCHES, dd as *mut _),
            fromDeviceY(vc.currY, GE_INCHES, dd as *mut _),
            fromDeviceX(vc.currX + dx, GE_INCHES, dd as *mut _),
            fromDeviceY(vc.currY + dy, GE_INCHES, dd as *mut _),
            gc as *const _,
            dd as *mut _,
        );
        vc.currX += dx;
        vc.currY += dy;
    }
}

/// Draw a stroke with rotation support, taking arguments in user units (inches)
unsafe fn _draw_stroke(
    vc: &mut vfontContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
    pendown: c_int,
    deltax: c_double,
    deltay: c_double,
) {
    unsafe {
        let theta = M_PI * vc.angle / 180.0;
        let dx = theta.cos() * deltax - theta.sin() * deltay;
        let dy = theta.sin() * deltax + theta.cos() * deltay;
        if pendown != 0 {
            linerel(dx, dy, vc, gc, dd);
        } else {
            moverel(dx, dy, vc);
        }
    }
}

/// Draw a Hershey stroke, converting from Hershey units to user units
unsafe fn _draw_hershey_stroke(
    vc: &mut vfontContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
    pendown: c_int,
    deltax: c_double,
    deltay: c_double,
) {
    unsafe {
        _draw_stroke(
            vc,
            gc,
            dd,
            pendown,
            fromDeviceWidth(
                hershey_x_units_to_user_units(deltax, gc, dd),
                GE_INCHES,
                dd as *mut _,
            ),
            fromDeviceHeight(
                hershey_y_units_to_user_units(deltay, gc, dd),
                GE_INCHES,
                dd as *mut _,
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// Label width calculation
// ---------------------------------------------------------------------------

unsafe fn _label_width_hershey(
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
    label: *const u16,
) -> c_double {
    unsafe {
        let mut ptr = label;
        let mut c: u16;
        let mut charsize: c_double = 1.0;
        let mut saved_charsize: c_double = 1.0;
        let mut width: c_double = 0.0;
        let mut saved_width: c_double = 0.0;

        loop {
            c = *ptr;
            if c == 0 {
                break;
            }

            if (c as c_int & RAW_HERSHEY_GLYPH) != 0 {
                let glyphnum = (c as c_int & GLYPH_SPEC) as usize;
                let glyph = _occidental_hershey_glyphs.with(|v| v.get()[glyphnum]);
                if !glyph.is_null() && *glyph != 0 {
                    width += charsize * (*glyph.add(1) as c_int - *glyph as c_int) as c_double;
                }
            } else if (c as c_int & RAW_ORIENTAL_HERSHEY_GLYPH) != 0 {
                let glyphnum = (c as c_int & GLYPH_SPEC) as usize;
                let glyph = _oriental_hershey_glyphs.with(|v| v.get()[glyphnum]);
                if !glyph.is_null() && *glyph != 0 {
                    width += charsize * (*glyph.add(1) as c_int - *glyph as c_int) as c_double;
                }
            } else if (c as c_int & CONTROL_CODE) != 0 {
                match c as c_int & !CONTROL_CODE {
                    C_BEGIN_SUBSCRIPT | C_BEGIN_SUPERSCRIPT => {
                        charsize *= SCRIPTSIZE;
                    }
                    C_END_SUBSCRIPT | C_END_SUPERSCRIPT => {
                        charsize /= SCRIPTSIZE;
                    }
                    C_PUSH_LOCATION => {
                        saved_width = width;
                        saved_charsize = charsize;
                    }
                    C_POP_LOCATION => {
                        width = saved_width;
                        charsize = saved_charsize;
                    }
                    C_RIGHT_ONE_EM => {
                        width += charsize * HERSHEY_EM;
                    }
                    C_RIGHT_HALF_EM => {
                        width += charsize * HERSHEY_EM / 2.0;
                    }
                    C_RIGHT_QUARTER_EM => {
                        width += charsize * HERSHEY_EM / 4.0;
                    }
                    C_RIGHT_SIXTH_EM => {
                        width += charsize * HERSHEY_EM / 6.0;
                    }
                    C_RIGHT_EIGHTH_EM => {
                        width += charsize * HERSHEY_EM / 8.0;
                    }
                    C_RIGHT_TWELFTH_EM => {
                        width += charsize * HERSHEY_EM / 12.0;
                    }
                    C_LEFT_ONE_EM => {
                        width -= charsize * HERSHEY_EM;
                    }
                    C_LEFT_HALF_EM => {
                        width -= charsize * HERSHEY_EM / 2.0;
                    }
                    C_LEFT_QUARTER_EM => {
                        width -= charsize * HERSHEY_EM / 4.0;
                    }
                    C_LEFT_SIXTH_EM => {
                        width -= charsize * HERSHEY_EM / 6.0;
                    }
                    C_LEFT_EIGHTH_EM => {
                        width -= charsize * HERSHEY_EM / 8.0;
                    }
                    C_LEFT_TWELFTH_EM => {
                        width -= charsize * HERSHEY_EM / 12.0;
                    }
                    _ => {}
                }
            } else {
                // Actual font character
                let raw_fontnum = ((c as c_int >> FONT_SHIFT) & ONE_BYTE) as usize;
                let char_c = c as c_int & !FONT_SPEC;
                let mut glyphnum = _hershey_font_info.with(|v| v.get()[raw_fontnum].chars[char_c as usize]) as c_int;

                // Check for composite character
                if glyphnum == ACC0 || glyphnum == ACC1 || glyphnum == ACC2 {
                    let mut composite = char_c as u8;
                    let mut character: u8 = 0;
                    let mut accent: u8 = 0;
                    if _composite_char(&mut composite, &mut character, &mut accent) != 0 {
                        glyphnum =
                            _hershey_font_info.with(|v| v.get()[raw_fontnum].chars[character as usize]) as c_int;
                    } else {
                        glyphnum = UNDE;
                    }
                }

                // Check for small kana
                if glyphnum & KS != 0 {
                    glyphnum -= KS;
                }

                let glyph = _occidental_hershey_glyphs.with(|v| v.get()[glyphnum as usize]);
                if !glyph.is_null() && *glyph != 0 {
                    width += charsize * (*glyph.add(1) as c_int - *glyph as c_int) as c_double;
                }
            }

            ptr = ptr.add(1);
        }

        hershey_x_units_to_user_units(width, gc, dd)
    }
}

// ---------------------------------------------------------------------------
// Label height calculation
// ---------------------------------------------------------------------------

unsafe fn _label_height_hershey(
    _gc: *const R_GE_gcontext,
    _dd: *mut GEDevDesc,
    _label: *const u16,
) -> c_double {
    unsafe { hershey_y_units_to_user_units(HERSHEY_LARGE_CAPHEIGHT, _gc, _dd) }
}

// ---------------------------------------------------------------------------
// Hershey glyph drawing
// ---------------------------------------------------------------------------

unsafe fn _draw_hershey_glyph(
    vc: &mut vfontContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
    glyphnum: c_int,
    charsize: c_double,
    glyph_type: c_int,
    oblique: c_int,
) {
    unsafe {
        let g = gc as *mut R_GE_gcontext;
        let shear = if oblique != 0 { SHEAR } else { 0.0 };
        let glyph: *const u8;
        match glyph_type {
            ORIENTAL => {
                glyph = _oriental_hershey_glyphs.with(|v| v.get()[glyphnum as usize]);
            }
            _ => {
                glyph = _occidental_hershey_glyphs.with(|v| v.get()[glyphnum as usize]);
            }
        }

        if glyph.is_null() || *glyph == 0 {
            return;
        }

        let mut xcurr = charsize * *glyph as c_double;
        let xfinal = charsize * *glyph.add(1) as c_double;
        let mut ycurr: c_double = 0.0;
        let yfinal: c_double = 0.0;
        let mut glyph_ptr = glyph.add(2);
        let mut pendown: c_int = 0;

        while *glyph_ptr != 0 {
            let xnewint = *glyph_ptr as c_int;
            if xnewint == ' ' as c_int {
                pendown = 0;
            } else {
                let xnew = charsize * xnewint as c_double;
                let ynew = charsize
                    * ('R' as c_int - (*glyph_ptr.add(1) as c_int + HERSHEY_BASELINE as c_int))
                        as c_double;
                let dx = xnew - xcurr;
                let dy = ynew - ycurr;
                _draw_hershey_stroke(vc, gc, dd, pendown, dx + shear * dy, dy);
                xcurr = xnew;
                ycurr = ynew;
                pendown = 1;
            }
            glyph_ptr = glyph_ptr.add(2);
        }

        // Final penup stroke
        let dx = xfinal - xcurr;
        let dy = yfinal - ycurr;
        _draw_hershey_stroke(vc, gc, dd, 0, dx + shear * dy, dy);
    }
}

/// Draw a Hershey penup stroke for repositioning during composite characters
unsafe fn _draw_hershey_penup_stroke(
    vc: &mut vfontContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
    dx: c_double,
    dy: c_double,
    charsize: c_double,
    oblique: c_int,
) {
    unsafe {
        let shear = if oblique != 0 { SHEAR } else { 0.0 };
        _draw_hershey_stroke(
            vc,
            gc,
            dd,
            0, // pen up
            charsize * (dx + shear * dy),
            charsize * dy,
        );
    }
}

// ---------------------------------------------------------------------------
// Draw entire Hershey string
// ---------------------------------------------------------------------------

unsafe fn _draw_hershey_string(
    vc: &mut vfontContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
    string: *const u16,
) {
    unsafe {
        let g = gc as *mut R_GE_gcontext;
        let mut ptr = string;
        let mut c: u16;
        let mut charsize: c_double = 1.0;
        let mut line_width_type: c_int = 0; // 0,1,2 = unset,occidental,oriental

        loop {
            c = *ptr;
            ptr = ptr.add(1);
            if c == 0 {
                break;
            }

            if c as c_int & RAW_HERSHEY_GLYPH != 0 {
                if line_width_type != 1 {
                    (*g).lwd = hershey_line_width_to_lwd(HERSHEY_STROKE_WIDTH, gc);
                    line_width_type = 1;
                }
                _draw_hershey_glyph(vc, gc, dd, c as c_int & GLYPH_SPEC, charsize, OCCIDENTAL, 0);
            } else if c as c_int & RAW_ORIENTAL_HERSHEY_GLYPH != 0 {
                if line_width_type != 2 {
                    (*g).lwd = hershey_line_width_to_lwd(HERSHEY_STROKE_WIDTH, gc);
                    line_width_type = 2;
                }
                _draw_hershey_glyph(vc, gc, dd, c as c_int & GLYPH_SPEC, charsize, ORIENTAL, 0);
            } else if c as c_int & CONTROL_CODE != 0 {
                match c as c_int & !CONTROL_CODE {
                    C_BEGIN_SUPERSCRIPT => {
                        _draw_hershey_stroke(
                            vc,
                            gc,
                            dd,
                            0,
                            SUPERSCRIPT_DX * charsize * HERSHEY_EM,
                            SUPERSCRIPT_DY * charsize * HERSHEY_EM,
                        );
                        charsize *= SCRIPTSIZE;
                    }
                    C_END_SUPERSCRIPT => {
                        charsize /= SCRIPTSIZE;
                        _draw_hershey_stroke(
                            vc,
                            gc,
                            dd,
                            0,
                            -SUPERSCRIPT_DX * charsize * HERSHEY_EM,
                            -SUPERSCRIPT_DY * charsize * HERSHEY_EM,
                        );
                    }
                    C_BEGIN_SUBSCRIPT => {
                        _draw_hershey_stroke(
                            vc,
                            gc,
                            dd,
                            0,
                            SUBSCRIPT_DX * charsize * HERSHEY_EM,
                            SUBSCRIPT_DY * charsize * HERSHEY_EM,
                        );
                        charsize *= SCRIPTSIZE;
                    }
                    C_END_SUBSCRIPT => {
                        charsize /= SCRIPTSIZE;
                        _draw_hershey_stroke(
                            vc,
                            gc,
                            dd,
                            0,
                            -SUBSCRIPT_DX * charsize * HERSHEY_EM,
                            -SUBSCRIPT_DY * charsize * HERSHEY_EM,
                        );
                    }
                    C_PUSH_LOCATION | C_POP_LOCATION => {
                        // No-op in drawing mode
                    }
                    C_RIGHT_ONE_EM => {
                        _draw_hershey_stroke(vc, gc, dd, 0, charsize * HERSHEY_EM, 0.0);
                    }
                    C_RIGHT_HALF_EM => {
                        _draw_hershey_stroke(vc, gc, dd, 0, charsize * HERSHEY_EM / 2.0, 0.0);
                    }
                    C_RIGHT_QUARTER_EM => {
                        _draw_hershey_stroke(vc, gc, dd, 0, charsize * HERSHEY_EM / 4.0, 0.0);
                    }
                    C_RIGHT_SIXTH_EM => {
                        _draw_hershey_stroke(vc, gc, dd, 0, charsize * HERSHEY_EM / 6.0, 0.0);
                    }
                    C_RIGHT_EIGHTH_EM => {
                        _draw_hershey_stroke(vc, gc, dd, 0, charsize * HERSHEY_EM / 8.0, 0.0);
                    }
                    C_LEFT_ONE_EM => {
                        _draw_hershey_stroke(vc, gc, dd, 0, -charsize * HERSHEY_EM, 0.0);
                    }
                    C_LEFT_HALF_EM => {
                        _draw_hershey_stroke(vc, gc, dd, 0, -charsize * HERSHEY_EM / 2.0, 0.0);
                    }
                    C_LEFT_QUARTER_EM => {
                        _draw_hershey_stroke(vc, gc, dd, 0, -charsize * HERSHEY_EM / 4.0, 0.0);
                    }
                    C_LEFT_SIXTH_EM => {
                        _draw_hershey_stroke(vc, gc, dd, 0, -charsize * HERSHEY_EM / 6.0, 0.0);
                    }
                    C_LEFT_EIGHTH_EM => {
                        _draw_hershey_stroke(vc, gc, dd, 0, -charsize * HERSHEY_EM / 8.0, 0.0);
                    }
                    _ => {}
                }
            } else {
                // Actual font character
                let raw_fontnum = ((c as c_int >> FONT_SHIFT) & ONE_BYTE) as usize;
                let oblique = _hershey_font_info.with(|v| v.get()[raw_fontnum].obliquing);
                let char_c = c as c_int & !FONT_SPEC;
                let glyphnum = _hershey_font_info.with(|v| v.get()[raw_fontnum].chars[char_c as usize]) as c_int;

                let mut small_kana = false;
                let mut glyphnum_val = glyphnum;

                if glyphnum_val & KS != 0 {
                    glyphnum_val -= KS;
                    small_kana = true;
                }

                match glyphnum_val {
                    ACC0 | ACC1 | ACC2 => {
                        // Composite (accented) character
                        let mut composite = char_c as u8;
                        let mut character: u8 = 0;
                        let mut accent: u8 = 0;

                        let char_glyphnum;
                        let accent_glyphnum;

                        if _composite_char(&mut composite, &mut character, &mut accent) != 0 {
                            char_glyphnum =
                                _hershey_font_info.with(|v| v.get()[raw_fontnum].chars[character as usize]) as c_int;
                            accent_glyphnum =
                                _hershey_font_info.with(|v| v.get()[raw_fontnum].chars[accent as usize]) as c_int;
                        } else {
                            char_glyphnum = UNDE;
                            accent_glyphnum = 0;
                        }

                        let char_glyph = _occidental_hershey_glyphs.with(|v| v.get()[char_glyphnum as usize]);
                        let accent_glyph = _occidental_hershey_glyphs.with(|v| v.get()[accent_glyphnum as usize]);

                        let char_width = if !char_glyph.is_null() && *char_glyph != 0 {
                            *char_glyph.add(1) as c_int - *char_glyph as c_int
                        } else {
                            0
                        };

                        let accent_width = if !accent_glyph.is_null() && *accent_glyph != 0 {
                            *accent_glyph.add(1) as c_int - *accent_glyph as c_int
                        } else {
                            0
                        };

                        if line_width_type != 1 {
                            (*g).lwd = hershey_line_width_to_lwd(HERSHEY_STROKE_WIDTH, gc);
                            line_width_type = 1;
                        }
                        _draw_hershey_glyph(
                            vc,
                            gc,
                            dd,
                            char_glyphnum,
                            charsize,
                            OCCIDENTAL,
                            oblique,
                        );

                        // Back up to draw accent
                        _draw_hershey_penup_stroke(
                            vc,
                            gc,
                            dd,
                            -0.5 * char_width as c_double - 0.5 * accent_width as c_double,
                            0.0,
                            charsize,
                            oblique,
                        );

                        // Repositioning for uppercase and uppercase italic
                        if glyphnum_val == ACC1 {
                            _draw_hershey_penup_stroke(
                                vc,
                                gc,
                                dd,
                                0.0,
                                ACCENT_UP_SHIFT,
                                charsize,
                                oblique,
                            );
                        } else if glyphnum_val == ACC2 {
                            _draw_hershey_penup_stroke(
                                vc,
                                gc,
                                dd,
                                ACCENT_RIGHT_SHIFT,
                                ACCENT_UP_SHIFT,
                                charsize,
                                oblique,
                            );
                        }

                        // Draw the accent
                        _draw_hershey_glyph(
                            vc,
                            gc,
                            dd,
                            accent_glyphnum,
                            charsize,
                            OCCIDENTAL,
                            oblique,
                        );

                        // Undo special repositioning if any
                        if glyphnum_val == ACC1 {
                            _draw_hershey_penup_stroke(
                                vc,
                                gc,
                                dd,
                                0.0,
                                -ACCENT_UP_SHIFT,
                                charsize,
                                oblique,
                            );
                        } else if glyphnum_val == ACC2 {
                            _draw_hershey_penup_stroke(
                                vc,
                                gc,
                                dd,
                                -ACCENT_RIGHT_SHIFT,
                                -ACCENT_UP_SHIFT,
                                charsize,
                                oblique,
                            );
                        }

                        // Move forward
                        _draw_hershey_penup_stroke(
                            vc,
                            gc,
                            dd,
                            0.5 * char_width as c_double - 0.5 * accent_width as c_double,
                            0.0,
                            charsize,
                            oblique,
                        );
                    }
                    _ => {
                        // Ordinary glyph (possibly small Kana)
                        if small_kana {
                            let kana_glyph = _occidental_hershey_glyphs.with(|v| v.get()[glyphnum_val as usize]);
                            let kana_width = if !kana_glyph.is_null() && *kana_glyph != 0 {
                                *kana_glyph.add(1) as c_int - *kana_glyph as c_int
                            } else {
                                0
                            };
                            let shift = 0.5 * (1.0 - SMALL_KANA_SIZE);

                            _draw_hershey_penup_stroke(
                                vc,
                                gc,
                                dd,
                                shift * kana_width as c_double,
                                0.0,
                                charsize,
                                oblique,
                            );
                            if line_width_type != 2 {
                                (*g).lwd = hershey_line_width_to_lwd(HERSHEY_STROKE_WIDTH, gc);
                                line_width_type = 2;
                            }
                            _draw_hershey_glyph(
                                vc,
                                gc,
                                dd,
                                glyphnum_val,
                                SMALL_KANA_SIZE * charsize,
                                OCCIDENTAL,
                                oblique,
                            );
                            _draw_hershey_penup_stroke(
                                vc,
                                gc,
                                dd,
                                shift * kana_width as c_double,
                                0.0,
                                charsize,
                                oblique,
                            );
                        } else {
                            if glyphnum_val >= BEGINNING_OF_KANA {
                                if line_width_type != 2 {
                                    (*g).lwd = hershey_line_width_to_lwd(
                                        HERSHEY_ORIENTAL_STROKE_WIDTH,
                                        gc,
                                    );
                                    line_width_type = 2;
                                }
                            } else if line_width_type != 1 {
                                (*g).lwd = hershey_line_width_to_lwd(HERSHEY_STROKE_WIDTH, gc);
                                line_width_type = 1;
                            }
                            _draw_hershey_glyph(
                                vc,
                                gc,
                                dd,
                                glyphnum_val,
                                charsize,
                                OCCIDENTAL,
                                oblique,
                            );
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Composite character lookup
// ---------------------------------------------------------------------------

unsafe fn _composite_char(composite: *mut u8, character: *mut u8, accent: *mut u8) -> c_int {
    unsafe {
        let mut found = 0;
        let given = *composite;
        let mut idx = 0;

        loop {
            let compchar = _hershey_accented_char_info.with(|v| v.get()[idx]);
            if compchar.composite == 0 {
                break;
            }
            if compchar.composite == given {
                found = 1;
                *character = compchar.character;
                *accent = compchar.accent;
            }
            idx += 1;
            if idx >= _hershey_accented_char_info.with(|v| v.get().len()) {
                break;
            }
        }

        found
    }
}

// ---------------------------------------------------------------------------
// R_FINITE helper
// ---------------------------------------------------------------------------

fn R_FINITE(x: c_double) -> bool {
    x.is_finite()
}

// ---------------------------------------------------------------------------
// LTY_SOLID and GE_ROUND_CAP/JOIN constants
// TODO: These are defined in engine.rs; re-export or import
// ---------------------------------------------------------------------------

const LTY_SOLID_LOCAL: c_int = 0;
const GE_ROUND_CAP_LOCAL: c_int = 1;
const GE_ROUND_JOIN_LOCAL: c_int = 1;

// ---------------------------------------------------------------------------
// Public API functions
// ---------------------------------------------------------------------------

/// Calculate the width of a string rendered with Hershey vector fonts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GE_VStrWidth(
    s: *const c_char,
    _enc: c_int,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> c_double {
    unsafe {
        let vmax = vmaxget();

        let fontfamily_char = if !gc.is_null() {
            (*gc).fontfamily[7]
        } else {
            0
        };
        let fontindex = (fontfamily_char as c_int).saturating_sub(1);
        let fontface = if !gc.is_null() { (*gc).fontface } else { 1 };

        let codestring = _controlify(dd, s as *const u8, fontindex, fontface);

        let label_width = _label_width_hershey(gc, dd, codestring);

        vmaxset(vmax);

        label_width
    }
}

/// Calculate the height of a string rendered with Hershey vector fonts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GE_VStrHeight(
    s: *const c_char,
    _enc: c_int,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> c_double {
    unsafe {
        let vmax = vmaxget();

        let fontfamily_char = if !gc.is_null() {
            (*gc).fontfamily[7]
        } else {
            0
        };
        let fontindex = (fontfamily_char as c_int).saturating_sub(1);
        let fontface = if !gc.is_null() { (*gc).fontface } else { 1 };

        let codestring = _controlify(dd, s as *const u8, fontindex, fontface);

        let label_height = _label_height_hershey(gc, dd, codestring);

        vmaxset(vmax);

        label_height
    }
}

/// Render text using Hershey vector fonts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GE_VText(
    x: c_double,
    y: c_double,
    s: *const c_char,
    _enc: c_int,
    x_justify: c_double,
    y_justify: c_double,
    rotation: c_double,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) {
    unsafe {
        let vmax = vmaxget();

        let mut vc = vfontContext {
            currX: fromDeviceX(x, GE_INCHES, dd as *mut _),
            currY: fromDeviceY(y, GE_INCHES, dd as *mut _),
            angle: rotation,
        };

        // Override gc settings for lty and lwd
        let gc_mut = gc as *mut R_GE_gcontext;
        (*gc_mut).lty = LTY_SOLID_LOCAL;
        (*gc_mut).lwd = hershey_line_width_to_lwd(HERSHEY_STROKE_WIDTH, gc);
        (*gc_mut).lend = GE_ROUND_CAP_LOCAL;
        (*gc_mut).ljoin = GE_ROUND_JOIN_LOCAL;

        let fontfamily_char = if !gc.is_null() {
            (*gc).fontfamily[7]
        } else {
            0
        };
        let fontindex = (fontfamily_char as c_int).saturating_sub(1);
        let fontface = if !gc.is_null() { (*gc).fontface } else { 1 };

        let codestring = _controlify(dd, s as *const u8, fontindex, fontface);

        // Calculate dimensions
        let label_width = _label_width_hershey(gc, dd, codestring);
        let label_height = _label_height_hershey(gc, dd, codestring);

        let mut x_justify_val = x_justify;
        let mut y_justify_val = y_justify;
        if !R_FINITE(x_justify_val) {
            x_justify_val = 0.5;
        }
        if !R_FINITE(y_justify_val) {
            y_justify_val = 0.5;
        }

        let x_offset = 0.0 - x_justify_val;
        let y_offset = 0.0 - y_justify_val * 1.0;

        // Move to starting position
        _draw_stroke(
            &mut vc,
            gc,
            dd,
            0,
            fromDeviceWidth(x_offset * label_width, GE_INCHES, dd as *mut _),
            fromDeviceHeight(y_offset * label_height, GE_INCHES, dd as *mut _),
        );

        // Draw the string
        _draw_hershey_string(&mut vc, gc, dd, codestring);

        vmaxset(vmax);
    }
}
