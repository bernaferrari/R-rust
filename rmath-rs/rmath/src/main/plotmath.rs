#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/main/plotmath.c (3237 lines)
 *
 *  Math expression rendering for R graphics.
 *  Implements TeX-like mathematical typesetting for expressions like
 *  sqrt, integral, sum, fraction, subscript, superscript, etc.
 */

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};

use crate::main::engine::{
    GE_INCHES, GEDevDesc, GEMetricInfo, GEPolyline, GEStrWidth, GEText, LTY_SOLID, R_GE_gcontext,
    fromDeviceHeight, fromDeviceX, fromDeviceY, toDeviceHeight, toDeviceWidth,
};
use crate::sexp::accessors::{
    CADDDR, CADDR, CADR, CAR, CDR, CHAR, LENGTH, PRINTNAME, STRING_ELT, TYPEOF,
};
use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

const CE_NATIVE: c_int = 0;
const CE_SYMBOL: c_int = 5;

// ===========================================================================
// TeX Math Styles (TeXBook Appendix G, Page 441)
// ===========================================================================

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum STYLE {
    SS1 = 1,
    SS = 2,
    S1 = 3,
    S = 4,
    T1 = 5,
    T = 6,
    D1 = 7,
    D = 8,
}

// ===========================================================================
// Math Context
// ===========================================================================

struct mathContext {
    BoxColor: u32,
    BaseCex: c_double,
    ReferenceX: c_double,
    ReferenceY: c_double,
    CurrentX: c_double,
    CurrentY: c_double,
    CurrentAngle: c_double,
    CosAngle: c_double,
    SinAngle: c_double,
    CurrentStyle: STYLE,
}

static MetricUnit: c_int = GE_INCHES;

// ===========================================================================
// Font Definitions
// ===========================================================================

#[derive(Clone, Copy)]
enum FontType {
    Plain = 1,
    Bold = 2,
    Italic = 3,
    BoldItalic = 4,
    Symbol = 5,
}

// ===========================================================================
// Constants
// ===========================================================================

static ItalicFactor: c_double = 0.15;

// Special math ASCII codes
const A_HAT: c_int = 94;
const A_TILDE: c_int = 126;
const S_SPACE: c_int = 32;
const S_PARENLEFT: c_int = 40;
const S_PARENRIGHT: c_int = 41;
const S_ASTERISKMATH: c_int = 42;
const S_COMMA: c_int = 44;
const S_SLASH: c_int = 47;
const S_FRACTION: c_int = 164;
const S_ELLIPSIS: c_int = 188;
const S_INTERSECTION: c_int = 199;
const S_UNION: c_int = 200;
const S_PRODUCT: c_int = 213;
const S_RADICAL: c_int = 214;
const S_SUM: c_int = 229;
const S_INTEGRAL: c_int = 242;
const S_ANGLELEFT: c_int = 225;
const S_BRACKETLEFTTP: c_int = 233;
const S_BRACKETLEFTBT: c_int = 235;
const S_ANGLERIGHT: c_int = 241;
const S_BRACKETRIGHTTP: c_int = 249;
const S_BRACKETRIGHTBT: c_int = 251;

const N_LIM: c_int = 1001;
const N_LIMINF: c_int = 1002;
const N_INF: c_int = 1004;
const N_SUP: c_int = 1005;
const N_MIN: c_int = 1006;
const N_MAX: c_int = 1007;

const SUBS: c_double = 0.7;
const ACCENT_GAP: c_double = 0.2;
const HAT_HEIGHT: c_double = 0.3;
const NTILDE: c_int = 8;
const DELTA: c_double = 0.05;
const DelimSymbolMag: c_double = 1.25;
const OperatorSymbolMag: c_double = 1.25;
const RADICAL_GAP: c_double = 0.4;
const RADICAL_SPACE: c_double = 0.2;

// ===========================================================================
// TeX Layout Parameters
// ===========================================================================

#[derive(Clone, Copy)]
enum TEXPAR {
    sigma2 = 0,
    sigma5 = 1,
    sigma6 = 2,
    sigma8 = 3,
    sigma9 = 4,
    sigma10 = 5,
    sigma11 = 6,
    sigma12 = 7,
    sigma13 = 8,
    sigma14 = 9,
    sigma15 = 10,
    sigma16 = 11,
    sigma17 = 12,
    sigma18 = 13,
    sigma19 = 14,
    sigma20 = 15,
    sigma21 = 16,
    sigma22 = 17,
    xi8 = 18,
    xi9 = 19,
    xi10 = 20,
    xi11 = 21,
    xi12 = 22,
    xi13 = 23,
}

// ===========================================================================
// Bounding Box
// ===========================================================================

#[derive(Clone, Copy, Default)]
struct BBOX {
    height: c_double,
    depth: c_double,
    width: c_double,
    italic: c_double,
    simple: c_int,
}

// ===========================================================================
// Symbol Table Entry
// ===========================================================================

struct SymTab {
    name: *const c_char,
    code: c_int,
}

// ===========================================================================
// Drawing position helpers
// ===========================================================================

unsafe fn ConvertedX(mc: &mathContext, dd: *mut GEDevDesc) -> c_double {
    unsafe {
        let rx = mc.ReferenceX + (mc.CurrentX - mc.ReferenceX) * mc.CosAngle
            - (mc.CurrentY - mc.ReferenceY) * mc.SinAngle;
        crate::main::engine::toDeviceX(rx, MetricUnit, dd as *mut _)
    }
}

unsafe fn ConvertedY(mc: &mathContext, dd: *mut GEDevDesc) -> c_double {
    unsafe {
        let ry = mc.ReferenceY
            + (mc.CurrentY - mc.ReferenceY) * mc.CosAngle
            + (mc.CurrentX - mc.ReferenceX) * mc.SinAngle;
        crate::main::engine::toDeviceY(ry, MetricUnit, dd as *mut _)
    }
}

fn PMoveAcross(amt: c_double, mc: &mut mathContext) {
    mc.CurrentX += amt;
}
fn PMoveUp(amt: c_double, mc: &mut mathContext) {
    mc.CurrentY += amt;
}
fn PMoveTo(x: c_double, y: c_double, mc: &mut mathContext) {
    mc.CurrentX = x;
    mc.CurrentY = y;
}

// ===========================================================================
// Font metric helpers
// ===========================================================================

