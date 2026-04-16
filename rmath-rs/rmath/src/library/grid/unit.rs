/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Port of R's src/library/grid/src/unit.c (2043 lines)
 *
 *  unit -- unit objects and coordinate transformation for grid.
 */

use std::ffi::c_void;
use std::os::raw::{c_char, c_double, c_int};
use std::cell::Cell;

use crate::attrib_core::{R_NamesSymbol, R_classgets, getAttrib, setAttrib};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};
use crate::sexp::symbol::Rf_install;

use super::gpar::pGEDevDesc;
use super::types::pGEcontext;

unsafe extern "C" {
    fn Rf_inherits(x: SEXP, klass: *const c_char) -> c_int;
}

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

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct LViewportContext {
    pub xscalemin: c_double,
    pub xscalemax: c_double,
    pub yscalemin: c_double,
    pub yscalemax: c_double,
}

/* ==============================
 * Helper macros as functions
 * ============================== */

/// uValue(X) - get value from a unit scalar (single unit)
unsafe fn uValue(x: SEXP) -> c_double {
    *REAL(VECTOR_ELT(x, 0)).add(0)
}

/// uData(X) - get data from a unit scalar
unsafe fn uData(x: SEXP) -> SEXP {
    VECTOR_ELT(x, 1)
}

/// uUnit(X) - get unit type from a unit scalar
unsafe fn uUnit(x: SEXP) -> c_int {
    *INTEGER(VECTOR_ELT(x, 2)).add(0)
}

/// isAbsolute(X) - check if unit is an absolute unit type
unsafe fn isAbsolute(x: c_int) -> bool {
    x > 1000
        || (x >= L_MYLINES && x <= L_MYSTRINGHEIGHT)
        || (x < L_GROBX && x > L_NPC && x != L_NATIVE && x != L_SNPC)
}

/// isArith(X) - check if unit is an arithmetic unit type
unsafe fn isArith(x: c_int) -> bool {
    x >= L_SUM && x <= L_MAX
}

/// isStringUnit(X) - check if unit is a string-related unit type
unsafe fn isStringUnit(x: c_int) -> bool {
    x >= L_STRINGWIDTH && x <= L_STRINGDESCENT
}

/// isGrobUnit(X) - check if unit is a grob-related unit type
unsafe fn isGrobUnit(x: c_int) -> bool {
    x >= L_GROBX && x <= L_GROBDESCENT
}

type TransformToInchesFn =
    unsafe fn(SEXP, c_int, LViewportContext, pGEcontext, c_double, c_double, pGEDevDesc) -> c_double;

#[inline]
fn transformDimensionToNPC(value: c_double, cm: c_double) -> c_double {
    if cm == 0.0 {
        0.0
    } else {
        value * 2.54 / cm
    }
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

unsafe fn transformArithmeticUnitToINCHES(
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
 * Global null layout mode
 * ============================== */

thread_local! { static L_nullLayoutMode: Cell<c_int> = Cell::new(0); }

thread_local! { pub static L_nullLayoutMode_ptr: Cell<c_int> = Cell::new(0); }

/* ==============================
 * unit() -- construct a unit object
 * ============================== */

pub unsafe fn unit(value: c_double, unit_id: c_int) -> SEXP {
    let units = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 1));
    SET_VECTOR_ELT(units, 0 as R_xlen_t, Rf_allocVector(SEXPTYPE::VECSXP, 3));
    let u = VECTOR_ELT(units, 0);
    SET_VECTOR_ELT(u, 0 as R_xlen_t, Rf_ScalarReal(value));
    SET_VECTOR_ELT(u, 1 as R_xlen_t, R_NilValue());
    SET_VECTOR_ELT(u, 2 as R_xlen_t, Rf_ScalarInteger(unit_id));
    let cl = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, 2));
    SET_STRING_ELT(cl, 0 as R_xlen_t, Rf_mkChar(c"unit".as_ptr()));
    SET_STRING_ELT(cl, 1 as R_xlen_t, Rf_mkChar(c"unit_v2".as_ptr()));
    R_classgets(units, cl);
    Rf_unprotect(2);
    units
}

