
/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported from r-source/src/library/methods/src/init.c
 *
 *  Registration table for methods package.
 *  In the monolithic crate, R_registerRoutines is a no-op since
 *  all symbols are already visible.
 */

/// R_init_methods - registration entry point (stub).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_init_methods(_dll: *mut std::ffi::c_void) {
    // R_registerRoutines, R_useDynamicSymbols, R_forceSymbols
    // are no-ops in the monolithic crate
}
