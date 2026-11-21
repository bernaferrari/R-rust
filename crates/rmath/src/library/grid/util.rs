//! Port of R's src/library/grid/src/util.c -- grid utility functions.
//!
//! Contains list element access, numeric extraction, rectangle geometry,
//! text bounding rectangle calculation, and external pointer utilities.

use std::ffi::{CStr, c_void};
use std::os::raw::{c_char, c_double, c_int};

use crate::main::memory_main::{R_ExternalPtrAddr, R_MakeExternalPtr};
use crate::mainutils::engine::{GEStrHeight, GEStrWidth, fromDeviceHeight, fromDeviceWidth};
use crate::mainutils::errors::Rf_error;
use crate::sexp::accessors::{LENGTH, TYPEOF, XLENGTH};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

use super::matrix::{identity, location, multiply, rotation, trans, translation};
use super::types::*;
use super::{
    alloc_vector, char_ptr, get_attrib, integer_elt, protect, real_elt, set_vector_elt, string_elt,
    string_value_elt, vector_elt,
};

/* ==================== GE expression metric stubs ==================== */

fn GEExpressionWidth(_expr: SEXP, _gc: pGEcontext, _dd: pGEDevDesc) -> c_double {
    // SAFETY: static NUL-terminated literal.

    Rf_error(b"grid expression widths are not supported\0".as_ptr() as *const c_char);

    unreachable!()
}

fn GEExpressionHeight(_expr: SEXP, _gc: pGEcontext, _dd: pGEDevDesc) -> c_double {
    // SAFETY: static NUL-terminated literal.

    Rf_error(b"grid expression heights are not supported\0".as_ptr() as *const c_char);

    unreachable!()
}

const CE_SYMBOL: c_int = crate::sexp::encoding::CE_SYMBOL;

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
pub fn getListElement(list: SEXP, str: *const c_char) -> SEXP {
    let mut elmt: SEXP = R_NilValue();
    if str.is_null() {
        return elmt;
    }
    let names = get_attrib(list, crate::attrib_core::R_NamesSymbol());
    if names.is_null() {
        return elmt;
    }
    let len = LENGTH(list) as i32;
    // SAFETY: `str` is a borrowed NUL-terminated name supplied by the caller.
    let target_str = unsafe { CStr::from_ptr(str) }.to_str().unwrap_or("");
    for i in 0..len {
        let name_str = string_value_elt(names, i as R_xlen_t).unwrap_or_default();
        if name_str == target_str {
            elmt = vector_elt(list, i as R_xlen_t);
            break;
        }
    }
    elmt
}

/// Set the list element named str to value.
pub fn setListElement(list: SEXP, str: *const c_char, value: SEXP) {
    if str.is_null() {
        return;
    }
    let names = get_attrib(list, crate::attrib_core::R_NamesSymbol());
    if names.is_null() {
        return;
    }
    let len = LENGTH(list) as i32;
    // SAFETY: `str` is a borrowed NUL-terminated name supplied by the caller.
    let target_str = unsafe { CStr::from_ptr(str) }.to_str().unwrap_or("");
    for i in 0..len {
        let name_str = string_value_elt(names, i as R_xlen_t).unwrap_or_default();
        if name_str == target_str {
            set_vector_elt(list, i as R_xlen_t, value);
            break;
        }
    }
}

/* ==================== Numeric extraction ==================== */

/// Extract a numeric value from either a REALSXP or INTSXP.
/// Returns NA_REAL if index is negative or out of bounds.
#[inline]
pub fn numeric(x: SEXP, index: c_int) -> c_double {
    use crate::sexp::ffi::NA_REAL;
    if index < 0 {
        return NA_REAL;
    }
    let idx = index as R_xlen_t;
    if TYPEOF(x) == SEXPTYPE::REALSXP && XLENGTH(x) > idx {
        return real_elt(x, idx);
    } else if TYPEOF(x) == SEXPTYPE::INTSXP && XLENGTH(x) > idx {
        return integer_elt(x, idx) as c_double;
    }
    NA_REAL
}

/* ==================== Rectangle operations ==================== */

/// Fill a rectangle struct with the four corners.
pub fn rect(
    x1: c_double,
    x2: c_double,
    x3: c_double,
    x4: c_double,
    y1: c_double,
    y2: c_double,
    y3: c_double,
    y4: c_double,
    r: &mut LRect,
) {
    r.x1 = x1;
    r.x2 = x2;
    r.x3 = x3;
    r.x4 = x4;
    r.y1 = y1;
    r.y2 = y2;
    r.y3 = y3;
    r.y4 = y4;
}