unsafe fn metric_info(
    ch: c_int,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> (c_double, c_double, c_double) {
    unsafe {
        let mut h = 0.0;
        let mut d = 0.0;
        let mut w = 0.0;
        GEMetricInfo(ch, gc as *const _, &mut h, &mut d, &mut w, dd as *mut _);
        (h, d, w)
    }
}

unsafe fn xHeight(gc: *const R_GE_gcontext, dd: *mut GEDevDesc) -> c_double {
    unsafe {
        let (h, _, _) = metric_info('x' as c_int, gc, dd);
        fromDeviceHeight(h, MetricUnit, dd as *mut _)
    }
}

unsafe fn XHeight(gc: *const R_GE_gcontext, dd: *mut GEDevDesc) -> c_double {
    unsafe {
        let (h, _, _) = metric_info('X' as c_int, gc, dd);
        fromDeviceHeight(h, MetricUnit, dd as *mut _)
    }
}

unsafe fn AxisHeight(gc: *const R_GE_gcontext, dd: *mut GEDevDesc) -> c_double {
    unsafe {
        let (h, _, _) = metric_info('+' as c_int, gc, dd);
        fromDeviceHeight(0.5 * h, MetricUnit, dd as *mut _)
    }
}

unsafe fn Quad(gc: *const R_GE_gcontext, dd: *mut GEDevDesc) -> c_double {
    unsafe {
        let (_, _, w) = metric_info('M' as c_int, gc, dd);
        fromDeviceHeight(w, MetricUnit, dd as *mut _)
    }
}

unsafe fn FigHeight(gc: *const R_GE_gcontext, dd: *mut GEDevDesc) -> c_double {
    unsafe {
        let (h, _, _) = metric_info('0' as c_int, gc, dd);
        fromDeviceHeight(h, MetricUnit, dd as *mut _)
    }
}

unsafe fn DescDepth(gc: *const R_GE_gcontext, dd: *mut GEDevDesc) -> c_double {
    unsafe {
        let (_, d, _) = metric_info('g' as c_int, gc, dd);
        fromDeviceHeight(d, MetricUnit, dd as *mut _)
    }
}

fn RuleThickness() -> c_double {
    0.015
}

unsafe fn thin_space(gc: *const R_GE_gcontext, dd: *mut GEDevDesc) -> c_double {
    unsafe {
        let (_, _, w) = metric_info('M' as c_int, gc, dd);
        fromDeviceHeight(0.16666666666666666666 * w, MetricUnit, dd as *mut _)
    }
}

unsafe fn medium_space(gc: *const R_GE_gcontext, dd: *mut GEDevDesc) -> c_double {
    unsafe {
        let (_, _, w) = metric_info('M' as c_int, gc, dd);
        fromDeviceHeight(0.22222222222222222222 * w, MetricUnit, dd as *mut _)
    }
}

unsafe fn thick_space(gc: *const R_GE_gcontext, dd: *mut GEDevDesc) -> c_double {
    unsafe {
        let (_, _, w) = metric_info('M' as c_int, gc, dd);
        fromDeviceHeight(0.27777777777777777777 * w, MetricUnit, dd as *mut _)
    }
}

unsafe fn mu_space(gc: *const R_GE_gcontext, dd: *mut GEDevDesc) -> c_double {
    unsafe {
        let (_, _, w) = metric_info('M' as c_int, gc, dd);
        fromDeviceHeight(0.05555555555555555555 * w, MetricUnit, dd as *mut _)
    }
}

// ===========================================================================
// TeX layout parameter calculation
// ===========================================================================

unsafe fn TeX(which: TEXPAR, gc: *const R_GE_gcontext, dd: *mut GEDevDesc) -> c_double {
    unsafe {
        match which {
            TEXPAR::sigma2 | TEXPAR::sigma5 => xHeight(gc, dd),
            TEXPAR::sigma6 => Quad(gc, dd),
            TEXPAR::sigma8 => {
                AxisHeight(gc, dd)
                    + 3.51 * RuleThickness()
                    + 0.15 * XHeight(gc, dd)
                    + SUBS * DescDepth(gc, dd)
            }
            TEXPAR::sigma9 => {
                AxisHeight(gc, dd) + 1.51 * RuleThickness() + 0.08333333 * XHeight(gc, dd)
            }
            TEXPAR::sigma10 => {
                AxisHeight(gc, dd) + 1.51 * RuleThickness() + 0.1333333 * XHeight(gc, dd)
            }
            TEXPAR::sigma11 => {
                -AxisHeight(gc, dd)
                    + 3.51 * RuleThickness()
                    + SUBS * FigHeight(gc, dd)
                    + 0.344444 * XHeight(gc, dd)
            }
            TEXPAR::sigma12 => {
                -AxisHeight(gc, dd)
                    + 1.51 * RuleThickness()
                    + SUBS * FigHeight(gc, dd)
                    + 0.08333333 * XHeight(gc, dd)
            }
            TEXPAR::sigma13 => 0.95 * xHeight(gc, dd),
            TEXPAR::sigma14 => 0.825 * xHeight(gc, dd),
            TEXPAR::sigma15 => 0.7 * xHeight(gc, dd),
            TEXPAR::sigma16 => 0.35 * xHeight(gc, dd),
            TEXPAR::sigma17 => 0.45 * XHeight(gc, dd),
            TEXPAR::sigma18 => 0.3861111 * XHeight(gc, dd),
            TEXPAR::sigma19 => 0.05 * XHeight(gc, dd),
            TEXPAR::sigma20 => 2.39 * XHeight(gc, dd),
            TEXPAR::sigma21 => 1.01 * XHeight(gc, dd),
            TEXPAR::sigma22 => AxisHeight(gc, dd),
            TEXPAR::xi8 => RuleThickness(),
            TEXPAR::xi9 | TEXPAR::xi10 | TEXPAR::xi11 | TEXPAR::xi12 | TEXPAR::xi13 => {
                0.15 * XHeight(gc, dd)
            }
        }
    }
}

// ===========================================================================
// Style management
// ===========================================================================

fn GetStyle(mc: &mathContext) -> STYLE {
    mc.CurrentStyle
}

unsafe fn SetStyle(newstyle: STYLE, mc: &mut mathContext, gc: *const R_GE_gcontext) {
    unsafe {
        let g = gc as *mut R_GE_gcontext;
        match newstyle {
            STYLE::D | STYLE::T | STYLE::D1 | STYLE::T1 => {
                (*g).cex = 1.0 * mc.BaseCex;
            }
            STYLE::S | STYLE::S1 => {
                (*g).cex = 0.7 * mc.BaseCex;
            }
            STYLE::SS | STYLE::SS1 => {
                (*g).cex = 0.5 * mc.BaseCex;
            }
        }
        mc.CurrentStyle = newstyle;
    }
}

unsafe fn SetPrimeStyle(s: STYLE, mc: &mut mathContext, gc: *const R_GE_gcontext) {
    unsafe {
        SetStyle(
            match s {
                STYLE::D | STYLE::D1 => STYLE::D1,
                STYLE::T | STYLE::T1 => STYLE::T1,
                STYLE::S | STYLE::S1 => STYLE::S1,
                STYLE::SS | STYLE::SS1 => STYLE::SS1,
            },
            mc,
            gc,
        );
    }
}

unsafe fn SetSupStyle(s: STYLE, mc: &mut mathContext, gc: *const R_GE_gcontext) {
    unsafe {
        SetStyle(
            match s {
                STYLE::D | STYLE::T => STYLE::S,
                STYLE::D1 | STYLE::T1 => STYLE::S1,
                STYLE::S | STYLE::SS => STYLE::SS,
                STYLE::S1 | STYLE::SS1 => STYLE::SS1,
            },
            mc,
            gc,
        );
    }
}

unsafe fn SetSubStyle(s: STYLE, mc: &mut mathContext, gc: *const R_GE_gcontext) {
    unsafe {
        SetStyle(
            match s {
                STYLE::D | STYLE::T | STYLE::D1 | STYLE::T1 => STYLE::S1,
                STYLE::S | STYLE::SS | STYLE::S1 | STYLE::SS1 => STYLE::SS1,
            },
            mc,
            gc,
        );
    }
}

unsafe fn SetNumStyle(s: STYLE, mc: &mut mathContext, gc: *const R_GE_gcontext) {
    unsafe {
        match s {
            STYLE::D => SetStyle(STYLE::T, mc, gc),
            STYLE::D1 => SetStyle(STYLE::T1, mc, gc),
            _ => SetSupStyle(s, mc, gc),
        }
    }
}

unsafe fn SetDenomStyle(s: STYLE, mc: &mut mathContext, gc: *const R_GE_gcontext) {
    unsafe {
        if s > STYLE::T {
            SetStyle(STYLE::T1, mc, gc);
        } else {
            SetSubStyle(s, mc, gc);
        }
    }
}

fn IsCompactStyle(s: STYLE) -> c_int {
    match s {
        STYLE::D1 | STYLE::T1 | STYLE::S1 | STYLE::SS1 => 1,
        _ => 0,
    }
}

// ===========================================================================
// Utility
// ===========================================================================

fn fmax(x: c_double, y: c_double) -> c_double {
    if x > y { x } else { y }
}
fn R_FINITE(x: c_double) -> bool {
    x.is_finite()
}

// ===========================================================================
// BBox operations
// ===========================================================================

fn MakeBBox(h: c_double, d: c_double, w: c_double) -> BBOX {
    BBOX {
        height: h,
        depth: d,
        width: w,
        italic: 0.0,
        simple: 0,
    }
}
fn NullBBox() -> BBOX {
    BBOX::default()
}

fn ShiftBBox(mut b: BBOX, sv: c_double) -> BBOX {
    b.height += sv;
    b.depth -= sv;
    b
}

fn EnlargeBBox(mut b: BBOX, dh: c_double, dd: c_double, dw: c_double) -> BBOX {
    b.height += dh;
    b.depth += dd;
    b.width += dw;
    b
}

fn CombineBBoxes(mut a: BBOX, b: BBOX) -> BBOX {
    a.height = fmax(a.height, b.height);
    a.depth = fmax(a.depth, b.depth);
    a.width += b.width;
    a.italic = b.italic;
    a.simple = b.simple;
    a
}

fn CombineAlignedBBoxes(mut a: BBOX, b: BBOX) -> BBOX {
    a.height = fmax(a.height, b.height);
    a.depth = fmax(a.depth, b.depth);
    a.width = fmax(a.width, b.width);
    a.italic = 0.0;
    a.simple = 0;
    a
}

fn CombineOffsetBBoxes(
    mut a: BBOX,
    i1: c_int,
    b: BBOX,
    i2: c_int,
    xo: c_double,
    yo: c_double,
) -> BBOX {
    let w1 = a.width + if i1 != 0 { a.italic } else { 0.0 };
    let w2 = b.width + if i2 != 0 { b.italic } else { 0.0 };
    a.width = fmax(w1, w2 + xo);
    a.height = fmax(a.height, b.height + yo);
    a.depth = fmax(a.depth, b.depth - yo);
    a.italic = 0.0;
    a.simple = 0;
    a
}

fn CenterShift(b: BBOX) -> c_double {
    0.5 * (b.height - b.depth)
}

// ===========================================================================
// Expression helpers
// ===========================================================================

unsafe fn isSymbol(t: c_int) -> bool {
    t == crate::sexp::ffi::SEXPTYPE::SYMSXP.0 as c_int
}
unsafe fn isLang(t: c_int) -> bool {
    t == crate::sexp::ffi::SEXPTYPE::LANGSXP.0 as c_int
}
unsafe fn isReal(t: c_int) -> bool {
    t == crate::sexp::ffi::SEXPTYPE::REALSXP.0 as c_int
}
unsafe fn isInt(t: c_int) -> bool {
    t == crate::sexp::ffi::SEXPTYPE::INTSXP.0 as c_int
}
unsafe fn isCplx(t: c_int) -> bool {
    t == crate::sexp::ffi::SEXPTYPE::CPLXSXP.0 as c_int
}
unsafe fn isStr(t: c_int) -> bool {
    t == crate::sexp::ffi::SEXPTYPE::STRSXP.0 as c_int
}

unsafe fn NameMatch(expr: SEXP, aString: *const c_char) -> c_int {
    unsafe {
        if !isSymbol(TYPEOF(expr)) {
            return 0;
        }
        let p = CHAR(PRINTNAME(expr));
        if CStr::from_ptr(aString) == CStr::from_ptr(p as *const c_char) {
            1
        } else {
            0
        }
    }
}

unsafe fn StringMatch(expr: SEXP, aString: *const c_char) -> c_int {
    unsafe {
        let p = CHAR(STRING_ELT(expr, 0));
        if CStr::from_ptr(aString) == CStr::from_ptr(p as *const c_char) {
            1
        } else {
            0
        }
    }
}

unsafe fn translateChar(x: SEXP) -> *const c_char {
    unsafe { CHAR(x) as *const c_char }
}
unsafe fn asChar(expr: SEXP) -> SEXP {
    expr
} // TODO: full impl
fn PrintDefaults() {} // TODO: full impl
fn mbcslocale() -> bool {
    false
} // TODO: full impl

// ===========================================================================
// Font helpers
// ===========================================================================

unsafe fn GetFont(gc: *const R_GE_gcontext) -> FontType {
    unsafe {
        match (*gc).fontface {
            2 => FontType::Bold,
            3 => FontType::Italic,
            4 => FontType::BoldItalic,
            5 => FontType::Symbol,
            _ => FontType::Plain,
        }
    }
}

unsafe fn SetFont(f: FontType, gc: *const R_GE_gcontext) -> FontType {
    unsafe {
        let prev = GetFont(gc);
        let g = gc as *mut R_GE_gcontext;
        (*g).fontface = f as c_int;
        prev
    }
}

unsafe fn UsingItalics(gc: *const R_GE_gcontext) -> c_int {
    unsafe {
        if (*gc).fontface == FontType::Italic as c_int
            || (*gc).fontface == FontType::BoldItalic as c_int
        {
            1
        } else {
            0
        }
    }
}

// ===========================================================================
// The Full Adobe Symbol Font Table
// ===========================================================================

static mut SymbolTable: [SymTab; 192] = [
    SymTab {
        name: b"space\0".as_ptr() as *const c_char,
        code: 32,
    },
    SymTab {
        name: b"exclam\0".as_ptr() as *const c_char,
        code: 33,
    },
    SymTab {
        name: b"universal\0".as_ptr() as *const c_char,
        code: 34,
    },
    SymTab {
        name: b"numbersign\0".as_ptr() as *const c_char,
        code: 35,
    },
    SymTab {
        name: b"existential\0".as_ptr() as *const c_char,
        code: 36,
    },
    SymTab {
        name: b"percent\0".as_ptr() as *const c_char,
        code: 37,
    },
    SymTab {
        name: b"ampersand\0".as_ptr() as *const c_char,
        code: 38,
    },
    SymTab {
        name: b"suchthat\0".as_ptr() as *const c_char,
        code: 39,
    },
    SymTab {
        name: b"parenleft\0".as_ptr() as *const c_char,
        code: 40,
    },
    SymTab {
        name: b"parenright\0".as_ptr() as *const c_char,
        code: 41,
    },
    SymTab {
        name: b"asteriskmath\0".as_ptr() as *const c_char,
        code: 42,
    },
    SymTab {
        name: b"plus\0".as_ptr() as *const c_char,
        code: 43,
    },
    SymTab {
        name: b"comma\0".as_ptr() as *const c_char,
        code: 44,
    },
    SymTab {
        name: b"minus\0".as_ptr() as *const c_char,
        code: 45,
    },
    SymTab {
        name: b"period\0".as_ptr() as *const c_char,
        code: 46,
    },
    SymTab {
        name: b"slash\0".as_ptr() as *const c_char,
        code: 47,
    },
    SymTab {
        name: b"0\0".as_ptr() as *const c_char,
        code: 48,
    },
    SymTab {
        name: b"1\0".as_ptr() as *const c_char,
        code: 49,
    },
    SymTab {
        name: b"2\0".as_ptr() as *const c_char,
        code: 50,
    },
    SymTab {
        name: b"3\0".as_ptr() as *const c_char,
        code: 51,
    },
    SymTab {
        name: b"4\0".as_ptr() as *const c_char,
        code: 52,
    },
    SymTab {
        name: b"5\0".as_ptr() as *const c_char,
        code: 53,
    },
    SymTab {
        name: b"6\0".as_ptr() as *const c_char,
        code: 54,
    },
    SymTab {
        name: b"7\0".as_ptr() as *const c_char,
        code: 55,
    },
    SymTab {
        name: b"8\0".as_ptr() as *const c_char,
        code: 56,
    },
    SymTab {
        name: b"9\0".as_ptr() as *const c_char,
        code: 57,
    },
    SymTab {
        name: b"colon\0".as_ptr() as *const c_char,
        code: 58,
    },
    SymTab {
        name: b"semicolon\0".as_ptr() as *const c_char,
        code: 59,
    },
    SymTab {
        name: b"less\0".as_ptr() as *const c_char,
        code: 60,
    },
    SymTab {
        name: b"equal\0".as_ptr() as *const c_char,
        code: 61,
    },
    SymTab {
        name: b"greater\0".as_ptr() as *const c_char,
        code: 62,
    },
    SymTab {
        name: b"question\0".as_ptr() as *const c_char,
        code: 63,
    },
    SymTab {
        name: b"congruent\0".as_ptr() as *const c_char,
        code: 64,
    },
    SymTab {
        name: b"Alpha\0".as_ptr() as *const c_char,
        code: 65,
    },
    SymTab {
        name: b"Beta\0".as_ptr() as *const c_char,
        code: 66,
    },
    SymTab {
        name: b"Chi\0".as_ptr() as *const c_char,
        code: 67,
    },
    SymTab {
        name: b"Delta\0".as_ptr() as *const c_char,
        code: 68,
    },
    SymTab {
        name: b"Epsilon\0".as_ptr() as *const c_char,
        code: 69,
    },
    SymTab {
        name: b"Phi\0".as_ptr() as *const c_char,
        code: 70,
    },
    SymTab {
        name: b"Gamma\0".as_ptr() as *const c_char,
        code: 71,
    },
    SymTab {
        name: b"Eta\0".as_ptr() as *const c_char,
        code: 72,
    },
    SymTab {
        name: b"Iota\0".as_ptr() as *const c_char,
        code: 73,
    },
    SymTab {
        name: b"theta1\0".as_ptr() as *const c_char,
        code: 74,
    },
    SymTab {
        name: b"vartheta\0".as_ptr() as *const c_char,
        code: 74,
    },
    SymTab {
        name: b"Kappa\0".as_ptr() as *const c_char,
        code: 75,
    },
    SymTab {
        name: b"Lambda\0".as_ptr() as *const c_char,
        code: 76,
    },
    SymTab {
        name: b"Mu\0".as_ptr() as *const c_char,
        code: 77,
    },
    SymTab {
        name: b"Nu\0".as_ptr() as *const c_char,
        code: 78,
    },
    SymTab {
        name: b"Omicron\0".as_ptr() as *const c_char,
        code: 79,
    },
    SymTab {
        name: b"Pi\0".as_ptr() as *const c_char,
        code: 80,
    },
    SymTab {
        name: b"Theta\0".as_ptr() as *const c_char,
        code: 81,
    },
    SymTab {
        name: b"Rho\0".as_ptr() as *const c_char,
        code: 82,
    },
    SymTab {
        name: b"Sigma\0".as_ptr() as *const c_char,
        code: 83,
    },
    SymTab {
        name: b"Tau\0".as_ptr() as *const c_char,
        code: 84,
    },
    SymTab {
        name: b"Upsilon\0".as_ptr() as *const c_char,
        code: 85,
    },
    SymTab {
        name: b"sigma1\0".as_ptr() as *const c_char,
        code: 86,
    },
    SymTab {
        name: b"varsigma\0".as_ptr() as *const c_char,
        code: 86,
    },
    SymTab {
        name: b"stigma\0".as_ptr() as *const c_char,
        code: 86,
    },
    SymTab {
        name: b"Omega\0".as_ptr() as *const c_char,
        code: 87,
    },
    SymTab {
        name: b"Xi\0".as_ptr() as *const c_char,
        code: 88,
    },
    SymTab {
        name: b"Psi\0".as_ptr() as *const c_char,
        code: 89,
    },
    SymTab {
        name: b"Zeta\0".as_ptr() as *const c_char,
        code: 90,
    },
    SymTab {
        name: b"bracketleft\0".as_ptr() as *const c_char,
        code: 91,
    },
    SymTab {
        name: b"therefore\0".as_ptr() as *const c_char,
        code: 92,
    },
    SymTab {
        name: b"bracketright\0".as_ptr() as *const c_char,
        code: 93,
    },
    SymTab {
        name: b"perpendicular\0".as_ptr() as *const c_char,
        code: 94,
    },
    SymTab {
        name: b"underscore\0".as_ptr() as *const c_char,
        code: 95,
    },
    SymTab {
        name: b"radicalex\0".as_ptr() as *const c_char,
        code: 96,
    },
    SymTab {
        name: b"alpha\0".as_ptr() as *const c_char,
        code: 97,
    },
    SymTab {
        name: b"beta\0".as_ptr() as *const c_char,
        code: 98,
    },
    SymTab {
        name: b"chi\0".as_ptr() as *const c_char,
        code: 99,
    },
    SymTab {
        name: b"delta\0".as_ptr() as *const c_char,
        code: 100,
    },
    SymTab {
        name: b"epsilon\0".as_ptr() as *const c_char,
        code: 101,
    },
    SymTab {
        name: b"phi\0".as_ptr() as *const c_char,
        code: 102,
    },
    SymTab {
        name: b"gamma\0".as_ptr() as *const c_char,
        code: 103,
    },
    SymTab {
        name: b"eta\0".as_ptr() as *const c_char,
        code: 104,
    },
    SymTab {
        name: b"iota\0".as_ptr() as *const c_char,
        code: 105,
    },
    SymTab {
        name: b"phi1\0".as_ptr() as *const c_char,
        code: 106,
    },
    SymTab {
        name: b"varphi\0".as_ptr() as *const c_char,
        code: 106,
    },
    SymTab {
        name: b"kappa\0".as_ptr() as *const c_char,
        code: 107,
    },
    SymTab {
        name: b"lambda\0".as_ptr() as *const c_char,
        code: 108,
    },
    SymTab {
        name: b"mu\0".as_ptr() as *const c_char,
        code: 109,
    },
    SymTab {
        name: b"nu\0".as_ptr() as *const c_char,
        code: 110,
    },
    SymTab {
        name: b"omicron\0".as_ptr() as *const c_char,
        code: 111,
    },
    SymTab {
        name: b"pi\0".as_ptr() as *const c_char,
        code: 112,
    },
    SymTab {
        name: b"theta\0".as_ptr() as *const c_char,
        code: 113,
    },
    SymTab {
        name: b"rho\0".as_ptr() as *const c_char,
        code: 114,
    },
    SymTab {
        name: b"sigma\0".as_ptr() as *const c_char,
        code: 115,
    },
    SymTab {
        name: b"tau\0".as_ptr() as *const c_char,
        code: 116,
    },
    SymTab {
        name: b"upsilon\0".as_ptr() as *const c_char,
        code: 117,
    },
    SymTab {
        name: b"omega1\0".as_ptr() as *const c_char,
        code: 118,
    },
    SymTab {
        name: b"omega\0".as_ptr() as *const c_char,
        code: 119,
    },
    SymTab {
        name: b"xi\0".as_ptr() as *const c_char,
        code: 120,
    },
    SymTab {
        name: b"psi\0".as_ptr() as *const c_char,
        code: 121,
    },
    SymTab {
        name: b"zeta\0".as_ptr() as *const c_char,
        code: 122,
    },
    SymTab {
        name: b"braceleft\0".as_ptr() as *const c_char,
        code: 123,
    },
    SymTab {
        name: b"bar\0".as_ptr() as *const c_char,
        code: 124,
    },
    SymTab {
        name: b"braceright\0".as_ptr() as *const c_char,
        code: 125,
    },
    SymTab {
        name: b"similar\0".as_ptr() as *const c_char,
        code: 126,
    },
    SymTab {
        name: b"Upsilon1\0".as_ptr() as *const c_char,
        code: 161,
    },
    SymTab {
        name: b"minute\0".as_ptr() as *const c_char,
        code: 162,
    },
    SymTab {
        name: b"lessequal\0".as_ptr() as *const c_char,
        code: 163,
    },
    SymTab {
        name: b"fraction\0".as_ptr() as *const c_char,
        code: 164,
    },
    SymTab {
        name: b"infinity\0".as_ptr() as *const c_char,
        code: 165,
    },
    SymTab {
        name: b"florin\0".as_ptr() as *const c_char,
        code: 166,
    },
    SymTab {
        name: b"club\0".as_ptr() as *const c_char,
        code: 167,
    },
    SymTab {
        name: b"diamond\0".as_ptr() as *const c_char,
        code: 168,
    },
    SymTab {
        name: b"heart\0".as_ptr() as *const c_char,
        code: 169,
    },
    SymTab {
        name: b"spade\0".as_ptr() as *const c_char,
        code: 170,
    },
    SymTab {
        name: b"arrowboth\0".as_ptr() as *const c_char,
        code: 171,
    },
    SymTab {
        name: b"arrowleft\0".as_ptr() as *const c_char,
        code: 172,
    },
    SymTab {
        name: b"arrowup\0".as_ptr() as *const c_char,
        code: 173,
    },
    SymTab {
        name: b"arrowright\0".as_ptr() as *const c_char,
        code: 174,
    },
    SymTab {
        name: b"arrowdown\0".as_ptr() as *const c_char,
        code: 175,
    },
    SymTab {
        name: b"degree\0".as_ptr() as *const c_char,
        code: 176,
    },
    SymTab {
        name: b"plusminus\0".as_ptr() as *const c_char,
        code: 177,
    },
    SymTab {
        name: b"second\0".as_ptr() as *const c_char,
        code: 178,
    },
    SymTab {
        name: b"greaterequal\0".as_ptr() as *const c_char,
        code: 179,
    },
    SymTab {
        name: b"multiply\0".as_ptr() as *const c_char,
        code: 180,
    },
    SymTab {
        name: b"proportional\0".as_ptr() as *const c_char,
        code: 181,
    },
    SymTab {
        name: b"partialdiff\0".as_ptr() as *const c_char,
        code: 182,
    },
    SymTab {
        name: b"bullet\0".as_ptr() as *const c_char,
        code: 183,
    },
    SymTab {
        name: b"divide\0".as_ptr() as *const c_char,
        code: 184,
    },
    SymTab {
        name: b"notequal\0".as_ptr() as *const c_char,
        code: 185,
    },
    SymTab {
        name: b"equivalence\0".as_ptr() as *const c_char,
        code: 186,
    },
    SymTab {
        name: b"approxequal\0".as_ptr() as *const c_char,
        code: 187,
    },
    SymTab {
        name: b"ellipsis\0".as_ptr() as *const c_char,
        code: 188,
    },
    SymTab {
        name: b"arrowvertex\0".as_ptr() as *const c_char,
        code: 189,
    },
    SymTab {
        name: b"arrowhorizex\0".as_ptr() as *const c_char,
        code: 190,
    },
    SymTab {
        name: b"carriagereturn\0".as_ptr() as *const c_char,
        code: 191,
    },
    SymTab {
        name: b"aleph\0".as_ptr() as *const c_char,
        code: 192,
    },
    SymTab {
        name: b"Ifraktur\0".as_ptr() as *const c_char,
        code: 193,
    },
    SymTab {
        name: b"Rfraktur\0".as_ptr() as *const c_char,
        code: 194,
    },
    SymTab {
        name: b"weierstrass\0".as_ptr() as *const c_char,
        code: 195,
    },
    SymTab {
        name: b"circlemultiply\0".as_ptr() as *const c_char,
        code: 196,
    },
    SymTab {
        name: b"circleplus\0".as_ptr() as *const c_char,
        code: 197,
    },
    SymTab {
        name: b"emptyset\0".as_ptr() as *const c_char,
        code: 198,
    },
    SymTab {
        name: b"intersection\0".as_ptr() as *const c_char,
        code: 199,
    },
    SymTab {
        name: b"union\0".as_ptr() as *const c_char,
        code: 200,
    },
    SymTab {
        name: b"propersuperset\0".as_ptr() as *const c_char,
        code: 201,
    },
    SymTab {
        name: b"reflexsuperset\0".as_ptr() as *const c_char,
        code: 202,
    },
    SymTab {
        name: b"notsubset\0".as_ptr() as *const c_char,
        code: 203,
    },
    SymTab {
        name: b"propersubset\0".as_ptr() as *const c_char,
        code: 204,
    },
    SymTab {
        name: b"reflexsubset\0".as_ptr() as *const c_char,
        code: 205,
    },
    SymTab {
        name: b"element\0".as_ptr() as *const c_char,
        code: 206,
    },
    SymTab {
        name: b"notelement\0".as_ptr() as *const c_char,
        code: 207,
    },
    SymTab {
        name: b"angle\0".as_ptr() as *const c_char,
        code: 208,
    },
    SymTab {
        name: b"nabla\0".as_ptr() as *const c_char,
        code: 209,
    },
    SymTab {
        name: b"registerserif\0".as_ptr() as *const c_char,
        code: 210,
    },
    SymTab {
        name: b"copyrightserif\0".as_ptr() as *const c_char,
        code: 211,
    },
    SymTab {
        name: b"trademarkserif\0".as_ptr() as *const c_char,
        code: 212,
    },
    SymTab {
        name: b"product\0".as_ptr() as *const c_char,
        code: 213,
    },
    SymTab {
        name: b"radical\0".as_ptr() as *const c_char,
        code: 214,
    },
    SymTab {
        name: b"dotmath\0".as_ptr() as *const c_char,
        code: 215,
    },
    SymTab {
        name: b"logicaland\0".as_ptr() as *const c_char,
        code: 217,
    },
    SymTab {
        name: b"logicalor\0".as_ptr() as *const c_char,
        code: 218,
    },
    SymTab {
        name: b"arrowdblboth\0".as_ptr() as *const c_char,
        code: 219,
    },
    SymTab {
        name: b"arrowdblleft\0".as_ptr() as *const c_char,
        code: 220,
    },
    SymTab {
        name: b"arrowdblup\0".as_ptr() as *const c_char,
        code: 221,
    },
    SymTab {
        name: b"arrowdblright\0".as_ptr() as *const c_char,
        code: 222,
    },
    SymTab {
        name: b"arrowdbldown\0".as_ptr() as *const c_char,
        code: 223,
    },
    SymTab {
        name: b"lozenge\0".as_ptr() as *const c_char,
        code: 224,
    },
    SymTab {
        name: b"angleleft\0".as_ptr() as *const c_char,
        code: 225,
    },
    SymTab {
        name: b"registersans\0".as_ptr() as *const c_char,
        code: 226,
    },
    SymTab {
        name: b"copyrightsans\0".as_ptr() as *const c_char,
        code: 227,
    },
    SymTab {
        name: b"trademarksans\0".as_ptr() as *const c_char,
        code: 228,
    },
    SymTab {
        name: b"summation\0".as_ptr() as *const c_char,
        code: 229,
    },
    SymTab {
        name: b"parenlefttp\0".as_ptr() as *const c_char,
        code: 230,
    },
    SymTab {
        name: b"parenleftex\0".as_ptr() as *const c_char,
        code: 231,
    },
    SymTab {
        name: b"parenleftbt\0".as_ptr() as *const c_char,
        code: 232,
    },
    SymTab {
        name: b"bracketlefttp\0".as_ptr() as *const c_char,
        code: 233,
    },
    SymTab {
        name: b"bracketleftex\0".as_ptr() as *const c_char,
        code: 234,
    },
    SymTab {
        name: b"bracketleftbt\0".as_ptr() as *const c_char,
        code: 235,
    },
    SymTab {
        name: b"bracelefttp\0".as_ptr() as *const c_char,
        code: 236,
    },
    SymTab {
        name: b"braceleftmid\0".as_ptr() as *const c_char,
        code: 237,
    },
    SymTab {
        name: b"braceleftbt\0".as_ptr() as *const c_char,
        code: 238,
    },
    SymTab {
        name: b"braceex\0".as_ptr() as *const c_char,
        code: 239,
    },
    SymTab {
        name: b"angleright\0".as_ptr() as *const c_char,
        code: 241,
    },
    SymTab {
        name: b"integral\0".as_ptr() as *const c_char,
        code: 242,
    },
    SymTab {
        name: b"integraltp\0".as_ptr() as *const c_char,
        code: 243,
    },
    SymTab {
        name: b"integralex\0".as_ptr() as *const c_char,
        code: 244,
    },
    SymTab {
        name: b"integralbt\0".as_ptr() as *const c_char,
        code: 245,
    },
    SymTab {
        name: b"parenrighttp\0".as_ptr() as *const c_char,
        code: 246,
    },
    SymTab {
        name: b"parenrightex\0".as_ptr() as *const c_char,
        code: 247,
    },
    SymTab {
        name: b"parenrightbt\0".as_ptr() as *const c_char,
        code: 248,
    },
    SymTab {
        name: b"bracketrighttp\0".as_ptr() as *const c_char,
        code: 249,
    },
    SymTab {
        name: b"bracketrightex\0".as_ptr() as *const c_char,
        code: 250,
    },
    SymTab {
        name: b"bracketrightbt\0".as_ptr() as *const c_char,
        code: 251,
    },
    SymTab {
        name: b"bracerighttp\0".as_ptr() as *const c_char,
        code: 252,
    },
    SymTab {
        name: b"bracerightmid\0".as_ptr() as *const c_char,
        code: 253,
    },
    SymTab {
        name: b"bracerightbt\0".as_ptr() as *const c_char,
        code: 254,
    },
    SymTab {
        name: std::ptr::null(),
        code: 0,
    },
];

unsafe fn SymbolCode(expr: SEXP) -> c_int {
    unsafe {
        let t = std::ptr::addr_of!(SymbolTable);
        let mut i = 0;
        while i < (*t).len() && (*t)[i].code != 0 {
            if NameMatch(expr, (*t)[i].name) != 0 {
                return (*t)[i].code;
            }
            i += 1;
        }
        0
    }
}

unsafe fn TranslatedSymbol(expr: SEXP) -> c_int {
    unsafe {
        let c = SymbolCode(expr);
        if (0o101..=0o132).contains(&c)
            || (0o141..=0o172).contains(&c)
            || c == 0o300
            || c == 0o241
            || c == 0o242
            || c == 0o245
            || c == 0o260
            || c == 0o262
            || c == 0o266
            || c == 0o321
        {
            c
        } else {
            0
        }
    }
}

// ===========================================================================
// Binary operator table
// ===========================================================================

static mut BinTable: [SymTab; 13] = [
    SymTab {
        name: b"!\0".as_ptr() as *const c_char,
        code: 0o41,
    },
    SymTab {
        name: b"*\0".as_ptr() as *const c_char,
        code: 0o52,
    },
    SymTab {
        name: b"+\0".as_ptr() as *const c_char,
        code: 0o53,
    },
    SymTab {
        name: b"-\0".as_ptr() as *const c_char,
        code: 0o55,
    },
    SymTab {
        name: b"/\0".as_ptr() as *const c_char,
        code: 0o57,
    },
    SymTab {
        name: b":\0".as_ptr() as *const c_char,
        code: 0o72,
    },
    SymTab {
        name: b"%+-%\0".as_ptr() as *const c_char,
        code: 0o261,
    },
    SymTab {
        name: b"%*%\0".as_ptr() as *const c_char,
        code: 0o264,
    },
    SymTab {
        name: b"%/%\0".as_ptr() as *const c_char,
        code: 0o270,
    },
    SymTab {
        name: b"%intersection%\0".as_ptr() as *const c_char,
        code: 0o307,
    },
    SymTab {
        name: b"%union%\0".as_ptr() as *const c_char,
        code: 0o310,
    },
    SymTab {
        name: b"%.%\0".as_ptr() as *const c_char,
        code: 0o327,
    },
    SymTab {
        name: std::ptr::null(),
        code: 0,
    },
];

unsafe fn BinAtom(expr: SEXP) -> c_int {
    unsafe {
        let t = std::ptr::addr_of!(BinTable);
        let mut i = 0;
        while i < (*t).len() && (*t)[i].code != 0 {
            if NameMatch(expr, (*t)[i].name) != 0 {
                return (*t)[i].code;
            }
            i += 1;
        }
        0
    }
}

// ===========================================================================
// Relation operator table
// ===========================================================================

static mut RelTable: [SymTab; 28] = [
    SymTab {
        name: b"<\0".as_ptr() as *const c_char,
        code: 60,
    },
    SymTab {
        name: b"==\0".as_ptr() as *const c_char,
        code: 61,
    },
    SymTab {
        name: b">\0".as_ptr() as *const c_char,
        code: 62,
    },
    SymTab {
        name: b"%=~%\0".as_ptr() as *const c_char,
        code: 64,
    },
    SymTab {
        name: b"!=\0".as_ptr() as *const c_char,
        code: 185,
    },
    SymTab {
        name: b"<=\0".as_ptr() as *const c_char,
        code: 163,
    },
    SymTab {
        name: b">=\0".as_ptr() as *const c_char,
        code: 179,
    },
    SymTab {
        name: b"%==%\0".as_ptr() as *const c_char,
        code: 186,
    },
    SymTab {
        name: b"%~~%\0".as_ptr() as *const c_char,
        code: 187,
    },
    SymTab {
        name: b"%prop%\0".as_ptr() as *const c_char,
        code: 181,
    },
    SymTab {
        name: b"%~%\0".as_ptr() as *const c_char,
        code: 126,
    },
    SymTab {
        name: b"%<->%\0".as_ptr() as *const c_char,
        code: 171,
    },
    SymTab {
        name: b"%<-%\0".as_ptr() as *const c_char,
        code: 172,
    },
    SymTab {
        name: b"%up%\0".as_ptr() as *const c_char,
        code: 173,
    },
    SymTab {
        name: b"%->%\0".as_ptr() as *const c_char,
        code: 174,
    },
    SymTab {
        name: b"%down%\0".as_ptr() as *const c_char,
        code: 175,
    },
    SymTab {
        name: b"%<=>%\0".as_ptr() as *const c_char,
        code: 219,
    },
    SymTab {
        name: b"%<=%\0".as_ptr() as *const c_char,
        code: 220,
    },
    SymTab {
        name: b"%dblup%\0".as_ptr() as *const c_char,
        code: 221,
    },
    SymTab {
        name: b"%=>%\0".as_ptr() as *const c_char,
        code: 222,
    },
    SymTab {
        name: b"%dbldown%\0".as_ptr() as *const c_char,
        code: 223,
    },
    SymTab {
        name: b"%supset%\0".as_ptr() as *const c_char,
        code: 201,
    },
    SymTab {
        name: b"%supseteq%\0".as_ptr() as *const c_char,
        code: 202,
    },
    SymTab {
        name: b"%notsubset%\0".as_ptr() as *const c_char,
        code: 203,
    },
    SymTab {
        name: b"%subset%\0".as_ptr() as *const c_char,
        code: 204,
    },
    SymTab {
        name: b"%subseteq%\0".as_ptr() as *const c_char,
        code: 205,
    },
    SymTab {
        name: b"%in%\0".as_ptr() as *const c_char,
        code: 206,
    },
    SymTab {
        name: b"%notin%\0".as_ptr() as *const c_char,
        code: 207,
    },
];

unsafe fn RelAtom(expr: SEXP) -> c_int {
    unsafe {
        let t = std::ptr::addr_of!(RelTable);
        let mut i = 0;
        while i < (*t).len() && (*t)[i].code != 0 {
            if NameMatch(expr, (*t)[i].name) != 0 {
                return (*t)[i].code;
            }
            i += 1;
        }
        0
    }
}

// ===========================================================================
// Operator table
// ===========================================================================

static mut OpTable: [SymTab; 12] = [
    SymTab {
        name: b"prod\0".as_ptr() as *const c_char,
        code: S_PRODUCT,
    },
    SymTab {
        name: b"sum\0".as_ptr() as *const c_char,
        code: S_SUM,
    },
    SymTab {
        name: b"union\0".as_ptr() as *const c_char,
        code: S_UNION,
    },
    SymTab {
        name: b"intersect\0".as_ptr() as *const c_char,
        code: S_INTERSECTION,
    },
    SymTab {
        name: b"lim\0".as_ptr() as *const c_char,
        code: N_LIM,
    },
    SymTab {
        name: b"liminf\0".as_ptr() as *const c_char,
        code: N_LIMINF,
    },
    SymTab {
        name: b"limsup\0".as_ptr() as *const c_char,
        code: N_LIMINF,
    },
    SymTab {
        name: b"inf\0".as_ptr() as *const c_char,
        code: N_INF,
    },
    SymTab {
        name: b"sup\0".as_ptr() as *const c_char,
        code: N_SUP,
    },
    SymTab {
        name: b"min\0".as_ptr() as *const c_char,
        code: N_MIN,
    },
    SymTab {
        name: b"max\0".as_ptr() as *const c_char,
        code: N_MAX,
    },
    SymTab {
        name: std::ptr::null(),
        code: 0,
    },
];

unsafe fn OpAtom(expr: SEXP) -> c_int {
    unsafe {
        let t = std::ptr::addr_of!(OpTable);
        let mut i = 0;
        while i < (*t).len() && (*t)[i].code != 0 {
            if NameMatch(expr, (*t)[i].name) != 0 {
                return (*t)[i].code;
            }
            i += 1;
        }
        0
    }
}

// ===========================================================================
// Accent table
// ===========================================================================

static mut AccentTable: [SymTab; 5] = [
    SymTab {
        name: b"hat\0".as_ptr() as *const c_char,
        code: 94,
    },
    SymTab {
        name: b"ring\0".as_ptr() as *const c_char,
        code: 176,
    },
    SymTab {
        name: b"tilde\0".as_ptr() as *const c_char,
        code: 126,
    },
    SymTab {
        name: b"dot\0".as_ptr() as *const c_char,
        code: 215,
    },
    SymTab {
        name: std::ptr::null(),
        code: 0,
    },
];

unsafe fn AccentCode(expr: SEXP) -> c_int {
    unsafe {
        let t = std::ptr::addr_of!(AccentTable);
        let mut i = 0;
        while i < (*t).len() && (*t)[i].code != 0 {
            if NameMatch(expr, (*t)[i].name) != 0 {
                return (*t)[i].code;
            }
            i += 1;
        }
        0
    }
}

// ===========================================================================
// Atom predicates
// ===========================================================================

unsafe fn FormulaExpression(e: SEXP) -> c_int {
    unsafe { if isLang(TYPEOF(e)) { 1 } else { 0 } }
}
unsafe fn NameAtom(e: SEXP) -> c_int {
    unsafe { if isSymbol(TYPEOF(e)) { 1 } else { 0 } }
}
unsafe fn NumberAtom(e: SEXP) -> c_int {
    unsafe {
        let t = TYPEOF(e);
        if isReal(t) || isInt(t) || isCplx(t) {
            1
        } else {
            0
        }
    }
}
unsafe fn StringAtom(e: SEXP) -> c_int {
    unsafe { if isStr(TYPEOF(e)) { 1 } else { 0 } }
}
unsafe fn SpaceAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"~\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn SuperAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"^\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn SubAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"[\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn WideTildeAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"widetilde\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn WideHatAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"widehat\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn BarAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"bar\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn AccentAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && AccentCode(e) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn OverAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0
            && (NameMatch(e, b"over\0".as_ptr() as *const c_char) != 0
                || NameMatch(e, b"frac\0".as_ptr() as *const c_char) != 0)
        {
            1
        } else {
            0
        }
    }
}
unsafe fn UnderlAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"underline\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn AtopAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"atop\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn GroupAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"group\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn BGroupAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"bgroup\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn ParenAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"(\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn IntAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"integral\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn RadicalAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0
            && (NameMatch(e, b"root\0".as_ptr() as *const c_char) != 0
                || NameMatch(e, b"sqrt\0".as_ptr() as *const c_char) != 0)
        {
            1
        } else {
            0
        }
    }
}
unsafe fn AbsAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"abs\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn CurlyAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"{\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn BoldAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"bold\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn ItalicAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0
            && (NameMatch(e, b"italic\0".as_ptr() as *const c_char) != 0
                || NameMatch(e, b"math\0".as_ptr() as *const c_char) != 0)
        {
            1
        } else {
            0
        }
    }
}
unsafe fn PlainAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"plain\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn SymbolFaceAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"symbol\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn BoldItalicAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0
            && (NameMatch(e, b"bolditalic\0".as_ptr() as *const c_char) != 0
                || NameMatch(e, b"boldmath\0".as_ptr() as *const c_char) != 0)
        {
            1
        } else {
            0
        }
    }
}
unsafe fn StyleAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) == 0 {
            return 0;
        }
        if NameMatch(e, b"displaystyle\0".as_ptr() as *const c_char) != 0
            || NameMatch(e, b"textstyle\0".as_ptr() as *const c_char) != 0
            || NameMatch(e, b"scriptstyle\0".as_ptr() as *const c_char) != 0
            || NameMatch(e, b"scriptscriptstyle\0".as_ptr() as *const c_char) != 0
        {
            1
        } else {
            0
        }
    }
}
unsafe fn PhantomAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) == 0 {
            return 0;
        }
        if NameMatch(e, b"phantom\0".as_ptr() as *const c_char) != 0
            || NameMatch(e, b"vphantom\0".as_ptr() as *const c_char) != 0
        {
            1
        } else {
            0
        }
    }
}
unsafe fn ConcatenateAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"paste\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn ListAtom(e: SEXP) -> c_int {
    unsafe {
        if NameAtom(e) != 0 && NameMatch(e, b"list\0".as_ptr() as *const c_char) != 0 {
            1
        } else {
            0
        }
    }
}
unsafe fn DotsAtom(e: SEXP) -> c_int {
    unsafe {
        if NameMatch(e, b"cdots\0".as_ptr() as *const c_char) != 0
            || NameMatch(e, b"...\0".as_ptr() as *const c_char) != 0
            || NameMatch(e, b"ldots\0".as_ptr() as *const c_char) != 0
        {
            1
        } else {
            0
        }
    }
}

