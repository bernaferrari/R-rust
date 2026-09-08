//! Port of R's src/library/grid/src/util.c -- grid utility functions.
//!
//! Contains list element access, numeric extraction, rectangle geometry,
//! text bounding rectangle calculation, and external pointer utilities.

use std::ffi::{CStr, c_void};
use std::os::raw::{c_char, c_double, c_int};

use crate::main::memory_main::{R_ExternalPtrAddr, R_MakeExternalPtr};
use crate::mainutils::engine::{GEStrHeight, GEStrWidth, fromDeviceHeight, fromDeviceWidth};
use crate::mainutils::errors::Rf_error;
use crate::sexp::accessors::{
    CHAR, INTEGER, LENGTH, LOGICAL, REAL, SET_VECTOR_ELT, STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::constructors::{Rf_allocVector, Rf_mkString};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

use super::matrix::{identity, location, multiply, rotation, trans, translation};
use super::types::*;

/* ==================== GE expression metric stubs ==================== */

unsafe fn GEExpressionWidth(_expr: SEXP, _gc: pGEcontext, _dd: pGEDevDesc) -> c_double {
    unsafe {
        Rf_error(b"grid expression widths are not supported\0".as_ptr() as *const c_char);
    }
    unreachable!()
}

unsafe fn GEExpressionHeight(_expr: SEXP, _gc: pGEcontext, _dd: pGEDevDesc) -> c_double {
    unsafe {
        Rf_error(b"grid expression heights are not supported\0".as_ptr() as *const c_char);
    }
    unreachable!()
}

const CE_SYMBOL: c_int = 5;

/* ==================== Helper: fmax2/fmin2 ==================== */

/// Maximum of two doubles, propagating NaN.
#[inline]
fn fmax2(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        x + y // NaN propagation
    } else {
        x.max(y)
    }
}

/// Minimum of two doubles, propagating NaN.
#[inline]
fn fmin2(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        x + y // NaN propagation
    } else {
        x.min(y)
    }
}

/* ==================== List element access ==================== */

/// Get the list element named str, or return R_NilValue.
/// Copied from the Writing R Extensions manual (which copied it from nls).
pub unsafe fn getListElement(list: SEXP, str: *mut c_char) -> SEXP {
    let mut elmt: SEXP = unsafe { R_NilValue() };
    if str.is_null() {
        return elmt;
    }
    let names = unsafe { crate::attrib_core::getAttrib(list, crate::attrib_core::R_NamesSymbol()) };
    if names.is_null() {
        return elmt;
    }
    let len = unsafe { LENGTH(list) } as i32;
    let target_str = unsafe { CStr::from_ptr(str) }.to_str().unwrap_or("");
    for i in 0..len {
        let name_cstr = unsafe { CHAR(STRING_ELT(names, i as R_xlen_t)) };
        let name_str = unsafe { CStr::from_ptr(name_cstr) }.to_str().unwrap_or("");
        if name_str == target_str {
            elmt = unsafe { VECTOR_ELT(list, i as R_xlen_t) };
            break;
        }
    }
    elmt
}

/// Set the list element named str to value.
pub unsafe fn setListElement(list: SEXP, str: *mut c_char, value: SEXP) {
    if str.is_null() {
        return;
    }
    let names = unsafe { crate::attrib_core::getAttrib(list, crate::attrib_core::R_NamesSymbol()) };
    if names.is_null() {
        return;
    }
    let len = unsafe { LENGTH(list) } as i32;
    let target_str = unsafe { CStr::from_ptr(str) }.to_str().unwrap_or("");
    for i in 0..len {
        let name_cstr = unsafe { CHAR(STRING_ELT(names, i as R_xlen_t)) };
        let name_str = unsafe { CStr::from_ptr(name_cstr) }.to_str().unwrap_or("");
        if name_str == target_str {
            unsafe {
                SET_VECTOR_ELT(list, i as R_xlen_t, value);
            }
            break;
        }
    }
}

