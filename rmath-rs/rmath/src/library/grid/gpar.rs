/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Port of R's src/library/grid/src/gpar.c (722 lines)
 *
 *  gpar -- graphical parameter accessors and context management for grid.
 */

use std::ffi::c_void;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::attrib_core::{R_NamesSymbol, setAttrib};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

use super::grid::getDevice;
use super::state::{gridStateElement, setGridStateElement};
use super::types::{GSS_GPAR, GSS_GPSAVED};

// GP_* constants from types.rs
pub const GP_FILL: c_int = 0;
pub const GP_COL: c_int = 1;
pub const GP_GAMMA: c_int = 2;
pub const GP_LTY: c_int = 3;
pub const GP_LWD: c_int = 4;
pub const GP_CEX: c_int = 5;
pub const GP_FONTSIZE: c_int = 6;
pub const GP_LINEHEIGHT: c_int = 7;
pub const GP_FONT: c_int = 8;
pub const GP_FONTFAMILY: c_int = 9;
pub const GP_ALPHA: c_int = 10;
pub const GP_LINEEND: c_int = 11;
pub const GP_LINEJOIN: c_int = 12;
pub const GP_LINEMITRE: c_int = 13;
pub const GP_LEX: c_int = 14;
pub const GP_FONTFACE: c_int = 15;

// R_TRANWHITE stub (transparent white for grid)
const R_TRANWHITE: c_int = 0x7FFFFFFF;

// pGEcontext and pGEDevDesc from types.rs
pub use super::types::pGEDevDesc;
pub use super::types::pGEcontext;

/* ==============================
 * Simple gpar accessor functions
 * ============================== */

pub unsafe fn gpFontSizeSXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_FONTSIZE as R_xlen_t)
}

pub unsafe fn gpFontSize(gp: SEXP, i: c_int) -> c_double {
    let fontsize = gpFontSizeSXP(gp);
    let len = LENGTH(fontsize);
    *REAL(fontsize).add((i % len) as usize)
}

pub unsafe fn gpLineHeightSXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_LINEHEIGHT as R_xlen_t)
}

pub unsafe fn gpLineHeight(gp: SEXP, i: c_int) -> c_double {
    let lineheight = gpLineHeightSXP(gp);
    let len = LENGTH(lineheight);
    *REAL(lineheight).add((i % len) as usize)
}

pub unsafe fn gpColSXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_COL as R_xlen_t)
}

pub unsafe fn gpCol(gp: SEXP, i: c_int) -> c_int {
    let col = gpColSXP(gp);
    if Rf_isNull(col) != 0 {
        R_TRANWHITE
    } else {
        // STUB: RGBpar3 not yet available
        R_TRANWHITE
    }
}

pub unsafe fn gpFillSXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_FILL as R_xlen_t)
}

pub unsafe fn gpFill(gp: SEXP, i: c_int) -> c_int {
    let fill = gpFillSXP(gp);
    if Rf_isNull(fill) != 0 {
        R_TRANWHITE
    } else {
        // STUB: RGBpar3 not yet available
        R_TRANWHITE
    }
}

pub unsafe fn gpGammaSXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_GAMMA as R_xlen_t)
}

pub unsafe fn gpGamma(gp: SEXP, i: c_int) -> c_double {
    let gamma = gpGammaSXP(gp);
    let len = LENGTH(gamma);
    *REAL(gamma).add((i % len) as usize)
}

pub unsafe fn gpLineTypeSXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_LTY as R_xlen_t)
}

pub unsafe fn gpLineType(gp: SEXP, i: c_int) -> c_int {
    let linetype = gpLineTypeSXP(gp);
    // STUB: GE_LTYpar not yet available
    let len = LENGTH(linetype);
    if TYPEOF(linetype) == SEXPTYPE::REALSXP.0 {
        *REAL(linetype).add((i % len) as usize) as c_int
    } else if TYPEOF(linetype) == SEXPTYPE::INTSXP.0 {
        *INTEGER(linetype).add((i % len) as usize)
    } else {
        1
    }
}

pub unsafe fn gpLineWidthSXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_LWD as R_xlen_t)
}

pub unsafe fn gpLineWidth(gp: SEXP, i: c_int) -> c_double {
    let linewidth = gpLineWidthSXP(gp);
    let len = LENGTH(linewidth);
    *REAL(linewidth).add((i % len) as usize)
}

pub unsafe fn gpCexSXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_CEX as R_xlen_t)
}

pub unsafe fn gpCex(gp: SEXP, i: c_int) -> c_double {
    let cex = gpCexSXP(gp);
    let len = LENGTH(cex);
    *REAL(cex).add((i % len) as usize)
}

pub unsafe fn gpFontSXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_FONT as R_xlen_t)
}

pub unsafe fn gpFont(gp: SEXP, i: c_int) -> c_int {
    let font = gpFontSXP(gp);
    let len = LENGTH(font);
    *INTEGER(font).add((i % len) as usize)
}