// ===========================================================================
// Forward declarations
// ===========================================================================

// NOTE: Rust does not support forward declarations for free functions.
// RenderElement, RenderOffsetElement, and RenderExpression are defined
// after RenderFormula which calls them. Rust allows this as long as the
// functions are visible before the call site in the same module.
// They are defined later in this file.

// ===========================================================================
// Glyph and text rendering
// ===========================================================================

unsafe fn GlyphBBox(chr: c_int, gc: *const R_GE_gcontext, dd: *mut GEDevDesc) -> BBOX {
    unsafe {
        let mut chr1 = chr;
        let dd_ref = &mut *dd;
        let dev = dd_ref.dev;
        if (*dev).wantSymbolUTF8 != 0 && (*gc).fontface == 5 {
            chr1 = -crate::main::util_main::Rf_AdobeSymbol2ucs2(chr);
        }
        let (h, d, w) = metric_info(chr1, gc, dd);
        BBOX {
            height: fromDeviceHeight(h, MetricUnit, dd as *mut _),
            depth: fromDeviceHeight(d, MetricUnit, dd as *mut _),
            width: fromDeviceHeight(w, MetricUnit, dd as *mut _),
            italic: 0.0,
            simple: 1,
        }
    }
}

unsafe fn RenderItalicCorr(
    mut b: BBOX,
    draw: c_int,
    mc: &mut mathContext,
    _gc: *const R_GE_gcontext,
    _dd: *mut GEDevDesc,
) -> BBOX {
    if b.italic > 0.0 {
        if draw != 0 {
            PMoveAcross(b.italic, mc);
        }
        b.width += b.italic;
        b.italic = 0.0;
    }
    b
}