/* ==============================
 * isSimpleUnit / isNewUnit / upgradeUnit
 * ============================== */

unsafe fn isSimpleUnit(unit: SEXP) -> bool {
    Rf_inherits(unit, c"simpleUnit".as_ptr()) != 0
}

unsafe fn isNewUnit(unit: SEXP) -> bool {
    Rf_inherits(unit, c"unit_v2".as_ptr()) != 0
}

unsafe fn upgradeUnit(unit: SEXP) -> SEXP {
    // TODO: call R's upgradeUnit function once the eval bridge is available.
    unit
}

/* ==============================
 * unitScalar -- extract underlying scalar unit list structure
 * ============================== */

pub unsafe fn unitScalar(unit: SEXP, index: c_int) -> SEXP {
    let l = LENGTH(unit);
    if l == 0 {
        // C raises an error here; this port returns NilValue until the error bridge lands.
        return R_NilValue();
    }
    let i = index % l;
    if isSimpleUnit(unit) {
        let new_unit = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 3));
        SET_VECTOR_ELT(
            new_unit,
            0 as R_xlen_t,
            Rf_ScalarReal(*REAL(unit).add(i as usize)),
        );
        SET_VECTOR_ELT(new_unit, 1 as R_xlen_t, R_NilValue());
        let unit_attr = getAttrib(unit, Rf_install(c"unit".as_ptr()));
        let unit_val = if TYPEOF(unit_attr) == SEXPTYPE::INTSXP && LENGTH(unit_attr) > 0 {
            *INTEGER(unit_attr).add(0)
        } else {
            0
        };
        SET_VECTOR_ELT(new_unit, 2 as R_xlen_t, Rf_ScalarInteger(unit_val));
        Rf_unprotect(1);
        return new_unit;
    }
    if isNewUnit(unit) {
        return VECTOR_ELT(unit, i as R_xlen_t);
    }
    // Fallback: try to upgrade
    let unit2 = Rf_protect(upgradeUnit(unit));
    let res = unitScalar(unit2, index);
    Rf_unprotect(1);
    res
}

/* ==============================
 * unitValue -- get value of unit at index
 * ============================== */

pub unsafe fn unitValue(unit: SEXP, index: c_int) -> c_double {
    if isSimpleUnit(unit) {
        return *REAL(unit).add((index % LENGTH(unit)) as usize);
    }
    uValue(unitScalar(unit, index))
}

/* ==============================
 * unitUnit -- get unit type at index
 * ============================== */

pub unsafe fn unitUnit(unit: SEXP, index: c_int) -> c_int {
    if isSimpleUnit(unit) {
        let unit_attr = getAttrib(unit, Rf_install(c"unit".as_ptr()));
        if TYPEOF(unit_attr) == SEXPTYPE::INTSXP && LENGTH(unit_attr) > 0 {
            return *INTEGER(unit_attr).add(0);
        }
        return 0;
    }
    uUnit(unitScalar(unit, index))
}

/* ==============================
 * unitData -- get data component of unit at index
 * ============================== */

pub unsafe fn unitData(unit: SEXP, index: c_int) -> SEXP {
    if isSimpleUnit(unit) {
        return R_NilValue();
    }
    uData(unitScalar(unit, index))
}

/* ==============================
 * unitLength -- get length of a unit object
 * ============================== */

pub unsafe fn unitLength(u: SEXP) -> c_int {
    if isNewUnit(u) {
        return LENGTH(u);
    }
    LENGTH(upgradeUnit(u))
}

/* ==============================
 * pureNullUnitValue -- evaluate null unit value
 * ============================== */

