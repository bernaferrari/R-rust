#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/registration.c -- native routine registration.
//!
//! Implements `R_init_base` for registering .Call and .Fortran routines
//! that are accessible from S code via the R executable.

use std::os::raw::c_int;

pub use crate::mainutils::rdynload::{
    DllInfo, R_CMethodDef, R_CallMethodDef, R_ExternalMethodDef, R_FortranMethodDef,
};

// ---------------------------------------------------------------------------
// R_registerRoutines, R_useDynamicSymbols, R_forceSymbols
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_registerRoutines(
    dll: *mut DllInfo,
    c: *const R_CMethodDef,
    call: *const R_CallMethodDef,
    fortran: *const R_FortranMethodDef,
    external: *const R_ExternalMethodDef,
) -> c_int {
    unsafe { crate::mainutils::rdynload::R_registerRoutines(dll, c, call, fortran, external) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_useDynamicSymbols(dll: *mut DllInfo, value: c_int) -> c_int {
    unsafe { crate::mainutils::rdynload::R_useDynamicSymbols(dll, value != 0) as c_int }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_forceSymbols(dll: *mut DllInfo, value: c_int) -> c_int {
    unsafe { crate::mainutils::rdynload::R_forceSymbols(dll, value != 0) as c_int }
}

/// Keep the public registration ABI reachable when `rmath` is linked as an
/// `rlib` into an embedding executable. Native packages resolve these names
/// through the process loader rather than through Rust references.
pub(crate) fn retain_native_registration_exports() {
    std::hint::black_box(R_registerRoutines as *const ());
    std::hint::black_box(R_useDynamicSymbols as *const ());
    std::hint::black_box(R_forceSymbols as *const ());
}

// ---------------------------------------------------------------------------
// R_init_base
// ---------------------------------------------------------------------------

/// Initialize the base package's registered routines.
pub unsafe fn R_init_base(_dll: *mut DllInfo) {
    // In the full R implementation, this registers:
    // - callMethods: R_addTaskCallback, R_getTaskCallbackNames, R_removeTaskCallback
    // - fortranMethods: dqrcf, dqrdc2, dqrqty, dqrqy, dqrrsd, dqrxb, dtrco
    // these are stubs since the routines themselves aren't ported.
}