pub unsafe fn gpFontFamilySXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_FONTFAMILY as R_xlen_t)
}

pub unsafe fn gpFontFamily(gp: SEXP, i: c_int) -> *const c_char {
    let fontfamily = gpFontFamilySXP(gp);
    let len = LENGTH(fontfamily);
    CHAR(STRING_ELT(fontfamily, (i % len) as R_xlen_t))
}

pub unsafe fn gpAlphaSXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_ALPHA as R_xlen_t)
}

pub unsafe fn gpAlpha(gp: SEXP, i: c_int) -> c_double {
    let alpha = gpAlphaSXP(gp);
    let len = LENGTH(alpha);
    *REAL(alpha).add((i % len) as usize)
}

pub unsafe fn gpLineEndSXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_LINEEND as R_xlen_t)
}

pub unsafe fn gpLineEnd(gp: SEXP, i: c_int) -> c_int {
    // STUB: GE_LENDpar not yet available
    let lineend = gpLineEndSXP(gp);
    let len = LENGTH(lineend);
    if TYPEOF(lineend) == SEXPTYPE::INTSXP.0 {
        *INTEGER(lineend).add((i % len) as usize)
    } else {
        1 // GE_LINE_CAP_ROUND
    }
}

pub unsafe fn gpLineJoinSXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_LINEJOIN as R_xlen_t)
}

pub unsafe fn gpLineJoin(gp: SEXP, i: c_int) -> c_int {
    // STUB: GE_LJOINpar not yet available
    let linejoin = gpLineJoinSXP(gp);
    let len = LENGTH(linejoin);
    if TYPEOF(linejoin) == SEXPTYPE::INTSXP.0 {
        *INTEGER(linejoin).add((i % len) as usize)
    } else {
        1 // GE_LINE_JOIN_ROUND
    }
}

pub unsafe fn gpLineMitreSXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_LINEMITRE as R_xlen_t)
}

pub unsafe fn gpLineMitre(gp: SEXP, i: c_int) -> c_double {
    let linemitre = gpLineMitreSXP(gp);
    let len = LENGTH(linemitre);
    *REAL(linemitre).add((i % len) as usize)
}

pub unsafe fn gpLexSXP(gp: SEXP) -> SEXP {
    VECTOR_ELT(gp, GP_LEX as R_xlen_t)
}

pub unsafe fn gpLex(gp: SEXP, i: c_int) -> c_double {
    let lex = gpLexSXP(gp);
    let len = LENGTH(lex);
    *REAL(lex).add((i % len) as usize)
}

/* ==============================
 * "gpIsScalar" variants (set scalar flag)
 * ============================== */

pub unsafe fn gpFontSize2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    let fontsize = gpFontSizeSXP(gp);
    *gpIsScalar.add(GP_FONTSIZE as usize) = if LENGTH(fontsize) == 1 { 1 } else { 0 };
    *REAL(fontsize).add((i % LENGTH(fontsize)) as usize)
}

pub unsafe fn gpLineHeight2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    let lineheight = gpLineHeightSXP(gp);
    *gpIsScalar.add(GP_LINEHEIGHT as usize) = if LENGTH(lineheight) == 1 { 1 } else { 0 };
    *REAL(lineheight).add((i % LENGTH(lineheight)) as usize)
}

pub unsafe fn gpCol2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_int {
    let col = gpColSXP(gp);
    *gpIsScalar.add(GP_COL as usize) = if LENGTH(col) == 1 { 1 } else { 0 };
    gpCol(gp, i)
}

pub unsafe fn gpFill2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_int {
    let fill = gpFillSXP(gp);
    *gpIsScalar.add(GP_FILL as usize) = if LENGTH(fill) == 1 { 1 } else { 0 };
    gpFill(gp, i)
}

pub unsafe fn gpGamma2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    let gamma = gpGammaSXP(gp);
    *gpIsScalar.add(GP_GAMMA as usize) = if LENGTH(gamma) == 1 { 1 } else { 0 };
    *REAL(gamma).add((i % LENGTH(gamma)) as usize)
}

pub unsafe fn gpLineType2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_int {
    let linetype = gpLineTypeSXP(gp);
    *gpIsScalar.add(GP_LTY as usize) = if LENGTH(linetype) == 1 { 1 } else { 0 };
    gpLineType(gp, i)
}

pub unsafe fn gpLineWidth2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    let linewidth = gpLineWidthSXP(gp);
    *gpIsScalar.add(GP_LWD as usize) = if LENGTH(linewidth) == 1 { 1 } else { 0 };
    *REAL(linewidth).add((i % LENGTH(linewidth)) as usize)
}

pub unsafe fn gpCex2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    let cex = gpCexSXP(gp);
    *gpIsScalar.add(GP_CEX as usize) = if LENGTH(cex) == 1 { 1 } else { 0 };
    *REAL(cex).add((i % LENGTH(cex)) as usize)
}