pub unsafe fn pureNullUnitValue(unit: SEXP, index: c_int) -> c_double {
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

/* ==============================
 * pureNullUnit -- check if a unit is "pure null"
 * ============================== */

pub unsafe fn pureNullUnit(unit: SEXP, index: c_int, _dd: pGEDevDesc) -> c_int {
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

/* ==============================
 * evaluateNullUnit -- evaluate null unit value based on context
 * ============================== */

unsafe fn evaluateNullUnit(
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

pub unsafe fn transformX(
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
        return evaluateNullUnit(
            pureNullUnitValue(x, index),
            widthCM,
            nullLMode,
            nullAMode,
        );
    }
    transformXtoINCHES(x, index, vpc, gc, widthCM, heightCM, dd)
}

/* ==============================
 * transformY -- transform y unit to inches
 * ============================== */

pub unsafe fn transformY(
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
        return evaluateNullUnit(
            pureNullUnitValue(y, index),
            heightCM,
            nullLMode,
            nullAMode,
        );
    }
    transformYtoINCHES(y, index, vpc, gc, widthCM, heightCM, dd)
}

/* ==============================
 * transformWidth -- transform width unit to inches
 * ============================== */

pub unsafe fn transformWidth(
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

pub unsafe fn transformHeight(
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

pub unsafe fn transformXtoINCHES(
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
            x, index, vpc, _gc, widthCM, heightCM, _dd, transformXtoINCHES,
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
        L_CM => value / 2.54,
        L_INCHES => value,
        L_MM => value / 25.4,
        L_POINTS => value / 72.27,
        L_PICAS => value / (12.0 * 72.27), // picas to inches
        L_BIGPOINTS => value / 72.0,
        L_LINES | L_CHAR | L_STRINGWIDTH | L_STRINGHEIGHT | L_STRINGASCENT | L_STRINGDESCENT
        | L_GROBX | L_GROBY | L_GROBWIDTH | L_GROBHEIGHT | L_GROBASCENT | L_GROBDESCENT => {
            // TODO: requires device context or grob evaluation.
            0.0
        }
        L_NULL => 0.0,
        _ => 0.0,
    }
}

/* ==============================
 * transformYtoINCHES -- transform y unit to inches
 * ============================== */

pub unsafe fn transformYtoINCHES(
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
            y, index, vpc, _gc, widthCM, heightCM, _dd, transformYtoINCHES,
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
        L_CM => value / 2.54,
        L_INCHES => value,
        L_MM => value / 25.4,
        L_POINTS => value / 72.27,
        L_PICAS => value / (12.0 * 72.27),
        L_BIGPOINTS => value / 72.0,
        L_NULL => 0.0,
        _ => 0.0,
    }
}

/* ==============================
 * transformWidthtoINCHES -- transform width unit to inches
 * ============================== */

pub unsafe fn transformWidthtoINCHES(
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
            w, index, vpc, _gc, widthCM, heightCM, _dd, transformWidthtoINCHES,
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
        L_CM => value / 2.54,
        L_INCHES => value,
        L_MM => value / 25.4,
        L_POINTS => value / 72.27,
        L_PICAS => value / (12.0 * 72.27),
        L_BIGPOINTS => value / 72.0,
        L_NULL => 0.0,
        _ => 0.0,
    }
}

/* ==============================
 * transformHeighttoINCHES -- transform height unit to inches
 * ============================== */

pub unsafe fn transformHeighttoINCHES(
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
            h, index, vpc, _gc, widthCM, heightCM, _dd, transformHeighttoINCHES,
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
        L_CM => value / 2.54,
        L_INCHES => value,
        L_MM => value / 25.4,
        L_POINTS => value / 72.27,
        L_PICAS => value / (12.0 * 72.27),
        L_BIGPOINTS => value / 72.0,
        L_NULL => 0.0,
        _ => 0.0,
    }
}

/* ==============================
 * transformLocn -- transform x,y location
 * ============================== */

pub unsafe fn transformLocn(
    x: SEXP,
    y: SEXP,
    index: c_int,
    vpc: LViewportContext,
    gc: pGEcontext,
    widthCM: c_double,
    heightCM: c_double,
    dd: pGEDevDesc,
    _t: *mut [[f64; 3]; 3],
    xx: *mut c_double,
    yy: *mut c_double,
) {
    *xx = transformXtoINCHES(x, index, vpc, gc, widthCM, heightCM, dd);
    *yy = transformYtoINCHES(y, index, vpc, gc, widthCM, heightCM, dd);
}

