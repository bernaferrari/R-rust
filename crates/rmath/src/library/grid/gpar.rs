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
use crate::{mainutils::colors::RGBpar3, mainutils::engine as ge};

use super::grid::getDevice;
use super::state::{gridStateElement, setGridStateElement};
use super::{
    alloc_vector, char_ptr, integer_elt, mk_string, protect, real_elt, scalar_integer,
    scalar_real, set_vector_elt, string_elt, vector_elt,
};
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

pub fn gpFontSizeSXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_FONTSIZE as R_xlen_t)
}

pub fn gpFontSize(gp: SEXP, i: c_int) -> c_double {
    let fontsize = gpFontSizeSXP(gp);
    let len = LENGTH(fontsize);
    real_elt(fontsize, (i % len) as R_xlen_t)
}

pub fn gpLineHeightSXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_LINEHEIGHT as R_xlen_t)
}

pub fn gpLineHeight(gp: SEXP, i: c_int) -> c_double {
    let lineheight = gpLineHeightSXP(gp);
    let len = LENGTH(lineheight);
    real_elt(lineheight, (i % len) as R_xlen_t)
}

pub fn gpColSXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_COL as R_xlen_t)
}

pub fn gpCol(gp: SEXP, i: c_int) -> c_int {
    if gp.is_null() || Rf_isNull(gp) != 0 {
        return R_TRANWHITE;
    }
    let col = gpColSXP(gp);
    if Rf_isNull(col) != 0 {
        R_TRANWHITE
    } else {
        // SAFETY: `col` is a live checked gpar element and `i` is the
        // caller's recycle index, as in the original C.
        unsafe { RGBpar3(col as *mut c_void, i, ge::R_TRANWHITE) as c_int }
    }
}

pub fn gpFillSXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_FILL as R_xlen_t)
}

pub fn gpFill(gp: SEXP, i: c_int) -> c_int {
    if gp.is_null() || Rf_isNull(gp) != 0 {
        return R_TRANWHITE;
    }
    let fill = gpFillSXP(gp);
    if Rf_isNull(fill) != 0 {
        R_TRANWHITE
    } else {
        // SAFETY: `fill` is a live checked gpar element and `i` is the
        // caller's recycle index, as in the original C.
        unsafe { RGBpar3(fill as *mut c_void, i, ge::R_TRANWHITE) as c_int }
    }
}

pub fn gpGammaSXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_GAMMA as R_xlen_t)
}

pub fn gpGamma(gp: SEXP, i: c_int) -> c_double {
    let gamma = gpGammaSXP(gp);
    let len = LENGTH(gamma);
    real_elt(gamma, (i % len) as R_xlen_t)
}

pub fn gpLineTypeSXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_LTY as R_xlen_t)
}

pub fn gpLineType(gp: SEXP, i: c_int) -> c_int {
    let linetype = gpLineTypeSXP(gp);
    // SAFETY: `linetype` is a live checked gpar element and `i` is the
    // caller's recycle index, as in the original C.
    unsafe { ge::GE_LTYpar(linetype, i) as c_int }
}

pub fn gpLineWidthSXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_LWD as R_xlen_t)
}

pub fn gpLineWidth(gp: SEXP, i: c_int) -> c_double {
    let linewidth = gpLineWidthSXP(gp);
    let len = LENGTH(linewidth);
    real_elt(linewidth, (i % len) as R_xlen_t)
}

pub fn gpCexSXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_CEX as R_xlen_t)
}

pub fn gpCex(gp: SEXP, i: c_int) -> c_double {
    let cex = gpCexSXP(gp);
    let len = LENGTH(cex);
    real_elt(cex, (i % len) as R_xlen_t)
}

pub fn gpFontSXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_FONT as R_xlen_t)
}

pub fn gpFont(gp: SEXP, i: c_int) -> c_int {
    let font = gpFontSXP(gp);
    let len = LENGTH(font);
    integer_elt(font, (i % len) as R_xlen_t)
}

pub fn gpFontFamilySXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_FONTFAMILY as R_xlen_t)
}