unsafe fn RenderGap(
    gap: c_double,
    draw: c_int,
    mc: &mut mathContext,
    _gc: *const R_GE_gcontext,
    _dd: *mut GEDevDesc,
) -> BBOX {
    if draw != 0 {
        PMoveAcross(gap, mc);
    }
    MakeBBox(0.0, 0.0, gap)
}

unsafe fn RenderSymbolChar(
    ascii: c_int,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let prev = if ascii == A_HAT || ascii == A_TILDE {
            SetFont(FontType::Plain, gc)
        } else {
            SetFont(FontType::Symbol, gc)
        };
        let bbox = GlyphBBox(ascii, gc, dd);
        if draw != 0 {
            let s = [ascii as i8, 0];
            GEText(
                ConvertedX(mc, dd),
                ConvertedY(mc, dd),
                s.as_ptr(),
                CE_SYMBOL,
                0.0,
                0.0,
                mc.CurrentAngle,
                gc as *const _,
                dd as *mut _,
            );
            PMoveAcross(bbox.width, mc);
        }
        SetFont(prev, gc);
        bbox
    }
}

unsafe fn RenderSymbolStr(
    str: *const c_char,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let mut s = str;
        let mut glyphBBox: BBOX;
        let mut resultBBox = NullBBox();
        let mut lastItalicCorr = 0.0;
        let prevfont = GetFont(gc);
        let mut font = prevfont;

        if !str.is_null() {
            while *s != 0 {
                let c = *s as u8;
                if c.is_ascii_digit() && font as c_int != FontType::Plain as c_int {
                    font = FontType::Plain;
                    SetFont(FontType::Plain, gc);
                } else if font as c_int != prevfont as c_int {
                    font = prevfont;
                    SetFont(prevfont, gc);
                }
                glyphBBox = GlyphBBox(c as c_int, gc, dd);
                if UsingItalics(gc) != 0 {
                    glyphBBox.italic = ItalicFactor * glyphBBox.height;
                } else {
                    glyphBBox.italic = 0.0;
                }
                if draw != 0 {
                    let mut chr = [0i8; 2];
                    chr[0] = c as i8;
                    PMoveAcross(lastItalicCorr, mc);
                    GEText(
                        ConvertedX(mc, dd),
                        ConvertedY(mc, dd),
                        chr.as_ptr(),
                        CE_SYMBOL,
                        0.0,
                        0.0,
                        mc.CurrentAngle,
                        gc as *const _,
                        dd as *mut _,
                    );
                    PMoveAcross(glyphBBox.width, mc);
                }
                resultBBox.width += lastItalicCorr;
                resultBBox = CombineBBoxes(resultBBox, glyphBBox);
                lastItalicCorr = glyphBBox.italic;
                s = s.add(1);
            }
            if font as c_int != prevfont as c_int {
                SetFont(prevfont, gc);
            }
        }
        resultBBox.simple = 1;
        resultBBox
    }
}

unsafe fn RenderChar(
    ascii: c_int,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let bbox = GlyphBBox(ascii, gc, dd);
        if draw != 0 {
            let mut s = [0i8; 7];
            std::ptr::write_bytes(s.as_mut_ptr(), 0, 7);
            s[0] = ascii as i8;
            GEText(
                ConvertedX(mc, dd),
                ConvertedY(mc, dd),
                s.as_ptr(),
                CE_NATIVE,
                0.0,
                0.0,
                mc.CurrentAngle,
                gc as *const _,
                dd as *mut _,
            );
            PMoveAcross(bbox.width, mc);
        }
        bbox
    }
}

unsafe fn RenderStr(
    str: *const c_char,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let mut glyphBBox = NullBBox();
        let mut resultBBox = NullBBox();
        let mut nc: c_int = 0;
        let enc = if (*gc).fontface == 5 {
            CE_SYMBOL
        } else {
            CE_NATIVE
        };
        if !str.is_null() {
            let mut p = str;
            while *p != 0 {
                glyphBBox = GlyphBBox(*p as u8 as c_int, gc, dd);
                resultBBox = CombineBBoxes(resultBBox, glyphBBox);
                p = p.add(1);
                nc += 1;
            }
            if nc > 1 {
                let wd = GEStrWidth(str, enc, gc as *const _, dd as *mut _);
                resultBBox.width = fromDeviceHeight(wd, MetricUnit, dd as *mut _);
            }
            if draw != 0 {
                GEText(
                    ConvertedX(mc, dd),
                    ConvertedY(mc, dd),
                    str,
                    enc,
                    0.0,
                    0.0,
                    mc.CurrentAngle,
                    gc as *const _,
                    dd as *mut _,
                );
                PMoveAcross(resultBBox.width, mc);
            }
            if UsingItalics(gc) != 0 {
                resultBBox.italic = ItalicFactor * glyphBBox.height;
            } else {
                resultBBox.italic = 0.0;
            }
        }
        resultBBox.simple = 1;
        resultBBox
    }
}

unsafe fn RenderSymbol(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let code = TranslatedSymbol(expr);
        if code != 0 {
            RenderSymbolChar(code, draw, mc, gc, dd)
        } else {
            RenderSymbolStr(CHAR(PRINTNAME(expr)), draw, mc, gc, dd)
        }
    }
}