pub unsafe fn gpFont2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_int {
    let font = gpFontSXP(gp);
    *gpIsScalar.add(GP_FONT as usize) = if LENGTH(font) == 1 { 1 } else { 0 };
    *INTEGER(font).add((i % LENGTH(font)) as usize)
}

pub unsafe fn gpFontFamily2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> *const c_char {
    let fontfamily = gpFontFamilySXP(gp);
    *gpIsScalar.add(GP_FONTFAMILY as usize) = if LENGTH(fontfamily) == 1 { 1 } else { 0 };
    CHAR(STRING_ELT(fontfamily, (i % LENGTH(fontfamily)) as R_xlen_t))
}

pub unsafe fn gpAlpha2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    let alpha = gpAlphaSXP(gp);
    *gpIsScalar.add(GP_ALPHA as usize) = if LENGTH(alpha) == 1 { 1 } else { 0 };
    *REAL(alpha).add((i % LENGTH(alpha)) as usize)
}

pub unsafe fn gpLineEnd2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_int {
    let lineend = gpLineEndSXP(gp);
    *gpIsScalar.add(GP_LINEEND as usize) = if LENGTH(lineend) == 1 { 1 } else { 0 };
    gpLineEnd(gp, i)
}

pub unsafe fn gpLineJoin2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_int {
    let linejoin = gpLineJoinSXP(gp);
    *gpIsScalar.add(GP_LINEJOIN as usize) = if LENGTH(linejoin) == 1 { 1 } else { 0 };
    gpLineJoin(gp, i)
}

pub unsafe fn gpLineMitre2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    let linemitre = gpLineMitreSXP(gp);
    *gpIsScalar.add(GP_LINEMITRE as usize) = if LENGTH(linemitre) == 1 { 1 } else { 0 };
    *REAL(linemitre).add((i % LENGTH(linemitre)) as usize)
}

pub unsafe fn gpLex2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    let lex = gpLexSXP(gp);
    *gpIsScalar.add(GP_LEX as usize) = if LENGTH(lex) == 1 { 1 } else { 0 };
    *REAL(lex).add((i % LENGTH(lex)) as usize)
}

/* ==============================
 * resolveGPar -- STUB
 * ============================== */

#[unsafe(no_mangle)]
pub unsafe fn resolveGPar(gp: SEXP, _byName: c_int) -> SEXP {
    gp
}

/* ==============================
 * gcontextFromgpar -- STUB
 * ============================== */

#[unsafe(no_mangle)]
pub unsafe fn gcontextFromgpar(_gp: SEXP, _i: c_int, _gc: pGEcontext, _dd: pGEDevDesc) {
    // STUB: sets GE context from gpar
}

/* ==============================
 * initGContext -- STUB
 * ============================== */

pub unsafe fn initGContext(
    _gp: SEXP,
    _gc: pGEcontext,
    _dd: pGEDevDesc,
    gpIsScalar: *mut c_int,
    _gcCache: pGEcontext,
) {
    if !gpIsScalar.is_null() {
        ptr::write_bytes(gpIsScalar, 0, GP_FONTFACE as usize + 1);
    }
}

/* ==============================
 * updateGContext -- STUB
 * ============================== */

pub unsafe fn updateGContext(
    _gp: SEXP,
    _i: c_int,
    _gc: pGEcontext,
    _dd: pGEDevDesc,
    gpIsScalar: *mut c_int,
    _gcCache: pGEcontext,
) {
    if !gpIsScalar.is_null() {
        ptr::write_bytes(gpIsScalar, 0, GP_FONTFACE as usize + 1);
    }
}

/* ==============================
 * initGPar -- STUB (initializes grid gpar for a device)
 * ============================== */

pub unsafe fn initGPar(_dd: pGEDevDesc) {
    // STUB: full initialization requires GE device access
}

/* ==============================
 * R-callable get/set gpar functions
 * ============================== */

pub unsafe fn L_getGPar() -> SEXP {
    let dd = getDevice();
    if dd.is_null() {
        return R_NilValue();
    }
    gridStateElement(dd, GSS_GPAR)
}

pub unsafe fn L_setGPar(gpars: SEXP) -> SEXP {
    let dd = getDevice();
    if !dd.is_null() {
        setGridStateElement(dd, GSS_GPAR, gpars);
    }
    R_NilValue()
}

pub unsafe fn L_getGPsaved() -> SEXP {
    let dd = getDevice();
    if dd.is_null() {
        return R_NilValue();
    }
    gridStateElement(dd, GSS_GPSAVED)
}

pub unsafe fn L_setGPsaved(gpars: SEXP) -> SEXP {
    let dd = getDevice();
    if !dd.is_null() {
        setGridStateElement(dd, GSS_GPSAVED, gpars);
    }
    R_NilValue()
}
