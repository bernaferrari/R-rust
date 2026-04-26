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
use crate::{mainutils::colors::RGBpar3, mainutils::engine as ge};

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

// Grid's transparent-white default is shared with the graphics engine.
const R_TRANWHITE: c_int = ge::R_TRANWHITE as c_int;

// pGEcontext and pGEDevDesc from types.rs
pub use super::types::pGEDevDesc;
pub use super::types::pGEcontext;

/* ==============================
 * Simple gpar accessor functions
 * ============================== */

pub unsafe fn gpFontSizeSXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_FONTSIZE as R_xlen_t) }
}

pub unsafe fn gpFontSize(gp: SEXP, i: c_int) -> c_double {
    unsafe {
        let fontsize = gpFontSizeSXP(gp);
        let len = LENGTH(fontsize);
        *REAL(fontsize).add((i % len) as usize)
    }
}

pub unsafe fn gpLineHeightSXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_LINEHEIGHT as R_xlen_t) }
}

pub unsafe fn gpLineHeight(gp: SEXP, i: c_int) -> c_double {
    unsafe {
        let lineheight = gpLineHeightSXP(gp);
        let len = LENGTH(lineheight);
        *REAL(lineheight).add((i % len) as usize)
    }
}

pub unsafe fn gpColSXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_COL as R_xlen_t) }
}

pub unsafe fn gpCol(gp: SEXP, i: c_int) -> c_int {
    unsafe {
        if gp.is_null() || Rf_isNull(gp) != 0 {
            return R_TRANWHITE;
        }
        let col = gpColSXP(gp);
        if Rf_isNull(col) != 0 {
            R_TRANWHITE
        } else {
            RGBpar3(col as *mut c_void, i, ge::R_TRANWHITE) as c_int
        }
    }
}

pub unsafe fn gpFillSXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_FILL as R_xlen_t) }
}

pub unsafe fn gpFill(gp: SEXP, i: c_int) -> c_int {
    unsafe {
        if gp.is_null() || Rf_isNull(gp) != 0 {
            return R_TRANWHITE;
        }
        let fill = gpFillSXP(gp);
        if Rf_isNull(fill) != 0 {
            R_TRANWHITE
        } else {
            RGBpar3(fill as *mut c_void, i, ge::R_TRANWHITE) as c_int
        }
    }
}

pub unsafe fn gpGammaSXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_GAMMA as R_xlen_t) }
}

pub unsafe fn gpGamma(gp: SEXP, i: c_int) -> c_double {
    unsafe {
        let gamma = gpGammaSXP(gp);
        let len = LENGTH(gamma);
        *REAL(gamma).add((i % len) as usize)
    }
}

pub unsafe fn gpLineTypeSXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_LTY as R_xlen_t) }
}

pub unsafe fn gpLineType(gp: SEXP, i: c_int) -> c_int {
    unsafe {
        let linetype = gpLineTypeSXP(gp);
        ge::GE_LTYpar(linetype, i) as c_int
    }
}

pub unsafe fn gpLineWidthSXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_LWD as R_xlen_t) }
}

pub unsafe fn gpLineWidth(gp: SEXP, i: c_int) -> c_double {
    unsafe {
        let linewidth = gpLineWidthSXP(gp);
        let len = LENGTH(linewidth);
        *REAL(linewidth).add((i % len) as usize)
    }
}

pub unsafe fn gpCexSXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_CEX as R_xlen_t) }
}

pub unsafe fn gpCex(gp: SEXP, i: c_int) -> c_double {
    unsafe {
        let cex = gpCexSXP(gp);
        let len = LENGTH(cex);
        *REAL(cex).add((i % len) as usize)
    }
}

pub unsafe fn gpFontSXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_FONT as R_xlen_t) }
}

pub unsafe fn gpFont(gp: SEXP, i: c_int) -> c_int {
    unsafe {
        let font = gpFontSXP(gp);
        let len = LENGTH(font);
        *INTEGER(font).add((i % len) as usize)
    }
}

pub unsafe fn gpFontFamilySXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_FONTFAMILY as R_xlen_t) }
}