unsafe fn RenderSymbolString(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let code = TranslatedSymbol(expr);
        if code != 0 {
            RenderSymbolChar(code, draw, mc, gc, dd)
        } else {
            RenderStr(CHAR(PRINTNAME(expr)), draw, mc, gc, dd)
        }
    }
}

unsafe fn RenderNumber(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let prev = SetFont(FontType::Plain, gc);
        PrintDefaults();
        let bbox = RenderStr(CHAR(asChar(expr)), draw, mc, gc, dd);
        SetFont(prev, gc);
        bbox
    }
}

unsafe fn RenderString(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe { RenderStr(translateChar(STRING_ELT(expr, 0)), draw, mc, gc, dd) }
}

unsafe fn RenderDots(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let bbox = RenderSymbolChar(S_ELLIPSIS, 0, mc, gc, dd);
        if NameMatch(expr, b"cdots\0".as_ptr() as *const c_char) != 0
            || NameMatch(expr, b"...\0".as_ptr() as *const c_char) != 0
        {
            let shift = AxisHeight(gc, dd) - 0.5 * bbox.height;
            if draw != 0 {
                PMoveUp(shift, mc);
                RenderSymbolChar(S_ELLIPSIS, 1, mc, gc, dd);
                PMoveUp(-shift, mc);
            }
            ShiftBBox(bbox, shift)
        } else {
            if draw != 0 {
                RenderSymbolChar(S_ELLIPSIS, 1, mc, gc, dd);
            }
            bbox
        }
    }
}

unsafe fn RenderAtom(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        if NameAtom(expr) != 0 {
            if DotsAtom(expr) != 0 {
                RenderDots(expr, draw, mc, gc, dd)
            } else {
                RenderSymbol(expr, draw, mc, gc, dd)
            }
        } else if NumberAtom(expr) != 0 {
            RenderNumber(expr, draw, mc, gc, dd)
        } else if StringAtom(expr) != 0 {
            RenderString(expr, draw, mc, gc, dd)
        } else {
            NullBBox()
        }
    }
}

// ===========================================================================
// Helper: draw a polyline with saved lty/lwd
// ===========================================================================

unsafe fn draw_polyline_saved(
    n: c_int,
    x: &[c_double],
    y: &[c_double],
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) {
    unsafe {
        let g = gc as *mut R_GE_gcontext;
        let savedlty = (*g).lty;
        let savedlwd = (*g).lwd;
        (*g).lty = LTY_SOLID as c_int;
        if (*g).lwd > 1.0 {
            (*g).lwd = 1.0;
        }
        GEPolyline(n, x.as_ptr(), y.as_ptr(), gc as *const _, dd as *mut _);
        (*g).lty = savedlty;
        (*g).lwd = savedlwd;
    }
}

// ===========================================================================
// Slash rendering
// ===========================================================================

unsafe fn RenderSlash(
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let depth = 0.5 * TeX(TEXPAR::sigma22, gc, dd);
        let height = XHeight(gc, dd) + 0.5 * TeX(TEXPAR::sigma22, gc, dd);
        let width = 0.5 * xHeight(gc, dd);
        if draw != 0 {
            PMoveAcross(0.5 * width, mc);
            PMoveUp(-depth, mc);
            let x0 = ConvertedX(mc, dd);
            let y0 = ConvertedY(mc, dd);
            PMoveAcross(width, mc);
            PMoveUp(depth + height, mc);
            let x1 = ConvertedX(mc, dd);
            let y1 = ConvertedY(mc, dd);
            PMoveUp(-height, mc);
            draw_polyline_saved(2, &[x0, x1], &[y0, y1], gc, dd);
            PMoveAcross(0.5 * width, mc);
        }
        MakeBBox(height, depth, 2.0 * width)
    }
}

// ===========================================================================
// Space, Binary operators
// ===========================================================================

unsafe fn RenderSpace(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let nexpr = LENGTH(expr);
        if nexpr == 2 {
            let op = RenderSymbolChar(' ' as c_int, draw, mc, gc, dd);
            CombineBBoxes(op, RenderElement(CADR(expr), draw, mc, gc, dd))
        } else if nexpr == 3 {
            let a = RenderElement(CADR(expr), draw, mc, gc, dd);
            let op = RenderSymbolChar(' ' as c_int, draw, mc, gc, dd);
            let b = RenderElement(CADDR(expr), draw, mc, gc, dd);
            let mut r = CombineBBoxes(a, op);
            r = CombineBBoxes(r, b);
            r
        } else {
            NullBBox()
        }
    }
}

unsafe fn RenderBin(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let op = BinAtom(CAR(expr));
        let nexpr = LENGTH(expr);
        if nexpr == 3 {
            if op == S_ASTERISKMATH {
                let b = RenderElement(CADR(expr), draw, mc, gc, dd);
                let b = RenderItalicCorr(b, draw, mc, gc, dd);
                CombineBBoxes(b, RenderElement(CADDR(expr), draw, mc, gc, dd))
            } else if op == S_SLASH {
                let mut b = RenderElement(CADR(expr), draw, mc, gc, dd);
                b = RenderItalicCorr(b, draw, mc, gc, dd);
                b = CombineBBoxes(b, RenderGap(0.0, draw, mc, gc, dd));
                b = CombineBBoxes(b, RenderSlash(draw, mc, gc, dd));
                b = CombineBBoxes(b, RenderGap(0.0, draw, mc, gc, dd));
                CombineBBoxes(b, RenderElement(CADDR(expr), draw, mc, gc, dd))
            } else {
                let gap = if mc.CurrentStyle > STYLE::S {
                    medium_space(gc, dd)
                } else {
                    0.0
                };
                let mut b = RenderElement(CADR(expr), draw, mc, gc, dd);
                b = RenderItalicCorr(b, draw, mc, gc, dd);
                b = CombineBBoxes(b, RenderGap(gap, draw, mc, gc, dd));
                b = CombineBBoxes(b, RenderSymbolChar(op, draw, mc, gc, dd));
                b = CombineBBoxes(b, RenderGap(gap, draw, mc, gc, dd));
                CombineBBoxes(b, RenderElement(CADDR(expr), draw, mc, gc, dd))
            }
        } else if nexpr == 2 {
            let gap = if mc.CurrentStyle > STYLE::S {
                thin_space(gc, dd)
            } else {
                0.0
            };
            let mut b = RenderSymbolChar(op, draw, mc, gc, dd);
            b = CombineBBoxes(b, RenderGap(gap, draw, mc, gc, dd));
            CombineBBoxes(b, RenderElement(CADR(expr), draw, mc, gc, dd))
        } else {
            NullBBox()
        }
    }
}

// ===========================================================================
// Subscript / Superscript
// ===========================================================================

unsafe fn RenderSub(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let body = CADR(expr);
        let sub = CADDR(expr);
        let style = GetStyle(mc);
        let savedX = mc.CurrentX;
        let savedY = mc.CurrentY;
        let mut bodyBBox = RenderElement(body, draw, mc, gc, dd);
        bodyBBox = RenderItalicCorr(bodyBBox, draw, mc, gc, dd);
        let mut v = if bodyBBox.simple != 0 {
            0.0
        } else {
            bodyBBox.depth + TeX(TEXPAR::sigma19, gc, dd)
        };
        let s16 = TeX(TEXPAR::sigma16, gc, dd);
        SetSubStyle(style, mc, gc);
        let subBBox = RenderElement(sub, 0, mc, gc, dd);
        v = fmax(
            fmax(v, s16),
            subBBox.height - 0.8 * TeX(TEXPAR::sigma5, gc, dd),
        );
        let subBBox = RenderOffsetElement(sub, 0.0, -v, draw, mc, gc, dd);
        let bodyBBox = CombineBBoxes(bodyBBox, subBBox);
        SetStyle(style, mc, gc);
        if draw != 0 {
            PMoveTo(savedX + bodyBBox.width, savedY, mc);
        }
        bodyBBox
    }
}

unsafe fn RenderSup(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let mut body = CADR(expr);
        let sup = CADDR(expr);
        let style = GetStyle(mc);
        let savedX = mc.CurrentX;
        let savedY = mc.CurrentY;
        let mut haveSub = false;
        let mut sub = R_NilValue();

        if FormulaExpression(body) != 0 && SubAtom(CAR(body)) != 0 {
            sub = CADDR(body);
            body = CADR(body);
            haveSub = true;
        }

        let mut bodyBBox = RenderElement(body, draw, mc, gc, dd);
        let delta = bodyBBox.italic;
        bodyBBox = RenderItalicCorr(bodyBBox, draw, mc, gc, dd);
        let width = bodyBBox.width;

        let (u, v) = if bodyBBox.simple != 0 {
            (0.0, 0.0)
        } else {
            (
                bodyBBox.height - TeX(TEXPAR::sigma18, gc, dd),
                bodyBBox.depth + TeX(TEXPAR::sigma19, gc, dd),
            )
        };

        let theta = TeX(TEXPAR::xi8, gc, dd);
        let s5 = TeX(TEXPAR::sigma5, gc, dd);
        let s17 = TeX(TEXPAR::sigma17, gc, dd);

        let p = if style == STYLE::D {
            TeX(TEXPAR::sigma13, gc, dd)
        } else if IsCompactStyle(style) != 0 {
            TeX(TEXPAR::sigma15, gc, dd)
        } else {
            TeX(TEXPAR::sigma14, gc, dd)
        };

        let mut u = u;
        SetSupStyle(style, mc, gc);
        let mut supBBox = RenderElement(sup, 0, mc, gc, dd);
        u = fmax(fmax(u, p), supBBox.depth + 0.25 * s5);

        if haveSub {
            SetSubStyle(style, mc, gc);
            let subBBox = RenderElement(sub, 0, mc, gc, dd);
            let mut v = fmax(v, s17);
            if (u - supBBox.depth) - (subBBox.height - v) < 4.0 * theta {
                let psi = 0.8 * s5 - (u - supBBox.depth);
                if psi > 0.0 {
                    u += psi;
                    v -= psi;
                }
            }
            let v_final = v;
            if draw != 0 {
                PMoveTo(savedX, savedY, mc);
            }
            let subBBox = RenderOffsetElement(sub, width, -v_final, draw, mc, gc, dd);
            if draw != 0 {
                PMoveTo(savedX, savedY, mc);
            }
            SetSupStyle(style, mc, gc);
            supBBox = RenderOffsetElement(sup, width + delta, u, draw, mc, gc, dd);
            bodyBBox = CombineAlignedBBoxes(bodyBBox, subBBox);
            bodyBBox = CombineAlignedBBoxes(bodyBBox, supBBox);
        } else {
            supBBox = RenderOffsetElement(sup, 0.0, u, draw, mc, gc, dd);
            bodyBBox = CombineBBoxes(bodyBBox, supBBox);
        }

        if draw != 0 {
            PMoveTo(savedX + bodyBBox.width, savedY, mc);
        }
        SetStyle(style, mc, gc);
        bodyBBox
    }
}

// ===========================================================================
// Accents
// ===========================================================================

unsafe fn RenderWideTilde(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let savedX = mc.CurrentX;
        let savedY = mc.CurrentY;
        let bbox = RenderElement(CADR(expr), draw, mc, gc, dd);
        let height = bbox.height;
        let totalwidth = bbox.width + bbox.italic;
        let delta_v = totalwidth * (1.0 - 2.0 * DELTA) / (NTILDE as c_double);
        let start = DELTA * totalwidth;
        let accentGap = ACCENT_GAP * XHeight(gc, dd);
        let hatHeight = 0.5 * HAT_HEIGHT * XHeight(gc, dd);
        let c = std::f64::consts::TAU / (NTILDE as c_double);

        if draw != 0 {
            let baseX = savedX;
            let baseY = savedY + height + accentGap;
            let mut xv = [0.0; NTILDE as usize + 3];
            let mut yv = [0.0; NTILDE as usize + 3];
            PMoveTo(baseX, baseY, mc);
            xv[0] = ConvertedX(mc, dd);
            yv[0] = ConvertedY(mc, dd);
            for i in 0..=NTILDE {
                let xval = start + i as c_double * delta_v;
                let yval = 0.5 * hatHeight * ((c * i as c_double).sin() + 1.0);
                PMoveTo(baseX + xval, baseY + yval, mc);
                xv[i as usize + 1] = ConvertedX(mc, dd);
                yv[i as usize + 1] = ConvertedY(mc, dd);
            }
            PMoveTo(baseX + totalwidth, baseY + hatHeight, mc);
            xv[NTILDE as usize + 2] = ConvertedX(mc, dd);
            yv[NTILDE as usize + 2] = ConvertedY(mc, dd);
            draw_polyline_saved((NTILDE + 3) as c_int, &xv, &yv, gc, dd);
            PMoveTo(savedX + totalwidth, savedY, mc);
        }
        MakeBBox(height + accentGap + hatHeight, bbox.depth, totalwidth)
    }
}

unsafe fn RenderWideHat(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let savedX = mc.CurrentX;
        let savedY = mc.CurrentY;
        let bbox = RenderElement(CADR(expr), draw, mc, gc, dd);
        let accentGap = ACCENT_GAP * XHeight(gc, dd);
        let hatHeight = HAT_HEIGHT * XHeight(gc, dd);
        let totalwidth = bbox.width + bbox.italic;

        if draw != 0 {
            PMoveTo(savedX, savedY + bbox.height + accentGap, mc);
            let x0 = ConvertedX(mc, dd);
            let y0 = ConvertedY(mc, dd);
            PMoveAcross(0.5 * totalwidth, mc);
            PMoveUp(hatHeight, mc);
            let x1 = ConvertedX(mc, dd);
            let y1 = ConvertedY(mc, dd);
            PMoveAcross(0.5 * totalwidth, mc);
            PMoveUp(-hatHeight, mc);
            let x2 = ConvertedX(mc, dd);
            let y2 = ConvertedY(mc, dd);
            draw_polyline_saved(3, &[x0, x1, x2], &[y0, y1, y2], gc, dd);
            PMoveTo(savedX + bbox.width, savedY, mc);
        }
        EnlargeBBox(bbox, accentGap + hatHeight, 0.0, 0.0)
    }
}

unsafe fn RenderBar(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let savedX = mc.CurrentX;
        let savedY = mc.CurrentY;
        let bbox = RenderElement(CADR(expr), draw, mc, gc, dd);
        let accentGap = ACCENT_GAP * XHeight(gc, dd);
        let offset = bbox.italic;

        if draw != 0 {
            PMoveTo(savedX + offset, savedY + bbox.height + accentGap, mc);
            let x0 = ConvertedX(mc, dd);
            let y0 = ConvertedY(mc, dd);
            PMoveAcross(bbox.width, mc);
            let x1 = ConvertedX(mc, dd);
            let y1 = ConvertedY(mc, dd);
            draw_polyline_saved(2, &[x0, x1], &[y0, y1], gc, dd);
            PMoveTo(savedX + bbox.width, savedY, mc);
        }
        EnlargeBBox(bbox, accentGap, 0.0, 0.0)
    }
}

