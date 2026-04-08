/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Port of R's src/library/grid/src/unit.c (2043 lines)
 *
 *  unit -- unit objects and coordinate transformation for grid.
 */

use std::ffi::c_void;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

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

/* ==============================
 * Global null layout mode
 * ============================== */

thread_local! { static L_nullLayoutMode: Cell<c_int> = Cell::new(0); }

thread_local! { pub static L_nullLayoutMode_ptr: Cell<c_int> = Cell::new(0); }

/* ==============================
 * unit() -- construct a unit object
 * ============================== */

pub unsafe fn unit(value: c_double, unit_id: c_int) -> SEXP {
    let units = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, 1));
    SET_VECTOR_ELT(units, 0 as R_xlen_t, Rf_allocVector(SEXPTYPE::VECSXP.0, 3));
    let u = VECTOR_ELT(units, 0);
    SET_VECTOR_ELT(u, 0 as R_xlen_t, Rf_ScalarReal(value));
    SET_VECTOR_ELT(u, 1 as R_xlen_t, R_NilValue());
    SET_VECTOR_ELT(u, 2 as R_xlen_t, Rf_ScalarInteger(unit_id));
    let cl = Rf_protect(Rf_allocVector(SEXPTYPE::STRSXP.0, 2));
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
    // STUB: calls R's upgradeUnit function
    unit
}

/* ==============================
 * unitScalar -- extract underlying scalar unit list structure
 * ============================== */

pub unsafe fn unitScalar(unit: SEXP, index: c_int) -> SEXP {
    let l = LENGTH(unit);
    if l == 0 {
        // error in C, just return NilValue in stub
        return R_NilValue();
    }
    let i = index % l;
    if isSimpleUnit(unit) {
        let new_unit = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, 3));
        SET_VECTOR_ELT(
            new_unit,
            0 as R_xlen_t,
            Rf_ScalarReal(*REAL(unit).add(i as usize)),
        );
        SET_VECTOR_ELT(new_unit, 1 as R_xlen_t, R_NilValue());
        let unit_attr = getAttrib(unit, Rf_install(c"unit".as_ptr()));
        let unit_val = if TYPEOF(unit_attr) == SEXPTYPE::INTSXP.0 && LENGTH(unit_attr) > 0 {
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
        if TYPEOF(unit_attr) == SEXPTYPE::INTSXP.0 && LENGTH(unit_attr) > 0 {
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
    let mut result: c_double = 0.0;
    let u = unitUnit(unit, index);
    let value = unitValue(unit, index);
    let data: SEXP;
    match u {
        L_SUM => {
            data = unitData(unit, index);
            let n = unitLength(data);
            for i in 0..n {
                result += pureNullUnitValue(data, i);
            }
            result *= value;
        }
        L_MIN => {
            data = unitData(unit, index);
            let n = unitLength(data);
            result = c_double::MAX;
            for i in 0..n {
                let temp = pureNullUnitValue(data, i);
                if temp < result {
                    result = temp;
                }
            }
            result *= value;
        }
        L_MAX => {
            data = unitData(unit, index);
            let n = unitLength(data);
            result = c_double::MIN;
            for i in 0..n {
                let temp = pureNullUnitValue(data, i);
                if temp > result {
                    result = temp;
                }
            }
            result *= value;
        }
        _ => {
            result = value;
        }
    }
    result
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
    _thisCM: c_double,
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
                // result = thisCM; -- would need thisCM
                result = 0.0;
            }
            _ => {}
        }
    }
    result
}

/* ==============================
 * transformX -- transform x unit to inches (STUB)
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
    transformXtoINCHES(x, index, vpc, gc, widthCM, heightCM, dd)
}

/* ==============================
 * transformY -- transform y unit to inches (STUB)
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
    transformYtoINCHES(y, index, vpc, gc, widthCM, heightCM, dd)
}

/* ==============================
 * transformWidth -- transform width unit to inches (STUB)
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
    transformWidthtoINCHES(width, index, vpc, gc, widthCM, heightCM, dd)
}

/* ==============================
 * transformHeight -- transform height unit to inches (STUB)
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
    transformHeighttoINCHES(height, index, vpc, gc, widthCM, heightCM, dd)
}

/* ==============================
 * transformXtoINCHES -- transform x unit to inches (STUB)
 * ============================== */