pub unsafe fn gpFontFamily(gp: SEXP, i: c_int) -> *const c_char {
    unsafe {
        let fontfamily = gpFontFamilySXP(gp);
        let len = LENGTH(fontfamily);
        CHAR(STRING_ELT(fontfamily, (i % len) as R_xlen_t))
    }
}

pub unsafe fn gpAlphaSXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_ALPHA as R_xlen_t) }
}

pub unsafe fn gpAlpha(gp: SEXP, i: c_int) -> c_double {
    unsafe {
        let alpha = gpAlphaSXP(gp);
        let len = LENGTH(alpha);
        *REAL(alpha).add((i % len) as usize)
    }
}

pub unsafe fn gpLineEndSXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_LINEEND as R_xlen_t) }
}

pub unsafe fn gpLineEnd(gp: SEXP, i: c_int) -> c_int {
    unsafe {
        let lineend = gpLineEndSXP(gp);
        ge::GE_LENDpar(lineend, i)
    }
}

pub unsafe fn gpLineJoinSXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_LINEJOIN as R_xlen_t) }
}

pub unsafe fn gpLineJoin(gp: SEXP, i: c_int) -> c_int {
    unsafe {
        let linejoin = gpLineJoinSXP(gp);
        ge::GE_LJOINpar(linejoin, i)
    }
}

pub unsafe fn gpLineMitreSXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_LINEMITRE as R_xlen_t) }
}

pub unsafe fn gpLineMitre(gp: SEXP, i: c_int) -> c_double {
    unsafe {
        let linemitre = gpLineMitreSXP(gp);
        let len = LENGTH(linemitre);
        *REAL(linemitre).add((i % len) as usize)
    }
}

pub unsafe fn gpLexSXP(gp: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(gp, GP_LEX as R_xlen_t) }
}

pub unsafe fn gpLex(gp: SEXP, i: c_int) -> c_double {
    unsafe {
        let lex = gpLexSXP(gp);
        let len = LENGTH(lex);
        *REAL(lex).add((i % len) as usize)
    }
}

/* ==============================
 * "gpIsScalar" variants (set scalar flag)
 * ============================== */

pub unsafe fn gpFontSize2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    unsafe {
        let fontsize = gpFontSizeSXP(gp);
        *gpIsScalar.add(GP_FONTSIZE as usize) = if LENGTH(fontsize) == 1 { 1 } else { 0 };
        *REAL(fontsize).add((i % LENGTH(fontsize)) as usize)
    }
}

pub unsafe fn gpLineHeight2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    unsafe {
        let lineheight = gpLineHeightSXP(gp);
        *gpIsScalar.add(GP_LINEHEIGHT as usize) = if LENGTH(lineheight) == 1 { 1 } else { 0 };
        *REAL(lineheight).add((i % LENGTH(lineheight)) as usize)
    }
}

pub unsafe fn gpCol2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_int {
    unsafe {
        let col = gpColSXP(gp);
        *gpIsScalar.add(GP_COL as usize) = if LENGTH(col) == 1 { 1 } else { 0 };
        gpCol(gp, i)
    }
}

pub unsafe fn gpFill2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_int {
    unsafe {
        let fill = gpFillSXP(gp);
        *gpIsScalar.add(GP_FILL as usize) = if LENGTH(fill) == 1 { 1 } else { 0 };
        gpFill(gp, i)
    }
}

pub unsafe fn gpGamma2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    unsafe {
        let gamma = gpGammaSXP(gp);
        *gpIsScalar.add(GP_GAMMA as usize) = if LENGTH(gamma) == 1 { 1 } else { 0 };
        *REAL(gamma).add((i % LENGTH(gamma)) as usize)
    }
}

pub unsafe fn gpLineType2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_int {
    unsafe {
        let linetype = gpLineTypeSXP(gp);
        *gpIsScalar.add(GP_LTY as usize) = if LENGTH(linetype) == 1 { 1 } else { 0 };
        gpLineType(gp, i)
    }
}

pub unsafe fn gpLineWidth2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    unsafe {
        let linewidth = gpLineWidthSXP(gp);
        *gpIsScalar.add(GP_LWD as usize) = if LENGTH(linewidth) == 1 { 1 } else { 0 };
        *REAL(linewidth).add((i % LENGTH(linewidth)) as usize)
    }
}