/* ==============================
 * transformDimn -- transform width,height dimensions
 * ============================== */

pub unsafe fn transformDimn(
    w: SEXP,
    h: SEXP,
    index: c_int,
    vpc: LViewportContext,
    gc: pGEcontext,
    widthCM: c_double,
    heightCM: c_double,
    dd: pGEDevDesc,
    _rotationAngle: c_double,
    ww: *mut c_double,
    hh: *mut c_double,
) {
    *ww = transformWidthtoINCHES(w, index, vpc, gc, widthCM, heightCM, dd);
    *hh = transformHeighttoINCHES(h, index, vpc, gc, widthCM, heightCM, dd);
}

/* ==============================
 * transformXYFromINCHES -- convert inches to specified unit
 * ============================== */

pub unsafe fn transformXYFromINCHES(
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
        L_CM => location * 2.54,
        L_INCHES => location,
        L_MM => location * 25.4,
        _ => location, // TODO: other unit types need device-dependent context.
    }
}

/* ==============================
 * transformWidthHeightFromINCHES -- convert inches to width/height unit
 * ============================== */

pub unsafe fn transformWidthHeightFromINCHES(
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
        L_CM => value * 2.54,
        L_INCHES => value,
        L_MM => value * 25.4,
        _ => value, // TODO: other unit types need device-dependent context.
    }
}

/* ==============================
 * NPC conversion helpers
 * ============================== */

pub unsafe fn transformXYtoNPC(x: c_double, from: c_int, min: c_double, max: c_double) -> c_double {
    match from {
        L_NATIVE => {
            let range = max - min;
            if range == 0.0 { 0.5 } else { (x - min) / range }
        }
        L_NPC | L_SNPC => x,
        _ => x, // TODO: unsupported units need device-dependent context.
    }
}

pub unsafe fn transformWHtoNPC(x: c_double, from: c_int, min: c_double, max: c_double) -> c_double {
    match from {
        L_NATIVE => {
            let range = max - min;
            if range == 0.0 { 0.0 } else { x / range }
        }
        L_NPC | L_SNPC => x,
        _ => x, // TODO: unsupported units need device-dependent context.
    }
}

pub unsafe fn transformXYfromNPC(x: c_double, to: c_int, min: c_double, max: c_double) -> c_double {
    match to {
        L_NATIVE => {
            let range = max - min;
            min + x * range
        }
        L_NPC | L_SNPC => x,
        _ => x, // TODO: unsupported units need device-dependent context.
    }
}

pub unsafe fn transformWHfromNPC(x: c_double, to: c_int, min: c_double, max: c_double) -> c_double {
    match to {
        L_NATIVE => {
            let range = max - min;
            x * range
        }
        L_NPC | L_SNPC => x,
        _ => x, // TODO: unsupported units need device-dependent context.
    }
}

/* ==============================
 * R-callable unit construction/validation functions
 * ============================== */

/// Validate that `units` inherits from "unit" class. Returns the input if valid.
pub unsafe fn validUnits(units: SEXP) -> SEXP {
    if Rf_inherits(units, b"unit\0".as_ptr() as *const c_char) != 0 {
        units
    } else {
        R_NilValue()
    }
}

/// Construct a unit_v2 object from parallel `amount`, `data`, and `unit_type` vectors.
pub unsafe fn constructUnits(amount: SEXP, data: SEXP, unit_type: SEXP) -> SEXP {
    let n = LENGTH(amount);
    let answer = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, n));
    for i in 0..n as R_xlen_t {
        let this_unit = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 3));
        SET_VECTOR_ELT(this_unit, 0, Rf_ScalarReal(*REAL(amount).add(i as usize)));
        SET_VECTOR_ELT(this_unit, 1, VECTOR_ELT(data, i));
        SET_VECTOR_ELT(
            this_unit,
            2,
            Rf_ScalarInteger(*INTEGER(unit_type).add(i as usize)),
        );
        SET_VECTOR_ELT(answer, i, this_unit);
        Rf_unprotect(1);
    }
    let cl = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, 2));
    SET_STRING_ELT(cl, 0, Rf_mkChar(b"unit\0".as_ptr() as *const c_char));
    SET_STRING_ELT(cl, 1, Rf_mkChar(b"unit_v2\0".as_ptr() as *const c_char));
    R_classgets(answer, cl);
    Rf_unprotect(2);
    answer
}

