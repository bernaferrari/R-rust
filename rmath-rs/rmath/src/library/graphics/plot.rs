/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1997--2025  The R Core Team
 *  Copyright (C) 2002--2009  The R Foundation
 *  Copyright (C) 1995, 1996  Robert Gentleman and Ross Ihaka
 *
 *  Ported from r-source/src/library/graphics/src/plot.c
 *
 *  Core base graphics functions: plot.new, plot.window, plot.xy,
 *  axis, text, mtext, title, abline, box, rect, segments, arrows,
 *  polygon, path, raster, xspline, symbols, locator, identify,
 *  clip, convertX, convertY, dend, dendwindow, erase, strWidth,
 *  strHeight, and supporting utilities (isNAcol, FixupLty, FixupLwd,
 *  FixupCol, FixupVFont, labelformat, C_path, C_dend, C_dendwindow,
 *  C_erase, C_convertX, C_convertY).
 *
 *  Note: Many of the C_* functions (C_plot_window, C_axis, C_plotXY,
 *  C_segments, C_rect, C_raster, C_arrows, C_polygon, C_text, C_mtext,
 *  C_title, C_abline, C_box, C_locator, C_identify, C_strHeight,
 *  C_strWidth, C_symbols, C_xspline, C_clip) are already provided as
 *  stubs in base.rs and are NOT duplicated here.
 *
 *  All GE (Graphics Engine) functions are declared as extern "C" stubs
 *  since the GE is not yet fully ported. Functions are ported with real
 *  implementations that call into these GE stubs.
 */

use std::cmp::Ordering;
use std::ffi::{CStr, c_void};
use std::os::raw::{c_char, c_double, c_int, c_uint};

use crate::attrib_core::{R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, asLogical, asReal, coerceVector};
use crate::main::duplicate::duplicate;
use crate::main::errors::Rf_error;
use crate::mainutils::bind::isList;
use crate::mainutils::colors::RGBpar3;
use crate::mainutils::engine::GErecordGraphicOperation;
use crate::mainutils::format::{formatComplex, formatReal};
use crate::mainutils::objects::inherits2 as inherits;
use crate::mainutils::printutils::{EncodeComplex, EncodeInteger, EncodeLogical, EncodeReal0};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::constructors::{
    Rf_ScalarInteger as ScalarInteger, Rf_ScalarReal as ScalarReal, Rf_length as length,
    Rf_mkChar as mkChar,
};
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::memory_ext::{R_alloc, vmaxget, vmaxset};
use crate::sexp::protect::*;

use super::graphics::{
    GArrow, GBox, GCheckState, GClip, GConvert, GConvertX, GConvertXUnits, GConvertY,
    GConvertYUnits, GExpressionHeight, GExpressionWidth, GLine, GMMathText, GMapUnits, GMapWin2Fig,
    GMathText, GMode, GMtext, GNewPlot, GPath, GPolygon, GPolyline, GRaster, GRecording, GRect,
    GRestorePars, GSavePars, GScale, GSetState, GStrHeight, GStrWidth, GSymbol, GText, xNPCtoUsr,
    yNPCtoUsr,
};
use super::par::{ProcessInlinePars, dpptr, gpptr};

/* ========================================================================
 * Local type and constant definitions
 * ======================================================================== */

/// pGEDevDesc is an opaque pointer to the graphics device descriptor.
type pGEDevDesc = *mut c_void;

/// Rboolean type (0 = FALSE, 1 = TRUE).
type Rboolean = c_int;

/// rcolor: R color type (unsigned int).
type rcolor = c_uint;

/// cetype_t: character encoding type.
type cetype_t = c_int;

/// GUnit: graphics unit type.
type GUnit = c_int;

/* R color constants */
const R_TRANSPARENT: c_uint = 0xFFFFFFFE;
const R_TRANWHITE: c_uint = 0x00FFFFFF;

/* Coordinate system constants (must match graphics.rs GUnit values) */
const DEVICE: c_int = 0;
const NDC: c_int = 1;
const NIC: c_int = 2;
const NFC: c_int = 3;
const NPC: c_int = 6;
const INCHES: c_int = 5;
const USER: c_int = 10;
const LINES: c_int = 7;
const CHARS: c_int = 8;

/* cetype_t constants */
const CE_ANY: cetype_t = 0;
const CE_NATIVE: cetype_t = 1;
const CE_UTF8: cetype_t = 2;
const CE_LATIN1: cetype_t = 3;
const CE_BYTES: cetype_t = 4;
const CE_SYMBOL: cetype_t = 5;
const CE_ISOLATIN1: cetype_t = 6; /* alias for CE_LATIN1 */

/* GMode constants */
const GMODE_OFF: c_int = 0;
const GMODE_ON: c_int = 1;
const GMODE_RECORD: c_int = 2;

/* Math constants */
const FLT_EPSILON: c_double = 1.19209290e-07;
const DBL_MAX: c_double = f64::MAX;
const DBL_MIN: c_double = f64::MIN_POSITIVE;
const R_PosInf: c_double = f64::INFINITY;
const R_NegInf: c_double = f64::NEG_INFINITY;
const DBL_MAX_EXP: c_int = 1024;

unsafe fn GEcurrentDevice() -> pGEDevDesc {
    unsafe { crate::library::grdevices::device_registry::GEcurrentDevice() as pGEDevDesc }
}

#[inline]
fn bool_to_rflag(value: bool) -> c_int {
    value as c_int
}

#[inline]
unsafe fn isNumeric(x: SEXP) -> c_int {
    unsafe {
        let sxp_type = TYPEOF(x);
        bool_to_rflag(
            sxp_type == SEXPTYPE::INTSXP
                || sxp_type == SEXPTYPE::REALSXP
                || sxp_type == SEXPTYPE::CPLXSXP
                || sxp_type == SEXPTYPE::LGLSXP,
        )
    }
}

#[inline]
unsafe fn isReal(x: SEXP) -> c_int {
    unsafe { bool_to_rflag(TYPEOF(x) == SEXPTYPE::REALSXP) }
}

#[inline]
unsafe fn isInteger(x: SEXP) -> c_int {
    unsafe { bool_to_rflag(TYPEOF(x) == SEXPTYPE::INTSXP) }
}

#[inline]
unsafe fn isLogical(x: SEXP) -> c_int {
    unsafe { bool_to_rflag(TYPEOF(x) == SEXPTYPE::LGLSXP) }
}

#[inline]
unsafe fn isString(x: SEXP) -> c_int {
    unsafe { bool_to_rflag(TYPEOF(x) == SEXPTYPE::STRSXP) }
}

#[inline]
unsafe fn isExpression(x: SEXP) -> c_int {
    unsafe { bool_to_rflag(TYPEOF(x) == SEXPTYPE::EXPRSXP) }
}

#[inline]
fn R_FINITE(x: c_double) -> c_int {
    bool_to_rflag(x.is_finite())
}

