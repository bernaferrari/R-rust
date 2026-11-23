/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1997--2025  The R Core Team
 *  Copyright (C) 2002--2009  The R Foundation
 *  Copyright (C) 1995, 1996  Robert Gentleman and Ross Ihaka
 *
 *  Ported from r-source/src/library/graphics/src/plot.c
 *
 *  Base plot initialization and argument normalization shared with plot3d.
 */

use std::ffi::c_void;
use std::os::raw::{c_int, c_uint};

use super::graphics::{GMapWin2Fig, GNewPlot, GRecording, GScale, GSetState};
use crate::main::coerce::coerceVector;
use crate::main::errors::Rf_error_cstr;
use crate::mainutils::bind::isList;
use crate::mainutils::colors::RGBpar3;
use crate::mainutils::engine::{GE_LTYpar, GErecordGraphicOperation};
use crate::sexp::accessors::{
    CAR, CDR, INTEGER_ELT, REAL_ELT, Rf_isNull, SET_INTEGER_ELT, SET_REAL_ELT,
};
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarReal, Rf_allocVector, Rf_length as length,
};
use crate::sexp::ffi::{NA_REAL, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

pub fn FixupLty(lty: SEXP, dflt: c_int) -> SEXP {
    let n = length(lty);
    if n == 0 {
        return Rf_ScalarInteger(dflt);
    }
    let _input = protect(lty);
    let ans = Rf_allocVector(SEXPTYPE::INTSXP, n);
    let _answer = protect(ans);
    for i in 0..n {
        let code = GE_LTYpar(lty, i) as c_int;
        SET_INTEGER_ELT(ans, i, code);
    }
    ans
}

pub fn FixupLwd(lwd: SEXP, dflt: f64) -> SEXP {
    if length(lwd) == 0 {
        return Rf_ScalarReal(dflt);
    }
    let _input = protect(lwd);
    let lwd = coerceVector(lwd, SEXPTYPE::REALSXP.into());
    let _lwd = protect(lwd);
    let n = length(lwd);
    let ans = Rf_allocVector(SEXPTYPE::REALSXP, n);
    let _answer = protect(ans);
    for i in 0..n {
        let width = REAL_ELT(lwd, i);
        SET_REAL_ELT(ans, i, if width < 0.0 { NA_REAL } else { width });
    }
    ans
}

pub fn FixupCol(col: SEXP, dflt: c_uint) -> SEXP {
    let n = length(col);
    if n == 0 {
        return Rf_ScalarInteger(dflt as c_int);
    }
    let _input = protect(col);
    let ans = Rf_allocVector(SEXPTYPE::INTSXP, n);
    let _answer = protect(ans);
    let mut cur = col;
    let list = isList(col) != 0;
    for i in 0..n {
        let (value, index) = if list { (CAR(cur), 0) } else { (col, i) };
        let _value = protect(value);
        // SAFETY: the rooted value remains live throughout the registered color callback.
        let color = unsafe { RGBpar3(value.cast::<c_void>(), index, dflt) };
        SET_INTEGER_ELT(ans, i, color as c_int);
        if list {
            cur = CDR(cur);
        }
    }
    ans
}

pub fn FixupVFont(vfont: SEXP) -> SEXP {
    if Rf_isNull(vfont) != 0 {
        return R_NilValue();
    }
    let _input = protect(vfont);
    let vf = coerceVector(vfont, SEXPTYPE::INTSXP.into());
    let _vf = protect(vf);
    if length(vf) != 2 {
        Rf_error_cstr(c"invalid 'vfont' value");
    }
    let typeface = INTEGER_ELT(vf, 0);
    let maxindex = match typeface {
        1 => 7,         // serif
        2 | 7 => 4,     // sans serif, serif symbol
        3 => 3,         // script
        4 | 5 | 6 => 1, // gothic
        8 => 2,         // sans symbol
        _ => {
            Rf_error_cstr(c"invalid 'vfont' value");
            0
        }
    };
    let fontindex = INTEGER_ELT(vf, 1);
    if !(1..=maxindex).contains(&fontindex) {
        Rf_error_cstr(c"invalid 'vfont' value");
    }
    let ans = Rf_allocVector(SEXPTYPE::INTSXP, 2);
    SET_INTEGER_ELT(ans, 0, typeface);
    SET_INTEGER_ELT(ans, 1, fontindex);
    ans
}

/* ========================================================================
 * C_plot_new -- create a new plot frame (plot.new())
 * ======================================================================== */

pub fn C_plot_new(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let dd: *mut c_void = crate::library::grdevices::device_registry::GEcurrentDevice().cast();
    if dd.is_null() {
        Rf_error_cstr(c"no active graphics device");
    }
    let recording = GRecording(call, dd);
    let dd = GNewPlot(recording);
    if dd.is_null() {
        Rf_error_cstr(c"no active graphics device");
    }
    super::par::with_dpar_mut(dd, |dp| {
        dp.xlog = 0;
        dp.ylog = 0;
    });
    super::par::with_gpar_mut(dd, |gp| {
        gp.xlog = 0;
        gp.ylog = 0;
    });

    GScale(0.0, 1.0, 1, dd);
    GScale(0.0, 1.0, 2, dd);
    GMapWin2Fig(dd);
    GSetState(1, dd);

    if GRecording(call, dd) != 0 {
        GErecordGraphicOperation(op, args, dd);
    }
    R_NilValue()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn fixup_lwd_reads_compact_widths_without_materializing() {
        let session = RSession::new_for_gc_tests();
        let widths = session.compact_real_sequence(-1.0, 1.0, 3).unwrap();
        session.with_active(|| {
            let result = FixupLwd(widths.as_raw(), 1.0);
            let result = session.sexp(result).unwrap();
            assert!(crate::sexp::ffi::R_IsNA(result.real_elt(0).unwrap()));
            assert_eq!(result.real_elt(1).unwrap(), 0.0);
            assert_eq!(result.real_elt(2).unwrap(), 1.0);
            assert!(!crate::sexp::altrep::is_materialized(&widths));
        });
    }

    #[test]
    fn fixup_vfont_accepts_both_sans_symbol_fonts() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            for index in [1, 2] {
                let input = factory
                    .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 2)))
                    .unwrap();
                SET_INTEGER_ELT(input.as_raw(), 0, 8);
                SET_INTEGER_ELT(input.as_raw(), 1, index);
                let output = session.sexp(FixupVFont(input.as_raw())).unwrap();
                assert_eq!(output.integer_elt(0), Some(8));
                assert_eq!(output.integer_elt(1), Some(index));
            }
        });
    }
}
