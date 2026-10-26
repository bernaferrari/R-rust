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

/// Initialize the stats package's runtime-local cross-package callables.
/// C and Fortran routine tables are metadata in this statically linked port.
///
/// # Safety
/// Must run with an active initialized R instance, as required by
/// `R_RegisterCCallable`.
pub unsafe fn R_init_stats(_dll: *mut c_void) {
    // The C and Fortran registration tables are not used by the static port,
    // but stats' typed cross-package callable is part of its public contract.
    let package = c"stats";
    let name = c"rcont2";
    let callable: unsafe extern "C" fn(
        c_int,
        c_int,
        *const c_int,
        *const c_int,
        c_int,
        *const f64,
        *mut c_int,
        *mut c_int,
    ) = super::rcont::rcont2_c_callable;
    let erased = unsafe {
        std::mem::transmute::<
            unsafe extern "C" fn(
                c_int,
                c_int,
                *const c_int,
                *const c_int,
                c_int,
                *const f64,
                *mut c_int,
                *mut c_int,
            ),
            unsafe extern "C" fn(),
        >(callable)
    };
    unsafe {
        crate::mainutils::rdynload::R_RegisterCCallable(
            package.as_ptr(),
            name.as_ptr(),
            Some(erased),
        );
    }
}