/* ==================== Numeric extraction ==================== */

/// Extract a numeric value from either a REALSXP or INTSXP.
/// Returns NA_REAL if index is negative or out of bounds.
#[inline]
pub unsafe fn numeric(x: SEXP, index: c_int) -> c_double {
    use crate::sexp::ffi::NA_REAL;
    if index < 0 {
        return NA_REAL;
    }
    let idx = index as R_xlen_t;
    if unsafe { TYPEOF(x) } == SEXPTYPE::REALSXP && unsafe { XLENGTH(x) } > idx {
        return unsafe { *REAL(x).add(index as usize) };
    } else if unsafe { TYPEOF(x) } == SEXPTYPE::INTSXP && unsafe { XLENGTH(x) } > idx {
        return unsafe { *INTEGER(x).add(index as usize) as c_double };
    }
    NA_REAL
}

/* ==================== Rectangle operations ==================== */

/// Fill a rectangle struct with the four corners.
pub unsafe fn rect(
    x1: c_double,
    x2: c_double,
    x3: c_double,
    x4: c_double,
    y1: c_double,
    y2: c_double,
    y3: c_double,
    y4: c_double,
    r: *mut LRect,
) {
    unsafe {
        (*r).x1 = x1;
        (*r).x2 = x2;
        (*r).x3 = x3;
        (*r).x4 = x4;
        (*r).y1 = y1;
        (*r).y2 = y2;
        (*r).y3 = y3;
        (*r).y4 = y4;
    }
}

/// Copy a rectangle struct.
pub unsafe fn copyRect(r1: LRect, r: *mut LRect) {
    unsafe {
        *r = r1;
    }
}

/* ==================== Geometry: line/edge intersection ==================== */

/// Do two lines intersect?
/// Algorithm from Paul Bourke.
pub fn linesIntersect(
    x1: c_double,
    x2: c_double,
    x3: c_double,
    x4: c_double,
    y1: c_double,
    y2: c_double,
    y3: c_double,
    y4: c_double,
) -> c_int {
    let mut result: c_double = 0.0;
    let denom = (y4 - y3) * (x2 - x1) - (x4 - x3) * (y2 - y1);
    let mut ua = (x4 - x3) * (y1 - y3) - (y4 - y3) * (x1 - x3);
    if denom == 0.0 {
        if ua == 0.0 {
            if x1 == x2 {
                if !((y1 < y3 && fmax2(y1, y2) < fmin2(y3, y4))
                    || (y3 < y1 && fmax2(y3, y4) < fmin2(y1, y2)))
                {
                    result = 1.0;
                }
            } else if !((x1 < x3 && fmax2(x1, x2) < fmin2(x3, x4))
                || (x3 < x1 && fmax2(x3, x4) < fmin2(x1, x2)))
            {
                result = 1.0;
            }
        }
    } else {
        let ub = (x2 - x1) * (y1 - y3) - (y2 - y1) * (x1 - x3);
        ua = ua / denom;
        let ub = ub / denom;
        if (ua > 0.0 && ua < 1.0) && (ub > 0.0 && ub < 1.0) {
            result = 1.0;
        }
    }
    result as c_int
}

/// Do a line segment and a rectangle's edges intersect?
pub fn edgesIntersect(x1: c_double, x2: c_double, y1: c_double, y2: c_double, r: LRect) -> c_int {
    let mut result: c_int = 0;
    if linesIntersect(x1, x2, r.x1, r.x2, y1, y2, r.y1, r.y2) != 0
        || linesIntersect(x1, x2, r.x2, r.x3, y1, y2, r.y2, r.y3) != 0
        || linesIntersect(x1, x2, r.x3, r.x4, y1, y2, r.y3, r.y4) != 0
        || linesIntersect(x1, x2, r.x4, r.x1, y1, y2, r.y4, r.y1) != 0
    {
        result = 1;
    }
    result
}