pub fn gpFontFamily(gp: SEXP, i: c_int) -> *const c_char {
    let fontfamily = gpFontFamilySXP(gp);
    let len = LENGTH(fontfamily);
    char_ptr(string_elt(fontfamily, (i % len) as R_xlen_t))
}

pub fn gpAlphaSXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_ALPHA as R_xlen_t)
}

pub fn gpAlpha(gp: SEXP, i: c_int) -> c_double {
    let alpha = gpAlphaSXP(gp);
    let len = LENGTH(alpha);
    real_elt(alpha, (i % len) as R_xlen_t)
}

pub fn gpLineEndSXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_LINEEND as R_xlen_t)
}

pub fn gpLineEnd(gp: SEXP, i: c_int) -> c_int {
    let lineend = gpLineEndSXP(gp);
    // SAFETY: `lineend` is a live checked gpar element and `i` is the
    // caller's recycle index, as in the original C.
    unsafe { ge::GE_LENDpar(lineend, i) }
}

pub fn gpLineJoinSXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_LINEJOIN as R_xlen_t)
}

pub fn gpLineJoin(gp: SEXP, i: c_int) -> c_int {
    let linejoin = gpLineJoinSXP(gp);
    // SAFETY: `linejoin` is a live checked gpar element and `i` is the
    // caller's recycle index, as in the original C.
    unsafe { ge::GE_LJOINpar(linejoin, i) }
}

pub fn gpLineMitreSXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_LINEMITRE as R_xlen_t)
}

pub fn gpLineMitre(gp: SEXP, i: c_int) -> c_double {
    let linemitre = gpLineMitreSXP(gp);
    let len = LENGTH(linemitre);
    real_elt(linemitre, (i % len) as R_xlen_t)
}

pub fn gpLexSXP(gp: SEXP) -> SEXP {
    vector_elt(gp, GP_LEX as R_xlen_t)
}

pub fn gpLex(gp: SEXP, i: c_int) -> c_double {
    let lex = gpLexSXP(gp);
    let len = LENGTH(lex);
    real_elt(lex, (i % len) as R_xlen_t)
}

/* ==============================
 * "gpIsScalar" variants (set scalar flag)
 * ============================== */

pub fn gpFontSize2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> c_double {
    let fontsize = gpFontSizeSXP(gp);
    gpIsScalar[GP_FONTSIZE as usize] = if LENGTH(fontsize) == 1 { 1 } else { 0 };
    real_elt(fontsize, (i % LENGTH(fontsize)) as R_xlen_t)
}

pub fn gpLineHeight2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> c_double {
    let lineheight = gpLineHeightSXP(gp);
    gpIsScalar[GP_LINEHEIGHT as usize] = if LENGTH(lineheight) == 1 { 1 } else { 0 };
    real_elt(lineheight, (i % LENGTH(lineheight)) as R_xlen_t)
}

pub fn gpCol2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> c_int {
    let col = gpColSXP(gp);
    gpIsScalar[GP_COL as usize] = if LENGTH(col) == 1 { 1 } else { 0 };
    gpCol(gp, i)
}

pub fn gpFill2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> c_int {
    let fill = gpFillSXP(gp);
    gpIsScalar[GP_FILL as usize] = if LENGTH(fill) == 1 { 1 } else { 0 };
    gpFill(gp, i)
}

pub fn gpGamma2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> c_double {
    let gamma = gpGammaSXP(gp);
    gpIsScalar[GP_GAMMA as usize] = if LENGTH(gamma) == 1 { 1 } else { 0 };
    real_elt(gamma, (i % LENGTH(gamma)) as R_xlen_t)
}

pub fn gpLineType2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> c_int {
    let linetype = gpLineTypeSXP(gp);
    gpIsScalar[GP_LTY as usize] = if LENGTH(linetype) == 1 { 1 } else { 0 };
    gpLineType(gp, i)
}

pub fn gpLineWidth2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> c_double {
    let linewidth = gpLineWidthSXP(gp);
    gpIsScalar[GP_LWD as usize] = if LENGTH(linewidth) == 1 { 1 } else { 0 };
    real_elt(linewidth, (i % LENGTH(linewidth)) as R_xlen_t)
}

