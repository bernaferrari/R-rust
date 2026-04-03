#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/Rmain.c — main entry point stub.
//!
//! This is a minimal stub since the actual main() is provided by the
//! embedding application or the unix/system module.

use std::os::raw::c_int;

/// Flag indicating R is running as the main program.
static mut R_running_as_main_program: c_int = 0;

/// Set the running-as-main-program flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetRunningAsMainProgram(v: c_int) {
    unsafe {
        R_running_as_main_program = v;
    }
}

/// Get the running-as-main-program flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_RunningAsMainProgram() -> c_int {
    unsafe { R_running_as_main_program }
}

/// FORTRAN compatibility stub.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn MAIN_(_ac: c_int, _av: *mut *mut std::os::raw::c_char) -> c_int {
    0
}

/// FORTRAN compatibility stub.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn MAIN__(_ac: c_int, _av: *mut *mut std::os::raw::c_char) -> c_int {
    0
}

/// FORTRAN compatibility stub.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __main(_ac: c_int, _av: *mut *mut std::os::raw::c_char) -> c_int {
    0
}
