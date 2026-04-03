#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/registration.c -- native routine registration.
//!
//! Implements `R_init_base` for registering .Call and .Fortran routines
//! that are accessible from S code via the R executable.

use std::os::raw::{c_int, c_void};

// ---------------------------------------------------------------------------
// Stub: R_registerRoutines, R_useDynamicSymbols, R_forceSymbols
// ---------------------------------------------------------------------------

/// Opaque DllInfo type (placeholder).
#[repr(C)]
pub struct DllInfo {
    _private: [u8; 0],
}

// no_mangle removed (duplicate)
pub unsafe extern "C" fn R_registerRoutines(
    _dll: *mut DllInfo,
    _c: *const c_void,
    _call: *const c_void,
    _fortran: *const c_void,
    _external: *const c_void,
) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_useDynamicSymbols(_dll: *mut DllInfo, _value: c_int) {}

// no_mangle removed (duplicate)
pub unsafe extern "C" fn R_forceSymbols(_dll: *mut DllInfo, _value: c_int) {}

// ---------------------------------------------------------------------------
// R_init_base
// ---------------------------------------------------------------------------

/// Initialize the base package's registered routines.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_init_base(_dll: *mut DllInfo) {
    // In the full R implementation, this registers:
    // - callMethods: R_addTaskCallback, R_getTaskCallbackNames, R_removeTaskCallback
    // - fortranMethods: dqrcf, dqrdc2, dqrqty, dqrqy, dqrrsd, dqrxb, dtrco
    // For now, these are stubs since the routines themselves aren't ported.
}