pub fn gpCex2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> c_double {
    let cex = gpCexSXP(gp);
    gpIsScalar[GP_CEX as usize] = if LENGTH(cex) == 1 { 1 } else { 0 };
    real_elt(cex, (i % LENGTH(cex)) as R_xlen_t)
}

pub fn gpFont2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> c_int {
    let font = gpFontSXP(gp);
    gpIsScalar[GP_FONT as usize] = if LENGTH(font) == 1 { 1 } else { 0 };
    integer_elt(font, (i % LENGTH(font)) as R_xlen_t)
}

pub fn gpFontFamily2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> *const c_char {
    let fontfamily = gpFontFamilySXP(gp);
    gpIsScalar[GP_FONTFAMILY as usize] = if LENGTH(fontfamily) == 1 { 1 } else { 0 };
    char_ptr(string_elt(fontfamily, (i % LENGTH(fontfamily)) as R_xlen_t))
}

pub fn gpAlpha2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> c_double {
    let alpha = gpAlphaSXP(gp);
    gpIsScalar[GP_ALPHA as usize] = if LENGTH(alpha) == 1 { 1 } else { 0 };
    real_elt(alpha, (i % LENGTH(alpha)) as R_xlen_t)
}

pub fn gpLineEnd2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> c_int {
    let lineend = gpLineEndSXP(gp);
    gpIsScalar[GP_LINEEND as usize] = if LENGTH(lineend) == 1 { 1 } else { 0 };
    gpLineEnd(gp, i)
}

pub fn gpLineJoin2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> c_int {
    let linejoin = gpLineJoinSXP(gp);
    gpIsScalar[GP_LINEJOIN as usize] = if LENGTH(linejoin) == 1 { 1 } else { 0 };
    gpLineJoin(gp, i)
}

pub fn gpLineMitre2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> c_double {
    let linemitre = gpLineMitreSXP(gp);
    gpIsScalar[GP_LINEMITRE as usize] = if LENGTH(linemitre) == 1 { 1 } else { 0 };
    real_elt(linemitre, (i % LENGTH(linemitre)) as R_xlen_t)
}

pub fn gpLex2(gp: SEXP, i: c_int, gpIsScalar: &mut [c_int]) -> c_double {
    let lex = gpLexSXP(gp);
    gpIsScalar[GP_LEX as usize] = if LENGTH(lex) == 1 { 1 } else { 0 };
    real_elt(lex, (i % LENGTH(lex)) as R_xlen_t)
}

/* ==============================
 * resolveGPar -- partial helper
 * ============================== */

pub fn resolveGPar(gp: SEXP, _byName: c_int) -> SEXP {
    gp
}

/* ==============================
 * gcontextFromgpar -- partial helper
 * ============================== */