/// Convert a simpleUnit to a unit_v2 object.
pub unsafe fn asUnit(simple_unit: SEXP) -> SEXP {
    upgradeUnit(simple_unit)
}

/// Check that all units in a list have the same length. Returns that length or 0.
pub unsafe fn conformingUnits(unit_list: SEXP) -> SEXP {
    let n = LENGTH(unit_list);
    if n == 0 {
        return Rf_ScalarInteger(0);
    }
    let first_len = unitLength(VECTOR_ELT(unit_list, 0));
    for i in 1..n as R_xlen_t {
        if unitLength(VECTOR_ELT(unit_list, i)) != first_len {
            return Rf_ScalarInteger(0);
        }
    }
    Rf_ScalarInteger(first_len)
}

/// Match a unit type description to its integer code.
/// Requires R-level unit type lookup; returns R_NilValue until ported.
pub unsafe fn matchUnit(_units: SEXP, _unit: SEXP) -> SEXP {
    R_NilValue()
}

/// Add two unit objects element-wise, producing a SUM unit.
pub unsafe fn addUnits(u1: SEXP, u2: SEXP) -> SEXP {
    let n1 = unitLength(u1);
    let n2 = unitLength(u2);
    let nmax = n1.max(n2);
    if nmax == 0 {
        return R_NilValue();
    }
    let answer = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, nmax as usize));
    for i in 0..nmax as R_xlen_t {
        let this_unit = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 3));
        SET_VECTOR_ELT(this_unit, 0, Rf_ScalarReal(1.0));
        let data = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 2));
        SET_VECTOR_ELT(data, 0, unitScalar(u1, (i as c_int) % n1));
        SET_VECTOR_ELT(data, 1, unitScalar(u2, (i as c_int) % n2));
        SET_VECTOR_ELT(this_unit, 1, data);
        SET_VECTOR_ELT(this_unit, 2, Rf_ScalarInteger(L_SUM));
        SET_VECTOR_ELT(answer, i, this_unit);
        Rf_unprotect(2);
    }
    let cl = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, 2));
    SET_STRING_ELT(cl, 0, Rf_mkChar(b"unit\0".as_ptr() as *const c_char));
    SET_STRING_ELT(cl, 1, Rf_mkChar(b"unit_v2\0".as_ptr() as *const c_char));
    R_classgets(answer, cl);
    Rf_unprotect(2);
    answer
}

/// Multiply unit values by a numeric vector (recycled).
pub unsafe fn multUnits(units: SEXP, values: SEXP) -> SEXP {
    let n = unitLength(units);
    let nv = LENGTH(values);
    if n == 0 {
        return R_NilValue();
    }
    let answer = Rf_protect(crate::main::duplicate::Rf_duplicate(units));
    if isSimpleUnit(answer) {
        for i in 0..n as usize {
            *REAL(answer).add(i) *= *REAL(values).add(i % nv as usize);
        }
    } else {
        for i in 0..n {
            let u = unitScalar(answer, i);
            if !u.is_null() {
                let val = unitValue(answer, i) * *REAL(values).add((i as usize) % nv as usize);
                SET_VECTOR_ELT(u, 0, Rf_ScalarReal(val));
            }
        }
    }
    Rf_unprotect(1);
    answer
}

/// Negate unit values (flip sign).
pub unsafe fn flipUnits(units: SEXP) -> SEXP {
    let n = unitLength(units);
    if n == 0 {
        return R_NilValue();
    }
    let answer = Rf_protect(crate::main::duplicate::Rf_duplicate(units));
    if isSimpleUnit(answer) {
        for i in 0..n as usize {
            *REAL(answer).add(i) = -*REAL(answer).add(i);
        }
    } else {
        for i in 0..n {
            let u = unitScalar(answer, i);
            if !u.is_null() {
                let val = -unitValue(answer, i);
                SET_VECTOR_ELT(u, 0, Rf_ScalarReal(val));
            }
        }
    }
    Rf_unprotect(1);
    answer
}