pub unsafe fn gpCex2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    unsafe {
        let cex = gpCexSXP(gp);
        *gpIsScalar.add(GP_CEX as usize) = if LENGTH(cex) == 1 { 1 } else { 0 };
        *REAL(cex).add((i % LENGTH(cex)) as usize)
    }
}

pub unsafe fn gpFont2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_int {
    unsafe {
        let font = gpFontSXP(gp);
        *gpIsScalar.add(GP_FONT as usize) = if LENGTH(font) == 1 { 1 } else { 0 };
        *INTEGER(font).add((i % LENGTH(font)) as usize)
    }
}

pub unsafe fn gpFontFamily2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> *const c_char {
    unsafe {
        let fontfamily = gpFontFamilySXP(gp);
        *gpIsScalar.add(GP_FONTFAMILY as usize) = if LENGTH(fontfamily) == 1 { 1 } else { 0 };
        CHAR(STRING_ELT(fontfamily, (i % LENGTH(fontfamily)) as R_xlen_t))
    }
}

pub unsafe fn gpAlpha2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    unsafe {
        let alpha = gpAlphaSXP(gp);
        *gpIsScalar.add(GP_ALPHA as usize) = if LENGTH(alpha) == 1 { 1 } else { 0 };
        *REAL(alpha).add((i % LENGTH(alpha)) as usize)
    }
}

pub unsafe fn gpLineEnd2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_int {
    unsafe {
        let lineend = gpLineEndSXP(gp);
        *gpIsScalar.add(GP_LINEEND as usize) = if LENGTH(lineend) == 1 { 1 } else { 0 };
        gpLineEnd(gp, i)
    }
}

pub unsafe fn gpLineJoin2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_int {
    unsafe {
        let linejoin = gpLineJoinSXP(gp);
        *gpIsScalar.add(GP_LINEJOIN as usize) = if LENGTH(linejoin) == 1 { 1 } else { 0 };
        gpLineJoin(gp, i)
    }
}

pub unsafe fn gpLineMitre2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    unsafe {
        let linemitre = gpLineMitreSXP(gp);
        *gpIsScalar.add(GP_LINEMITRE as usize) = if LENGTH(linemitre) == 1 { 1 } else { 0 };
        *REAL(linemitre).add((i % LENGTH(linemitre)) as usize)
    }
}

pub unsafe fn gpLex2(gp: SEXP, i: c_int, gpIsScalar: *mut c_int) -> c_double {
    unsafe {
        let lex = gpLexSXP(gp);
        *gpIsScalar.add(GP_LEX as usize) = if LENGTH(lex) == 1 { 1 } else { 0 };
        *REAL(lex).add((i % LENGTH(lex)) as usize)
    }
}

/* ==============================
 * resolveGPar -- partial helper
 * ============================== */

pub unsafe fn resolveGPar(gp: SEXP, _byName: c_int) -> SEXP {
    gp
}

/* ==============================
 * gcontextFromgpar -- partial helper
 * ============================== */

pub unsafe fn gcontextFromgpar(_gp: SEXP, _i: c_int, _gc: pGEcontext, _dd: pGEDevDesc) {
    unsafe {
        let gp = resolveGPar(_gp, 0);
        if gp.is_null() || Rf_isNull(gp) != 0 {
            return;
        }

        // The real R_GE_gcontext layout is still opaque here, so we resolve and
        // validate the gpar values that feed it without trying to write fields.
        let _ = (
            gpCol(gp, _i),
            gpFill(gp, _i),
            gpGamma(gp, _i),
            gpLineType(gp, _i),
            gpLineWidth(gp, _i),
            gpCex(gp, _i),
            gpFontSize(gp, _i),
            gpLineHeight(gp, _i),
            gpFont(gp, _i),
            gpFontFamily(gp, _i),
            gpAlpha(gp, _i),
            gpLineEnd(gp, _i),
            gpLineJoin(gp, _i),
            gpLineMitre(gp, _i),
            gpLex(gp, _i),
        );
        let _ = _gc;
        let _ = _dd;
    }
}