pub fn gcontextFromgpar(_gp: SEXP, _i: c_int, _gc: pGEcontext, _dd: pGEDevDesc) {
    let gp = resolveGPar(_gp, 0);
    if gp.is_null() || Rf_isNull(gp) != 0 || _gc.is_null() {
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

/* ==============================
 * initGContext -- partial helper
 * ============================== */

fn set_gpar_scalar_flags(gp: SEXP, gpIsScalar: &mut [c_int]) {
    gpIsScalar.fill(0);

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
        if let Some(flag) = gpIsScalar.get_mut(slot as usize) {
            *flag = if LENGTH(value) == 1 { 1 } else { 0 };
        }
    }
}

pub fn initGContext(
    gp: SEXP,
    gc: pGEcontext,
    dd: pGEDevDesc,
    gpIsScalar: &mut [c_int],
    gcCache: pGEcontext,
) {
    gcontextFromgpar(gp, 0, gc, dd);
    set_gpar_scalar_flags(gp, gpIsScalar);
    let _ = gcCache;
}

/* ==============================
 * updateGContext -- partial helper
 * ============================== */

pub fn updateGContext(
    gp: SEXP,
    i: c_int,
    gc: pGEcontext,
    dd: pGEDevDesc,
    gpIsScalar: &mut [c_int],
    gcCache: pGEcontext,
) {
    gcontextFromgpar(gp, i, gc, dd);
    set_gpar_scalar_flags(gp, gpIsScalar);
    let _ = gcCache;
}

/* ==============================
 * initGPar -- initializes grid gpar for a device
 * ============================== */

pub fn initGPar(dd: pGEDevDesc) {
    let gp = alloc_vector(SEXPTYPE::VECSXP, GP_FONTFACE + 1);
    let _gp_guard = protect(gp);

    // GP_FILL (0): transparent white
    set_vector_elt(gp, GP_FILL as R_xlen_t, scalar_integer(R_TRANWHITE));

    // GP_COL (1): black (R colour index 1)
    set_vector_elt(gp, GP_COL as R_xlen_t, scalar_integer(1));

    // GP_GAMMA (2): 1.0
    set_vector_elt(gp, GP_GAMMA as R_xlen_t, scalar_real(1.0));

    // GP_LTY (3): solid line type
    set_vector_elt(gp, GP_LTY as R_xlen_t, scalar_integer(0x01010101));

    // GP_LWD (4): line width 1.0
    set_vector_elt(gp, GP_LWD as R_xlen_t, scalar_real(1.0));

    // GP_CEX (5): character expansion 1.0
    set_vector_elt(gp, GP_CEX as R_xlen_t, scalar_real(1.0));

    // GP_FONTSIZE (6): 12 pt
    set_vector_elt(gp, GP_FONTSIZE as R_xlen_t, scalar_real(12.0));

    // GP_LINEHEIGHT (7): 1.2
    set_vector_elt(gp, GP_LINEHEIGHT as R_xlen_t, scalar_real(1.2));

    // GP_FONT (8): plain (1)
    set_vector_elt(gp, GP_FONT as R_xlen_t, scalar_integer(1));

    // GP_FONTFAMILY (9): "" (default)
    set_vector_elt(
        gp,
        GP_FONTFAMILY as R_xlen_t,
        mk_string(""),
    );

    // GP_ALPHA (10): 1.0 (fully opaque)
    set_vector_elt(gp, GP_ALPHA as R_xlen_t, scalar_real(1.0));

    // GP_LINEEND (11): round cap
    set_vector_elt(
        gp,
        GP_LINEEND as R_xlen_t,
        scalar_integer(ge::GE_ROUND_CAP),
    );

    // GP_LINEJOIN (12): round join
    set_vector_elt(
        gp,
        GP_LINEJOIN as R_xlen_t,
        scalar_integer(ge::GE_ROUND_JOIN),
    );

    // GP_LINEMITRE (13): 10.0
    set_vector_elt(gp, GP_LINEMITRE as R_xlen_t, scalar_real(10.0));

    // GP_LEX (14): line expansion 1.0
    set_vector_elt(gp, GP_LEX as R_xlen_t, scalar_real(1.0));

    // GP_FONTFACE (15): 1 (plain)
    set_vector_elt(gp, GP_FONTFACE as R_xlen_t, scalar_integer(1));

    setGridStateElement(dd, GSS_GPAR, gp);
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn make_gpar() -> SEXP {
        unsafe {
            let gp = Rf_allocVector(SEXPTYPE::VECSXP, GP_FONTFACE + 1);
            let _gp_guard = protect(gp);

            set_vector_elt(gp, GP_FILL as R_xlen_t, scalar_integer(1));
            set_vector_elt(gp, GP_COL as R_xlen_t, scalar_integer(1));
            set_vector_elt(gp, GP_GAMMA as R_xlen_t, scalar_real(1.0));
            set_vector_elt(
                gp,
                GP_LTY as R_xlen_t,
                Rf_mkString(b"dashed\0".as_ptr() as *const c_char),
            );
            set_vector_elt(gp, GP_LWD as R_xlen_t, scalar_real(1.0));
            set_vector_elt(gp, GP_CEX as R_xlen_t, scalar_real(1.0));
            set_vector_elt(gp, GP_FONTSIZE as R_xlen_t, scalar_real(12.0));
            set_vector_elt(gp, GP_LINEHEIGHT as R_xlen_t, scalar_real(1.2));
            set_vector_elt(gp, GP_FONT as R_xlen_t, scalar_integer(1));

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
            set_vector_elt(gp, GP_FONTFAMILY as R_xlen_t, fontfamily);

            set_vector_elt(gp, GP_ALPHA as R_xlen_t, scalar_real(1.0));
            set_vector_elt(gp, GP_LINEEND as R_xlen_t, scalar_integer(2));

            let linejoin = Rf_allocVector(SEXPTYPE::INTSXP, 2);
            *INTEGER(linejoin) = 1;
            *INTEGER(linejoin).add(1) = 2;
            set_vector_elt(gp, GP_LINEJOIN as R_xlen_t, linejoin);

            set_vector_elt(gp, GP_LINEMITRE as R_xlen_t, scalar_real(10.0));
            set_vector_elt(gp, GP_LEX as R_xlen_t, scalar_real(1.0));
            set_vector_elt(gp, GP_FONTFACE as R_xlen_t, scalar_integer(1));

            gp
        }
    }

    #[test]
    fn test_gp_linetype_parses_names_and_patterns() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let gp = make_gpar();
            let _gp_guard = protect(gp);
            set_vector_elt(
                gp,
                GP_LTY as R_xlen_t,
                Rf_mkString(b"dotdash\0".as_ptr() as *const c_char),
            );
            assert_eq!(gpLineType(gp, 0), ge::LTY_DOTDASH);

            set_vector_elt(
                gp,
                GP_LTY as R_xlen_t,
                Rf_mkString(b"0f\0".as_ptr() as *const c_char),
            );
            assert_eq!(gpLineType(gp, 0), 0x0f);
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
            let _gp_guard = protect(gp);

            set_vector_elt(
                gp,
                GP_LINEEND as R_xlen_t,
                Rf_mkString(b"butt\0".as_ptr() as *const c_char),
            );
            assert_eq!(gpLineEnd(gp, 0), ge::GE_BUTT_CAP);

            set_vector_elt(
                gp,
                GP_LINEJOIN as R_xlen_t,
                Rf_mkString(b"bevel\0".as_ptr() as *const c_char),
            );
            assert_eq!(gpLineJoin(gp, 0), ge::GE_BEVEL_JOIN);
        }
    }

    #[test]
    fn test_init_and_update_gcontext_set_scalar_flags() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let gp = make_gpar();
            let _gp_guard = protect(gp);
            let mut flags = [c_int::MIN; GP_FONTFACE as usize + 1];

            initGContext(
                gp,
                ptr::null(),
                ptr::null_mut(),
                &mut flags,
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
                &mut flags,
                ptr::null(),
            );
            assert_eq!(flags[GP_COL as usize], 1);
            assert_eq!(flags[GP_LWD as usize], 1);
            assert_eq!(flags[GP_FONTFAMILY as usize], 0);
            assert_eq!(flags[GP_LINEJOIN as usize], 0);
        }
    }
}

/* ==============================
 * R-callable get/set gpar functions
 * ============================== */

pub fn L_getGPar() -> SEXP {
    // SAFETY: device registry bridge; a null result means no open device.
    let dd = unsafe { getDevice() };
    if dd.is_null() {
        return R_NilValue();
    }
    gridStateElement(dd, GSS_GPAR)
}

pub fn L_setGPar(gpars: SEXP) -> SEXP {
    // SAFETY: device registry bridge; a null result means no open device.
    let dd = unsafe { getDevice() };
    if !dd.is_null() {
        setGridStateElement(dd, GSS_GPAR, gpars);
    }
    R_NilValue()
}

pub fn L_getGPsaved() -> SEXP {
    // SAFETY: device registry bridge; a null result means no open device.
    let dd = unsafe { getDevice() };
    if dd.is_null() {
        return R_NilValue();
    }
    gridStateElement(dd, GSS_GPSAVED)
}

pub fn L_setGPsaved(gpars: SEXP) -> SEXP {
    // SAFETY: device registry bridge; a null result means no open device.
    let dd = unsafe { getDevice() };
    if !dd.is_null() {
        setGridStateElement(dd, GSS_GPSAVED, gpars);
    }
    R_NilValue()
}