unsafe fn strcmp(s1: *const c_char, s2: *const c_char) -> c_int {
    unsafe {
        match (s1.is_null(), s2.is_null()) {
            (true, true) => return 0,
            (true, false) => return -1,
            (false, true) => return 1,
            (false, false) => {}
        }

        match CStr::from_ptr(s1)
            .to_bytes()
            .cmp(CStr::from_ptr(s2).to_bytes())
        {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }
}

/* OutDec global */
const OutDec: c_int = 46; /* '.' */

/* GPar structure - minimal definition with fields used in this file.
 * The real GPar is defined in Graphics.h and has many more fields.
 * When the GE is fully ported, this should be replaced. */
#[repr(C)]
struct GPar {
    /* General */
    adj: c_double,
    ann: c_int,
    bg: c_uint,
    bty: c_char,
    cex: c_double,
    lheight: c_double,
    col: c_uint,
    crt: c_double,
    din: [c_double; 2],
    err: c_int,
    fg: c_uint,
    family: [c_char; 201],
    font: c_int,
    gamma: c_double,
    lab: [c_int; 3],
    las: c_int,
    lty: c_int,
    lwd: c_double,
    mgp: [c_double; 3],
    mkh: c_double,
    pch: c_int,
    ps: c_double,
    smo: c_int,
    srt: c_double,
    tck: c_double,
    tcl: c_double,
    xaxp: [c_double; 3],
    xaxs: c_char,
    xaxt: c_char,
    xlog: c_int,
    xpd: c_int,
    oldxpd: c_int,
    yaxp: [c_double; 3],
    yaxs: c_char,
    yaxt: c_char,
    ylog: c_int,
    /* Annotation */
    cexbase: c_double,
    cexmain: c_double,
    cexlab: c_double,
    cexsub: c_double,
    cexaxis: c_double,
    fontmain: c_int,
    fontlab: c_int,
    fontsub: c_int,
    fontaxis: c_int,
    colmain: c_uint,
    collab: c_uint,
    colsub: c_uint,
    colaxis: c_uint,
    /* Layout */
    mar: [c_double; 4],
    oma: [c_double; 4],
    pin: [c_double; 2],
    plt: [c_double; 4],
    fig: [c_double; 4],
    /* Coordinate system */
    usr: [c_double; 4],
    logusr: [c_double; 4],
    new: c_int,
    state: c_int,
    valid: c_int,
}

/* GECtx (gcontext) structure stub */
#[repr(C)]
struct GECtx {
    _opaque: [u8; 0],
}

/* NA_STRING constant */
const NA_STRING: SEXP = 1 as SEXP; /* placeholder; real NA_STRING is a special CHARSXP */

/* ========================================================================
 * Utility: TypeCheck
 * ======================================================================== */

#[inline(always)]
unsafe fn TypeCheck(s: SEXP, stype: c_int) {
    unsafe {
        if TYPEOF(s) != stype {
            Rf_error(b"invalid type passed to graphics function\0".as_ptr() as *const c_char);
        }
    }
}

/* ========================================================================
 * Utility: isNAcol -- check if element i of a colour object is NA.
 * ======================================================================== */

pub unsafe fn isNAcol(col: SEXP, index: c_int, ncol: c_int) -> Rboolean {
    unsafe {
        let mut result: Rboolean = 1; /* TRUE by default */
        if Rf_isNull(col) != 0 {
            result = 1;
        } else {
            if isLogical(col) != 0 {
                result =
                    (LOGICAL(col).add((index % ncol) as usize).read() == NA_LOGICAL) as Rboolean;
            } else if isString(col) != 0 {
                result = (strcmp(
                    CHAR(STRING_ELT(col, (index % ncol) as R_xlen_t)),
                    b"NA\0".as_ptr() as *const c_char,
                ) == 0) as Rboolean;
            } else if isInteger(col) != 0 {
                result =
                    (INTEGER(col).add((index % ncol) as usize).read() == NA_INTEGER) as Rboolean;
            } else if isReal(col) != 0 {
                result = (R_FINITE(REAL(col).add((index % ncol) as usize).read()) == 0) as Rboolean;
            } else {
                Rf_error(b"invalid color specification\0".as_ptr() as *const c_char);
            }
        }
        result
    }
}

/* ========================================================================
 * Utility: getInlinePar -- extract specified par from list of inline pars
 * ======================================================================== */

unsafe fn getInlinePar(s: SEXP, name: *mut c_char) -> SEXP {
    unsafe {
        let mut result: SEXP = R_NilValue();
        let mut found: c_int = 0;
        if isList(s) != 0 && found == 0 {
            let mut cur = s;
            while cur != R_NilValue() {
                if isList(CAR(cur)) != 0 {
                    result = getInlinePar(CAR(cur), name);
                    if result != R_NilValue() {
                        found = 1;
                    }
                } else if TAG(cur) != R_NilValue() {
                    if strcmp(CHAR(PRINTNAME(TAG(cur))), name) == 0 {
                        result = CAR(cur);
                        found = 1;
                    }
                }
                cur = CDR(cur);
            }
        }
        result
    }
}

/* ========================================================================
 * FixupPch -- fix up plotting character specification.
 * ======================================================================== */

unsafe fn FixupPch(pch: SEXP, dflt: c_int) -> SEXP {
    unsafe {
        let n = length(pch);
        if n == 0 {
            return ScalarInteger(dflt);
        }
        let ans = Rf_allocVector(SEXPTYPE::INTSXP, n);
        let _ans_guard = protect(ans);
        if isList(pch) != 0 {
            let mut i: c_int = 0;
            let mut cur = pch;
            while cur != R_NilValue() {
                INTEGER(ans).add(i as usize).write(asInteger(CAR(cur)));
                i += 1;
                cur = CDR(cur);
            }
        } else if isInteger(pch) != 0 {
            for i in 0..n as usize {
                INTEGER(ans).add(i).write(INTEGER(pch).add(i).read());
            }
        } else if isReal(pch) != 0 {
            for i in 0..n as usize {
                let v = REAL(pch).add(i).read();
                INTEGER(ans).add(i).write(if R_FINITE(v) != 0 {
                    v as c_int
                } else {
                    NA_INTEGER
                });
            }
        } else if isString(pch) != 0 {
            for i in 0..n as usize {
                /* Delegate to shared engine conversion. */
                INTEGER(ans)
                    .add(i)
                    .write(GEstring_to_pch(STRING_ELT(pch, i as R_xlen_t)));
            }
        } else if isLogical(pch) != 0 {
            for i in 0..n as usize {
                if LOGICAL(pch).add(i).read() == NA_LOGICAL {
                    INTEGER(ans).add(i).write(NA_INTEGER);
                } else {
                    Rf_error(
                        b"only NA allowed in logical plotting symbol\0".as_ptr() as *const c_char
                    );
                }
            }
        } else {
            Rf_error(b"invalid plotting symbol\0".as_ptr() as *const c_char);
        }
        ans
    }
}

/* GEstring_to_pch delegate */
unsafe fn GEstring_to_pch(s: SEXP) -> c_int {
    unsafe { crate::mainutils::engine::GEstring_to_pch(s) }
}

/* GE_LTYpar delegate */
unsafe fn GE_LTYpar(lty: SEXP, i: c_int) -> c_int {
    unsafe { crate::mainutils::engine::GE_LTYpar(lty, i) as c_int }
}

/* ========================================================================
 * FixupLty -- fix up line type specification.
 * ======================================================================== */

pub unsafe fn FixupLty(lty: SEXP, dflt: c_int) -> SEXP {
    unsafe {
        let n = length(lty);
        let mut ans: SEXP = R_NilValue();
        if n == 0 {
            ans = ScalarInteger(dflt);
        } else {
            ans = Rf_allocVector(SEXPTYPE::INTSXP, n);
            for i in 0..n as usize {
                INTEGER(ans).add(i).write(GE_LTYpar(lty, i as c_int));
            }
        }
        ans
    }
}

/* ========================================================================
 * FixupLwd -- fix up line width specification.
 * ======================================================================== */

pub unsafe fn FixupLwd(lwd: SEXP, dflt: c_double) -> SEXP {
    unsafe {
        let n = length(lwd);
        let mut ans: SEXP = R_NilValue();
        if n == 0 {
            ans = ScalarReal(dflt);
        } else {
            let lwd = coerceVector(lwd, SEXPTYPE::REALSXP.into());
            let _lwd_guard = protect(lwd);
            let n = length(lwd);
            let ans_p = Rf_allocVector(SEXPTYPE::REALSXP, n);
            for i in 0..n as usize {
                let mut w = REAL(lwd).add(i).read();
                if w < 0.0 {
                    w = NA_REAL;
                }
                REAL(ans_p).add(i).write(w);
            }
            ans = ans_p;
        }
        ans
    }
}

/* ========================================================================
 * FixupFont -- fix up font specification.
 * ======================================================================== */

unsafe fn FixupFont(font: SEXP, dflt: c_int) -> SEXP {
    unsafe {
        let n = length(font);
        let mut ans: SEXP = R_NilValue();
        if n == 0 {
            ans = ScalarInteger(dflt);
        } else if isLogical(font) != 0 {
            ans = Rf_allocVector(SEXPTYPE::INTSXP, n);
            for i in 0..n as usize {
                let mut k = LOGICAL(font).add(i).read();
                if k < 1 || k > 5 {
                    k = NA_INTEGER;
                }
                INTEGER(ans).add(i).write(k);
            }
        } else if isInteger(font) != 0 {
            ans = Rf_allocVector(SEXPTYPE::INTSXP, n);
            for i in 0..n as usize {
                let mut k = INTEGER(font).add(i).read();
                if k < 1 || k > 5 {
                    k = NA_INTEGER;
                }
                INTEGER(ans).add(i).write(k);
            }
        } else if isReal(font) != 0 {
            ans = Rf_allocVector(SEXPTYPE::INTSXP, n);
            for i in 0..n as usize {
                let mut k = REAL(font).add(i).read() as c_int;
                if k < 1 || k > 5 {
                    k = NA_INTEGER;
                }
                INTEGER(ans).add(i).write(k);
            }
        } else {
            Rf_error(b"invalid font specification\0".as_ptr() as *const c_char);
        }
        ans
    }
}

/* ========================================================================
 * FixupCol -- fix up colour specification.
 * ======================================================================== */

pub unsafe fn FixupCol(col: SEXP, dflt: c_uint) -> SEXP {
    unsafe {
        let n = length(col);
        let ans: SEXP;
        /* bg = dpptr(GEcurrentDevice())->bg; but we use dflt for stub */
        let bg: c_uint = dflt;
        if n == 0 {
            ans = ScalarInteger(dflt as c_int);
            let _ans_guard = protect(ans);
        } else {
            ans = Rf_allocVector(SEXPTYPE::INTSXP, n);
            let _ans_guard = protect(ans);
            if isList(col) != 0 {
                let mut cur = col;
                for i in 0..n as usize {
                    INTEGER(ans)
                        .add(i)
                        .write(RGBpar3(CAR(cur) as *mut c_void, 0, bg) as c_int);
                    cur = CDR(cur);
                }
            } else {
                for i in 0..n as usize {
                    INTEGER(ans)
                        .add(i)
                        .write(RGBpar3(col as *mut c_void, i as c_int, bg) as c_int);
                }
            }
            return ans;
        }
        ans
    }
}

/* ========================================================================
 * FixupCex -- fix up character expansion specification.
 * ======================================================================== */

unsafe fn FixupCex(cex: SEXP, dflt: c_double) -> SEXP {
    unsafe {
        let n = length(cex);
        let ans: SEXP;
        if n == 0 {
            ans = Rf_allocVector(SEXPTYPE::REALSXP, 1);
            if R_FINITE(dflt) != 0 && dflt > 0.0 {
                REAL(ans).add(0).write(dflt);
            } else {
                REAL(ans).add(0).write(NA_REAL);
            }
        } else {
            ans = Rf_allocVector(SEXPTYPE::REALSXP, n);
            if isReal(cex) != 0 {
                for i in 0..n as usize {
                    let c = REAL(cex).add(i).read();
                    if R_FINITE(c) != 0 && c > 0.0 {
                        REAL(ans).add(i).write(c);
                    } else {
                        REAL(ans).add(i).write(NA_REAL);
                    }
                }
            } else if isInteger(cex) != 0 || isLogical(cex) != 0 {
                for i in 0..n as usize {
                    let mut c = INTEGER(cex).add(i).read() as c_double;
                    if c == NA_INTEGER as c_double || c <= 0.0 {
                        c = NA_REAL;
                    }
                    REAL(ans).add(i).write(c);
                }
            } else {
                Rf_error(b"invalid 'cex' value\0".as_ptr() as *const c_char);
            }
        }
        ans
    }
}

/* ========================================================================
 * FixupVFont -- fix up vector font specification.
 * ======================================================================== */

pub unsafe fn FixupVFont(vfont: SEXP) -> SEXP {
    unsafe {
        if Rf_isNull(vfont) != 0 {
            return R_NilValue();
        }
        let vf = coerceVector(vfont, SEXPTYPE::INTSXP.into());
        let _vf_guard = protect(vf);
        if length(vf) != 2 {
            Rf_error(b"invalid 'vfont' value\0".as_ptr() as *const c_char);
        }
        let typeface = INTEGER(vf).add(0).read();
        if typeface < 1 || typeface > 8 {
            Rf_error(b"invalid 'vfont' value\0".as_ptr() as *const c_char);
        }
        let mut maxindex: c_int = 0;
        match typeface {
            1 => {
                maxindex = 7;
            } /* serif */
            2 | 7 => {
                maxindex = 4;
            } /* sans serif, serif symbol */
            3 => {
                maxindex = 3;
            } /* script */
            4 | 5 | 6 => {
                maxindex = 1;
            } /* gothic */
            8 => {
                maxindex = 2;
            } /* sans serif symbol */
            _ => {
                maxindex = 1;
            }
        }
        let fontindex = INTEGER(vf).add(1).read();
        if fontindex < 1 || fontindex > maxindex {
            Rf_error(b"invalid 'vfont' value\0".as_ptr() as *const c_char);
        }
        let ans = Rf_allocVector(SEXPTYPE::INTSXP, 2);
        INTEGER(ans).add(0).write(INTEGER(vf).add(0).read());
        INTEGER(ans).add(1).write(INTEGER(vf).add(1).read());
        ans
    }
}

/* ========================================================================
 * GetTextArg -- extract and possibly set text arguments
 * ======================================================================== */

unsafe fn GetTextArg(
    spec: SEXP,
    ptxt: *mut SEXP,
    pcol: *mut c_uint,
    pcex: *mut c_double,
    pfont: *mut c_int,
) {
    unsafe {
        let stype = TYPEOF(spec);
        let mut txt: SEXP = R_NilValue();
        let mut cex: c_double = NA_REAL;
        let mut col: c_uint = R_TRANWHITE;
        let mut colspecd: c_int = 0;
        let mut font: c_int = NA_INTEGER;

        match stype {
            tt if tt == SEXPTYPE::LANGSXP || tt == SEXPTYPE::SYMSXP => {
                txt = coerceVector(spec, SEXPTYPE::EXPRSXP.into());
            }
            tt if tt == SEXPTYPE::VECSXP => {
                if length(spec) == 0 {
                    *ptxt = R_NilValue();
                    return;
                }
                let nms = getAttrib(spec, R_NamesSymbol());
                if nms == R_NilValue() {
                    txt = VECTOR_ELT(spec, 0);
                    let ttype = TYPEOF(txt);
                    if ttype == SEXPTYPE::LANGSXP || ttype == SEXPTYPE::SYMSXP {
                        txt = coerceVector(txt, SEXPTYPE::EXPRSXP.into());
                    } else if isExpression(txt) == 0 {
                        txt = coerceVector(txt, SEXPTYPE::STRSXP.into());
                    }
                } else {
                    let n = length(nms);
                    for i in 0..n as usize {
                        let nm_str = CHAR(STRING_ELT(nms, i as R_xlen_t));
                        if strcmp(nm_str, b"cex\0".as_ptr() as *const c_char) == 0 {
                            cex = asReal(VECTOR_ELT(spec, i as R_xlen_t));
                        } else if strcmp(nm_str, b"col\0".as_ptr() as *const c_char) == 0 {
                            let colsxp = VECTOR_ELT(spec, i as R_xlen_t);
                            if isNAcol(colsxp, 0, length(colsxp)) == 0 {
                                col = asInteger(FixupCol(colsxp, R_TRANWHITE)) as c_uint;
                                colspecd = 1;
                            }
                        } else if strcmp(nm_str, b"font\0".as_ptr() as *const c_char) == 0 {
                            font =
                                asInteger(FixupFont(VECTOR_ELT(spec, i as R_xlen_t), NA_INTEGER));
                        } else if strcmp(nm_str, b"\0".as_ptr() as *const c_char) == 0 {
                            txt = VECTOR_ELT(spec, i as R_xlen_t);
                            let ttype = TYPEOF(txt);
                            if ttype == SEXPTYPE::LANGSXP || ttype == SEXPTYPE::SYMSXP {
                                txt = coerceVector(txt, SEXPTYPE::EXPRSXP.into());
                            } else if isExpression(txt) == 0 {
                                txt = coerceVector(txt, SEXPTYPE::STRSXP.into());
                            }
                        } else {
                            Rf_error(b"invalid graphics parameter\0".as_ptr() as *const c_char);
                        }
                    }
                }
            }
            tt if tt == SEXPTYPE::STRSXP || tt == SEXPTYPE::EXPRSXP => {
                txt = spec;
            }
            _ => {
                txt = coerceVector(spec, SEXPTYPE::STRSXP.into());
            }
        }
        if txt != R_NilValue() {
            *ptxt = txt;
            if R_FINITE(cex) != 0 {
                *pcex = cex;
            }
            if colspecd != 0 {
                *pcol = col;
            }
            if font != NA_INTEGER {
                *pfont = font;
            }
        }
    }
}

/* ========================================================================
 * GetAxisLimits -- compute limits slightly beyond left/right
 * ======================================================================== */

unsafe fn GetAxisLimits(
    left: c_double,
    right: c_double,
    logflag: Rboolean,
    low: *mut c_double,
    high: *mut c_double,
) {
    unsafe {
        let mut l = left;
        let mut r = right;
        if logflag != 0 {
            l = l.ln();
            r = r.ln();
        }
        if l > r {
            let eps = l;
            l = r;
            r = eps;
        }
        let mut eps = r - l;
        if eps == 0.0 {
            eps = 0.5 * FLT_EPSILON;
        } else if eps == R_PosInf {
            eps = r * FLT_EPSILON;
            eps -= l * FLT_EPSILON;
        } else {
            eps *= FLT_EPSILON;
        }
        let mut lo = l - eps;
        let mut hi = r + eps;
        if logflag != 0 {
            *low = lo.exp();
            if hi < (std::f64::consts::LN_2 * DBL_MAX_EXP as c_double) {
                *high = hi.exp();
            } else {
                *high = DBL_MAX;
            }
        } else {
            if lo == R_NegInf {
                lo = -DBL_MAX;
            }
            if hi == R_PosInf {
                hi = DBL_MAX;
            }
            *low = lo;
            *high = hi;
        }
    }
}

/* ========================================================================
 * ComputePAdjValue -- compute perpendicular adjustment value
 * ======================================================================== */

unsafe fn ComputePAdjValue(padj: c_double, side: c_int, las: c_int) -> c_double {
    if R_FINITE(padj) == 0 {
        match las {
            0 => {
                /* parallel to axis */
                return 0.0;
            }
            1 => {
                /* horizontal */
                match side {
                    1 | 3 => return 0.0,
                    2 | 4 => return 0.5,
                    _ => {} // intentionally unhandled: invalid side value
                }
            }
            2 => {
                /* perpendicular to axis */
                return 0.5;
            }
            3 => {
                /* vertical */
                match side {
                    1 | 3 => return 0.5,
                    2 | 4 => return 0.0,
                    _ => {} // intentionally unhandled: invalid side value
                }
            }
            _ => {} // intentionally unhandled: unknown justification code
        }
    }
    padj
}

/* ========================================================================
 * ComputeAdjValue -- compute adjustment value for mtext
 * ======================================================================== */

unsafe fn ComputeAdjValue(adj: c_double, side: c_int, las: c_int) -> c_double {
    if R_FINITE(adj) == 0 {
        match las {
            0 => {
                /* parallel to axis */
                return 0.5;
            }
            1 => {
                /* horizontal */
                match side {
                    1 | 3 => return 0.5,
                    2 => return 1.0,
                    4 => return 0.0,
                    _ => {} // intentionally unhandled: invalid side value
                }
            }
            2 => {
                /* perpendicular to axis */
                match side {
                    1 | 2 => return 1.0,
                    3 | 4 => return 0.0,
                    _ => {} // intentionally unhandled: invalid side value
                }
            }
            3 => {
                /* vertical */
                match side {
                    1 => return 1.0,
                    3 => return 0.0,
                    2 | 4 => return 0.5,
                    _ => {} // intentionally unhandled: invalid side value
                }
            }
            _ => {} // intentionally unhandled: unknown justification code
        }
    }
    adj
}

/* ========================================================================
 * ComputeAtValueFromAdj / ComputeAtValue -- compute "at" from adj
 * ======================================================================== */

unsafe fn ComputeAtValueFromAdj(
    adj: c_double,
    side: c_int,
    outer: Rboolean,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        let at: c_double;
        if side % 2 == 0 {
            at = if outer != 0 { adj } else { yNPCtoUsr(adj, dd) };
        } else {
            at = if outer != 0 { adj } else { xNPCtoUsr(adj, dd) };
        }
        at
    }
}