pub unsafe fn transformXtoINCHES(
    x: SEXP,
    index: c_int,
    vpc: LViewportContext,
    _gc: pGEcontext,
    _widthCM: c_double,
    _heightCM: c_double,
    _dd: pGEDevDesc,
) -> c_double {
    let u = unitUnit(x, index);
    let value = unitValue(x, index);
    match u {
        L_NATIVE => {
            // native units: value is in data coordinate system
            // map from [xscalemin, xscalemax] to [0, 1] then to inches
            let range = vpc.xscalemax - vpc.xscalemin;
            if range == 0.0 {
                0.0
            } else {
                (value - vpc.xscalemin) / range
            }
        }
        L_NPC => value,       // 0..1 range
        L_CM => value / 2.54, // cm to inches
        L_INCHES => value,
        L_MM => value / 25.4,              // mm to inches
        L_POINTS => value / 72.27,         // points to inches
        L_PICAS => value / (12.0 * 72.27), // picas to inches
        L_BIGPOINTS => value / 72.0,       // bigpoints to inches
        L_LINES | L_CHAR | L_STRINGWIDTH | L_STRINGHEIGHT | L_STRINGASCENT | L_STRINGDESCENT
        | L_GROBX | L_GROBY | L_GROBWIDTH | L_GROBHEIGHT | L_GROBASCENT | L_GROBDESCENT => {
            // STUB: these require device context or grob evaluation
            0.0
        }
        L_NULL => 0.0,
        _ => {
            // STUB for remaining unit types
            0.0
        }
    }
}

/* ==============================
 * transformYtoINCHES -- transform y unit to inches (STUB)
 * ============================== */

pub unsafe fn transformYtoINCHES(
    y: SEXP,
    index: c_int,
    vpc: LViewportContext,
    _gc: pGEcontext,
    _widthCM: c_double,
    _heightCM: c_double,
    _dd: pGEDevDesc,
) -> c_double {
    let u = unitUnit(y, index);
    let value = unitValue(y, index);
    match u {
        L_NATIVE => {
            let range = vpc.yscalemax - vpc.yscalemin;
            if range == 0.0 {
                0.0
            } else {
                (value - vpc.yscalemin) / range
            }
        }
        L_NPC => value,
        L_CM => value / 2.54,
        L_INCHES => value,
        L_MM => value / 25.4,
        L_POINTS => value / 72.27,
        L_PICAS => value / (12.0 * 72.27),
        L_BIGPOINTS => value / 72.0,
        L_NULL => 0.0,
        _ => 0.0, // STUB
    }
}

/* ==============================
 * transformWidthtoINCHES -- transform width unit to inches (STUB)
 * ============================== */

pub unsafe fn transformWidthtoINCHES(
    w: SEXP,
    index: c_int,
    vpc: LViewportContext,
    _gc: pGEcontext,
    widthCM: c_double,
    _heightCM: c_double,
    _dd: pGEDevDesc,
) -> c_double {
    let u = unitUnit(w, index);
    let value = unitValue(w, index);
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
        L_CM => value / 2.54,
        L_INCHES => value,
        L_MM => value / 25.4,
        L_POINTS => value / 72.27,
        L_PICAS => value / (12.0 * 72.27),
        L_BIGPOINTS => value / 72.0,
        L_NULL => 0.0,
        _ => 0.0, // STUB
    }
}

/* ==============================
 * transformHeighttoINCHES -- transform height unit to inches (STUB)
 * ============================== */

pub unsafe fn transformHeighttoINCHES(
    h: SEXP,
    index: c_int,
    vpc: LViewportContext,
    _gc: pGEcontext,
    _widthCM: c_double,
    heightCM: c_double,
    _dd: pGEDevDesc,
) -> c_double {
    let u = unitUnit(h, index);
    let value = unitValue(h, index);
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
        L_CM => value / 2.54,
        L_INCHES => value,
        L_MM => value / 25.4,
        L_POINTS => value / 72.27,
        L_PICAS => value / (12.0 * 72.27),
        L_BIGPOINTS => value / 72.0,
        L_NULL => 0.0,
        _ => 0.0, // STUB
    }
}

/* ==============================
 * transformLocn -- transform x,y location (STUB)
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
 * transformDimn -- transform width,height dimensions (STUB)
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
 * transformXYFromINCHES -- convert inches to specified unit (STUB)
 * ============================== */

