#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::ffi::c_void;
use std::os::raw::{c_char, c_double, c_int};

use crate::sexp::ffi::SEXP;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum STYLE {
    SS1 = 1,
    SS  = 2,
    S1  = 3,
    S   = 4,
    T1  = 5,
    T   = 6,
    D1  = 7,
    D   = 8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontType {
    PlainFont      = 1,
    BoldFont       = 2,
    ItalicFont     = 3,
    BoldItalicFont = 4,
    SymbolFont     = 5,
}

#[repr(C)]
pub struct mathContext {
    pub BoxColor: u32,
    pub BaseCex: c_double,
    pub ReferenceX: c_double,
    pub ReferenceY: c_double,
    pub CurrentX: c_double,
    pub CurrentY: c_double,
    pub CurrentAngle: c_double,
    pub CosAngle: c_double,
    pub SinAngle: c_double,
    pub CurrentStyle: STYLE,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BBOX {
    pub height: c_double,
    pub depth: c_double,
    pub width: c_double,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TEXPAR {
    sigma2,  sigma5,  sigma6,  sigma8,  sigma9,  sigma10, sigma11,
    sigma12, sigma13, sigma14, sigma15, sigma16, sigma17, sigma18,
    sigma19, sigma20, sigma21, sigma22, xi8, xi9, xi10, xi11, xi12, xi13,
}

const SUBS: c_double = 0.7;
const MetricUnit: c_int = 0;
static ItalicFactor: c_double = 0.15;

#[derive(Clone)]
pub struct SymTab {
    pub name: &'static str,
    pub code: c_int,
}

static SymbolTable: &[SymTab] = &[
    SymTab { name: "\0", code: 0 },
];

fn MakeBBox(height: c_double, depth: c_double, width: c_double) -> BBOX {
    BBOX { height, depth, width }
}

fn NullBBox() -> BBOX {
    BBOX { height: 0.0, depth: 0.0, width: 0.0 }
}

fn ShiftBBox(bbox: BBOX, shiftV: c_double) -> BBOX {
    BBOX {
        height: bbox.height + shiftV,
        depth: bbox.depth - shiftV,
        width: bbox.width,
    }
}

fn EnlargeBBox(bbox: BBOX, deltaHeight: c_double, deltaDepth: c_double, deltaWidth: c_double) -> BBOX {
    BBOX {
        height: bbox.height + deltaHeight,
        depth: bbox.depth + deltaDepth,
        width: bbox.width + deltaWidth,
    }
}

fn CombineBBoxes(bbox1: BBOX, bbox2: BBOX) -> BBOX {
    BBOX {
        height: fmax2(bbox1.height, bbox2.height),
        depth: fmax2(bbox1.depth, bbox2.depth),
        width: bbox1.width + bbox2.width,
    }
}

fn CombineAlignedBBoxes(bbox1: BBOX, bbox2: BBOX) -> BBOX {
    let height = fmax2(bbox1.height, bbox2.height);
    let depth = fmax2(bbox1.depth, bbox2.depth);
    BBOX {
        height,
        depth,
        width: bbox1.width + bbox2.width,
    }
}

fn CombineOffsetBBoxes(bbox1: BBOX, italic1: c_int, bbox2: BBOX, italic2: c_int, space: c_double) -> BBOX {
    let height = fmax2(bbox1.height, bbox2.height);
    let depth = fmax2(bbox1.depth, bbox2.depth);
    let italic_offset = if italic1 != 0 && italic2 != 0 { 0.0 } else { space };
    BBOX {
        height,
        depth,
        width: bbox1.width + italic_offset + bbox2.width,
    }
}

fn CenterShift(bbox: BBOX) -> c_double {
    0.5 * (bbox.height - bbox.depth)
}

fn fmax2(x: c_double, y: c_double) -> c_double {
    if x < y { y } else { x }
}

unsafe fn toDeviceX(_x: c_double, _unit: c_int, _dd: *mut c_void) -> c_double { _x }
unsafe fn toDeviceY(_y: c_double, _unit: c_int, _dd: *mut c_void) -> c_double { _y }
unsafe fn fromDeviceHeight(_h: c_double, _unit: c_int, _dd: *mut c_void) -> c_double { _h }
unsafe fn fromDeviceWidth(_w: c_double, _unit: c_int, _dd: *mut c_void) -> c_double { _w }
unsafe fn GEMetricInfo(_chr: c_int, _gc: *const c_void, _h: *mut c_double, _d: *mut c_double, _w: *mut c_double, _dd: *mut c_void) {}
unsafe fn GELine(_x1: c_double, _y1: c_double, _x2: c_double, _y2: c_double, _gc: *const c_void, _dd: *mut c_void) {}
unsafe fn GEText(_x: c_double, _y: c_double, _str: *const c_char, _xc: c_double, _yc: c_double, _rot: c_double, _gc: *const c_void, _dd: *mut c_void) {}
unsafe fn GESymbol(_x: c_double, _y: c_double, _chr: c_int, _font: c_int, _gc: *const c_void, _dd: *mut c_void) {}
unsafe fn GEExpressionMetric(_expr: SEXP, _gc: *const c_void, _w: *mut c_double, _h: *mut c_double, _dd: *mut c_void) {}
unsafe fn GESetClip(_x1: c_double, _y1: c_double, _x2: c_double, _y2: c_double, _dd: *mut c_void) {}
unsafe fn GENewPage(_gc: *const c_void, _dd: *mut c_void) {}

unsafe fn ConvertedX(mc: &mathContext, _dd: *mut c_void) -> c_double {
    let rotatedX = mc.ReferenceX
        + (mc.CurrentX - mc.ReferenceX) * mc.CosAngle
        - (mc.CurrentY - mc.ReferenceY) * mc.SinAngle;
    toDeviceX(rotatedX, MetricUnit, _dd)
}

unsafe fn ConvertedY(mc: &mathContext, _dd: *mut c_void) -> c_double {
    let rotatedY = mc.ReferenceY
        + (mc.CurrentY - mc.ReferenceY) * mc.CosAngle
        + (mc.CurrentX - mc.ReferenceX) * mc.SinAngle;
    toDeviceY(rotatedY, MetricUnit, _dd)
}

unsafe fn PMoveAcross(xamount: c_double, mc: &mut mathContext) {
    mc.CurrentX += xamount;
}

unsafe fn PMoveUp(yamount: c_double, mc: &mut mathContext) {
    mc.CurrentY += yamount;
}

unsafe fn PMoveTo(x: c_double, y: c_double, mc: &mut mathContext) {
    mc.CurrentX = x;
    mc.CurrentY = y;
}

unsafe fn xHeight(gc: *const c_void, dd: *mut c_void) -> c_double {
    let mut height = 0.0;
    let mut depth = 0.0;
    let mut width = 0.0;
    GEMetricInfo('x' as c_int, gc, &mut height, &mut depth, &mut width, dd);
    fromDeviceHeight(height, MetricUnit, dd)
}

unsafe fn XHeight(gc: *const c_void, dd: *mut c_void) -> c_double {
    let mut height = 0.0;
    let mut depth = 0.0;
    let mut width = 0.0;
    GEMetricInfo('X' as c_int, gc, &mut height, &mut depth, &mut width, dd);
    fromDeviceHeight(height, MetricUnit, dd)
}

unsafe fn AxisHeight(gc: *const c_void, dd: *mut c_void) -> c_double {
    let mut height = 0.0;
    let mut depth = 0.0;
    let mut width = 0.0;
    GEMetricInfo('+' as c_int, gc, &mut height, &mut depth, &mut width, dd);
    fromDeviceHeight(0.5 * height, MetricUnit, dd)
}

unsafe fn Quad(gc: *const c_void, dd: *mut c_void) -> c_double {
    let mut height = 0.0;
    let mut depth = 0.0;
    let mut width = 0.0;
    GEMetricInfo('M' as c_int, gc, &mut height, &mut depth, &mut width, dd);
    fromDeviceHeight(width, MetricUnit, dd)
}

unsafe fn FigHeight(gc: *const c_void, dd: *mut c_void) -> c_double {
    let mut height = 0.0;
    let mut depth = 0.0;
    let mut width = 0.0;
    GEMetricInfo('0' as c_int, gc, &mut height, &mut depth, &mut width, dd);
    fromDeviceHeight(height, MetricUnit, dd)
}

unsafe fn DescDepth(gc: *const c_void, dd: *mut c_void) -> c_double {
    let mut height = 0.0;
    let mut depth = 0.0;
    let mut width = 0.0;
    GEMetricInfo('g' as c_int, gc, &mut height, &mut depth, &mut width, dd);
    fromDeviceHeight(depth, MetricUnit, dd)
}

fn RuleThickness() -> c_double {
    0.015
}

unsafe fn ThinSpace(gc: *const c_void, dd: *mut c_void) -> c_double {
    let mut height = 0.0;
    let mut depth = 0.0;
    let mut width = 0.0;
    GEMetricInfo('M' as c_int, gc, &mut height, &mut depth, &mut width, dd);
    fromDeviceHeight((1.0 / 6.0) * width, MetricUnit, dd)
}

unsafe fn MediumSpace(gc: *const c_void, dd: *mut c_void) -> c_double {
    let mut height = 0.0;
    let mut depth = 0.0;
    let mut width = 0.0;
    GEMetricInfo('M' as c_int, gc, &mut height, &mut depth, &mut width, dd);
    fromDeviceHeight((2.0 / 9.0) * width, MetricUnit, dd)
}

unsafe fn ThickSpace(gc: *const c_void, dd: *mut c_void) -> c_double {
    let mut height = 0.0;
    let mut depth = 0.0;
    let mut width = 0.0;
    GEMetricInfo('M' as c_int, gc, &mut height, &mut depth, &mut width, dd);
    fromDeviceHeight((5.0 / 18.0) * width, MetricUnit, dd)
}

unsafe fn MuSpace(gc: *const c_void, dd: *mut c_void) -> c_double {
    let mut height = 0.0;
    let mut depth = 0.0;
    let mut width = 0.0;
    GEMetricInfo('M' as c_int, gc, &mut height, &mut depth, &mut width, dd);
    fromDeviceHeight((1.0 / 18.0) * width, MetricUnit, dd)
}

unsafe fn TeX(which: TEXPAR, gc: *const c_void, dd: *mut c_void) -> c_double {
    match which {
        TEXPAR::sigma2 | TEXPAR::sigma5 => xHeight(gc, dd),
        TEXPAR::sigma6 => Quad(gc, dd),
        TEXPAR::sigma8 => AxisHeight(gc, dd) + 3.51 * RuleThickness() + 0.15 * XHeight(gc, dd) + SUBS * DescDepth(gc, dd),
        TEXPAR::sigma9 => AxisHeight(gc, dd) + 1.51 * RuleThickness() + 0.15 * XHeight(gc, dd) + SUBS * DescDepth(gc, dd),
        TEXPAR::sigma10 => AxisHeight(gc, dd) + 1.51 * RuleThickness() + 0.15 * XHeight(gc, dd) + (2.0 * SUBS) * DescDepth(gc, dd),
        TEXPAR::sigma11 => AxisHeight(gc, dd) + 1.51 * RuleThickness() + 0.15 * XHeight(gc, dd) + (3.0 * SUBS) * DescDepth(gc, dd),
        TEXPAR::sigma12 => 0.75 * FigHeight(gc, dd),
        TEXPAR::sigma13 => 0.75 * FigHeight(gc, dd),
        TEXPAR::sigma14 => 0.75 * FigHeight(gc, dd),
        TEXPAR::sigma15 => 0.75 * FigHeight(gc, dd),
        TEXPAR::sigma16 => 0.5 * FigHeight(gc, dd),
        TEXPAR::sigma17 => 0.5 * FigHeight(gc, dd),
        TEXPAR::sigma18 => 0.5 * FigHeight(gc, dd),
        TEXPAR::sigma19 => 0.5 * FigHeight(gc, dd),
        TEXPAR::sigma20 => 0.5 * FigHeight(gc, dd),
        TEXPAR::sigma21 => 0.5 * FigHeight(gc, dd),
        TEXPAR::sigma22 => 0.5 * FigHeight(gc, dd),
        TEXPAR::xi8 => 0.75 * FigHeight(gc, dd),
        TEXPAR::xi9 => 0.75 * FigHeight(gc, dd),
        TEXPAR::xi10 => 0.75 * FigHeight(gc, dd),
        TEXPAR::xi11 => 0.75 * FigHeight(gc, dd),
        TEXPAR::xi12 => 0.75 * FigHeight(gc, dd),
        TEXPAR::xi13 => 0.75 * FigHeight(gc, dd),
    }
}

fn GetStyle(mc: &mathContext) -> STYLE {
    mc.CurrentStyle
}

unsafe fn SetStyle(newstyle: STYLE, mc: &mut mathContext, gc: *const c_void, dd: *mut c_void) {
    mc.CurrentStyle = newstyle;
    mc.BaseCex = match newstyle {
        STYLE::SS1 | STYLE::SS | STYLE::S1 | STYLE::S => {
            let x = xHeight(gc, dd);
            let y = TeX(TEXPAR::sigma5, gc, dd);
            if y > 0.0 { x / y } else { 1.0 }
        }
    };
}

unsafe fn SetPrimeStyle(style: STYLE, mc: &mut mathContext, gc: *const c_void, dd: *mut c_void) {
    let newstyle = match style {
        STYLE::D => STYLE::D1,
        STYLE::T => STYLE::T1,
        STYLE::S => STYLE::S1,
        STYLE::SS => STYLE::SS1,
        _ => style,
    };
    SetStyle(newstyle, mc, gc, dd);
}

unsafe fn SetSupStyle(style: STYLE, mc: &mut mathContext, gc: *const c_void, dd: *mut c_void) {
    let newstyle = match style {
        STYLE::D => STYLE::S,
        STYLE::T => STYLE::S,
        STYLE::D1 => STYLE::S1,
        STYLE::T1 => STYLE::S1,
        STYLE::S => STYLE::SS,
        STYLE::S1 => STYLE::SS1,
        STYLE::SS => STYLE::SS,
        STYLE::SS1 => STYLE::SS1,
    };
    SetStyle(newstyle, mc, gc, dd);
}

unsafe fn SetSubStyle(style: STYLE, mc: &mut mathContext, gc: *const c_void, dd: *mut c_void) {
    let newstyle = match style {
        STYLE::D => STYLE::S1,
        STYLE::T => STYLE::S1,
        STYLE::D1 => STYLE::S,
        STYLE::T1 => STYLE::S,
        STYLE::S => STYLE::SS1,
        STYLE::S1 => STYLE::SS,
        STYLE::SS => STYLE::SS1,
        STYLE::SS1 => STYLE::SS,
    };
    SetStyle(newstyle, mc, gc, dd);
}

unsafe fn SetNumStyle(style: STYLE, mc: &mut mathContext, gc: *const c_void, dd: *mut c_void) {
    let newstyle = match style {
        STYLE::D | STYLE::T => STYLE::S,
        STYLE::D1 | STYLE::T1 => STYLE::S1,
        _ => style,
    };
    SetStyle(newstyle, mc, gc, dd);
}

unsafe fn SetDenomStyle(style: STYLE, mc: &mut mathContext, gc: *const c_void, dd: *mut c_void) {
    let newstyle = match style {
        STYLE::D | STYLE::T => STYLE::S1,
        STYLE::D1 | STYLE::T1 => STYLE::S,
        _ => style,
    };
    SetStyle(newstyle, mc, gc, dd);
}

fn IsCompactStyle(style: STYLE, _mc: &mathContext, _gc: *const c_void) -> c_int {
    match style {
        STYLE::SS1 | STYLE::SS | STYLE::S1 | STYLE::S => 1,
        _ => 0,
    }
}

fn GetFont(_gc: *const c_void) -> FontType {
    FontType::PlainFont
}

fn SetFont(font: FontType, _gc: *const c_void) -> FontType {
    font
}

fn UsingItalics(_gc: *const c_void) -> c_int {
    0
}

pub unsafe fn GEMathText(
    x: c_double,
    y: c_double,
    expr: SEXP,
    xc: c_double,
    yc: c_double,
    rot: c_double,
    gc: *const c_void,
    dd: *mut c_void,
) {
    if expr.is_null() {
        return;
    }
    let _ = (x, y, xc, yc, rot, gc, dd);
}