unsafe fn ComputeAtValue(
    at: c_double,
    adj: c_double,
    side: c_int,
    las: c_int,
    outer: Rboolean,
    dd: pGEDevDesc,
) -> c_double {
    unsafe {
        if R_FINITE(at) == 0 {
            match las {
                0 => {
                    /* parallel to axis */
                    return ComputeAtValueFromAdj(adj, side, outer, dd);
                }
                1 => {
                    /* horizontal */
                    match side {
                        1 | 3 => {
                            return ComputeAtValueFromAdj(adj, side, outer, dd);
                        }
                        2 | 4 => {
                            return if outer != 0 { 0.5 } else { yNPCtoUsr(0.5, dd) };
                        }
                        _ => {} // intentionally unhandled: invalid side value
                    }
                }
                2 => {
                    /* perpendicular to axis */
                    match side {
                        1 | 3 => {
                            return if outer != 0 { 0.5 } else { xNPCtoUsr(0.5, dd) };
                        }
                        2 | 4 => {
                            return if outer != 0 { 0.5 } else { yNPCtoUsr(0.5, dd) };
                        }
                        _ => {} // intentionally unhandled: invalid side value
                    }
                }
                3 => {
                    /* vertical */
                    match side {
                        1 | 3 => {
                            return if outer != 0 { 0.5 } else { xNPCtoUsr(0.5, dd) };
                        }
                        2 | 4 => {
                            return ComputeAtValueFromAdj(adj, side, outer, dd);
                        }
                        _ => {} // intentionally unhandled: invalid side value
                    }
                }
                _ => {} // intentionally unhandled: unknown position code
            }
        }
        at
    }
}