/* ==============================
 * initGContext -- partial helper
 * ============================== */

unsafe fn set_gpar_scalar_flags(gp: SEXP, gpIsScalar: *mut c_int) {
    unsafe {
        if gpIsScalar.is_null() {
            return;
        }

        ptr::write_bytes(gpIsScalar, 0, GP_FONTFACE as usize + 1);

        if gp.is_null() || Rf_isNull(gp) != 0 {
            return;
        }

        let slots = [
            (GP_FILL, gpFillSXP(gp)),
            (GP_COL, gpColSXP(gp)),
            (GP_GAMMA, gpGammaSXP(gp)),
            (GP_LTY, gpLineTypeSXP(gp)),
            (GP_LWD, gpLineWidthSXP(gp)),
            (GP_CEX, gpCexSXP(gp)),
            (GP_FONTSIZE, gpFontSizeSXP(gp)),
            (GP_LINEHEIGHT, gpLineHeightSXP(gp)),
            (GP_FONT, gpFontSXP(gp)),
            (GP_FONTFAMILY, gpFontFamilySXP(gp)),
            (GP_ALPHA, gpAlphaSXP(gp)),
            (GP_LINEEND, gpLineEndSXP(gp)),
            (GP_LINEJOIN, gpLineJoinSXP(gp)),
            (GP_LINEMITRE, gpLineMitreSXP(gp)),
            (GP_LEX, gpLexSXP(gp)),
        ];

        for (slot, value) in slots {
            *gpIsScalar.add(slot as usize) = if LENGTH(value) == 1 { 1 } else { 0 };
        }
    }
}

pub unsafe fn initGContext(
    gp: SEXP,
    gc: pGEcontext,
    dd: pGEDevDesc,
    gpIsScalar: *mut c_int,
    gcCache: pGEcontext,
) {
    unsafe {
        gcontextFromgpar(gp, 0, gc, dd);
        set_gpar_scalar_flags(gp, gpIsScalar);
        let _ = gcCache;
    }
}

/* ==============================
 * updateGContext -- partial helper
 * ============================== */

pub unsafe fn updateGContext(
    gp: SEXP,
    i: c_int,
    gc: pGEcontext,
    dd: pGEDevDesc,
    gpIsScalar: *mut c_int,
    gcCache: pGEcontext,
) {
    unsafe {
        gcontextFromgpar(gp, i, gc, dd);
        set_gpar_scalar_flags(gp, gpIsScalar);
        let _ = gcCache;
    }
}

/* ==============================
 * initGPar -- initializes grid gpar for a device
 * ============================== */