/// Do two rectangles intersect?
/// For each edge in r1, does the edge intersect with any edge in r2?
pub fn intersect(r1: LRect, r2: LRect) -> c_int {
    let mut result: c_int = 0;
    if edgesIntersect(r1.x1, r1.x2, r1.y1, r1.y2, r2) != 0
        || edgesIntersect(r1.x2, r1.x3, r1.y2, r1.y3, r2) != 0
        || edgesIntersect(r1.x3, r1.x4, r1.y3, r1.y4, r2) != 0
        || edgesIntersect(r1.x4, r1.x1, r1.y4, r1.y1, r2) != 0
    {
        result = 1;
    }
    result
}

/* ==================== Text bounding rectangle ==================== */

/// Calculate the bounding rectangle for a string.
/// x and y are assumed to be in INCHES.
pub unsafe fn textRect(
    x: c_double,
    y: c_double,
    text: SEXP,
    i: c_int,
    gc: pGEcontext,
    xadj: c_double,
    yadj: c_double,
    rot: c_double,
    dd: pGEDevDesc,
    r: *mut LRect,
) {
    use crate::sexp::ffi::NA_REAL;

    let mut bl: LLocation = [0.0; 3];
    let mut br: LLocation = [0.0; 3];
    let mut tr: LLocation = [0.0; 3];
    let mut tl: LLocation = [0.0; 3];
    let mut tbl: LLocation = [0.0; 3];
    let mut tbr: LLocation = [0.0; 3];
    let mut ttr: LLocation = [0.0; 3];
    let mut ttl: LLocation = [0.0; 3];
    let mut thisLocation: LTransform = [[0.0; 3]; 3];
    let mut thisRotation: LTransform = [[0.0; 3]; 3];
    let mut thisJustification: LTransform = [[0.0; 3]; 3];
    let mut tempTransform: LTransform = [[0.0; 3]; 3];
    let mut transform: LTransform = [[0.0; 3]; 3];
    unsafe {
        let mut w = 0.0;
        let mut h = 0.0;

        let text_len = XLENGTH(text) as i32;
        let idx = ((i % text_len) + text_len) % text_len;
        if TYPEOF(text) == SEXPTYPE::EXPRSXP {
            let expr = VECTOR_ELT(text, idx as R_xlen_t);
            w = fromDeviceWidth(GEExpressionWidth(expr, gc, dd), 1, dd as *mut c_void);
            h = fromDeviceHeight(GEExpressionHeight(expr, gc, dd), 1, dd as *mut c_void);
        } else {
            let string = CHAR(STRING_ELT(text, idx as R_xlen_t));
            let enc = CE_SYMBOL;
            w = fromDeviceWidth(
                GEStrWidth(string, enc, gc as *const c_void, dd as *mut c_void),
                1,
                dd as *mut c_void,
            );
            h = fromDeviceHeight(
                GEStrHeight(string, enc, gc as *const c_void, dd as *mut c_void),
                1,
                dd as *mut c_void,
            );
        }

        if !w.is_finite() || !h.is_finite() {
            if !w.is_finite() {
                w = 0.0;
            }
            if !h.is_finite() {
                h = 0.0;
            }
            let msg = b"Unable to calculate text width/height (using zero)\0";
            crate::main::errors::Rf_warning1(msg.as_ptr() as *const c_char);
        }

        if w >= 0.0 {
            if h >= 0.0 {
                location(0.0, 0.0, bl.as_mut_ptr() as *mut _);
                location(w, 0.0, br.as_mut_ptr() as *mut _);
                location(w, h, tr.as_mut_ptr() as *mut _);
                location(0.0, h, tl.as_mut_ptr() as *mut _);
            } else {
                location(0.0, h, bl.as_mut_ptr() as *mut _);
                location(w, h, br.as_mut_ptr() as *mut _);
                location(w, 0.0, tr.as_mut_ptr() as *mut _);
                location(0.0, 0.0, tl.as_mut_ptr() as *mut _);
            }
        } else if h >= 0.0 {
            location(w, 0.0, bl.as_mut_ptr() as *mut _);
            location(0.0, 0.0, br.as_mut_ptr() as *mut _);
            location(0.0, h, tr.as_mut_ptr() as *mut _);
            location(w, h, tl.as_mut_ptr() as *mut _);
        } else {
            location(w, h, bl.as_mut_ptr() as *mut _);
            location(0.0, h, br.as_mut_ptr() as *mut _);
            location(0.0, 0.0, tr.as_mut_ptr() as *mut _);
            location(w, 0.0, tl.as_mut_ptr() as *mut _);
        }

        translation(
            -xadj * w,
            -yadj * h,
            thisJustification.as_mut_ptr() as *mut _,
        );
        translation(x, y, thisLocation.as_mut_ptr() as *mut _);

        if rot != 0.0 {
            rotation(rot, thisRotation.as_mut_ptr() as *mut _);
        } else {
            identity(thisRotation.as_mut_ptr() as *mut _);
        }

        multiply(
            thisJustification.as_ptr() as *const _,
            thisRotation.as_ptr() as *const _,
            tempTransform.as_mut_ptr() as *mut _,
        );
        multiply(
            tempTransform.as_ptr() as *const _,
            thisLocation.as_ptr() as *const _,
            transform.as_mut_ptr() as *mut _,
        );

        trans(
            bl.as_ptr() as *const _,
            transform.as_ptr() as *const _,
            tbl.as_mut_ptr() as *mut _,
        );
        trans(
            br.as_ptr() as *const _,
            transform.as_ptr() as *const _,
            tbr.as_mut_ptr() as *mut _,
        );
        trans(
            tr.as_ptr() as *const _,
            transform.as_ptr() as *const _,
            ttr.as_mut_ptr() as *mut _,
        );
        trans(
            tl.as_ptr() as *const _,
            transform.as_ptr() as *const _,
            ttl.as_mut_ptr() as *mut _,
        );

        rect(
            tbl[0], tbr[0], ttr[0], ttl[0], tbl[1], tbr[1], ttr[1], ttl[1], r,
        );
    }
}