/* ========================================================================
 * getxlimits / getylimits -- get clipping limits based on xpd
 * ======================================================================== */

unsafe fn getxlimits(x: *mut c_double, dd: pGEDevDesc) {
    unsafe {
        let gp = gpptr(dd) as *mut GPar;
        match (*gp).xpd {
            0 => {
                *x.add(0) = (*gp).usr[0];
                *x.add(1) = (*gp).usr[1];
            }
            1 => {
                *x.add(0) = GConvertX(0.0, NFC, USER, dd);
                *x.add(1) = GConvertX(1.0, NFC, USER, dd);
            }
            2 => {
                *x.add(0) = GConvertX(0.0, NDC, USER, dd);
                *x.add(1) = GConvertX(1.0, NDC, USER, dd);
            }
            _ => {} // intentionally unhandled: invalid axis value
        }
    }
}

unsafe fn getylimits(y: *mut c_double, dd: pGEDevDesc) {
    unsafe {
        let gp = gpptr(dd) as *mut GPar;
        match (*gp).xpd {
            0 => {
                *y.add(0) = (*gp).usr[2];
                *y.add(1) = (*gp).usr[3];
            }
            1 => {
                *y.add(0) = GConvertY(0.0, NFC, USER, dd);
                *y.add(1) = GConvertY(1.0, NFC, USER, dd);
            }
            2 => {
                *y.add(0) = GConvertY(0.0, NDC, USER, dd);
                *y.add(1) = GConvertY(1.0, NDC, USER, dd);
            }
            _ => {} // intentionally unhandled: invalid axis value
        }
    }
}