/// Convert units to absolute units.
/// Requires device context to resolve relative units (NPC, native, etc.).
pub unsafe fn absoluteUnits(_units: SEXP) -> SEXP {
    R_NilValue()
}

/// Summarize units with a reduction operation (sum/min/max).
/// `op_type` should be L_SUM, L_MIN, or L_MAX.
pub unsafe fn summaryUnits(units: SEXP, op_type: SEXP) -> SEXP {
    let op = *INTEGER(op_type);
    let this_unit = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 3));
    SET_VECTOR_ELT(this_unit, 0, Rf_ScalarReal(1.0));
    SET_VECTOR_ELT(this_unit, 1, units);
    SET_VECTOR_ELT(this_unit, 2, Rf_ScalarInteger(op));
    let answer = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, 1));
    SET_VECTOR_ELT(answer, 0, this_unit);
    let cl = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP, 2));
    SET_STRING_ELT(cl, 0, Rf_mkChar(b"unit\0".as_ptr() as *const c_char));
    SET_STRING_ELT(cl, 1, Rf_mkChar(b"unit_v2\0".as_ptr() as *const c_char));
    R_classgets(answer, cl);
    Rf_unprotect(3);
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(lhs: c_double, rhs: c_double) {
        assert!(
            (lhs - rhs).abs() < 1e-12,
            "left={lhs:?}, right={rhs:?}"
        );
    }

    #[test]
    fn arithmetic_values_sum_min_and_max_with_multiplier() {
        approx_eq(combineArithmeticUnitValues(L_SUM, 2.0, &[1.0, 2.0, 3.0]), 12.0);
        approx_eq(combineArithmeticUnitValues(L_MIN, 0.5, &[3.0, 1.0, 4.0]), 0.5);
        approx_eq(combineArithmeticUnitValues(L_MAX, 1.5, &[3.0, 1.0, 4.0]), 6.0);
    }

    #[test]
    fn evaluate_null_unit_minimising_uses_available_dimension() {
        unsafe {
            approx_eq(evaluateNullUnit(3.0, 9.0, 0, L_minimising), 9.0);
            approx_eq(evaluateNullUnit(3.0, 9.0, 0, L_plain), 0.0);
        }
    }

    #[test]
    fn inverse_inches_to_npc_uses_axis_and_square_dimensions() {
        unsafe {
            approx_eq(
                transformXYFromINCHES(
                    1.0,
                    L_NPC,
                    10.0,
                    20.0,
                    std::ptr::null(),
                    12.7,
                    7.62,
                    std::ptr::null_mut(),
                ),
                0.4,
            );
            approx_eq(
                transformXYFromINCHES(
                    1.0,
                    L_SNPC,
                    10.0,
                    20.0,
                    std::ptr::null(),
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
                    std::ptr::null(),
                    12.7,
                    7.62,
                    std::ptr::null_mut(),
                ),
                2.0,
            );
        }
    }

    #[test]
    fn npc_helpers_round_trip_native_coordinates() {
        unsafe {
            approx_eq(transformXYtoNPC(15.0, L_NATIVE, 10.0, 20.0), 0.5);
            approx_eq(transformXYfromNPC(0.5, L_NATIVE, 10.0, 20.0), 15.0);
            approx_eq(transformWHtoNPC(5.0, L_NATIVE, 0.0, 10.0), 0.5);
            approx_eq(transformWHfromNPC(0.5, L_NATIVE, 0.0, 10.0), 5.0);
            approx_eq(transformXYtoNPC(0.25, L_SNPC, 0.0, 10.0), 0.25);
            approx_eq(transformXYfromNPC(0.25, L_SNPC, 0.0, 10.0), 0.25);
        }
    }
}
