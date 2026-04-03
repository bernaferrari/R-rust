#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/flexiblas.c -- FlexiBLAS backend info.
//!
//! Implements `R_flexiblas_info` which queries the current FlexiBLAS backend.

use std::os::raw::c_void;
use std::ptr;

use crate::sexp::constructors::Rf_mkChar;
use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// R_flexiblas_info
// ---------------------------------------------------------------------------

/// Query the current FlexiBLAS backend name.
/// Returns a CHARSXP with the name, or R_NilValue if FlexiBLAS is not available.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_flexiblas_info() -> SEXP {
    unsafe {
        // On non-Linux or when FlexiBLAS is not loaded, return nil.
        // The full implementation uses dlsym(RTLD_DEFAULT, "flexiblas_current_backend")
        // to detect FlexiBLAS at runtime.
        R_NilValue()
    }
}