/* ========================================================================
 * xypoints -- validate and coerce x0,y0,x1,y1 coordinate args
 * ======================================================================== */

unsafe fn xypoints(args: SEXP, n: *mut c_int) {
    unsafe {
        let mut k: c_int = 0;
        let mut kmin: c_int = 0;

        if isNumeric(CAR(args)) == 0 {
            Rf_error(b"invalid first argument\0".as_ptr() as *const c_char);
        }
        SETCAR(args, coerceVector(CAR(args), SEXPTYPE::REALSXP.into()));
        k = length(CAR(args));
        *n = k;
        kmin = k;
        let args2 = CDR(args);

        if isNumeric(CAR(args2)) == 0 {
            Rf_error(b"invalid second argument\0".as_ptr() as *const c_char);
        }
        k = length(CAR(args2));
        SETCAR(args2, coerceVector(CAR(args2), SEXPTYPE::REALSXP.into()));
        if k > *n {
            *n = k;
        }
        if k < kmin {
            kmin = k;
        }
        let args3 = CDR(args2);

        if isNumeric(CAR(args3)) == 0 {
            Rf_error(b"invalid third argument\0".as_ptr() as *const c_char);
        }
        k = length(CAR(args3));
        SETCAR(args3, coerceVector(CAR(args3), SEXPTYPE::REALSXP.into()));
        if k > *n {
            *n = k;
        }
        if k < kmin {
            kmin = k;
        }
        let args4 = CDR(args3);

        if isNumeric(CAR(args4)) == 0 {
            Rf_error(b"invalid fourth argument\0".as_ptr() as *const c_char);
        }
        k = length(CAR(args4));
        SETCAR(args4, coerceVector(CAR(args4), SEXPTYPE::REALSXP.into()));
        if k > *n {
            *n = k;
        }
        if k < kmin {
            kmin = k;
        }

        if *n > 0 && kmin == 0 {
            Rf_error(
                b"cannot mix zero-length and non-zero-length coordinates\0".as_ptr()
                    as *const c_char,
            );
        }
    }
}

/* ========================================================================
 * drawPolygon -- helper for C_polygon
 * ======================================================================== */

unsafe fn drawPolygon(
    n: c_int,
    x: *const c_double,
    y: *const c_double,
    lty: c_int,
    fill: c_int,
    border: c_int,
    dd: pGEDevDesc,
) {
    unsafe {
        let gp = gpptr(dd) as *mut GPar;
        if lty == NA_INTEGER {
            (*gp).lty = (*(dpptr(dd) as *mut GPar)).lty;
        } else {
            (*gp).lty = lty;
        }
        GPolygon(
            n,
            x as *mut c_double,
            y as *mut c_double,
            USER,
            fill,
            border,
            dd,
        );
    }
}

/* ========================================================================
 * drawPointsLines -- helper for C_locator
 * ======================================================================== */

unsafe fn drawPointsLines(
    xp: c_double,
    yp: c_double,
    xold: c_double,
    yold: c_double,
    type_: u8,
    first: c_int,
    dd: pGEDevDesc,
) {
    unsafe {
        let gp = gpptr(dd) as *mut GPar;
        if type_ == b'p' || type_ == b'o' {
            GSymbol(xp, yp, DEVICE, (*gp).pch, dd);
        }
        if (type_ == b'l' || type_ == b'o') && first == 0 {
            GLine(xold, yold, xp, yp, DEVICE, dd);
        }
    }
}

/* ========================================================================
 * drawLabel -- helper for C_identify
 * ======================================================================== */

unsafe fn drawLabel(
    xi: c_double,
    yi: c_double,
    pos: c_int,
    offset: c_double,
    l: *const c_char,
    enc: cetype_t,
    dd: pGEDevDesc,
) {
    unsafe {
        let mut xi = xi;
        let mut yi = yi;
        match pos {
            4 => {
                xi = xi + offset;
                GText(xi, yi, INCHES, l, enc, 0.0, dd_dev_yCharOffset(dd), 0.0, dd);
            }
            2 => {
                xi = xi - offset;
                GText(xi, yi, INCHES, l, enc, 1.0, dd_dev_yCharOffset(dd), 0.0, dd);
            }
            3 => {
                yi = yi + offset;
                GText(xi, yi, INCHES, l, enc, 0.5, 0.0, 0.0, dd);
            }
            1 => {
                yi = yi - offset;
                GText(
                    xi,
                    yi,
                    INCHES,
                    l,
                    enc,
                    0.5,
                    1.0 - (0.5 - dd_dev_yCharOffset(dd)),
                    0.0,
                    dd,
                );
            }
            0 | _ => {
                GText(xi, yi, INCHES, l, enc, 0.0, 0.0, 0.0, dd);
            }
        }
    }
}

/* Stub for dd->dev->yCharOffset */
unsafe fn dd_dev_yCharOffset(_dd: pGEDevDesc) -> c_double {
    0.3
}

/* ========================================================================
 * SymbolRange / CheckSymbolPar -- helpers for C_symbols
 * ======================================================================== */

unsafe fn SymbolRange(
    x: *const c_double,
    n: c_int,
    xmax: *mut c_double,
    xmin: *mut c_double,
) -> Rboolean {
    unsafe {
        *xmax = -DBL_MAX;
        *xmin = DBL_MAX;
        for i in 0..n as usize {
            if R_FINITE(*x.add(i)) != 0 {
                if *xmax < *x.add(i) {
                    *xmax = *x.add(i);
                }
                if *xmin > *x.add(i) {
                    *xmin = *x.add(i);
                }
            }
        }
        if *xmax >= *xmin && *xmin >= 0.0 { 1 } else { 0 }
    }
}

unsafe fn CheckSymbolPar(p: SEXP, nr: *mut c_int, nc: *mut c_int) {
    unsafe {
        let dim = getAttrib(p, R_DimSymbol());
        match length(dim) {
            0 => {
                *nr = length(p);
                *nc = 1;
            }
            1 => {
                *nr = INTEGER(dim).add(0).read();
                *nc = 1;
            }
            2 => {
                *nr = INTEGER(dim).add(0).read();
                *nc = INTEGER(dim).add(1).read();
            }
            _ => {
                *nr = 0;
                *nc = 0;
            }
        }
        if *nr == 0 || *nc == 0 {
            Rf_error(b"invalid symbol parameter vector\0".as_ptr() as *const c_char);
        }
    }
}