pub unsafe fn initGPar(dd: pGEDevDesc) {
    unsafe {
        let gp = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, GP_FONTFACE + 1));

        // GP_FILL (0): transparent white
        SET_VECTOR_ELT(gp, GP_FILL as R_xlen_t, Rf_ScalarInteger(R_TRANWHITE));

        // GP_COL (1): black (R colour index 1)
        SET_VECTOR_ELT(gp, GP_COL as R_xlen_t, Rf_ScalarInteger(1));

        // GP_GAMMA (2): 1.0
        SET_VECTOR_ELT(gp, GP_GAMMA as R_xlen_t, Rf_ScalarReal(1.0));

        // GP_LTY (3): solid line type
        SET_VECTOR_ELT(gp, GP_LTY as R_xlen_t, Rf_ScalarInteger(0x01010101));

        // GP_LWD (4): line width 1.0
        SET_VECTOR_ELT(gp, GP_LWD as R_xlen_t, Rf_ScalarReal(1.0));

        // GP_CEX (5): character expansion 1.0
        SET_VECTOR_ELT(gp, GP_CEX as R_xlen_t, Rf_ScalarReal(1.0));

        // GP_FONTSIZE (6): 12 pt
        SET_VECTOR_ELT(gp, GP_FONTSIZE as R_xlen_t, Rf_ScalarReal(12.0));

        // GP_LINEHEIGHT (7): 1.2
        SET_VECTOR_ELT(gp, GP_LINEHEIGHT as R_xlen_t, Rf_ScalarReal(1.2));

        // GP_FONT (8): plain (1)
        SET_VECTOR_ELT(gp, GP_FONT as R_xlen_t, Rf_ScalarInteger(1));

        // GP_FONTFAMILY (9): "" (default)
        SET_VECTOR_ELT(
            gp,
            GP_FONTFAMILY as R_xlen_t,
            Rf_mkString(b"\0".as_ptr() as *const c_char),
        );

        // GP_ALPHA (10): 1.0 (fully opaque)
        SET_VECTOR_ELT(gp, GP_ALPHA as R_xlen_t, Rf_ScalarReal(1.0));

        // GP_LINEEND (11): round cap
        SET_VECTOR_ELT(
            gp,
            GP_LINEEND as R_xlen_t,
            Rf_ScalarInteger(ge::GE_ROUND_CAP),
        );

        // GP_LINEJOIN (12): round join
        SET_VECTOR_ELT(
            gp,
            GP_LINEJOIN as R_xlen_t,
            Rf_ScalarInteger(ge::GE_ROUND_JOIN),
        );

        // GP_LINEMITRE (13): 10.0
        SET_VECTOR_ELT(gp, GP_LINEMITRE as R_xlen_t, Rf_ScalarReal(10.0));

        // GP_LEX (14): line expansion 1.0
        SET_VECTOR_ELT(gp, GP_LEX as R_xlen_t, Rf_ScalarReal(1.0));

        // GP_FONTFACE (15): 1 (plain)
        SET_VECTOR_ELT(gp, GP_FONTFACE as R_xlen_t, Rf_ScalarInteger(1));

        setGridStateElement(dd, GSS_GPAR, gp);
        Rf_unprotect(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn make_gpar() -> SEXP {
        unsafe {
            let gp = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP, GP_FONTFACE + 1));

            SET_VECTOR_ELT(gp, GP_FILL as R_xlen_t, Rf_ScalarInteger(1));
            SET_VECTOR_ELT(gp, GP_COL as R_xlen_t, Rf_ScalarInteger(1));
            SET_VECTOR_ELT(gp, GP_GAMMA as R_xlen_t, Rf_ScalarReal(1.0));
            SET_VECTOR_ELT(
                gp,
                GP_LTY as R_xlen_t,
                Rf_mkString(b"dashed\0".as_ptr() as *const c_char),
            );
            SET_VECTOR_ELT(gp, GP_LWD as R_xlen_t, Rf_ScalarReal(1.0));
            SET_VECTOR_ELT(gp, GP_CEX as R_xlen_t, Rf_ScalarReal(1.0));
            SET_VECTOR_ELT(gp, GP_FONTSIZE as R_xlen_t, Rf_ScalarReal(12.0));
            SET_VECTOR_ELT(gp, GP_LINEHEIGHT as R_xlen_t, Rf_ScalarReal(1.2));
            SET_VECTOR_ELT(gp, GP_FONT as R_xlen_t, Rf_ScalarInteger(1));

            let fontfamily = Rf_allocVector(SEXPTYPE::STRSXP, 2);
            SET_STRING_ELT(
                fontfamily,
                0,
                Rf_mkChar(b"sans\0".as_ptr() as *const c_char),
            );
            SET_STRING_ELT(
                fontfamily,
                1,
                Rf_mkChar(b"serif\0".as_ptr() as *const c_char),
            );
            SET_VECTOR_ELT(gp, GP_FONTFAMILY as R_xlen_t, fontfamily);

            SET_VECTOR_ELT(gp, GP_ALPHA as R_xlen_t, Rf_ScalarReal(1.0));
            SET_VECTOR_ELT(gp, GP_LINEEND as R_xlen_t, Rf_ScalarInteger(2));

            let linejoin = Rf_allocVector(SEXPTYPE::INTSXP, 2);
            *INTEGER(linejoin) = 1;
            *INTEGER(linejoin).add(1) = 2;
            SET_VECTOR_ELT(gp, GP_LINEJOIN as R_xlen_t, linejoin);

            SET_VECTOR_ELT(gp, GP_LINEMITRE as R_xlen_t, Rf_ScalarReal(10.0));
            SET_VECTOR_ELT(gp, GP_LEX as R_xlen_t, Rf_ScalarReal(1.0));
            SET_VECTOR_ELT(gp, GP_FONTFACE as R_xlen_t, Rf_ScalarInteger(1));

            gp
        }
    }

    #[test]
    fn test_gp_linetype_parses_names_and_patterns() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let gp = make_gpar();
            SET_VECTOR_ELT(
                gp,
                GP_LTY as R_xlen_t,
                Rf_mkString(b"dotdash\0".as_ptr() as *const c_char),
            );
            assert_eq!(gpLineType(gp, 0), ge::LTY_DOTDASH);

            SET_VECTOR_ELT(
                gp,
                GP_LTY as R_xlen_t,
                Rf_mkString(b"0f\0".as_ptr() as *const c_char),
            );
            assert_eq!(gpLineType(gp, 0), 0x0f);
            Rf_unprotect(1);
        }
    }

    #[test]
    fn test_gp_color_null_falls_back_to_transparent_white() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(gpCol(R_NilValue(), 0), R_TRANWHITE);
            assert_eq!(gpFill(R_NilValue(), 0), R_TRANWHITE);
        }
    }

    #[test]
    fn test_gp_lineend_and_join_delegate_to_engine_parsers() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let gp = make_gpar();

            SET_VECTOR_ELT(
                gp,
                GP_LINEEND as R_xlen_t,
                Rf_mkString(b"butt\0".as_ptr() as *const c_char),
            );
            assert_eq!(gpLineEnd(gp, 0), ge::GE_BUTT_CAP);

            SET_VECTOR_ELT(
                gp,
                GP_LINEJOIN as R_xlen_t,
                Rf_mkString(b"bevel\0".as_ptr() as *const c_char),
            );
            assert_eq!(gpLineJoin(gp, 0), ge::GE_BEVEL_JOIN);
            Rf_unprotect(1);
        }
    }

    #[test]
    fn test_init_and_update_gcontext_set_scalar_flags() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let gp = make_gpar();
            let mut flags = [c_int::MIN; GP_FONTFACE as usize + 1];

            initGContext(
                gp,
                ptr::null(),
                ptr::null_mut(),
                flags.as_mut_ptr(),
                ptr::null(),
            );

            assert_eq!(flags[GP_COL as usize], 1);
            assert_eq!(flags[GP_LWD as usize], 1);
            assert_eq!(flags[GP_FONTFAMILY as usize], 0);
            assert_eq!(flags[GP_LINEJOIN as usize], 0);

            flags.fill(c_int::MIN);
            updateGContext(
                gp,
                0,
                ptr::null(),
                ptr::null_mut(),
                flags.as_mut_ptr(),
                ptr::null(),
            );
            assert_eq!(flags[GP_COL as usize], 1);
            assert_eq!(flags[GP_LWD as usize], 1);
            assert_eq!(flags[GP_FONTFAMILY as usize], 0);
            assert_eq!(flags[GP_LINEJOIN as usize], 0);
            Rf_unprotect(1);
        }
    }
}

/* ==============================
 * R-callable get/set gpar functions
 * ============================== */

pub unsafe fn L_getGPar() -> SEXP {
    unsafe {
        let dd = getDevice();
        if dd.is_null() {
            return R_NilValue();
        }
        gridStateElement(dd, GSS_GPAR)
    }
}

pub unsafe fn L_setGPar(gpars: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        if !dd.is_null() {
            setGridStateElement(dd, GSS_GPAR, gpars);
        }
        R_NilValue()
    }
}

pub unsafe fn L_getGPsaved() -> SEXP {
    unsafe {
        let dd = getDevice();
        if dd.is_null() {
            return R_NilValue();
        }
        gridStateElement(dd, GSS_GPSAVED)
    }
}

pub unsafe fn L_setGPsaved(gpars: SEXP) -> SEXP {
    unsafe {
        let dd = getDevice();
        if !dd.is_null() {
            setGridStateElement(dd, GSS_GPSAVED, gpars);
        }
        R_NilValue()
    }
}
