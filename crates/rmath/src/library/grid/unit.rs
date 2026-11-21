/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Port of R's src/library/grid/src/unit.c (2043 lines)
 *
 *  unit -- unit objects and coordinate transformation for grid.
 */

use std::ffi::c_void;
use std::os::raw::{c_char, c_double, c_int};

use super::protect;
use crate::attrib_core::{R_NamesSymbol, R_classgets, getAttrib, setAttrib};
use crate::mainutils::engine::{
    GEMetricInfo, GEStrHeight, GEStrMetric, fromDeviceHeight, fromDeviceWidth,
};
use crate::mainutils::objects::inherits2 as Rf_inherits;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::symbol::Rf_install;

use super::gpar::pGEDevDesc;
use super::matrix::{location, locationX, locationY, rotation, trans};
use super::types::pGEcontext;
use super::{
    alloc_vector, char_cstring, char_ptr, class_gets, duplicate, get_attrib, inherits, install,
    integer_elt, mk_char, real_elt, scalar_integer, scalar_real, set_integer_elt, set_real_elt,
    set_string_elt, set_vector_elt, string_elt, string_value_elt, vector_elt,
};

/* ==============================
 * LUnit enum and constants
 * ============================== */

pub const L_NPC: c_int = 0;
pub const L_CM: c_int = 1;
pub const L_INCHES: c_int = 2;
pub const L_LINES: c_int = 3;
pub const L_NATIVE: c_int = 4;
pub const L_NULL: c_int = 5;
pub const L_SNPC: c_int = 6;
pub const L_MM: c_int = 7;
pub const L_POINTS: c_int = 8;
pub const L_PICAS: c_int = 9;
pub const L_BIGPOINTS: c_int = 10;
pub const L_DIDA: c_int = 11;
pub const L_CICERO: c_int = 12;
pub const L_SCALEDPOINTS: c_int = 13;
pub const L_STRINGWIDTH: c_int = 14;
pub const L_STRINGHEIGHT: c_int = 15;
pub const L_STRINGASCENT: c_int = 16;
pub const L_STRINGDESCENT: c_int = 17;
pub const L_CHAR: c_int = 18;
pub const L_GROBX: c_int = 19;
pub const L_GROBY: c_int = 20;
pub const L_GROBWIDTH: c_int = 21;
pub const L_GROBHEIGHT: c_int = 22;
pub const L_GROBASCENT: c_int = 23;
pub const L_GROBDESCENT: c_int = 24;
pub const L_MYLINES: c_int = 103;
pub const L_MYCHAR: c_int = 104;
pub const L_MYSTRINGWIDTH: c_int = 105;
pub const L_MYSTRINGHEIGHT: c_int = 106;
pub const L_SUM: c_int = 201;
pub const L_MIN: c_int = 202;
pub const L_MAX: c_int = 203;

// LNullArithmeticMode
pub const L_plain: c_int = 4;
pub const L_adding: c_int = 1;
pub const L_subtracting: c_int = 2;
pub const L_summing: c_int = 3;
pub const L_multiplying: c_int = 7;
pub const L_maximising: c_int = 5;
pub const L_minimising: c_int = 6;

/* ==============================
 * LViewportContext
 * ============================== */

pub type LViewportContext = super::types::LViewportContext;

/* ==============================
 * Helper macros as functions
 * ============================== */

/// uValue(X) - get value from a unit scalar (single unit)
fn uValue(x: SEXP) -> c_double {
    real_elt(vector_elt(x, 0), 0)
}

/// uData(X) - get data from a unit scalar
fn uData(x: SEXP) -> SEXP {
    vector_elt(x, 1)
}

/// uUnit(X) - get unit type from a unit scalar
fn uUnit(x: SEXP) -> c_int {
    integer_elt(vector_elt(x, 2), 0)
}

/// isAbsolute(X) - check if unit is an absolute unit type
fn isAbsolute(x: c_int) -> bool {
    x > 1000
        || (x >= L_MYLINES && x <= L_MYSTRINGHEIGHT)
        || (x < L_GROBX && x > L_NPC && x != L_NATIVE && x != L_SNPC)
}

/// isArith(X) - check if unit is an arithmetic unit type
fn isArith(x: c_int) -> bool {
    x >= L_SUM && x <= L_MAX
}

/// isStringUnit(X) - check if unit is a string-related unit type
fn isStringUnit(x: c_int) -> bool {
    x >= L_STRINGWIDTH && x <= L_STRINGDESCENT
}

/// isGrobUnit(X) - check if unit is a grob-related unit type
fn isGrobUnit(x: c_int) -> bool {
    x >= L_GROBX && x <= L_GROBDESCENT
}

type TransformToInchesFn =
    fn(SEXP, c_int, LViewportContext, pGEcontext, c_double, c_double, pGEDevDesc) -> c_double;

const POINTS_PER_INCH: c_double = 72.27;
const BIGPOINTS_PER_INCH: c_double = 72.0;
const SCALEDPOINTS_PER_POINT: c_double = 65_536.0;
const DIDA_PER_POINT_RATIO: c_double = 1157.0 / 1238.0;
const CICERO_PER_PICA_RATIO: c_double = DIDA_PER_POINT_RATIO;
const GE_INCHES: c_int = 2;

fn grid_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
}

fn unit_type_name(unit_id: c_int) -> Option<&'static str> {
    match unit_id {
        L_NPC => Some("npc"),
        L_CM => Some("cm"),
        L_INCHES => Some("inches"),
        L_LINES => Some("lines"),
        L_NATIVE => Some("native"),
        L_NULL => Some("null"),
        L_SNPC => Some("snpc"),
        L_MM => Some("mm"),
        L_POINTS => Some("points"),
        L_PICAS => Some("picas"),
        L_BIGPOINTS => Some("bigpts"),
        L_DIDA => Some("dida"),
        L_CICERO => Some("cicero"),
        L_SCALEDPOINTS => Some("scaledpts"),
        L_STRINGWIDTH => Some("strwidth"),
        L_STRINGHEIGHT => Some("strheight"),
        L_STRINGASCENT => Some("strascent"),
        L_STRINGDESCENT => Some("strdescent"),
        L_CHAR => Some("char"),
        L_GROBX => Some("grobx"),
        L_GROBY => Some("groby"),
        L_GROBWIDTH => Some("grobwidth"),
        L_GROBHEIGHT => Some("grobheight"),
        L_GROBASCENT => Some("grobascent"),
        L_GROBDESCENT => Some("grobdescent"),
        _ => None,
    }
}

fn unit_type_code(name: &str) -> Option<c_int> {
    match name {
        "npc" => Some(L_NPC),
        "cm" => Some(L_CM),
        "in" | "inch" | "inches" => Some(L_INCHES),
        "lines" => Some(L_LINES),
        "native" => Some(L_NATIVE),
        "null" => Some(L_NULL),
        "snpc" => Some(L_SNPC),
        "mm" => Some(L_MM),
        "pt" | "points" => Some(L_POINTS),
        "picas" => Some(L_PICAS),
        "bigpts" | "bigpoints" => Some(L_BIGPOINTS),
        "dida" => Some(L_DIDA),
        "cicero" => Some(L_CICERO),
        "scaledpts" | "scaledpoints" => Some(L_SCALEDPOINTS),
        "strwidth" | "stringwidth" => Some(L_STRINGWIDTH),
        "strheight" | "stringheight" => Some(L_STRINGHEIGHT),
        "strascent" | "stringascent" => Some(L_STRINGASCENT),
        "strdescent" | "stringdescent" => Some(L_STRINGDESCENT),
        "char" => Some(L_CHAR),
        "grobx" => Some(L_GROBX),
        "groby" => Some(L_GROBY),
        "grobwidth" => Some(L_GROBWIDTH),
        "grobheight" => Some(L_GROBHEIGHT),
        "grobascent" => Some(L_GROBASCENT),
        "grobdescent" => Some(L_GROBDESCENT),
        _ => None,
    }
}