/* ========================================================================
 * labelformat -- format labels from numbers to strings
 * ======================================================================== */

pub unsafe fn labelformat(labels: SEXP) -> SEXP {
    unsafe {
        let n = length(labels);
        let mut ans: SEXP = R_NilValue();
        let stype = TYPEOF(labels);
        match stype {
            tt if tt == SEXPTYPE::LGLSXP => {
                ans = Rf_allocVector(SEXPTYPE::STRSXP, n);
                let _ans_guard = protect(ans);
                for i in 0..n as usize {
                    let strp = EncodeLogical(LOGICAL(labels).add(i).read(), 0);
                    SET_STRING_ELT(ans, i as R_xlen_t, mkChar(strp));
                }
            }
            tt if tt == SEXPTYPE::INTSXP => {
                ans = Rf_allocVector(SEXPTYPE::STRSXP, n);
                let _ans_guard = protect(ans);
                for i in 0..n as usize {
                    let strp = EncodeInteger(INTEGER(labels).add(i).read(), 0);
                    SET_STRING_ELT(ans, i as R_xlen_t, mkChar(strp));
                }
            }
            tt if tt == SEXPTYPE::REALSXP => {
                let mut w: c_int = 0;
                let mut d: c_int = 0;
                let mut e: c_int = 0;
                formatReal(REAL(labels), n as R_xlen_t, &mut w, &mut d, &mut e, OutDec);
                ans = Rf_allocVector(SEXPTYPE::STRSXP, n);
                let _ans_guard = protect(ans);
                for i in 0..n as usize {
                    let strp = EncodeReal0(
                        REAL(labels).add(i).read(),
                        0,
                        d,
                        e,
                        b".\0".as_ptr() as *const c_char,
                    );
                    SET_STRING_ELT(ans, i as R_xlen_t, mkChar(strp));
                }
            }
            tt if tt == SEXPTYPE::CPLXSXP => {
                let mut w: c_int = 0;
                let mut d: c_int = 0;
                let mut e: c_int = 0;
                let mut wi: c_int = 0;
                let mut di: c_int = 0;
                let mut ei: c_int = 0;
                formatComplex(
                    COMPLEX(labels),
                    n as R_xlen_t,
                    &mut w,
                    &mut d,
                    &mut e,
                    &mut wi,
                    &mut di,
                    &mut ei,
                    OutDec,
                );
                ans = Rf_allocVector(SEXPTYPE::STRSXP, n);
                let _ans_guard = protect(ans);
                for i in 0..n as usize {
                    let cx = COMPLEX(labels).add(i).read();
                    let strp =
                        EncodeComplex(cx, 0, d, e, 0, di, ei, b".\0".as_ptr() as *const c_char);
                    SET_STRING_ELT(ans, i as R_xlen_t, mkChar(strp));
                }
            }
            tt if tt == SEXPTYPE::STRSXP => {
                ans = Rf_allocVector(SEXPTYPE::STRSXP, n);
                let _ans_guard = protect(ans);
                for i in 0..n as usize {
                    SET_STRING_ELT(ans, i as R_xlen_t, STRING_ELT(labels, i as R_xlen_t));
                }
            }
            _ => {
                Rf_error(b"invalid type for axis labels\0".as_ptr() as *const c_char);
            }
        }
        ans
    }
}

/* ========================================================================
 * C_plot_new -- create a new plot frame (plot.new())
 * ======================================================================== */

pub unsafe fn C_plot_new(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let dd = GEcurrentDevice();
        let recording = GRecording(call, dd);
        let dd = GNewPlot(recording);

        let dp = dpptr(dd) as *mut GPar;
        let gp = gpptr(dd) as *mut GPar;
        (*dp).xlog = 0;
        (*gp).xlog = 0;
        (*dp).ylog = 0;
        (*gp).ylog = 0;

        GScale(0.0, 1.0, 1, dd);
        GScale(0.0, 1.0, 2, dd);
        GMapWin2Fig(dd);
        GSetState(1, dd);

        if GRecording(call, dd) != 0 {
            GErecordGraphicOperation(op, args, dd);
        }
        R_NilValue()
    }
}

/* ========================================================================
 * C_path -- draw a path (possibly with holes)
 * ======================================================================== */

pub unsafe fn C_path(args: SEXP) -> SEXP {
    unsafe {
        let dd = GEcurrentDevice();
        GCheckState(dd);

        let mut args = CDR(args);
        if length(args) < 2 {
            Rf_error(b"too few arguments\0".as_ptr() as *const c_char);
        }

        SETCAR(args, coerceVector(CAR(args), SEXPTYPE::REALSXP.into()));
        let sx = CAR(args);
        args = CDR(args);
        SETCAR(args, coerceVector(CAR(args), SEXPTYPE::REALSXP.into()));
        let sy = CAR(args);
        args = CDR(args);
        let nx = length(sx);

        let nper = CAR(args);
        let _nper_guard = protect(nper);
        args = CDR(args);
        let npoly = length(nper);

        let _rule = CAR(args);
        let _rule_guard = protect(_rule);
        args = CDR(args);

        let _col = FixupCol(CAR(args), R_TRANWHITE);
        let _col_guard = protect(_col);
        args = CDR(args);
        let _border = FixupCol(CAR(args), 0);
        let _border_guard = protect(_border);
        args = CDR(args);
        let lty = FixupLty(CAR(args), 1);
        let _lty_guard = protect(lty);
        args = CDR(args);

        GSavePars(dd);
        ProcessInlinePars(args, dd);

        GMode(1, dd);

        let vmax = vmaxget();
        let xx = R_alloc(nx as usize, std::mem::size_of::<c_double>()) as *mut c_double;
        let yy = R_alloc(nx as usize, std::mem::size_of::<c_double>()) as *mut c_double;
        if xx.is_null() || yy.is_null() {
            Rf_error(b"unable to allocate memory (in GPath)\0".as_ptr() as *const c_char);
        }
        for i in 0..nx as usize {
            *xx.add(i) = REAL(sx).add(i).read();
            *yy.add(i) = REAL(sy).add(i).read();
            GConvert(xx.add(i), yy.add(i), USER, DEVICE, dd);
            if R_FINITE(*xx.add(i)) == 0 || R_FINITE(*yy.add(i)) == 0 {
                Rf_error(b"invalid 'x' or 'y' (in 'GPath')\0".as_ptr() as *const c_char);
            }
        }

        let gp = gpptr(dd) as *mut GPar;
        if INTEGER(lty).add(0).read() == NA_INTEGER {
            (*gp).lty = (*(dpptr(dd) as *mut GPar)).lty;
        } else {
            (*gp).lty = INTEGER(lty).add(0).read();
        }

        GPath(
            xx,
            yy,
            npoly,
            INTEGER(nper),
            1,
            INTEGER(_col).add(0).read(),
            INTEGER(_border).add(0).read(),
            dd,
        );

        GMode(0, dd);
        GRestorePars(dd);
        vmaxset(vmax);
        R_NilValue()
    }
}

/* ========================================================================
 * C_dend -- dendrogram plotting
 * ======================================================================== */

#[derive(Clone, Copy)]
pub(crate) struct DendrogramState {
    pub(crate) lptr: *mut c_int,
    pub(crate) rptr: *mut c_int,
    pub(crate) hght: *mut c_double,
    pub(crate) xpos: *mut c_double,
    pub(crate) hang: c_double,
    pub(crate) offset: c_double,
}

impl Default for DendrogramState {
    fn default() -> Self {
        DendrogramState {
            lptr: std::ptr::null_mut(),
            rptr: std::ptr::null_mut(),
            hght: std::ptr::null_mut(),
            xpos: std::ptr::null_mut(),
            hang: 0.0,
            offset: 0.0,
        }
    }
}