pub unsafe fn transformXYFromINCHES(
    location: c_double,
    unit_id: c_int,
    scalemin: c_double,
    scalemax: c_double,
    _gc: pGEcontext,
    _thisCM: c_double,
    _otherCM: c_double,
    _dd: pGEDevDesc,
) -> c_double {
    match unit_id {
        L_NPC => location, // STUB: should use thisCM
        L_NATIVE => {
            let range = scalemax - scalemin;
            if range == 0.0 {
                0.0
            } else {
                scalemin + location * range
            }
        }
        L_CM => location * 2.54,
        L_INCHES => location,
        L_MM => location * 25.4,
        _ => location, // STUB for others
    }
}

/* ==============================
 * transformWidthHeightFromINCHES -- convert inches to width/height unit (STUB)
 * ============================== */

pub unsafe fn transformWidthHeightFromINCHES(
    value: c_double,
    unit_id: c_int,
    scalemin: c_double,
    scalemax: c_double,
    _gc: pGEcontext,
    _thisCM: c_double,
    _otherCM: c_double,
    _dd: pGEDevDesc,
) -> c_double {
    match unit_id {
        L_NPC => value,
        L_NATIVE => {
            let range = scalemax - scalemin;
            if range == 0.0 { 0.0 } else { value / range }
        }
        L_CM => value * 2.54,
        L_INCHES => value,
        L_MM => value * 25.4,
        _ => value,
    }
}

/* ==============================
 * NPC conversion helpers
 * ============================== */

pub unsafe fn transformXYtoNPC(
    x: c_double,
    from: c_int,
    min: c_double,
    max: c_double,
) -> c_double {
    match from {
        L_NATIVE => {
            let range = max - min;
            if range == 0.0 { 0.5 } else { (x - min) / range }
        }
        L_NPC => x,
        _ => x, // STUB for other units
    }
}

pub unsafe fn transformWHtoNPC(
    x: c_double,
    from: c_int,
    min: c_double,
    max: c_double,
) -> c_double {
    match from {
        L_NATIVE => {
            let range = max - min;
            if range == 0.0 { 0.0 } else { x / range }
        }
        L_NPC => x,
        _ => x,
    }
}

pub unsafe fn transformXYfromNPC(
    x: c_double,
    to: c_int,
    min: c_double,
    max: c_double,
) -> c_double {
    match to {
        L_NATIVE => {
            let range = max - min;
            min + x * range
        }
        L_NPC => x,
        _ => x,
    }
}

pub unsafe fn transformWHfromNPC(
    x: c_double,
    to: c_int,
    min: c_double,
    max: c_double,
) -> c_double {
    match to {
        L_NATIVE => {
            let range = max - min;
            x * range
        }
        L_NPC => x,
        _ => x,
    }
}

/* ==============================
 * R-callable unit construction/validation functions (STUBs)
 * ============================== */

pub unsafe fn validUnits(_units: SEXP) -> SEXP {
    // STUB: should validate unit structure
    R_NilValue()
}

pub unsafe fn constructUnits(_amount: SEXP, _data: SEXP, _unit: SEXP) -> SEXP {
    // STUB: should construct unit objects
    R_NilValue()
}

pub unsafe fn asUnit(_simpleUnit: SEXP) -> SEXP {
    // STUB
    R_NilValue()
}

pub unsafe fn conformingUnits(_unitList: SEXP) -> SEXP {
    // STUB
    R_NilValue()
}

pub unsafe fn matchUnit(_units: SEXP, _unit: SEXP) -> SEXP {
    // STUB
    R_NilValue()
}

pub unsafe fn addUnits(_u1: SEXP, _u2: SEXP) -> SEXP {
    // STUB
    R_NilValue()
}

pub unsafe fn multUnits(_units: SEXP, _values: SEXP) -> SEXP {
    // STUB
    R_NilValue()
}

pub unsafe fn flipUnits(_units: SEXP) -> SEXP {
    // STUB
    R_NilValue()
}

pub unsafe fn absoluteUnits(_units: SEXP) -> SEXP {
    // STUB
    R_NilValue()
}

pub unsafe fn summaryUnits(_units: SEXP, _op_type: SEXP) -> SEXP {
    // STUB
    R_NilValue()
}
