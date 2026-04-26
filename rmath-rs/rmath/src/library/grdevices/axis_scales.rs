/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/grDevices/src/axis_scales.c
 *
 *  Axis tick mark creation and axis parameter computation.
 */

use crate::main::errors::Rf_error_unimplemented;
use crate::sexp::ffi::SEXP;

fn unsupported(name: &str) -> ! {
    Rf_error_unimplemented(name);
    unreachable!("Rf_error_unimplemented returned");
}

/// R_CreateAtVector - create an axis tick vector.
pub unsafe fn R_CreateAtVector(axp: SEXP, usr: SEXP, nint: SEXP, is_log: SEXP) -> SEXP {
    let _ = (axp, usr, nint, is_log);
    unsupported("grDevices::R_CreateAtVector")
}

/// R_GAxisPars - compute axis parameters (axp, n) from user range.
pub unsafe fn R_GAxisPars(usr: SEXP, is_log: SEXP, nintLog: SEXP) -> SEXP {
    let _ = (usr, is_log, nintLog);
    unsupported("grDevices::R_GAxisPars")
}