fn with_dendrogram_state<T>(f: impl FnOnce(&mut DendrogramState) -> T) -> T {
    with_required_current_instance(|instance| f(&mut instance.dendrogram_state))
}

unsafe fn drawdend(
    node: c_int,
    x: *mut c_double,
    y: *mut c_double,
    dnd_llabels: SEXP,
    dd: pGEDevDesc,
) {
    unsafe {
        let mut xl: c_double = 0.0;
        let mut xr: c_double = 0.0;
        let mut yl: c_double = 0.0;
        let mut yr: c_double = 0.0;
        let mut xx = [0.0; 4];
        let mut yy = [0.0; 4];
        let mut k: c_int;
        let state = with_dendrogram_state(|state| *state);

        *y = *state.hght.add((node - 1) as usize);

        /* left part */
        k = *state.lptr.add((node - 1) as usize);
        if k > 0 {
            drawdend(k, &mut xl, &mut yl, dnd_llabels, dd);
        } else {
            xl = *state.xpos.add((-k - 1) as usize);
            yl = if state.hang >= 0.0 {
                *y - state.hang
            } else {
                0.0
            };
            if STRING_ELT(dnd_llabels, (-k - 1) as R_xlen_t) != NA_STRING {
                GText(
                    xl,
                    yl - state.offset,
                    USER,
                    CHAR(STRING_ELT(dnd_llabels, (-k - 1) as R_xlen_t)),
                    getCharCE(STRING_ELT(dnd_llabels, (-k - 1) as R_xlen_t)),
                    1.0,
                    0.3,
                    90.0,
                    dd,
                );
            }
        }

        /* right part */
        k = *state.rptr.add((node - 1) as usize);
        if k > 0 {
            drawdend(k, &mut xr, &mut yr, dnd_llabels, dd);
        } else {
            xr = *state.xpos.add((-k - 1) as usize);
            yr = if state.hang >= 0.0 {
                *y - state.hang
            } else {
                0.0
            };
            if STRING_ELT(dnd_llabels, (-k - 1) as R_xlen_t) != NA_STRING {
                GText(
                    xr,
                    yr - state.offset,
                    USER,
                    CHAR(STRING_ELT(dnd_llabels, (-k - 1) as R_xlen_t)),
                    getCharCE(STRING_ELT(dnd_llabels, (-k - 1) as R_xlen_t)),
                    1.0,
                    0.3,
                    90.0,
                    dd,
                );
            }
        }

        xx[0] = xl;
        yy[0] = yl;
        xx[1] = xl;
        yy[1] = *y;
        xx[2] = xr;
        yy[2] = *y;
        xx[3] = xr;
        yy[3] = yr;
        GPolyline(
            4,
            xx.as_ptr() as *mut c_double,
            yy.as_ptr() as *mut c_double,
            USER,
            dd,
        );
        *x = 0.5 * (xl + xr);
    }
}

pub unsafe fn C_dend(args: SEXP) -> SEXP {
    unsafe {
        let mut x: c_double = 0.0;
        let mut y: c_double = 0.0;
        let n: c_int;

        let dnd_llabels: SEXP;
        let xpos: SEXP;
        let dd = GEcurrentDevice();
        GCheckState(dd);

        let mut args = CDR(args);
        if length(args) < 6 {
            Rf_error(b"too few arguments\0".as_ptr() as *const c_char);
        }

        /* n */
        n = asInteger(CAR(args));
        if n == NA_INTEGER || n < 2 {
            Rf_error(b"invalid dendrogram input\0".as_ptr() as *const c_char);
        }
        args = CDR(args);

        /* merge */
        if TYPEOF(CAR(args)) != SEXPTYPE::INTSXP || length(CAR(args)) != 2 * n {
            Rf_error(b"invalid dendrogram input\0".as_ptr() as *const c_char);
        }
        with_dendrogram_state(|state| {
            state.lptr = &mut *INTEGER(CAR(args)).add(0);
            state.rptr = &mut *INTEGER(CAR(args)).add(n as usize);
        });
        args = CDR(args);

        /* height */
        if TYPEOF(CAR(args)) != SEXPTYPE::REALSXP || length(CAR(args)) != n {
            Rf_error(b"invalid dendrogram input\0".as_ptr() as *const c_char);
        }
        with_dendrogram_state(|state| {
            state.hght = &mut *REAL(CAR(args)).add(0);
        });
        args = CDR(args);

        /* ord */
        if length(CAR(args)) != n + 1 {
            Rf_error(b"invalid dendrogram input\0".as_ptr() as *const c_char);
        }
        let xpos = coerceVector(CAR(args), SEXPTYPE::REALSXP.into());
        let _xpos_guard = protect(xpos);
        with_dendrogram_state(|state| {
            state.xpos = &mut *REAL(xpos).add(0);
        });
        args = CDR(args);

        /* hang */
        let hang = asReal(CAR(args));
        if R_FINITE(hang) == 0 {
            Rf_error(b"invalid dendrogram input\0".as_ptr() as *const c_char);
        }
        with_dendrogram_state(|state| {
            state.hang = hang * (*state.hght.add((n - 1) as usize) - *state.hght.add(0));
        });
        args = CDR(args);

        /* labels */
        if TYPEOF(CAR(args)) != SEXPTYPE::STRSXP || length(CAR(args)) != n + 1 {
            Rf_error(b"invalid dendrogram input\0".as_ptr() as *const c_char);
        }
        dnd_llabels = CAR(args);
        args = CDR(args);

        GSavePars(dd);
        ProcessInlinePars(args, dd);
        let gp = gpptr(dd) as *mut GPar;
        (*gp).cex = (*gp).cexbase * (*gp).cex;
        let offset = GConvertYUnits(
            GStrWidth(b"m\0".as_ptr() as *const c_char, CE_ANY, INCHES, dd),
            INCHES,
            USER,
            dd,
        );
        with_dendrogram_state(|state| {
            state.offset = offset;
        });

        if (*gp).xpd < 1 {
            (*gp).xpd = 1;
        }

        GMode(1, dd);
        drawdend(n, &mut x, &mut y, dnd_llabels, dd);
        GMode(0, dd);
        GRestorePars(dd);
        R_NilValue()
    }
}

/* ========================================================================
 * C_dendwindow -- set up dendrogram window
 * ======================================================================== */

