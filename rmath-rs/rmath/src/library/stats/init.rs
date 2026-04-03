#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

//! Stats package initialization (registration tables)
//! Port of r-source/src/library/stats/src/init.c
//!
//! This file defines the R_CMethodDef, R_CallMethodDef, R_FortranMethodDef,
//! and R_ExternalMethodDef tables used for .Call/.C/.Fortran registration.
//! In the Rust port, these are primarily informational since we link statically.

use std::os::raw::c_void;

// Registration table structures (informational, matching R's init.c layout)
// These are kept for reference but are not used at runtime in the static library.

#[repr(C)]
pub struct R_CMethodDef {
    pub name: *const i8,
    pub func: *const c_void,
    pub num_args: c_int,
}

#[repr(C)]
pub struct R_CallMethodDef {
    pub name: *const i8,
    pub func: *const c_void,
    pub num_args: c_int,
}

#[repr(C)]
pub struct R_ExternalMethodDef {
    pub name: *const i8,
    pub func: *const c_void,
    pub num_args: c_int,
}

use std::os::raw::c_int;

/// This is a placeholder for the R_init_stats function.
/// In the Rust port, this is not needed since we link statically,
/// but we keep the symbol for ABI compatibility.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_init_stats(_dll: *mut c_void) {
    // No-op in the Rust static library port.
    // The registration tables from the C version are not needed here
    // since all functions are linked directly.
}