/// Copy a rectangle struct.
pub fn copyRect(r1: LRect, r: &mut LRect) {
    *r = r1;
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
pub fn textRect(
    x: c_double,
    y: c_double,
    text: SEXP,
    i: c_int,
    gc: pGEcontext,
    xadj: c_double,
    yadj: c_double,
    rot: c_double,
    dd: pGEDevDesc,
    r: &mut LRect,
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
    {
        let mut w: f64;
        let mut h: f64;

        let text_len = XLENGTH(text) as i32;
        let idx = ((i % text_len) + text_len) % text_len;
        if TYPEOF(text) == SEXPTYPE::EXPRSXP {
            let expr = vector_elt(text, idx as R_xlen_t);
            // SAFETY: `gc`/`dd` are engine-owned device pointers lent by the
            // grid entry point.

            w = fromDeviceWidth(GEExpressionWidth(expr, gc, dd), 1, dd as *mut c_void);
            h = fromDeviceHeight(GEExpressionHeight(expr, gc, dd), 1, dd as *mut c_void);
        } else {
            let string = char_ptr(string_elt(text, idx as R_xlen_t));
            let enc = CE_SYMBOL;
            // SAFETY: `string` points into a live CHARSXP just validated by
            // `string_elt`; `gc`/`dd` are engine-owned device pointers lent by
            // the grid entry point.

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
            // SAFETY: static NUL-terminated literal.

            crate::main::errors::Rf_warning1(msg.as_ptr() as *const c_char);
        }

        if w >= 0.0 {
            if h >= 0.0 {
                location(0.0, 0.0, &mut bl);
                location(w, 0.0, &mut br);
                location(w, h, &mut tr);
                location(0.0, h, &mut tl);
            } else {
                location(0.0, h, &mut bl);
                location(w, h, &mut br);
                location(w, 0.0, &mut tr);
                location(0.0, 0.0, &mut tl);
            }
        } else if h >= 0.0 {
            location(w, 0.0, &mut bl);
            location(0.0, 0.0, &mut br);
            location(0.0, h, &mut tr);
            location(w, h, &mut tl);
        } else {
            location(w, h, &mut bl);
            location(0.0, h, &mut br);
            location(0.0, 0.0, &mut tr);
            location(w, 0.0, &mut tl);
        }

        translation(-xadj * w, -yadj * h, &mut thisJustification);
        translation(x, y, &mut thisLocation);

        if rot != 0.0 {
            rotation(rot, &mut thisRotation);
        } else {
            identity(&mut thisRotation);
        }

        multiply(&thisJustification, &thisRotation, &mut tempTransform);
        multiply(&tempTransform, &thisLocation, &mut transform);

        trans(&bl, &transform, &mut tbl);
        trans(&br, &transform, &mut tbr);
        trans(&tr, &transform, &mut ttr);
        trans(&tl, &transform, &mut ttl);

        rect(
            tbl[0], tbr[0], ttr[0], ttl[0], tbl[1], tbr[1], ttr[1], ttl[1], r,
        );
    }
}

/* ==================== External pointer utilities ==================== */

/// Create a persistent external pointer wrapping an SEXP.
/// The SEXP is stored in a VECSXP of length one, then wrapped in an external pointer.
pub fn L_CreateSEXPPtr(s: SEXP) -> SEXP {
    let data = alloc_vector(SEXPTYPE::VECSXP, 1);
    let _guard = protect(data);
    set_vector_elt(data, 0, s);
    // SAFETY: `data` is a live, protected VECSXP owned by this session; it
    // outlives the external pointer by virtue of the `prot` field.
    unsafe { R_MakeExternalPtr(data as *mut std::ffi::c_void, R_NilValue(), data) }
}

/// Get the SEXP stored in an external pointer created by L_CreateSEXPPtr.
pub fn L_GetSEXPPtr(sp: SEXP) -> SEXP {
    // SAFETY: `sp` is a live EXTPTRSXP created by `L_CreateSEXPPtr`.
    let data = unsafe { R_ExternalPtrAddr(sp) as SEXP };
    if data.is_null() {
        let msg = b"grid grob object is empty\0";
        // SAFETY: static NUL-terminated literals.

        crate::main::errors::Rf_error1(
            b"%s\0".as_ptr() as *const c_char,
            msg.as_ptr() as *const c_char,
        );

        unreachable!()
    }
    vector_elt(data, 0)
}

/// Set the SEXP stored in an external pointer created by L_CreateSEXPPtr.
pub fn L_SetSEXPPtr(sp: SEXP, s: SEXP) -> SEXP {
    // SAFETY: `sp` is a live EXTPTRSXP created by `L_CreateSEXPPtr`.
    let data = unsafe { R_ExternalPtrAddr(sp) as SEXP };
    if data.is_null() {
        let msg = b"grid grob object is empty\0";
        // SAFETY: static NUL-terminated literals.

        crate::main::errors::Rf_error1(
            b"%s\0".as_ptr() as *const c_char,
            msg.as_ptr() as *const c_char,
        );

        unreachable!()
    }
    set_vector_elt(data, 0, s);
    R_NilValue()
}