unsafe fn RenderAccent(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let accent = CAR(expr);
        let body = CADR(expr);
        let savedX = mc.CurrentX;
        let savedY = mc.CurrentY;
        let code = AccentCode(accent);
        let bodyBBox = RenderElement(body, 0, mc, gc, dd);
        let italic = bodyBBox.italic;

        let accentBBox = if code == 176 || code == 215 {
            RenderSymbolChar(code, 0, mc, gc, dd)
        } else {
            RenderChar(code, 0, mc, gc, dd)
        };

        let width = fmax(bodyBBox.width + bodyBBox.italic, accentBBox.width);
        let xoffset = 0.5 * (width - bodyBBox.width);
        let mut bodyBBox = RenderGap(xoffset, draw, mc, gc, dd);
        bodyBBox = CombineBBoxes(bodyBBox, RenderElement(body, draw, mc, gc, dd));
        bodyBBox = CombineBBoxes(bodyBBox, RenderGap(xoffset, draw, mc, gc, dd));
        PMoveTo(savedX, savedY, mc);

        let xoffset2 = 0.5 * (width - accentBBox.width) + 0.9 * italic;
        let yoffset = bodyBBox.height + accentBBox.depth + 0.1 * XHeight(gc, dd);
        if draw != 0 {
            PMoveTo(savedX + xoffset2, savedY + yoffset, mc);
            if code == 176 || code == 215 {
                RenderSymbolChar(code, draw, mc, gc, dd);
            } else {
                RenderChar(code, draw, mc, gc, dd);
            }
        }
        let bodyBBox = CombineOffsetBBoxes(bodyBBox, 0, accentBBox, 0, xoffset2, yoffset);
        if draw != 0 {
            PMoveTo(savedX + width, savedY, mc);
        }
        bodyBBox
    }
}

// ===========================================================================
// Fraction rendering
// ===========================================================================

unsafe fn NumDenomVShift(
    numBBox: BBOX,
    denomBBox: BBOX,
    u: &mut c_double,
    v: &mut c_double,
    mc: &mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) {
    unsafe {
        let a = TeX(TEXPAR::sigma22, gc, dd);
        let theta = TeX(TEXPAR::xi8, gc, dd);
        if mc.CurrentStyle > STYLE::T {
            *u = TeX(TEXPAR::sigma8, gc, dd);
            *v = TeX(TEXPAR::sigma11, gc, dd);
        } else {
            *u = TeX(TEXPAR::sigma9, gc, dd);
            *v = TeX(TEXPAR::sigma12, gc, dd);
        }
        let phi = if mc.CurrentStyle > STYLE::T {
            3.0 * theta
        } else {
            theta
        };
        let mut delta = (*u - numBBox.depth) - (a + 0.5 * theta);
        if delta < phi {
            *u += phi - delta;
        }
        delta = (a + 0.5 * theta) - (denomBBox.height - *v);
        if delta < phi {
            *v += phi - delta;
        }
    }
}

unsafe fn NumDenomHShift(
    numBBox: BBOX,
    denomBBox: BBOX,
    numShift: &mut c_double,
    denomShift: &mut c_double,
) {
    let nw = numBBox.width;
    let dw = denomBBox.width;
    if nw > dw {
        *numShift = 0.0;
        *denomShift = (nw - dw) / 2.0;
    } else {
        *numShift = (dw - nw) / 2.0;
        *denomShift = 0.0;
    }
}

unsafe fn RenderFraction(
    expr: SEXP,
    rule: c_int,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let numerator = CADR(expr);
        let denominator = CADDR(expr);
        let savedX = mc.CurrentX;
        let savedY = mc.CurrentY;
        let style = GetStyle(mc);

        SetNumStyle(style, mc, gc);
        let numBBox = RenderItalicCorr(RenderElement(numerator, 0, mc, gc, dd), 0, mc, gc, dd);
        SetDenomStyle(style, mc, gc);
        let denomBBox = RenderItalicCorr(RenderElement(denominator, 0, mc, gc, dd), 0, mc, gc, dd);
        SetStyle(style, mc, gc);

        let width = fmax(numBBox.width, denomBBox.width);
        let (mut nHShift, mut dHShift) = (0.0, 0.0);
        NumDenomHShift(numBBox, denomBBox, &mut nHShift, &mut dHShift);
        let (mut nVShift, mut dVShift) = (0.0, 0.0);
        NumDenomVShift(numBBox, denomBBox, &mut nVShift, &mut dVShift, mc, gc, dd);

        mc.CurrentX = savedX;
        mc.CurrentY = savedY;
        SetNumStyle(style, mc, gc);
        let numBBox = RenderOffsetElement(numerator, nHShift, nVShift, draw, mc, gc, dd);

        mc.CurrentX = savedX;
        mc.CurrentY = savedY;
        SetDenomStyle(style, mc, gc);
        let denomBBox = RenderOffsetElement(denominator, dHShift, -dVShift, draw, mc, gc, dd);

        SetStyle(style, mc, gc);

        if draw != 0 {
            if rule != 0 {
                mc.CurrentX = savedX;
                mc.CurrentY = savedY;
                PMoveUp(AxisHeight(gc, dd), mc);
                let x0 = ConvertedX(mc, dd);
                let y0 = ConvertedY(mc, dd);
                PMoveAcross(width, mc);
                let x1 = ConvertedX(mc, dd);
                let y1 = ConvertedY(mc, dd);
                draw_polyline_saved(2, &[x0, x1], &[y0, y1], gc, dd);
                PMoveUp(-AxisHeight(gc, dd), mc);
            }
            PMoveTo(savedX + width, savedY, mc);
        }
        CombineAlignedBBoxes(numBBox, denomBBox)
    }
}

unsafe fn RenderUnderline(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let body = CADR(expr);
        let savedX = mc.CurrentX;
        let savedY = mc.CurrentY;
        let bbox = RenderItalicCorr(RenderElement(body, 0, mc, gc, dd), 0, mc, gc, dd);
        let width = bbox.width;
        mc.CurrentX = savedX;
        mc.CurrentY = savedY;
        let bbox = RenderElement(body, draw, mc, gc, dd);
        let adepth = 0.1 * XHeight(gc, dd);
        let depth = bbox.depth + adepth;

        if draw != 0 {
            mc.CurrentX = savedX;
            mc.CurrentY = savedY;
            PMoveUp(-depth, mc);
            let x0 = ConvertedX(mc, dd);
            let y0 = ConvertedY(mc, dd);
            PMoveAcross(width, mc);
            let x1 = ConvertedX(mc, dd);
            let y1 = ConvertedY(mc, dd);
            draw_polyline_saved(2, &[x0, x1], &[y0, y1], gc, dd);
            PMoveUp(depth, mc);
            PMoveTo(savedX + width, savedY, mc);
        }
        EnlargeBBox(bbox, 0.0, adepth, 0.0)
    }
}

unsafe fn RenderOver(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe { RenderFraction(expr, 1, draw, mc, gc, dd) }
}

unsafe fn RenderUnderl(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe { RenderUnderline(expr, draw, mc, gc, dd) }
}

unsafe fn RenderAtop(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe { RenderFraction(expr, 0, draw, mc, gc, dd) }
}

// ===========================================================================
// Grouped expressions (group, bgroup, paren)
// ===========================================================================

unsafe fn DelimCode(expr: SEXP, head: SEXP) -> c_int {
    unsafe {
        if NameAtom(head) != 0 {
            if NameMatch(head, b"lfloor\0".as_ptr() as *const c_char) != 0 {
                return S_BRACKETLEFTBT;
            }
            if NameMatch(head, b"rfloor\0".as_ptr() as *const c_char) != 0 {
                return S_BRACKETRIGHTBT;
            }
            if NameMatch(head, b"lceil\0".as_ptr() as *const c_char) != 0 {
                return S_BRACKETLEFTTP;
            }
            if NameMatch(head, b"rceil\0".as_ptr() as *const c_char) != 0 {
                return S_BRACKETRIGHTTP;
            }
            if NameMatch(head, b"langle\0".as_ptr() as *const c_char) != 0 {
                return S_ANGLELEFT;
            }
            if NameMatch(head, b"rangle\0".as_ptr() as *const c_char) != 0 {
                return S_ANGLERIGHT;
            }
        } else if StringAtom(head) != 0 && LENGTH(head) > 0 {
            if StringMatch(head, b"|\0".as_ptr() as *const c_char) != 0 {
                return '|' as c_int;
            }
            if StringMatch(head, b"||\0".as_ptr() as *const c_char) != 0 {
                return '|' as c_int;
            }
            if StringMatch(head, b"(\0".as_ptr() as *const c_char) != 0 {
                return '(' as c_int;
            }
            if StringMatch(head, b")\0".as_ptr() as *const c_char) != 0 {
                return ')' as c_int;
            }
            if StringMatch(head, b"[\0".as_ptr() as *const c_char) != 0 {
                return '[' as c_int;
            }
            if StringMatch(head, b"]\0".as_ptr() as *const c_char) != 0 {
                return ']' as c_int;
            }
            if StringMatch(head, b"{\0".as_ptr() as *const c_char) != 0 {
                return '{' as c_int;
            }
            if StringMatch(head, b"}\0".as_ptr() as *const c_char) != 0 {
                return '}' as c_int;
            }
            if StringMatch(head, b"\0".as_ptr() as *const c_char) != 0
                || StringMatch(head, b".\0".as_ptr() as *const c_char) != 0
            {
                return '.' as c_int;
            }
        }
        '.' as c_int
    }
}

unsafe fn RenderDelimiter(
    delim: c_int,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let savecex = (*gc).cex;
        let g = gc as *mut R_GE_gcontext;
        (*g).cex = DelimSymbolMag * (*gc).cex;
        let bbox = RenderSymbolChar(delim, draw, mc, gc, dd);
        (*g).cex = savecex;
        bbox
    }
}

unsafe fn RenderGroup(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let cexSaved = (*gc).cex;
        let code1 = DelimCode(expr, CADR(expr));
        let code2 = DelimCode(expr, CADDDR(expr));
        let g = gc as *mut R_GE_gcontext;
        (*g).cex = DelimSymbolMag * (*gc).cex;
        let mut bbox = if code1 != '.' as c_int {
            RenderSymbolChar(code1, draw, mc, gc, dd)
        } else {
            NullBBox()
        };
        (*g).cex = cexSaved;
        bbox = CombineBBoxes(bbox, RenderElement(CADDR(expr), draw, mc, gc, dd));
        bbox = RenderItalicCorr(bbox, draw, mc, gc, dd);
        (*g).cex = DelimSymbolMag * (*gc).cex;
        let delimBBox = if code2 != '.' as c_int {
            RenderSymbolChar(code2, draw, mc, gc, dd)
        } else {
            NullBBox()
        };
        (*g).cex = cexSaved;
        CombineBBoxes(bbox, delimBBox)
    }
}

unsafe fn RenderDelim(
    which: c_int,
    dist: c_double,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let savedX = mc.CurrentX;
        let savedY = mc.CurrentY;
        let prev = SetFont(FontType::Symbol, gc);
        let (top, ext, bot, mid): (c_int, c_int, c_int, c_int);

        match which as u8 {
            b'.' => {
                SetFont(prev, gc);
                return NullBBox();
            }
            b'|' => {
                let v = 239;
                (top, ext, bot, mid) = (v, v, v, 0);
            }
            b'(' => (top, ext, bot, mid) = (230, 231, 232, 0),
            b')' => (top, ext, bot, mid) = (246, 247, 248, 0),
            b'[' => (top, ext, bot, mid) = (233, 234, 235, 0),
            b']' => (top, ext, bot, mid) = (249, 250, 251, 0),
            b'{' => (top, ext, bot, mid) = (236, 239, 238, 237),
            b'}' => (top, ext, bot, mid) = (252, 239, 254, 253),
            _ => {
                SetFont(prev, gc);
                return NullBBox();
            }
        };

        let mut topBBox = GlyphBBox(top, gc, dd);
        let extBBox = GlyphBBox(ext, gc, dd);
        let botBBox = GlyphBBox(bot, gc, dd);

        let mut dist = dist;
        if which == b'{' as c_int || which == b'}' as c_int {
            if 1.2 * (topBBox.height + topBBox.depth) > dist {
                dist = 1.2 * (topBBox.height + botBBox.depth);
            }
        } else {
            if 0.8 * (topBBox.height + topBBox.depth) > dist {
                dist = 0.8 * (topBBox.height + topBBox.depth);
            }
        }

        let extHeight = extBBox.height + extBBox.depth;
        let topShift = dist - topBBox.height + TeX(TEXPAR::sigma22, gc, dd);
        let botShift = dist - botBBox.depth - TeX(TEXPAR::sigma22, gc, dd);
        let extShift = 0.5 * (extBBox.height - extBBox.depth);

        topBBox = ShiftBBox(topBBox, topShift);
        let botBBox = ShiftBBox(botBBox, -botShift);
        let mut ansBBox = CombineAlignedBBoxes(topBBox, botBBox);

        if which == b'{' as c_int || which == b'}' as c_int {
            let midBBox = GlyphBBox(mid, gc, dd);
            let midShift = TeX(TEXPAR::sigma22, gc, dd) - 0.5 * (midBBox.height - midBBox.depth);
            let midBBox = ShiftBBox(midBBox, midShift);
            ansBBox = CombineAlignedBBoxes(ansBBox, midBBox);
            if draw != 0 {
                PMoveTo(savedX, savedY + topShift, mc);
                RenderSymbolChar(top, draw, mc, gc, dd);
                PMoveTo(savedX, savedY + midShift, mc);
                RenderSymbolChar(mid, draw, mc, gc, dd);
                PMoveTo(savedX, savedY - botShift, mc);
                RenderSymbolChar(bot, draw, mc, gc, dd);
                PMoveTo(savedX + ansBBox.width, savedY, mc);
            }
        } else if draw != 0 {
            PMoveTo(savedX, savedY + topShift, mc);
            RenderSymbolChar(top, draw, mc, gc, dd);
            PMoveTo(savedX, savedY - botShift, mc);
            RenderSymbolChar(bot, draw, mc, gc, dd);

            let ytop = TeX(TEXPAR::sigma22, gc, dd) + dist - (topBBox.height + topBBox.depth);
            let ybot = TeX(TEXPAR::sigma22, gc, dd) - dist + (botBBox.height + botBBox.depth);
            let n = (ytop - ybot).ceil() / (0.99 * extHeight) as c_double;
            let n = n as c_int;
            if n > 0 {
                let delta_v = (ytop - ybot) / n as c_double;
                for i in 0..n {
                    PMoveTo(
                        savedX,
                        savedY + ybot + (i as c_double + 0.5) * delta_v - extShift,
                        mc,
                    );
                    RenderSymbolChar(ext, draw, mc, gc, dd);
                }
            }
            PMoveTo(savedX + ansBBox.width, savedY, mc);
        }

        SetFont(prev, gc);
        ansBBox
    }
}