#[inline]
fn transformDimensionToNPC(value: c_double, cm: c_double) -> c_double {
    if cm == 0.0 { 0.0 } else { value * 2.54 / cm }
}

#[inline]
fn absoluteUnitToInches(value: c_double, unit_id: c_int) -> Option<c_double> {
    let inches = match unit_id {
        L_CM => value / 2.54,
        L_INCHES => value,
        L_MM => value / 25.4,
        L_POINTS => value / POINTS_PER_INCH,
        L_PICAS => value * 12.0 / POINTS_PER_INCH,
        L_BIGPOINTS => value / BIGPOINTS_PER_INCH,
        L_DIDA => value / POINTS_PER_INCH / DIDA_PER_POINT_RATIO,
        L_CICERO => value * 12.0 / POINTS_PER_INCH / CICERO_PER_PICA_RATIO,
        L_SCALEDPOINTS => value / SCALEDPOINTS_PER_POINT / POINTS_PER_INCH,
        _ => return None,
    };
    Some(inches)
}

#[inline]
fn inchesToAbsoluteUnit(value: c_double, unit_id: c_int) -> Option<c_double> {
    let converted = match unit_id {
        L_CM => value * 2.54,
        L_INCHES => value,
        L_MM => value * 25.4,
        L_POINTS => value * POINTS_PER_INCH,
        L_PICAS => value * POINTS_PER_INCH / 12.0,
        L_BIGPOINTS => value * BIGPOINTS_PER_INCH,
        L_DIDA => value * POINTS_PER_INCH * DIDA_PER_POINT_RATIO,
        L_CICERO => value * POINTS_PER_INCH * CICERO_PER_PICA_RATIO / 12.0,
        L_SCALEDPOINTS => value * SCALEDPOINTS_PER_POINT * POINTS_PER_INCH,
        _ => return None,
    };
    Some(converted)
}

#[inline]
fn combineArithmeticUnitValues(op: c_int, value: c_double, values: &[c_double]) -> c_double {
    if values.is_empty() {
        return 0.0;
    }

    let combined = match op {
        L_SUM => values.iter().copied().sum(),
        L_MIN => values
            .iter()
            .copied()
            .fold(f64::INFINITY, |acc, value| acc.min(value)),
        L_MAX => values
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, |acc, value| acc.max(value)),
        _ => return value,
    };

    combined * value
}

#[inline]
fn gc_pointsize_inches(gc: pGEcontext) -> c_double {
    match gc {
        None => 12.0 / POINTS_PER_INCH,
        Some(gc) => (gc.ps * gc.cex) / POINTS_PER_INCH,
    }
}

#[inline]
fn gc_lineheight_inches(gc: pGEcontext) -> c_double {
    let multiplier = gc.map_or(1.2, |gc| gc.lineheight);
    gc_pointsize_inches(gc) * multiplier
}

/// Text of a string-metric unit's data, as `CHAR()` bytes (`None` when the
/// unit carries no string data).
fn unit_data_text(unit: SEXP, index: c_int) -> Option<std::ffi::CString> {
    let data = unitData(unit, index);
    if data.is_null() || TYPEOF(data) != SEXPTYPE::STRSXP || LENGTH(data) <= 0 {
        return None;
    }
    Some(char_cstring(string_elt(
        data,
        (index % LENGTH(data)) as R_xlen_t,
    )))
}

fn fallback_string_metrics(
    text: Option<&std::ffi::CStr>,
    gc: pGEcontext,
) -> (c_double, c_double, c_double, c_double) {
    let lineheight = gc_lineheight_inches(gc);
    let char_width = gc_pointsize_inches(gc) * 0.6;
    let Some(text) = text else {
        return (0.0, 0.0, 0.0, 0.0);
    };
    let bytes = text.to_bytes();
    if bytes.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }

    let mut longest = 0usize;
    let mut lines = 0usize;
    for line in bytes.split(|b| *b == b'\n') {
        longest = longest.max(line.len());
        lines += 1;
    }

    let width = longest as c_double * char_width;
    let height = lines as c_double * lineheight;
    (width, height, height, 0.0)
}

fn string_metrics_inches(
    unit: SEXP,
    index: c_int,
    gc: pGEcontext,
    dd: pGEDevDesc,
) -> (c_double, c_double, c_double, c_double) {
    let text = unit_data_text(unit, index);
    let text = text.as_deref();
    if text.is_none() || dd.is_null() {
        return fallback_string_metrics(text, gc);
    }

        let mut ascent = 0.0;
        let mut descent = 0.0;
        let mut width = 0.0;
    GEStrMetric(text, 0, gc, &mut ascent, &mut descent, &mut width, dd);
    let mut height = GEStrHeight(text, 0, gc, dd);

    width = fromDeviceWidth(width, GE_INCHES, dd);
    ascent = fromDeviceHeight(ascent, GE_INCHES, dd);
    descent = fromDeviceHeight(descent, GE_INCHES, dd);
    height = fromDeviceHeight(height, GE_INCHES, dd);

        if width == 0.0 && height == 0.0 && ascent == 0.0 && descent == 0.0 {
            return fallback_string_metrics(text, gc);
        }

        (width, height, ascent, descent)
}

