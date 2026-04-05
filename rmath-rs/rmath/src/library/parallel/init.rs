
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/parallel/src/init.c
 *
 *  Registration table for the parallel package.
 */

use std::os::raw::c_void;

/// R_init_parallel - register routines for the parallel package (stub).
/// In the monolithic Rust crate, all symbols are already visible,
/// so no registration is needed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_init_parallel(_dll: *mut c_void) {
    // no-op: all symbols are already visible in the monolithic crate
}