unsafe fn RenderBGroup(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let extra = 0.2 * xHeight(gc, dd);
        let delim1 = DelimCode(expr, CADR(expr));
        let delim2 = DelimCode(expr, CADDDR(expr));
        let bbox = RenderElement(CADDR(expr), 0, mc, gc, dd);
        let dist = fmax(
            bbox.height - TeX(TEXPAR::sigma22, gc, dd),
            bbox.depth + TeX(TEXPAR::sigma22, gc, dd),
        );
        let mut bbox = RenderDelim(delim1, dist + extra, draw, mc, gc, dd);
        bbox = CombineBBoxes(bbox, RenderElement(CADDR(expr), draw, mc, gc, dd));
        bbox = RenderItalicCorr(bbox, draw, mc, gc, dd);
        CombineBBoxes(bbox, RenderDelim(delim2, dist + extra, draw, mc, gc, dd))
    }
}

unsafe fn RenderParen(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let mut bbox = RenderDelimiter(S_PARENLEFT, draw, mc, gc, dd);
        bbox = CombineBBoxes(bbox, RenderElement(CADR(expr), draw, mc, gc, dd));
        bbox = RenderItalicCorr(bbox, draw, mc, gc, dd);
        CombineBBoxes(bbox, RenderDelimiter(S_PARENRIGHT, draw, mc, gc, dd))
    }
}

// ===========================================================================
// Integral
// ===========================================================================

unsafe fn RenderIntSymbol(
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let savedX = mc.CurrentX;
        let savedY = mc.CurrentY;
        if GetStyle(mc) > STYLE::T {
            let bbox1 = RenderSymbolChar(243, 0, mc, gc, dd);
            let bbox2 = RenderSymbolChar(245, 0, mc, gc, dd);
            let shift = TeX(TEXPAR::sigma22, gc, dd) + 0.99 * bbox1.depth;
            PMoveUp(shift, mc);
            let bbox1 = ShiftBBox(RenderSymbolChar(243, draw, mc, gc, dd), shift);
            mc.CurrentX = savedX;
            mc.CurrentY = savedY;
            let shift2 = TeX(TEXPAR::sigma22, gc, dd) - 0.99 * bbox2.height;
            PMoveUp(shift2, mc);
            let bbox2 = ShiftBBox(RenderSymbolChar(245, draw, mc, gc, dd), shift2);
            if draw != 0 {
                PMoveTo(savedX + fmax(bbox1.width, bbox2.width), savedY, mc);
            } else {
                PMoveTo(savedX, savedY, mc);
            }
            CombineAlignedBBoxes(bbox1, bbox2)
        } else {
            RenderSymbolChar(0o362, draw, mc, gc, dd)
        }
    }
}

unsafe fn RenderInt(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let nexpr = LENGTH(expr);
        let style = GetStyle(mc);
        let savedX = mc.CurrentX;
        let savedY = mc.CurrentY;

        let mut opBBox = RenderIntSymbol(draw, mc, gc, dd);
        let width = opBBox.width;
        mc.CurrentX = savedX;
        mc.CurrentY = savedY;

        if nexpr > 2 {
            let hshift = 0.5 * width + thin_space(gc, dd);
            SetSubStyle(style, mc, gc);
            let lowerBBox = RenderElement(CADDR(expr), 0, mc, gc, dd);
            let vshift = opBBox.depth + CenterShift(lowerBBox);
            let lowerBBox = RenderOffsetElement(CADDR(expr), hshift, -vshift, draw, mc, gc, dd);
            opBBox = CombineAlignedBBoxes(opBBox, lowerBBox);
            SetStyle(style, mc, gc);
            mc.CurrentX = savedX;
            mc.CurrentY = savedY;
        }

        if nexpr > 3 {
            let hshift = width + thin_space(gc, dd);
            SetSupStyle(style, mc, gc);
            let upperBBox = RenderElement(CADDDR(expr), 0, mc, gc, dd);
            let vshift = opBBox.height - CenterShift(upperBBox);
            let upperBBox = RenderOffsetElement(CADDDR(expr), hshift, vshift, draw, mc, gc, dd);
            opBBox = CombineAlignedBBoxes(opBBox, upperBBox);
            SetStyle(style, mc, gc);
            mc.CurrentX = savedX;
            mc.CurrentY = savedY;
        }

        PMoveAcross(opBBox.width, mc);
        if nexpr > 1 {
            let bodyBBox = RenderElement(CADR(expr), draw, mc, gc, dd);
            opBBox = CombineBBoxes(opBBox, bodyBBox);
        }
        opBBox
    }
}

// ===========================================================================
// Operator expressions (sum, product, lim, etc.)
// ===========================================================================

unsafe fn RenderOpSymbol(
    op: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let cexSaved = (*gc).cex;
        let display = GetStyle(mc) > STYLE::T;
        let opId = OpAtom(op);

        if opId == S_SUM || opId == S_PRODUCT || opId == S_UNION || opId == S_INTERSECTION {
            if display {
                let g = gc as *mut R_GE_gcontext;
                (*g).cex = OperatorSymbolMag * (*gc).cex;
                let mut bbox = RenderSymbolChar(OpAtom(op), 0, mc, gc, dd);
                let shift = 0.5 * (bbox.height - bbox.depth) - TeX(TEXPAR::sigma22, gc, dd);
                if draw != 0 {
                    PMoveUp(-shift, mc);
                    bbox = RenderSymbolChar(opId, 1, mc, gc, dd);
                    PMoveUp(shift, mc);
                }
                (*g).cex = cexSaved;
                ShiftBBox(bbox, -shift)
            } else {
                RenderSymbolChar(opId, draw, mc, gc, dd)
            }
        } else {
            let prev = SetFont(FontType::Plain, gc);
            let bbox = RenderStr(CHAR(PRINTNAME(op)), draw, mc, gc, dd);
            SetFont(prev, gc);
            bbox
        }
    }
}

unsafe fn RenderOp(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let nexpr = LENGTH(expr);
        let style = GetStyle(mc);
        let savedX = mc.CurrentX;
        let savedY = mc.CurrentY;

        let opBBox = RenderOpSymbol(CAR(expr), 0, mc, gc, dd);
        let mut width = opBBox.width;

        let (mut lvshift, mut uvshift) = (0.0, 0.0);
        let mut lowerBBox = NullBBox();
        let mut upperBBox = NullBBox();

        if nexpr > 2 {
            SetSubStyle(style, mc, gc);
            lowerBBox = RenderElement(CADDR(expr), 0, mc, gc, dd);
            SetStyle(style, mc, gc);
            width = fmax(width, lowerBBox.width);
            lvshift = fmax(
                TeX(TEXPAR::xi10, gc, dd),
                TeX(TEXPAR::xi12, gc, dd) - lowerBBox.height,
            );
            lvshift = opBBox.depth + lowerBBox.height + lvshift;
        }

        if nexpr > 3 {
            SetSupStyle(style, mc, gc);
            upperBBox = RenderElement(CADDDR(expr), 0, mc, gc, dd);
            SetStyle(style, mc, gc);
            width = fmax(width, upperBBox.width);
            uvshift = fmax(
                TeX(TEXPAR::xi9, gc, dd),
                TeX(TEXPAR::xi11, gc, dd) - upperBBox.depth,
            );
            uvshift = opBBox.height + upperBBox.depth + uvshift;
        }

        let hshift = 0.5 * (width - opBBox.width);
        let mut opBBox = RenderGap(hshift, draw, mc, gc, dd);
        opBBox = CombineBBoxes(opBBox, RenderOpSymbol(CAR(expr), draw, mc, gc, dd));
        mc.CurrentX = savedX;
        mc.CurrentY = savedY;

        if nexpr > 2 {
            SetSubStyle(style, mc, gc);
            let lh = 0.5 * (width - lowerBBox.width);
            lowerBBox = RenderOffsetElement(CADDR(expr), lh, -lvshift, draw, mc, gc, dd);
            SetStyle(style, mc, gc);
            opBBox = CombineAlignedBBoxes(opBBox, lowerBBox);
            mc.CurrentX = savedX;
            mc.CurrentY = savedY;
        }

        if nexpr > 3 {
            SetSupStyle(style, mc, gc);
            let uh = 0.5 * (width - upperBBox.width);
            upperBBox = RenderOffsetElement(CADDDR(expr), uh, uvshift, draw, mc, gc, dd);
            SetStyle(style, mc, gc);
            opBBox = CombineAlignedBBoxes(opBBox, upperBBox);
            mc.CurrentX = savedX;
            mc.CurrentY = savedY;
        }

        opBBox = EnlargeBBox(
            opBBox,
            TeX(TEXPAR::xi13, gc, dd),
            TeX(TEXPAR::xi13, gc, dd),
            0.0,
        );
        if draw != 0 {
            PMoveAcross(width, mc);
        }
        let gapBBox = RenderGap(thin_space(gc, dd), draw, mc, gc, dd);
        let bodyBBox = RenderElement(CADR(expr), draw, mc, gc, dd);
        CombineBBoxes(CombineBBoxes(opBBox, gapBBox), bodyBBox)
    }
}

// ===========================================================================
// Radical (root, sqrt)
// ===========================================================================

unsafe fn RenderScript(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let style = GetStyle(mc);
        SetSupStyle(style, mc, gc);
        let bbox = RenderElement(expr, draw, mc, gc, dd);
        SetStyle(style, mc, gc);
        bbox
    }
}

unsafe fn RenderRadical(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let body = CADR(expr);
        let order = CADDR(expr);
        let style = GetStyle(mc);
        let savedX = mc.CurrentX;
        let savedY = mc.CurrentY;

        let radGap = RADICAL_GAP * xHeight(gc, dd);
        let radSpace = RADICAL_SPACE * xHeight(gc, dd);
        let radTrail = mu_space(gc, dd);

        SetPrimeStyle(style, mc, gc);
        let mut bodyBBox = RenderElement(body, 0, mc, gc, dd);
        bodyBBox = RenderItalicCorr(bodyBBox, 0, mc, gc, dd);

        let radWidth = 0.6 * XHeight(gc, dd);
        let radHeight = bodyBBox.height + radGap;
        let twiddleHeight = CenterShift(bodyBBox);

        let mut leadWidth = radWidth;
        let leadHeight = radHeight;

        let mut orderBBox = NullBBox();
        if order != R_NilValue() {
            SetSupStyle(style, mc, gc);
            let ob = RenderScript(order, 0, mc, gc, dd);
            leadWidth = fmax(leadWidth, ob.width + 0.4 * radWidth);
            let hshift = leadWidth - ob.width - 0.4 * radWidth;
            let mut vshift = leadHeight - ob.height;
            if vshift - ob.depth < twiddleHeight + radGap {
                vshift = twiddleHeight + ob.depth + radGap;
            }
            if draw != 0 {
                PMoveTo(savedX + hshift, savedY + vshift, mc);
                orderBBox = RenderScript(order, draw, mc, gc, dd);
            }
            orderBBox = EnlargeBBox(ob, vshift, 0.0, hshift);
        }

        if draw != 0 {
            PMoveTo(savedX + leadWidth - radWidth, savedY, mc);
            PMoveUp(0.8 * twiddleHeight, mc);
            let mut xv = [0.0; 5];
            let mut yv = [0.0; 5];
            xv[0] = ConvertedX(mc, dd);
            yv[0] = ConvertedY(mc, dd);
            PMoveUp(0.2 * twiddleHeight, mc);
            PMoveAcross(0.3 * radWidth, mc);
            xv[1] = ConvertedX(mc, dd);
            yv[1] = ConvertedY(mc, dd);
            PMoveUp(-(twiddleHeight + bodyBBox.depth), mc);
            PMoveAcross(0.3 * radWidth, mc);
            xv[2] = ConvertedX(mc, dd);
            yv[2] = ConvertedY(mc, dd);
            PMoveUp(bodyBBox.depth + bodyBBox.height + radGap, mc);
            PMoveAcross(0.4 * radWidth, mc);
            xv[3] = ConvertedX(mc, dd);
            yv[3] = ConvertedY(mc, dd);
            PMoveAcross(radSpace + bodyBBox.width + radTrail, mc);
            xv[4] = ConvertedX(mc, dd);
            yv[4] = ConvertedY(mc, dd);
            draw_polyline_saved(5, &xv, &yv, gc, dd);
            PMoveTo(savedX, savedY, mc);
        }

        let orderBBox =
            CombineAlignedBBoxes(orderBBox, RenderGap(leadWidth + radSpace, draw, mc, gc, dd));
        SetPrimeStyle(style, mc, gc);
        let mut orderBBox = CombineBBoxes(orderBBox, RenderElement(body, draw, mc, gc, dd));
        orderBBox = CombineBBoxes(orderBBox, RenderGap(2.0 * radTrail, draw, mc, gc, dd));
        orderBBox = EnlargeBBox(orderBBox, radGap, 0.0, 0.0);
        SetStyle(style, mc, gc);
        orderBBox
    }
}

// ===========================================================================
// Absolute value
// ===========================================================================

unsafe fn RenderAbs(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let bbox = RenderElement(CADR(expr), 0, mc, gc, dd);
        let height = bbox.height;
        let depth = bbox.depth;

        let mut resultBBox = RenderGap(mu_space(gc, dd), draw, mc, gc, dd);
        if draw != 0 {
            PMoveUp(-depth, mc);
            let x0 = ConvertedX(mc, dd);
            let y0 = ConvertedY(mc, dd);
            PMoveUp(depth + height, mc);
            let x1 = ConvertedX(mc, dd);
            let y1 = ConvertedY(mc, dd);
            draw_polyline_saved(2, &[x0, x1], &[y0, y1], gc, dd);
            PMoveUp(-height, mc);
        }
        resultBBox = CombineBBoxes(resultBBox, RenderGap(mu_space(gc, dd), draw, mc, gc, dd));
        resultBBox = CombineBBoxes(resultBBox, RenderElement(CADR(expr), draw, mc, gc, dd));
        resultBBox = RenderItalicCorr(resultBBox, draw, mc, gc, dd);
        resultBBox = CombineBBoxes(resultBBox, RenderGap(mu_space(gc, dd), draw, mc, gc, dd));
        if draw != 0 {
            PMoveUp(-depth, mc);
            let x0 = ConvertedX(mc, dd);
            let y0 = ConvertedY(mc, dd);
            PMoveUp(depth + height, mc);
            let x1 = ConvertedX(mc, dd);
            let y1 = ConvertedY(mc, dd);
            draw_polyline_saved(2, &[x0, x1], &[y0, y1], gc, dd);
            PMoveUp(-height, mc);
        }
        CombineBBoxes(resultBBox, RenderGap(mu_space(gc, dd), draw, mc, gc, dd))
    }
}

// ===========================================================================
// Curly braces, relations, bold, italic, etc.
// ===========================================================================