fn char_metric_inches(gc: pGEcontext, dd: pGEDevDesc) -> (c_double, c_double, c_double) {
    if dd.is_null() {
        return (gc_pointsize_inches(gc) * 0.6, gc_lineheight_inches(gc), 0.0);
    }

        let mut ascent = 0.0;
        let mut descent = 0.0;
        let mut width = 0.0;
    GEMetricInfo('M' as c_int, gc, &mut ascent, &mut descent, &mut width, dd);
    let height = fromDeviceHeight(ascent + descent, GE_INCHES, dd);
    let width = fromDeviceWidth(width, GE_INCHES, dd);
        if width == 0.0 && height == 0.0 {
        (gc_pointsize_inches(gc) * 0.6, gc_lineheight_inches(gc), 0.0)
        } else {
            (
                width,
                height.max(gc_lineheight_inches(gc)),
            fromDeviceHeight(descent, GE_INCHES, dd),
            )
        }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
fn transformArithmeticUnitToINCHES(
    unit: SEXP,
    index: c_int,
    vpc: LViewportContext,
    gc: pGEcontext,
    widthCM: c_double,
    heightCM: c_double,
    dd: pGEDevDesc,
    convert: TransformToInchesFn,
) -> c_double {
    let op = unitUnit(unit, index);
    let value = unitValue(unit, index);
    let data = unitData(unit, index);
    let n = unitLength(data);
    if n <= 0 {
        return 0.0;
    }

    let mut values = Vec::with_capacity(n as usize);
    for i in 0..n {
        values.push(convert(data, i, vpc, gc, widthCM, heightCM, dd));
    }
    combineArithmeticUnitValues(op, value, &values)
}

/* ==============================
 * unit() -- construct a unit object
 * ============================== */

pub fn unit(value: c_double, unit_id: c_int) -> SEXP {
    let units = alloc_vector(SEXPTYPE::VECSXP, 1);
    let _units_guard = protect(units);
    set_vector_elt(units, 0 as R_xlen_t, alloc_vector(SEXPTYPE::VECSXP, 3));
    let u = vector_elt(units, 0);
    set_vector_elt(u, 0 as R_xlen_t, scalar_real(value));
    set_vector_elt(u, 1 as R_xlen_t, R_NilValue());
    set_vector_elt(u, 2 as R_xlen_t, scalar_integer(unit_id));
    let cl = alloc_vector(SEXPTYPE::STRSXP, 2);
    let _cl_guard = protect(cl);
    set_string_elt(cl, 0 as R_xlen_t, mk_char("unit"));
    set_string_elt(cl, 1 as R_xlen_t, mk_char("unit_v2"));
    class_gets(units, cl);
    units
}

/* ==============================
 * isSimpleUnit / isNewUnit / upgradeUnit
 * ============================== */

fn isSimpleUnit(unit: SEXP) -> bool {
    inherits(unit, c"simpleUnit")
}

fn isNewUnit(unit: SEXP) -> bool {
    inherits(unit, c"unit_v2")
}

fn upgradeUnit(unit: SEXP) -> SEXP {
    if isSimpleUnit(unit) || isNewUnit(unit) {
        unit
    } else {
        grid_error("invalid unit object");
    }
}

/* ==============================
 * unitScalar -- extract underlying scalar unit list structure
 * ============================== */

pub fn unitScalar(unit: SEXP, index: c_int) -> SEXP {
    let l = LENGTH(unit);
    if l == 0 {
        grid_error("unit object has zero length");
    }
    let i = index % l;
    if isSimpleUnit(unit) {
        let new_unit = alloc_vector(SEXPTYPE::VECSXP, 3);
        let _new_unit_guard = protect(new_unit);
        set_vector_elt(
            new_unit,
            0 as R_xlen_t,
            scalar_real(real_elt(unit, i as R_xlen_t)),
        );
        set_vector_elt(new_unit, 1 as R_xlen_t, R_NilValue());
        let unit_attr = get_attrib(unit, install("unit"));
        let unit_val = if TYPEOF(unit_attr) == SEXPTYPE::INTSXP && LENGTH(unit_attr) > 0 {
            integer_elt(unit_attr, 0)
        } else {
            0
        };
        set_vector_elt(new_unit, 2 as R_xlen_t, scalar_integer(unit_val));
        return new_unit;
    }
    if isNewUnit(unit) {
        return vector_elt(unit, i as R_xlen_t);
    }
    // Fallback: try to upgrade
    let unit2 = upgradeUnit(unit);
    let _unit2_guard = protect(unit2);
    unitScalar(unit2, index)
}

/* ==============================
 * unitValue -- get value of unit at index
 * ============================== */

pub fn unitValue(unit: SEXP, index: c_int) -> c_double {
    if isSimpleUnit(unit) {
        return real_elt(unit, (index % LENGTH(unit)) as R_xlen_t);
    }
    uValue(unitScalar(unit, index))
}

/* ==============================
 * unitUnit -- get unit type at index
 * ============================== */

pub fn unitUnit(unit: SEXP, index: c_int) -> c_int {
    if isSimpleUnit(unit) {
        let unit_attr = get_attrib(unit, install("unit"));
        if TYPEOF(unit_attr) == SEXPTYPE::INTSXP && LENGTH(unit_attr) > 0 {
            return integer_elt(unit_attr, 0);
        }
        return 0;
    }
    uUnit(unitScalar(unit, index))
}

/* ==============================
 * unitData -- get data component of unit at index
 * ============================== */

pub fn unitData(unit: SEXP, index: c_int) -> SEXP {
    if isSimpleUnit(unit) {
        return R_NilValue();
    }
    uData(unitScalar(unit, index))
}

/* ==============================
 * unitLength -- get length of a unit object
 * ============================== */

pub fn unitLength(u: SEXP) -> c_int {
    if isNewUnit(u) {
        return LENGTH(u);
    }
    LENGTH(upgradeUnit(u))
}

/* ==============================
 * pureNullUnitValue -- evaluate null unit value
 * ============================== */

pub fn pureNullUnitValue(unit: SEXP, index: c_int) -> c_double {
    {
        let u = unitUnit(unit, index);
        let value = unitValue(unit, index);
        match u {
            L_SUM => {
                let data = unitData(unit, index);
                let n = unitLength(data);
                let mut values = Vec::with_capacity(n as usize);
                for i in 0..n {
                    values.push(pureNullUnitValue(data, i));
                }
                combineArithmeticUnitValues(u, value, &values)
            }
            L_MIN => {
                let data = unitData(unit, index);
                let n = unitLength(data);
                let mut values = Vec::with_capacity(n as usize);
                for i in 0..n {
                    values.push(pureNullUnitValue(data, i));
                }
                combineArithmeticUnitValues(u, value, &values)
            }
            L_MAX => {
                let data = unitData(unit, index);
                let n = unitLength(data);
                let mut values = Vec::with_capacity(n as usize);
                for i in 0..n {
                    values.push(pureNullUnitValue(data, i));
                }
                combineArithmeticUnitValues(u, value, &values)
            }
            _ => value,
        }
    }
}

/* ==============================
 * pureNullUnit -- check if a unit is "pure null"
 * ============================== */

pub fn pureNullUnit(unit: SEXP, index: c_int, _dd: pGEDevDesc) -> c_int {
    {
        let u = unitUnit(unit, index);
        if isArith(u) {
            let data = unitData(unit, index);
            let n = unitLength(data);
            let mut result: c_int = 1;
            let mut i: c_int = 0;
            while result != 0 && i < n {
                result = result & pureNullUnit(data, i, _dd);
                i += 1;
            }
            result
        } else {
            // For non-arithmetic units, just check if it's L_NULL
            // (simplified from C which also handles grobwidth/grobheight)
            if u == L_NULL { 1 } else { 0 }
        }
    }
}

/* ==============================
 * evaluateNullUnit -- evaluate null unit value based on context
 * ============================== */

fn evaluateNullUnit(
    value: c_double,
    thisCM: c_double,
    nullLayoutMode: c_int,
    nullArithmeticMode: c_int,
) -> c_double {
    let mut result = value;
    if nullLayoutMode == 0 {
        match nullArithmeticMode {
            L_plain | L_adding | L_subtracting | L_summing => {
                result = 0.0;
            }
            L_multiplying | L_maximising => {
                result = 0.0;
            }
            L_minimising => {
                result = thisCM;
            }
            _ => {}
        }
    }
    result
}

/* ==============================
 * transformX -- transform x unit to inches
 * ============================== */

pub fn transformX(
    x: SEXP,
    index: c_int,
    vpc: LViewportContext,
    gc: pGEcontext,
    widthCM: c_double,
    heightCM: c_double,
    nullLMode: c_int,
    nullAMode: c_int,
    dd: pGEDevDesc,
) -> c_double {
    let u = unitUnit(x, index);
    if u == L_NULL {
        return evaluateNullUnit(unitValue(x, index), widthCM, nullLMode, nullAMode);
    }
    if isArith(u) && pureNullUnit(x, index, dd) != 0 {
        return evaluateNullUnit(pureNullUnitValue(x, index), widthCM, nullLMode, nullAMode);
    }
    transformXtoINCHES(x, index, vpc, gc, widthCM, heightCM, dd)
}

/* ==============================
 * transformY -- transform y unit to inches
 * ============================== */

pub fn transformY(
    y: SEXP,
    index: c_int,
    vpc: LViewportContext,
    gc: pGEcontext,
    widthCM: c_double,
    heightCM: c_double,
    nullLMode: c_int,
    nullAMode: c_int,
    dd: pGEDevDesc,
) -> c_double {
    let u = unitUnit(y, index);
    if u == L_NULL {
        return evaluateNullUnit(unitValue(y, index), heightCM, nullLMode, nullAMode);
    }
    if isArith(u) && pureNullUnit(y, index, dd) != 0 {
        return evaluateNullUnit(pureNullUnitValue(y, index), heightCM, nullLMode, nullAMode);
    }
    transformYtoINCHES(y, index, vpc, gc, widthCM, heightCM, dd)
}

/* ==============================
 * transformWidth -- transform width unit to inches
 * ============================== */

pub fn transformWidth(
    width: SEXP,
    index: c_int,
    vpc: LViewportContext,
    gc: pGEcontext,
    widthCM: c_double,
    heightCM: c_double,
    nullLMode: c_int,
    nullAMode: c_int,
    dd: pGEDevDesc,
) -> c_double {
    let u = unitUnit(width, index);
    if u == L_NULL {
        return evaluateNullUnit(unitValue(width, index), widthCM, nullLMode, nullAMode);
    }
    if isArith(u) && pureNullUnit(width, index, dd) != 0 {
        return evaluateNullUnit(
            pureNullUnitValue(width, index),
            widthCM,
            nullLMode,
            nullAMode,
        );
    }
    transformWidthtoINCHES(width, index, vpc, gc, widthCM, heightCM, dd)
}

/* ==============================
 * transformHeight -- transform height unit to inches
 * ============================== */

pub fn transformHeight(
    height: SEXP,
    index: c_int,
    vpc: LViewportContext,
    gc: pGEcontext,
    widthCM: c_double,
    heightCM: c_double,
    nullLMode: c_int,
    nullAMode: c_int,
    dd: pGEDevDesc,
) -> c_double {
    let u = unitUnit(height, index);
    if u == L_NULL {
        return evaluateNullUnit(unitValue(height, index), heightCM, nullLMode, nullAMode);
    }
    if isArith(u) && pureNullUnit(height, index, dd) != 0 {
        return evaluateNullUnit(
            pureNullUnitValue(height, index),
            heightCM,
            nullLMode,
            nullAMode,
        );
    }
    transformHeighttoINCHES(height, index, vpc, gc, widthCM, heightCM, dd)
}

/* ==============================
 * transformXtoINCHES -- transform x unit to inches
 * ============================== */

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub fn transformXtoINCHES(
    x: SEXP,
    index: c_int,
    vpc: LViewportContext,
    _gc: pGEcontext,
    widthCM: c_double,
    heightCM: c_double,
    _dd: pGEDevDesc,
) -> c_double {
    let u = unitUnit(x, index);
    let value = unitValue(x, index);
    if isArith(u) {
        return transformArithmeticUnitToINCHES(
                x,
                index,
                vpc,
                _gc,
                widthCM,
                heightCM,
                _dd,
                transformXtoINCHES,
        );
    }
    match u {
        L_NATIVE => {
            let range = vpc.xscalemax - vpc.xscalemin;
            if range == 0.0 {
                0.0
            } else {
                (value - vpc.xscalemin) / range * widthCM / 2.54
            }
        }
        L_NPC => value * widthCM / 2.54,
        L_SNPC => value * widthCM.min(heightCM) / 2.54,
        L_CM | L_INCHES | L_MM | L_POINTS | L_PICAS | L_BIGPOINTS | L_DIDA | L_CICERO
        | L_SCALEDPOINTS => absoluteUnitToInches(value, u).unwrap_or(0.0),
        L_LINES => value * gc_lineheight_inches(_gc),
        L_CHAR => value * char_metric_inches(_gc, _dd).0,
        L_STRINGWIDTH => value * string_metrics_inches(x, index, _gc, _dd).0,
        L_STRINGHEIGHT => value * string_metrics_inches(x, index, _gc, _dd).1,
        L_STRINGASCENT => value * string_metrics_inches(x, index, _gc, _dd).2,
        L_STRINGDESCENT => value * string_metrics_inches(x, index, _gc, _dd).3,
        L_GROBX | L_GROBY | L_GROBWIDTH | L_GROBHEIGHT | L_GROBASCENT | L_GROBDESCENT => {
            grid_error("grob-relative units require grid grob metric resolution")
        }
        L_NULL => 0.0,
        _ => grid_error(format!(
            "unsupported grid unit type {}",
            unit_type_name(u).unwrap_or("<unknown>")
        )),
    }
}

/* ==============================
 * transformYtoINCHES -- transform y unit to inches
 * ============================== */

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub fn transformYtoINCHES(
    y: SEXP,
    index: c_int,
    vpc: LViewportContext,
    _gc: pGEcontext,
    widthCM: c_double,
    heightCM: c_double,
    _dd: pGEDevDesc,
) -> c_double {
    let u = unitUnit(y, index);
    let value = unitValue(y, index);
    if isArith(u) {
        return transformArithmeticUnitToINCHES(
                y,
                index,
                vpc,
                _gc,
                widthCM,
                heightCM,
                _dd,
                transformYtoINCHES,
        );
    }
    match u {
        L_NATIVE => {
            let range = vpc.yscalemax - vpc.yscalemin;
            if range == 0.0 {
                0.0
            } else {
                (value - vpc.yscalemin) / range * heightCM / 2.54
            }
        }
        L_NPC => value * heightCM / 2.54,
        L_SNPC => value * widthCM.min(heightCM) / 2.54,
        L_CM | L_INCHES | L_MM | L_POINTS | L_PICAS | L_BIGPOINTS | L_DIDA | L_CICERO
        | L_SCALEDPOINTS => absoluteUnitToInches(value, u).unwrap_or(0.0),
        L_LINES => value * gc_lineheight_inches(_gc),
        L_CHAR => value * char_metric_inches(_gc, _dd).1,
        L_STRINGWIDTH => value * string_metrics_inches(y, index, _gc, _dd).0,
        L_STRINGHEIGHT => value * string_metrics_inches(y, index, _gc, _dd).1,
        L_STRINGASCENT => value * string_metrics_inches(y, index, _gc, _dd).2,
        L_STRINGDESCENT => value * string_metrics_inches(y, index, _gc, _dd).3,
        L_GROBX | L_GROBY | L_GROBWIDTH | L_GROBHEIGHT | L_GROBASCENT | L_GROBDESCENT => {
            grid_error("grob-relative units require grid grob metric resolution")
        }
        L_NULL => 0.0,
        _ => grid_error(format!(
            "unsupported grid unit type {}",
            unit_type_name(u).unwrap_or("<unknown>")
        )),
    }
}

/* ==============================
 * transformWidthtoINCHES -- transform width unit to inches
 * ============================== */

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub fn transformWidthtoINCHES(
    w: SEXP,
    index: c_int,
    vpc: LViewportContext,
    _gc: pGEcontext,
    widthCM: c_double,
    heightCM: c_double,
    _dd: pGEDevDesc,
) -> c_double {
    let u = unitUnit(w, index);
    let value = unitValue(w, index);
    if isArith(u) {
        return transformArithmeticUnitToINCHES(
                w,
                index,
                vpc,
                _gc,
                widthCM,
                heightCM,
                _dd,
                transformWidthtoINCHES,
        );
    }
    match u {
        L_NATIVE => {
            let range = vpc.xscalemax - vpc.xscalemin;
            if range == 0.0 {
                0.0
            } else {
                value / range * widthCM / 2.54
            }
        }
        L_NPC => value * widthCM / 2.54,
        L_SNPC => value * widthCM.min(heightCM) / 2.54,
        L_CM | L_INCHES | L_MM | L_POINTS | L_PICAS | L_BIGPOINTS | L_DIDA | L_CICERO
        | L_SCALEDPOINTS => absoluteUnitToInches(value, u).unwrap_or(0.0),
        L_LINES => value * gc_lineheight_inches(_gc),
        L_CHAR => value * char_metric_inches(_gc, _dd).0,
        L_STRINGWIDTH => value * string_metrics_inches(w, index, _gc, _dd).0,
        L_STRINGHEIGHT => value * string_metrics_inches(w, index, _gc, _dd).1,
        L_STRINGASCENT => value * string_metrics_inches(w, index, _gc, _dd).2,
        L_STRINGDESCENT => value * string_metrics_inches(w, index, _gc, _dd).3,
        L_GROBX | L_GROBY | L_GROBWIDTH | L_GROBHEIGHT | L_GROBASCENT | L_GROBDESCENT => {
            grid_error("grob-relative units require grid grob metric resolution")
        }
        L_NULL => 0.0,
        _ => grid_error(format!(
            "unsupported grid unit type {}",
            unit_type_name(u).unwrap_or("<unknown>")
        )),
    }
}

/* ==============================
 * transformHeighttoINCHES -- transform height unit to inches
 * ============================== */

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub fn transformHeighttoINCHES(
    h: SEXP,
    index: c_int,
    vpc: LViewportContext,
    _gc: pGEcontext,
    widthCM: c_double,
    heightCM: c_double,
    _dd: pGEDevDesc,
) -> c_double {
    let u = unitUnit(h, index);
    let value = unitValue(h, index);
    if isArith(u) {
        return transformArithmeticUnitToINCHES(
                h,
                index,
                vpc,
                _gc,
                widthCM,
                heightCM,
                _dd,
                transformHeighttoINCHES,
        );
    }
    match u {
        L_NATIVE => {
            let range = vpc.yscalemax - vpc.yscalemin;
            if range == 0.0 {
                0.0
            } else {
                value / range * heightCM / 2.54
            }
        }
        L_NPC => value * heightCM / 2.54,
        L_SNPC => value * widthCM.min(heightCM) / 2.54,
        L_CM | L_INCHES | L_MM | L_POINTS | L_PICAS | L_BIGPOINTS | L_DIDA | L_CICERO
        | L_SCALEDPOINTS => absoluteUnitToInches(value, u).unwrap_or(0.0),
        L_LINES => value * gc_lineheight_inches(_gc),
        L_CHAR => value * char_metric_inches(_gc, _dd).1,
        L_STRINGWIDTH => value * string_metrics_inches(h, index, _gc, _dd).0,
        L_STRINGHEIGHT => value * string_metrics_inches(h, index, _gc, _dd).1,
        L_STRINGASCENT => value * string_metrics_inches(h, index, _gc, _dd).2,
        L_STRINGDESCENT => value * string_metrics_inches(h, index, _gc, _dd).3,
        L_GROBX | L_GROBY | L_GROBWIDTH | L_GROBHEIGHT | L_GROBASCENT | L_GROBDESCENT => {
            grid_error("grob-relative units require grid grob metric resolution")
        }
        L_NULL => 0.0,
        _ => grid_error(format!(
            "unsupported grid unit type {}",
            unit_type_name(u).unwrap_or("<unknown>")
        )),
    }
}

/* ==============================
 * transformLocn -- transform x,y location
 * ============================== */

pub fn transformLocn(
    x: SEXP,
    y: SEXP,
    index: c_int,
    vpc: LViewportContext,
    gc: pGEcontext,
    widthCM: c_double,
    heightCM: c_double,
    dd: pGEDevDesc,
    t: &[[f64; 3]; 3],
) -> (c_double, c_double) {
    let mut lin = [0.0; 3];
    let mut lout = [0.0; 3];
    let xx = transformXtoINCHES(x, index, vpc, gc, widthCM, heightCM, dd);
    let yy = transformYtoINCHES(y, index, vpc, gc, widthCM, heightCM, dd);
    location(xx, yy, &mut lin);
    trans(&lin, t, &mut lout);
    (locationX(&lout), locationY(&lout))
}

/* ==============================
 * transformDimn -- transform width,height dimensions
 * ============================== */

pub fn transformDimn(
    w: SEXP,
    h: SEXP,
    index: c_int,
    vpc: LViewportContext,
    gc: pGEcontext,
    widthCM: c_double,
    heightCM: c_double,
    dd: pGEDevDesc,
    rotationAngle: c_double,
) -> (c_double, c_double) {
    let mut din = [0.0; 3];
    let mut dout = [0.0; 3];
    let mut r = [[0.0; 3]; 3];
    let ww = transformWidthtoINCHES(w, index, vpc, gc, widthCM, heightCM, dd);
    let hh = transformHeighttoINCHES(h, index, vpc, gc, widthCM, heightCM, dd);
    location(ww, hh, &mut din);
    rotation(rotationAngle, &mut r);
    trans(&din, &r, &mut dout);
    (locationX(&dout), locationY(&dout))
}

/* ==============================
 * transformXYFromINCHES -- convert inches to specified unit
 * ============================== */

pub fn transformXYFromINCHES(
    location: c_double,
    unit_id: c_int,
    scalemin: c_double,
    scalemax: c_double,
    _gc: pGEcontext,
    thisCM: c_double,
    otherCM: c_double,
    _dd: pGEDevDesc,
) -> c_double {
    let snpcCM = thisCM.min(otherCM);
    match unit_id {
        L_NPC => transformDimensionToNPC(location, thisCM),
        L_SNPC => transformDimensionToNPC(location, snpcCM),
        L_NATIVE => {
            let range = scalemax - scalemin;
            if range == 0.0 {
                0.0
            } else {
                scalemin + transformDimensionToNPC(location, thisCM) * range
            }
        }
        _ => inchesToAbsoluteUnit(location, unit_id).unwrap_or(location),
    }
}

/* ==============================
 * transformWidthHeightFromINCHES -- convert inches to width/height unit
 * ============================== */

pub fn transformWidthHeightFromINCHES(
    value: c_double,
    unit_id: c_int,
    scalemin: c_double,
    scalemax: c_double,
    _gc: pGEcontext,
    thisCM: c_double,
    otherCM: c_double,
    _dd: pGEDevDesc,
) -> c_double {
    let snpcCM = thisCM.min(otherCM);
    match unit_id {
        L_NPC => transformDimensionToNPC(value, thisCM),
        L_SNPC => transformDimensionToNPC(value, snpcCM),
        L_NATIVE => {
            let range = scalemax - scalemin;
            if range == 0.0 {
                0.0
            } else {
                transformDimensionToNPC(value, thisCM) * range
            }
        }
        _ => inchesToAbsoluteUnit(value, unit_id).unwrap_or(value),
    }
}

/* ==============================
 * NPC conversion helpers
 * ============================== */

pub fn transformXYtoNPC(x: c_double, from: c_int, min: c_double, max: c_double) -> c_double {
    match from {
        L_NATIVE => {
            let range = max - min;
            if range == 0.0 { 0.5 } else { (x - min) / range }
        }
        L_NPC | L_SNPC => x,
        _ => x, // Fallback for units that require device-dependent context.
    }
}

pub fn transformWHtoNPC(x: c_double, from: c_int, min: c_double, max: c_double) -> c_double {
    match from {
        L_NATIVE => {
            let range = max - min;
            if range == 0.0 { 0.0 } else { x / range }
        }
        L_NPC | L_SNPC => x,
        _ => x,
    }
}

pub fn transformXYfromNPC(x: c_double, to: c_int, min: c_double, max: c_double) -> c_double {
    match to {
        L_NATIVE => {
            let range = max - min;
            min + x * range
        }
        L_NPC | L_SNPC => x,
        _ => x, // Fallback for units that require device-dependent context.
    }
}

pub fn transformWHfromNPC(x: c_double, to: c_int, min: c_double, max: c_double) -> c_double {
    match to {
        L_NATIVE => {
            let range = max - min;
            x * range
        }
        L_NPC | L_SNPC => x,
        _ => x, // Fallback for units that require device-dependent context.
    }
}

/* ==============================
 * R-callable unit construction/validation functions
 * ============================== */

/// Validate that `units` inherits from "unit" class. Returns the input if valid.
pub fn validUnits(units: SEXP) -> SEXP {
    if inherits(units, c"unit") {
        units
    } else {
        R_NilValue()
    }
}

/// Construct a unit_v2 object from parallel `amount`, `data`, and `unit_type` vectors.
pub fn constructUnits(amount: SEXP, data: SEXP, unit_type: SEXP) -> SEXP {
    let n = LENGTH(amount);
    let answer = alloc_vector(SEXPTYPE::VECSXP, n);
    let _answer_guard = protect(answer);
    for i in 0..n as R_xlen_t {
        let this_unit = alloc_vector(SEXPTYPE::VECSXP, 3);
        let _this_unit_guard = protect(this_unit);
        set_vector_elt(this_unit, 0, scalar_real(real_elt(amount, i)));
        set_vector_elt(this_unit, 1, vector_elt(data, i));
        set_vector_elt(this_unit, 2, scalar_integer(integer_elt(unit_type, i)));
        set_vector_elt(answer, i, this_unit);
    }
    let cl = alloc_vector(SEXPTYPE::STRSXP, 2);
    let _cl_guard = protect(cl);
    set_string_elt(cl, 0, mk_char("unit"));
    set_string_elt(cl, 1, mk_char("unit_v2"));
    class_gets(answer, cl);
    answer
}

/// Convert a simpleUnit to a unit_v2 object.
pub fn asUnit(simple_unit: SEXP) -> SEXP {
    upgradeUnit(simple_unit)
}

/// Check that all units in a list have the same length. Returns that length or 0.
pub fn conformingUnits(unit_list: SEXP) -> SEXP {
    let n = LENGTH(unit_list);
    if n == 0 {
        return scalar_integer(0);
    }
    let first_len = unitLength(vector_elt(unit_list, 0));
    for i in 1..n as R_xlen_t {
        if unitLength(vector_elt(unit_list, i)) != first_len {
            return scalar_integer(0);
        }
    }
    scalar_integer(first_len)
}

/// Match unit type descriptions to their integer codes.
pub fn matchUnit(units: SEXP, unit: SEXP) -> SEXP {
    let source = if !units.is_null() && units != R_NilValue() {
        units
    } else {
        unit
    };
    if source.is_null() || source == R_NilValue() || TYPEOF(source) != SEXPTYPE::STRSXP {
        grid_error("'units' must be a character vector");
    }

    let n = LENGTH(source);
    let answer = alloc_vector(SEXPTYPE::INTSXP, n);
    let _answer_guard = protect(answer);
    for i in 0..n {
        let Some(name) = string_value_elt(source, i as R_xlen_t) else {
            grid_error("invalid unit name");
        };
        let Some(code) = unit_type_code(&name) else {
            grid_error(format!("invalid unit '{}'", name));
        };
        set_integer_elt(answer, i as R_xlen_t, code);
    }
    answer
}

/// Add two unit objects element-wise, producing a SUM unit.
pub fn addUnits(u1: SEXP, u2: SEXP) -> SEXP {
    let n1 = unitLength(u1);
    let n2 = unitLength(u2);
    let nmax = n1.max(n2);
    if nmax == 0 {
        return R_NilValue();
    }
    let answer = alloc_vector(SEXPTYPE::VECSXP, nmax);
    let _answer_guard = protect(answer);
    for i in 0..nmax as R_xlen_t {
        let this_unit = alloc_vector(SEXPTYPE::VECSXP, 3);
        let _this_unit_guard = protect(this_unit);
        set_vector_elt(this_unit, 0, scalar_real(1.0));
        let data = alloc_vector(SEXPTYPE::VECSXP, 2);
        let _data_guard = protect(data);
        set_vector_elt(data, 0, unitScalar(u1, (i as c_int) % n1));
        set_vector_elt(data, 1, unitScalar(u2, (i as c_int) % n2));
        set_vector_elt(this_unit, 1, data);
        set_vector_elt(this_unit, 2, scalar_integer(L_SUM));
        set_vector_elt(answer, i, this_unit);
    }
    let cl = alloc_vector(SEXPTYPE::STRSXP, 2);
    let _cl_guard = protect(cl);
    set_string_elt(cl, 0, mk_char("unit"));
    set_string_elt(cl, 1, mk_char("unit_v2"));
    class_gets(answer, cl);
    answer
}

/// Multiply unit values by a numeric vector (recycled).
pub fn multUnits(units: SEXP, values: SEXP) -> SEXP {
    let n = unitLength(units);
    let nv = LENGTH(values);
    if n == 0 {
        return R_NilValue();
    }
    let answer = duplicate(units);
    let _answer_guard = protect(answer);
    if isSimpleUnit(answer) {
        for i in 0..n as R_xlen_t {
            let current = real_elt(answer, i);
            set_real_elt(answer, i, current * real_elt(values, i % nv as R_xlen_t));
        }
    } else {
        for i in 0..n {
            let u = unitScalar(answer, i);
            if !u.is_null() {
                let val =
                    unitValue(answer, i) * real_elt(values, (i as usize % nv as usize) as R_xlen_t);
                set_vector_elt(u, 0, scalar_real(val));
            }
        }
    }
    answer
}

/// Negate unit values (flip sign).
pub fn flipUnits(units: SEXP) -> SEXP {
    let n = unitLength(units);
    if n == 0 {
        return R_NilValue();
    }
    let answer = duplicate(units);
    let _answer_guard = protect(answer);
    if isSimpleUnit(answer) {
        for i in 0..n as R_xlen_t {
            set_real_elt(answer, i, -real_elt(answer, i));
        }
    } else {
        for i in 0..n {
            let u = unitScalar(answer, i);
            if !u.is_null() {
                let val = -unitValue(answer, i);
                set_vector_elt(u, 0, scalar_real(val));
            }
        }
    }
    answer
}

fn absolute_unit_scalar(unit_scalar: SEXP) -> SEXP {
    let unit_id = uUnit(unit_scalar);
    let value = uValue(unit_scalar);
    if unit_id == L_INCHES {
        return unit_scalar;
    }
    if isAbsolute(unit_id) {
        let Some(inches) = absoluteUnitToInches(value, unit_id) else {
            grid_error(format!(
                "cannot convert unit '{}' to absolute inches without device metrics",
                unit_type_name(unit_id).unwrap_or("<unknown>")
            ));
        };
        let out = alloc_vector(SEXPTYPE::VECSXP, 3);
        let _out_guard = protect(out);
        set_vector_elt(out, 0, scalar_real(inches));
        set_vector_elt(out, 1, R_NilValue());
        set_vector_elt(out, 2, scalar_integer(L_INCHES));
        return out;
    }
    if isArith(unit_id) {
        let data = uData(unit_scalar);
        let n = unitLength(data);
        let converted = alloc_vector(SEXPTYPE::VECSXP, n);
        let _converted_guard = protect(converted);
        for i in 0..n {
            set_vector_elt(
                converted,
                i as R_xlen_t,
                absolute_unit_scalar(unitScalar(data, i)),
            );
        }
        let out = alloc_vector(SEXPTYPE::VECSXP, 3);
        let _out_guard = protect(out);
        set_vector_elt(out, 0, scalar_real(value));
        set_vector_elt(out, 1, converted);
        set_vector_elt(out, 2, scalar_integer(unit_id));
        return out;
    }
    grid_error(format!(
        "cannot convert relative unit '{}' to absolute units without a viewport",
        unit_type_name(unit_id).unwrap_or("<unknown>")
    ));
}

/// Convert context-free absolute units to inches.
pub fn absoluteUnits(units: SEXP) -> SEXP {
    let n = unitLength(units);
    let answer = alloc_vector(SEXPTYPE::VECSXP, n);
    let _answer_guard = protect(answer);
    for i in 0..n {
        set_vector_elt(
            answer,
            i as R_xlen_t,
            absolute_unit_scalar(unitScalar(units, i)),
        );
    }
    let cl = alloc_vector(SEXPTYPE::STRSXP, 2);
    let _cl_guard = protect(cl);
    set_string_elt(cl, 0, mk_char("unit"));
    set_string_elt(cl, 1, mk_char("unit_v2"));
    class_gets(answer, cl);
    answer
}

/// Summarize units with a reduction operation (sum/min/max).
/// `op_type` should be L_SUM, L_MIN, or L_MAX.
pub fn summaryUnits(units: SEXP, op_type: SEXP) -> SEXP {
    let op = integer_elt(op_type, 0);
    let this_unit = alloc_vector(SEXPTYPE::VECSXP, 3);
    let _this_unit_guard = protect(this_unit);
    set_vector_elt(this_unit, 0, scalar_real(1.0));
    set_vector_elt(this_unit, 1, units);
    set_vector_elt(this_unit, 2, scalar_integer(op));
    let answer = alloc_vector(SEXPTYPE::VECSXP, 1);
    let _answer_guard = protect(answer);
    set_vector_elt(answer, 0, this_unit);
    let cl = alloc_vector(SEXPTYPE::STRSXP, 2);
    let _cl_guard = protect(cl);
    set_string_elt(cl, 0, mk_char("unit"));
    set_string_elt(cl, 1, mk_char("unit_v2"));
    class_gets(answer, cl);
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(lhs: c_double, rhs: c_double) {
        assert!((lhs - rhs).abs() < 1e-12, "left={lhs:?}, right={rhs:?}");
    }

    fn assert_r_error(action: impl FnOnce()) -> crate::sexp::context::RError {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action))
            .expect_err("expected RError panic");
        payload
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("expected RError payload")
            .clone()
    }

    #[test]
    fn arithmetic_values_sum_min_and_max_with_multiplier() {
        approx_eq(
            combineArithmeticUnitValues(L_SUM, 2.0, &[1.0, 2.0, 3.0]),
            12.0,
        );
        approx_eq(
            combineArithmeticUnitValues(L_MIN, 0.5, &[3.0, 1.0, 4.0]),
            0.5,
        );
        approx_eq(
            combineArithmeticUnitValues(L_MAX, 1.5, &[3.0, 1.0, 4.0]),
            6.0,
        );
    }

    #[test]
    fn evaluate_null_unit_minimising_uses_available_dimension() {
            approx_eq(evaluateNullUnit(3.0, 9.0, 0, L_minimising), 9.0);
            approx_eq(evaluateNullUnit(3.0, 9.0, 0, L_plain), 0.0);
        }

    #[test]
    fn inverse_inches_to_npc_uses_axis_and_square_dimensions() {
            approx_eq(
                transformXYFromINCHES(
                    1.0,
                    L_NPC,
                    10.0,
                    20.0,
                None,
                    12.7,
                    7.62,
                    std::ptr::null_mut(),
                ),
                0.2,
            );
            approx_eq(
                transformXYFromINCHES(
                    1.0,
                    L_SNPC,
                    10.0,
                    20.0,
                None,
                    12.7,
                    7.62,
                    std::ptr::null_mut(),
                ),
                1.0 / 3.0,
            );
            approx_eq(
                transformWidthHeightFromINCHES(
                    1.0,
                    L_NATIVE,
                    10.0,
                    20.0,
                None,
                    12.7,
                    7.62,
                    std::ptr::null_mut(),
                ),
                2.0,
            );
            approx_eq(
                transformXYFromINCHES(
                    1.0,
                    L_PICAS,
                    10.0,
                    20.0,
                None,
                    12.7,
                    7.62,
                    std::ptr::null_mut(),
                ),
                POINTS_PER_INCH / 12.0,
            );
        }

    #[test]
    fn absolute_units_round_trip_through_inches() {
        let cases = [
            (L_CM, 2.54),
            (L_INCHES, 1.0),
            (L_MM, 25.4),
            (L_POINTS, POINTS_PER_INCH),
            (L_PICAS, POINTS_PER_INCH / 12.0),
            (L_BIGPOINTS, BIGPOINTS_PER_INCH),
            (L_DIDA, POINTS_PER_INCH * DIDA_PER_POINT_RATIO),
            (L_CICERO, POINTS_PER_INCH * CICERO_PER_PICA_RATIO / 12.0),
            (L_SCALEDPOINTS, POINTS_PER_INCH * SCALEDPOINTS_PER_POINT),
        ];

        for (unit_id, absolute_value) in cases {
            let inches = absoluteUnitToInches(absolute_value, unit_id).unwrap();
            approx_eq(inches, 1.0);
            approx_eq(
                inchesToAbsoluteUnit(inches, unit_id).unwrap(),
                absolute_value,
            );
        }
    }

    #[test]
    fn match_unit_maps_names_without_r_fallbacks() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let names = Rf_allocVector(SEXPTYPE::STRSXP, 4);
            let _names_guard = protect(names);
            SET_STRING_ELT(names, 0, crate::sexp::constructors::mk_char_str("npc"));
            SET_STRING_ELT(names, 1, crate::sexp::constructors::mk_char_str("inches"));
            SET_STRING_ELT(names, 2, crate::sexp::constructors::mk_char_str("strwidth"));
            SET_STRING_ELT(
                names,
                3,
                crate::sexp::constructors::mk_char_str("grobheight"),
            );

            let result = matchUnit(names, R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(result).add(0), L_NPC);
            assert_eq!(*INTEGER(result).add(1), L_INCHES);
            assert_eq!(*INTEGER(result).add(2), L_STRINGWIDTH);
            assert_eq!(*INTEGER(result).add(3), L_GROBHEIGHT);
        }
    }

    #[test]
    fn match_unit_rejects_unknown_names() {
        let _session = crate::sexp::session::RSession::new();
        let err = assert_r_error(|| unsafe {
            let names = Rf_allocVector(SEXPTYPE::STRSXP, 1);
            let _names_guard = protect(names);
            SET_STRING_ELT(names, 0, crate::sexp::constructors::mk_char_str("pixels"));
            matchUnit(names, R_NilValue());
        });
        assert!(err.message.contains("invalid unit"));
    }

    #[test]
    fn absolute_units_convert_context_free_units_to_inches() {
        let _session = crate::sexp::session::RSession::new();

            let units = unit(2.54, L_CM);
            let result = absoluteUnits(units);
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            assert_eq!(unitUnit(result, 0), L_INCHES);
            approx_eq(unitValue(result, 0), 1.0);
        }

    #[test]
    fn absolute_units_reject_context_dependent_units() {
        let _session = crate::sexp::session::RSession::new();
        let err = assert_r_error(|| {
            absoluteUnits(unit(1.0, L_NPC));
        });
        assert!(err.message.contains("without a viewport"));
    }

    #[test]
    fn transform_to_inches_supports_extended_absolute_units() {
        let _session = crate::sexp::session::RSession::new();

            let units = [
                (unit(2.54, L_CM), 1.0),
                (unit(POINTS_PER_INCH / 12.0, L_PICAS), 1.0),
                (unit(POINTS_PER_INCH * DIDA_PER_POINT_RATIO, L_DIDA), 1.0),
                (
                    unit(POINTS_PER_INCH * CICERO_PER_PICA_RATIO / 12.0, L_CICERO),
                    1.0,
                ),
                (
                    unit(POINTS_PER_INCH * SCALEDPOINTS_PER_POINT, L_SCALEDPOINTS),
                    1.0,
                ),
            ];

            for (value, expected_inches) in units {
                approx_eq(
                    transformXtoINCHES(
                        value,
                        0,
                        LViewportContext::default(),
                    None,
                        0.0,
                        0.0,
                        std::ptr::null_mut(),
                    ),
                    expected_inches,
                );
            }
        }

    unsafe fn string_unit(value: c_double, text: &[u8], unit_id: c_int) -> SEXP {
        unsafe {
            let amount = Rf_allocVector(SEXPTYPE::REALSXP, 1);
            let _amount_guard = protect(amount);
            *REAL(amount).add(0) = value;
            let data = Rf_allocVector(SEXPTYPE::VECSXP, 1);
            let _data_guard = protect(data);
            let chars = Rf_allocVector(SEXPTYPE::STRSXP, 1);
            let _chars_guard = protect(chars);
            SET_STRING_ELT(
                chars,
                0,
                Rf_mkCharLen(text.as_ptr() as *const c_char, text.len() as c_int),
            );
            SET_VECTOR_ELT(data, 0, chars);
            let unit_type = Rf_allocVector(SEXPTYPE::INTSXP, 1);
            let _unit_type_guard = protect(unit_type);
            *INTEGER(unit_type).add(0) = unit_id;
            let result = constructUnits(amount, data, unit_type);
            result
        }
    }

    #[test]
    fn text_units_use_context_fallbacks_without_a_device() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let gc = Box::new(crate::mainutils::graphics_ffi::R_GE_gcontext {
                col: 0,
                fill: 0,
                gamma: 1.0,
                lwd: 1.0,
                lty: 0,
                lend: 0,
                ljoin: 0,
                lmitre: 1.0,
                cex: 2.0,
                ps: 10.0,
                lineheight: 1.5,
                fontface: 1,
                fontfamily: [0; 201],
                patternFill: R_NilValue(),
            });
            let gc_ptr = Some(&*gc);
            let line_unit = unit(2.0, L_LINES);
            let char_unit = unit(3.0, L_CHAR);
            let string_width = string_unit(1.0, b"abcd", L_STRINGWIDTH);
            let string_height = string_unit(1.0, b"one\ntwo", L_STRINGHEIGHT);

            approx_eq(
                transformXtoINCHES(
                    line_unit,
                    0,
                    LViewportContext::default(),
                    gc_ptr,
                    0.0,
                    0.0,
                    std::ptr::null_mut(),
                ),
                2.0 * (10.0 * 2.0 / POINTS_PER_INCH) * 1.5,
            );
            approx_eq(
                transformWidthtoINCHES(
                    char_unit,
                    0,
                    LViewportContext::default(),
                    gc_ptr,
                    0.0,
                    0.0,
                    std::ptr::null_mut(),
                ),
                3.0 * (10.0 * 2.0 / POINTS_PER_INCH) * 0.6,
            );
            approx_eq(
                transformWidthtoINCHES(
                    string_width,
                    0,
                    LViewportContext::default(),
                    gc_ptr,
                    0.0,
                    0.0,
                    std::ptr::null_mut(),
                ),
                4.0 * (10.0 * 2.0 / POINTS_PER_INCH) * 0.6,
            );
            approx_eq(
                transformHeighttoINCHES(
                    string_height,
                    0,
                    LViewportContext::default(),
                    gc_ptr,
                    0.0,
                    0.0,
                    std::ptr::null_mut(),
                ),
                2.0 * (10.0 * 2.0 / POINTS_PER_INCH) * 1.5,
            );
        }
    }

    #[test]
    fn npc_helpers_round_trip_native_coordinates() {
            approx_eq(transformXYtoNPC(15.0, L_NATIVE, 10.0, 20.0), 0.5);
            approx_eq(transformXYfromNPC(0.5, L_NATIVE, 10.0, 20.0), 15.0);
            approx_eq(transformWHtoNPC(5.0, L_NATIVE, 0.0, 10.0), 0.5);
            approx_eq(transformWHfromNPC(0.5, L_NATIVE, 0.0, 10.0), 5.0);
            approx_eq(transformXYtoNPC(0.25, L_SNPC, 0.0, 10.0), 0.25);
            approx_eq(transformXYfromNPC(0.25, L_SNPC, 0.0, 10.0), 0.25);
        }

    #[test]
    fn transform_locn_applies_viewport_transform_matrix() {
        let _session = crate::sexp::session::RSession::new();

            let x = unit(1.0, L_INCHES);
            let y = unit(2.0, L_INCHES);
            let mut t = [[0.0; 3]; 3];
            t[0][0] = 1.0;
            t[1][1] = 1.0;
            t[2][2] = 1.0;
            t[2][0] = 3.0;
            t[2][1] = -1.5;
            let (xx, yy) = transformLocn(
                x,
                y,
                0,
                LViewportContext::default(),
            None,
                0.0,
                0.0,
                std::ptr::null_mut(),
                &t,
            );
            approx_eq(xx, 4.0);
            approx_eq(yy, 0.5);
        }

    #[test]
    fn transform_dimn_applies_rotation_matrix() {
        let _session = crate::sexp::session::RSession::new();

            let w = unit(1.0, L_INCHES);
            let h = unit(0.0, L_INCHES);
            let (ww, hh) = transformDimn(
                w,
                h,
                0,
                LViewportContext::default(),
            None,
                0.0,
                0.0,
                std::ptr::null_mut(),
                90.0,
            );
            approx_eq(ww, 0.0);
            approx_eq(hh, 1.0);
        }
}