pub unsafe fn C_dendwindow(args: SEXP) -> SEXP {
    unsafe {
        let n: c_int;
        let mut pin: c_double;
        let ll: *mut c_double;
        let mut tmp: c_double;
        let mut yval: c_double;
        let y: *mut c_double;
        let mut ymin: c_double;
        let mut ymax: c_double;
        let mut yrange: c_double;
        let mut m: c_double;
        let mut imax: c_int = -1;

        let merge: SEXP;
        let height: SEXP;
        let llabels: SEXP;
        let mut str: SEXP;

        let dd = GEcurrentDevice();
        GCheckState(dd);
        let mut args = CDR(args);
        if length(args) < 5 {
            Rf_error(b"too few arguments\0".as_ptr() as *const c_char);
        }
        n = asInteger(CAR(args));
        if n == NA_INTEGER || n < 2 {
            Rf_error(b"invalid dendrogram input\0".as_ptr() as *const c_char);
        }
        args = CDR(args);
        if TYPEOF(CAR(args)) != SEXPTYPE::INTSXP || length(CAR(args)) != 2 * n {
            Rf_error(b"invalid dendrogram input\0".as_ptr() as *const c_char);
        }
        merge = CAR(args);
        args = CDR(args);
        if TYPEOF(CAR(args)) != SEXPTYPE::REALSXP || length(CAR(args)) != n {
            Rf_error(b"invalid dendrogram input\0".as_ptr() as *const c_char);
        }
        height = CAR(args);
        args = CDR(args);
        let hang = asReal(CAR(args));
        with_dendrogram_state(|state| {
            state.hang = hang;
        });
        if R_FINITE(hang) == 0 {
            Rf_error(b"invalid dendrogram input\0".as_ptr() as *const c_char);
        }
        args = CDR(args);
        if TYPEOF(CAR(args)) != SEXPTYPE::STRSXP || length(CAR(args)) != n + 1 {
            Rf_error(b"invalid dendrogram input\0".as_ptr() as *const c_char);
        }
        llabels = CAR(args);
        args = CDR(args);

        GSavePars(dd);
        ProcessInlinePars(args, dd);
        let gp = gpptr(dd) as *mut GPar;
        (*gp).cex = (*gp).cexbase * (*gp).cex;
        let offset = GStrWidth(b"m\0".as_ptr() as *const c_char, CE_ANY, INCHES, dd);
        with_dendrogram_state(|state| {
            state.offset = offset;
        });
        let vmax = vmaxget();

        y = R_alloc((n + 1) as usize, std::mem::size_of::<c_double>()) as *mut c_double;
        ll = R_alloc((n + 1) as usize, std::mem::size_of::<c_double>()) as *mut c_double;
        let (lptr, rptr, hang, offset) = with_dendrogram_state(|state| {
            state.lptr = &mut *INTEGER(merge).add(0);
            state.rptr = &mut *INTEGER(merge).add(n as usize);
            (state.lptr, state.rptr, state.hang, state.offset)
        });

        ymax = *REAL(height).add(0);
        ymin = ymax;
        for i in 1..n as usize {
            m = *REAL(height).add(i);
            if m > ymax {
                ymax = m;
            } else if m < ymin {
                ymin = m;
            }
        }
        pin = (*gp).pin[1];

        for i in 0..=(n as usize) {
            str = STRING_ELT(llabels, i as R_xlen_t);
            *ll.add(i) = if str == NA_STRING {
                0.0
            } else {
                GStrWidth(CHAR(str), getCharCE(str), INCHES, dd) + offset
            };
        }

        yval = -DBL_MAX;
        if hang >= 0.0 {
            ymin = ymax - (1.0 + hang) * (ymax - ymin);
            yrange = ymax - ymin;
            /* determine leaf heights */
            for i in 0..n as usize {
                let left = *lptr.add(i);
                if left < 0 {
                    *y.add((-left - 1) as usize) = *REAL(height).add(i);
                }
                let right = *rptr.add(i);
                if right < 0 {
                    *y.add((-right - 1) as usize) = *REAL(height).add(i);
                }
            }
            for i in 0..=(n as usize) {
                tmp = ((ymax - *y.add(i)) / yrange) * pin + *ll.add(i);
                if tmp > yval {
                    yval = tmp;
                    imax = i as c_int;
                }
            }
        } else {
            yrange = ymax;
            for i in 0..=(n as usize) {
                tmp = pin + *ll.add(i);
                if tmp > yval {
                    yval = tmp;
                    imax = i as c_int;
                }
            }
        }

        ymin = ymax - (pin / (pin - *ll.add(imax as usize))) * yrange;
        GScale(1.0, (n + 1) as c_double, 1, dd);
        GScale(ymin, ymax, 2, dd);
        GMapWin2Fig(dd);
        GRestorePars(dd);
        vmaxset(vmax);
        R_NilValue()
    }
}

/* ========================================================================
 * C_erase -- erase the current device/frame
 * ======================================================================== */

pub unsafe fn C_erase(args: SEXP) -> SEXP {
    unsafe {
        let dd = GEcurrentDevice();
        let mut args = CDR(args);
        let col = FixupCol(CAR(args), R_TRANWHITE);
        let _col_guard = protect(col);
        GSavePars(dd);
        GMode(1, dd);
        GRect(
            0.0,
            0.0,
            1.0,
            1.0,
            NDC,
            INTEGER(col).add(0).read(),
            R_TRANWHITE as c_int,
            dd,
        );
        GMode(0, dd);
        GRestorePars(dd);
        R_NilValue()
    }
}

/* ========================================================================
 * C_convertX -- convert x coordinates between coordinate systems
 * ======================================================================== */

pub unsafe fn C_convertX(args: SEXP) -> SEXP {
    unsafe {
        let mut args = CDR(args);
        let x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::REALSXP {
            Rf_error(b"invalid 'x' argument\0".as_ptr() as *const c_char);
        }
        let n = length(x);
        let mut from = asInteger(CADR(args));
        if from == NA_INTEGER || from <= 0 || from > 17 {
            Rf_error(b"invalid 'from' argument\0".as_ptr() as *const c_char);
        }
        let mut to = asInteger(CADDR(args));
        if to == NA_INTEGER || to <= 0 || to > 17 {
            Rf_error(b"invalid 'to' argument\0".as_ptr() as *const c_char);
        }
        from -= 1;
        to -= 1;

        let ans = duplicate(x);
        let _ans_guard = protect(ans);
        let rx = REAL(ans);
        for i in 0..n as usize {
            *rx.add(i) = GConvertX(*rx.add(i), from, to, GEcurrentDevice());
        }
        ans
    }
}

/* ========================================================================
 * C_convertY -- convert y coordinates between coordinate systems
 * ======================================================================== */

pub unsafe fn C_convertY(args: SEXP) -> SEXP {
    unsafe {
        let mut args = CDR(args);
        let x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::REALSXP {
            Rf_error(b"invalid 'x' argument\0".as_ptr() as *const c_char);
        }
        let n = length(x);
        let mut from = asInteger(CADR(args));
        if from == NA_INTEGER || from <= 0 || from > 17 {
            Rf_error(b"invalid 'from' argument\0".as_ptr() as *const c_char);
        }
        let mut to = asInteger(CADDR(args));
        if to == NA_INTEGER || to <= 0 || to > 17 {
            Rf_error(b"invalid 'to' argument\0".as_ptr() as *const c_char);
        }
        from -= 1;
        to -= 1;

        let ans = duplicate(x);
        let _ans_guard = protect(ans);
        let ry = REAL(ans);
        for i in 0..n as usize {
            *ry.add(i) = GConvertY(*ry.add(i), from, to, GEcurrentDevice());
        }
        ans
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn dendrogram_state_is_session_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        let mut merge = [1 as c_int, 2, 3, 4];
        let mut height = [1.0 as c_double, 2.0, 3.0, 4.0];
        let mut xpos = [5.0 as c_double, 6.0, 7.0, 8.0];

        left.with_protected(|| {
            with_dendrogram_state(|state| {
                state.lptr = merge.as_mut_ptr();
                state.rptr = unsafe { merge.as_mut_ptr().add(2) };
                state.hght = height.as_mut_ptr();
                state.xpos = xpos.as_mut_ptr();
                state.hang = 9.0;
                state.offset = 10.0;
            });
        });

        right.with_protected(|| {
            with_dendrogram_state(|state| {
                assert!(state.lptr.is_null());
                assert!(state.rptr.is_null());
                assert!(state.hght.is_null());
                assert!(state.xpos.is_null());
                assert_eq!(state.hang, 0.0);
                assert_eq!(state.offset, 0.0);
            });
        });

        left.with_protected(|| {
            with_dendrogram_state(|state| {
                assert_eq!(unsafe { *state.lptr }, 1);
                assert_eq!(unsafe { *state.rptr }, 3);
                assert_eq!(unsafe { *state.hght }, 1.0);
                assert_eq!(unsafe { *state.xpos }, 5.0);
                assert_eq!(state.hang, 9.0);
                assert_eq!(state.offset, 10.0);
            });
        });
    }
}