/* ==================== External pointer utilities ==================== */

/// Create a persistent external pointer wrapping an SEXP.
/// The SEXP is stored in a VECSXP of length one, then wrapped in an external pointer.
pub unsafe fn L_CreateSEXPPtr(s: SEXP) -> SEXP {
    let data = unsafe { Rf_allocVector(SEXPTYPE::VECSXP, 1) };
    let _guard = protect(data);
    unsafe {
        SET_VECTOR_ELT(data, 0, s);
        R_MakeExternalPtr(data as *mut std::ffi::c_void, R_NilValue(), data)
    }
}

/// Get the SEXP stored in an external pointer created by L_CreateSEXPPtr.
pub unsafe fn L_GetSEXPPtr(sp: SEXP) -> SEXP {
    let data = unsafe { R_ExternalPtrAddr(sp) as SEXP };
    if data.is_null() {
        let msg = b"grid grob object is empty\0";
        unsafe {
            crate::main::errors::Rf_error1(
                b"%s\0".as_ptr() as *const c_char,
                msg.as_ptr() as *const c_char,
            );
        }
        unreachable!()
    }
    unsafe { VECTOR_ELT(data, 0) }
}

/// Set the SEXP stored in an external pointer created by L_CreateSEXPPtr.
pub unsafe fn L_SetSEXPPtr(sp: SEXP, s: SEXP) -> SEXP {
    let data = unsafe { R_ExternalPtrAddr(sp) as SEXP };
    if data.is_null() {
        let msg = b"grid grob object is empty\0";
        unsafe {
            crate::main::errors::Rf_error1(
                b"%s\0".as_ptr() as *const c_char,
                msg.as_ptr() as *const c_char,
            );
        }
        unreachable!()
    }
    unsafe {
        SET_VECTOR_ELT(data, 0, s);
        R_NilValue()
    }
}