unsafe fn RenderCurly(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe { RenderElement(CADR(expr), draw, mc, gc, dd) }
}

unsafe fn RenderRel(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let op = RelAtom(CAR(expr));
        let nexpr = LENGTH(expr);
        if nexpr == 3 {
            let gap = if mc.CurrentStyle > STYLE::S {
                thick_space(gc, dd)
            } else {
                0.0
            };
            let mut b = RenderElement(CADR(expr), draw, mc, gc, dd);
            b = RenderItalicCorr(b, draw, mc, gc, dd);
            b = CombineBBoxes(b, RenderGap(gap, draw, mc, gc, dd));
            b = CombineBBoxes(b, RenderSymbolChar(op, draw, mc, gc, dd));
            b = CombineBBoxes(b, RenderGap(gap, draw, mc, gc, dd));
            CombineBBoxes(b, RenderElement(CADDR(expr), draw, mc, gc, dd))
        } else {
            NullBBox()
        }
    }
}

unsafe fn RenderBold(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let prev = SetFont(FontType::Bold, gc);
        let bbox = RenderElement(CADR(expr), draw, mc, gc, dd);
        SetFont(prev, gc);
        bbox
    }
}

unsafe fn RenderItalic(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let prev = SetFont(FontType::Italic, gc);
        let bbox = RenderElement(CADR(expr), draw, mc, gc, dd);
        SetFont(prev, gc);
        bbox
    }
}

unsafe fn RenderPlain(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let prev = SetFont(FontType::Plain, gc);
        let bbox = RenderElement(CADR(expr), draw, mc, gc, dd);
        SetFont(prev, gc);
        bbox
    }
}

unsafe fn RenderSymbolFace(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let prev = SetFont(FontType::Symbol, gc);
        let bbox = RenderElement(CADR(expr), draw, mc, gc, dd);
        SetFont(prev, gc);
        bbox
    }
}

unsafe fn RenderBoldItalic(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let prev = SetFont(FontType::BoldItalic, gc);
        let bbox = RenderElement(CADR(expr), draw, mc, gc, dd);
        SetFont(prev, gc);
        bbox
    }
}

unsafe fn RenderStyle(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let prevstyle = GetStyle(mc);
        let head = CAR(expr);
        if NameMatch(head, b"displaystyle\0".as_ptr() as *const c_char) != 0 {
            SetStyle(STYLE::D, mc, gc);
        } else if NameMatch(head, b"textstyle\0".as_ptr() as *const c_char) != 0 {
            SetStyle(STYLE::T, mc, gc);
        } else if NameMatch(head, b"scriptstyle\0".as_ptr() as *const c_char) != 0 {
            SetStyle(STYLE::S, mc, gc);
        } else {
            SetStyle(STYLE::SS, mc, gc);
        }
        let bbox = RenderElement(CADR(expr), draw, mc, gc, dd);
        SetStyle(prevstyle, mc, gc);
        bbox
    }
}

unsafe fn RenderPhantom(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let mut bbox = RenderElement(CADR(expr), 0, mc, gc, dd);
        if NameMatch(CAR(expr), b"vphantom\0".as_ptr() as *const c_char) != 0 {
            bbox.width = 0.0;
            bbox.italic = 0.0;
        } else {
            let _ = RenderGap(bbox.width, draw, mc, gc, dd);
        }
        bbox
    }
}

unsafe fn RenderConcatenate(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let mut bbox = NullBBox();
        let mut e = CDR(expr);
        let n = LENGTH(e);
        let mut i: c_int = 0;
        while i < n {
            bbox = CombineBBoxes(bbox, RenderElement(CAR(e), draw, mc, gc, dd));
            if i != n - 1 {
                bbox = RenderItalicCorr(bbox, draw, mc, gc, dd);
            }
            e = CDR(e);
            i += 1;
        }
        bbox
    }
}

unsafe fn RenderCommaList(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let mut bbox = NullBBox();
        let small = 0.4 * thin_space(gc, dd);
        let n = LENGTH(expr);
        let mut e = expr;
        let mut i: c_int = 0;
        while i < n {
            if NameAtom(CAR(e)) != 0 && NameMatch(CAR(e), b"...\0".as_ptr() as *const c_char) != 0 {
                if i > 0 {
                    bbox = CombineBBoxes(bbox, RenderSymbolChar(S_COMMA, draw, mc, gc, dd));
                    bbox = CombineBBoxes(bbox, RenderSymbolChar(S_SPACE, draw, mc, gc, dd));
                }
                bbox = CombineBBoxes(bbox, RenderSymbolChar(S_ELLIPSIS, draw, mc, gc, dd));
                bbox = CombineBBoxes(bbox, RenderGap(small, draw, mc, gc, dd));
            } else {
                if i > 0 {
                    bbox = CombineBBoxes(bbox, RenderSymbolChar(S_COMMA, draw, mc, gc, dd));
                    bbox = CombineBBoxes(bbox, RenderSymbolChar(S_SPACE, draw, mc, gc, dd));
                }
                bbox = CombineBBoxes(bbox, RenderElement(CAR(e), draw, mc, gc, dd));
            }
            e = CDR(e);
            i += 1;
        }
        bbox
    }
}

unsafe fn RenderList(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe { RenderCommaList(CDR(expr), draw, mc, gc, dd) }
}

// ===========================================================================
// General expression and formula rendering
// ===========================================================================

unsafe fn RenderExpression(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let mut bbox = if NameAtom(CAR(expr)) != 0 {
            RenderSymbolString(CAR(expr), draw, mc, gc, dd)
        } else {
            RenderElement(CAR(expr), draw, mc, gc, dd)
        };
        bbox = RenderItalicCorr(bbox, draw, mc, gc, dd);
        bbox = CombineBBoxes(bbox, RenderDelimiter(S_PARENLEFT, draw, mc, gc, dd));
        bbox = CombineBBoxes(bbox, RenderCommaList(CDR(expr), draw, mc, gc, dd));
        bbox = RenderItalicCorr(bbox, draw, mc, gc, dd);
        CombineBBoxes(bbox, RenderDelimiter(S_PARENRIGHT, draw, mc, gc, dd))
    }
}

unsafe fn RenderFormula(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let head = CAR(expr);
        if SpaceAtom(head) != 0 {
            RenderSpace(expr, draw, mc, gc, dd)
        } else if BinAtom(head) != 0 {
            RenderBin(expr, draw, mc, gc, dd)
        } else if SuperAtom(head) != 0 {
            RenderSup(expr, draw, mc, gc, dd)
        } else if SubAtom(head) != 0 {
            RenderSub(expr, draw, mc, gc, dd)
        } else if WideTildeAtom(head) != 0 {
            RenderWideTilde(expr, draw, mc, gc, dd)
        } else if WideHatAtom(head) != 0 {
            RenderWideHat(expr, draw, mc, gc, dd)
        } else if BarAtom(head) != 0 {
            RenderBar(expr, draw, mc, gc, dd)
        } else if AccentAtom(head) != 0 {
            RenderAccent(expr, draw, mc, gc, dd)
        } else if OverAtom(head) != 0 {
            RenderOver(expr, draw, mc, gc, dd)
        } else if UnderlAtom(head) != 0 {
            RenderUnderl(expr, draw, mc, gc, dd)
        } else if AtopAtom(head) != 0 {
            RenderAtop(expr, draw, mc, gc, dd)
        } else if ParenAtom(head) != 0 {
            RenderParen(expr, draw, mc, gc, dd)
        } else if BGroupAtom(head) != 0 {
            RenderBGroup(expr, draw, mc, gc, dd)
        } else if GroupAtom(head) != 0 {
            RenderGroup(expr, draw, mc, gc, dd)
        } else if IntAtom(head) != 0 {
            RenderInt(expr, draw, mc, gc, dd)
        } else if OpAtom(head) != 0 {
            RenderOp(expr, draw, mc, gc, dd)
        } else if RadicalAtom(head) != 0 {
            RenderRadical(expr, draw, mc, gc, dd)
        } else if AbsAtom(head) != 0 {
            RenderAbs(expr, draw, mc, gc, dd)
        } else if CurlyAtom(head) != 0 {
            RenderCurly(expr, draw, mc, gc, dd)
        } else if RelAtom(head) != 0 {
            RenderRel(expr, draw, mc, gc, dd)
        } else if BoldAtom(head) != 0 {
            RenderBold(expr, draw, mc, gc, dd)
        } else if ItalicAtom(head) != 0 {
            RenderItalic(expr, draw, mc, gc, dd)
        } else if PlainAtom(head) != 0 {
            RenderPlain(expr, draw, mc, gc, dd)
        } else if SymbolFaceAtom(head) != 0 {
            RenderSymbolFace(expr, draw, mc, gc, dd)
        } else if BoldItalicAtom(head) != 0 {
            RenderBoldItalic(expr, draw, mc, gc, dd)
        } else if StyleAtom(head) != 0 {
            RenderStyle(expr, draw, mc, gc, dd)
        } else if PhantomAtom(head) != 0 {
            RenderPhantom(expr, draw, mc, gc, dd)
        } else if ConcatenateAtom(head) != 0 {
            RenderConcatenate(expr, draw, mc, gc, dd)
        } else if ListAtom(head) != 0 {
            RenderList(expr, draw, mc, gc, dd)
        } else {
            RenderExpression(expr, draw, mc, gc, dd)
        }
    }
}

unsafe fn RenderElement(
    expr: SEXP,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        if FormulaExpression(expr) != 0 {
            RenderFormula(expr, draw, mc, gc, dd)
        } else {
            RenderAtom(expr, draw, mc, gc, dd)
        }
    }
}

unsafe fn RenderOffsetElement(
    expr: SEXP,
    x: c_double,
    y: c_double,
    draw: c_int,
    mc: &mut mathContext,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> BBOX {
    unsafe {
        let savedX = mc.CurrentX;
        let savedY = mc.CurrentY;
        if draw != 0 {
            mc.CurrentX += x;
            mc.CurrentY += y;
        }
        let mut bbox = RenderElement(expr, draw, mc, gc, dd);
        bbox.width += x;
        bbox.height += y;
        bbox.depth -= y;
        mc.CurrentX = savedX;
        mc.CurrentY = savedY;
        bbox
    }
}

// ===========================================================================
// R API Functions
// ===========================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn GEExpressionWidth(
    expr: SEXP,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> c_double {
    unsafe {
        let mut mc = mathContext {
            BoxColor: 4291543295,
            BaseCex: (*gc).cex,
            CurrentStyle: STYLE::D,
            ReferenceX: 0.0,
            ReferenceY: 0.0,
            CurrentX: 0.0,
            CurrentY: 0.0,
            CurrentAngle: 0.0,
            CosAngle: 0.0,
            SinAngle: 0.0,
        };
        SetFont(FontType::Plain, gc);
        let bbox = RenderElement(expr, 0, &mut mc, gc, dd);
        (toDeviceWidth(bbox.width, GE_INCHES, dd as *mut _)).abs()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn GEExpressionHeight(
    expr: SEXP,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) -> c_double {
    unsafe {
        let mut mc = mathContext {
            BoxColor: 4291543295,
            BaseCex: (*gc).cex,
            CurrentStyle: STYLE::D,
            ReferenceX: 0.0,
            ReferenceY: 0.0,
            CurrentX: 0.0,
            CurrentY: 0.0,
            CurrentAngle: 0.0,
            CosAngle: 0.0,
            SinAngle: 0.0,
        };
        SetFont(FontType::Plain, gc);
        let bbox = RenderElement(expr, 0, &mut mc, gc, dd);
        let height = bbox.height + bbox.depth;
        (toDeviceHeight(height, GE_INCHES, dd as *mut _)).abs()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn GEExpressionMetric(
    expr: SEXP,
    gc: *const R_GE_gcontext,
    ascent: *mut c_double,
    descent: *mut c_double,
    width: *mut c_double,
    dd: *mut GEDevDesc,
) {
    unsafe {
        let mut mc = mathContext {
            BoxColor: 4291543295,
            BaseCex: (*gc).cex,
            CurrentStyle: STYLE::D,
            ReferenceX: 0.0,
            ReferenceY: 0.0,
            CurrentX: 0.0,
            CurrentY: 0.0,
            CurrentAngle: 0.0,
            CosAngle: 0.0,
            SinAngle: 0.0,
        };
        SetFont(FontType::Plain, gc);
        let bbox = RenderElement(expr, 0, &mut mc, gc, dd);
        if !width.is_null() {
            *width = (toDeviceWidth(bbox.width, GE_INCHES, dd as *mut _)).abs();
        }
        if !ascent.is_null() {
            *ascent = (toDeviceHeight(bbox.height, GE_INCHES, dd as *mut _)).abs();
        }
        if !descent.is_null() {
            *descent = (toDeviceHeight(bbox.depth, GE_INCHES, dd as *mut _)).abs();
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn GEMathText(
    x: c_double,
    y: c_double,
    expr: SEXP,
    xc: c_double,
    yc: c_double,
    rot: c_double,
    gc: *const R_GE_gcontext,
    dd: *mut GEDevDesc,
) {
    unsafe {
        let mut ascent = 0.0;
        let mut descent = 0.0;
        let mut width = 0.0;
        GEMetricInfo(
            'M' as c_int,
            gc as *const _,
            &mut ascent,
            &mut descent,
            &mut width,
            dd as *mut _,
        );
        if ascent == 0.0 && descent == 0.0 && width == 0.0 {
            return;
        }

        let mut mc = mathContext {
            BoxColor: 4291543295,
            BaseCex: (*gc).cex,
            CurrentStyle: STYLE::D,
            ReferenceX: 0.0,
            ReferenceY: 0.0,
            CurrentX: 0.0,
            CurrentY: 0.0,
            CurrentAngle: 0.0,
            CosAngle: 0.0,
            SinAngle: 0.0,
        };

        SetFont(FontType::Plain, gc);
        let bbox = RenderElement(expr, 0, &mut mc, gc, dd);

        mc.ReferenceX = fromDeviceX(x, GE_INCHES, dd as *mut _);
        mc.ReferenceY = fromDeviceY(y, GE_INCHES, dd as *mut _);

        if R_FINITE(xc) {
            mc.CurrentX = mc.ReferenceX - xc * bbox.width;
        } else {
            mc.CurrentX = mc.ReferenceX - 0.5 * bbox.width;
        }

        if R_FINITE(yc) {
            mc.CurrentY = mc.ReferenceY + bbox.depth - yc * (bbox.height + bbox.depth);
        } else {
            mc.CurrentY = mc.ReferenceY + bbox.depth - 0.5 * (bbox.height + bbox.depth);
        }

        mc.CurrentAngle = rot;
        let rot_rad = rot * (std::f64::consts::FRAC_PI_2 / 90.0);
        mc.CosAngle = rot_rad.cos();
        mc.SinAngle = rot_rad.sin();

        RenderElement(expr, 1, &mut mc, gc, dd);
    }
}
