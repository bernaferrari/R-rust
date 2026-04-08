#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/Rmain.c — main entry point stub.
//!
//! This is a minimal stub since the actual main() is provided by the
//! embedding application or the unix/system module.

use std::cell::Cell;
use std::os::raw::c_int;

thread_local! { static R_running_as_main_program: Cell<c_int> = Cell::new(0); }

/// Set the running-as-main-program flag.
pub unsafe fn R_SetRunningAsMainProgram(v: c_int) {
    R_running_as_main_program.with(|v_| v_.set(v));
}

/// Get the running-as-main-program flag.
pub unsafe fn R_RunningAsMainProgram() -> c_int {
    R_running_as_main_program.with(|v| v.get())
}

/// FORTRAN compatibility stub.
pub unsafe fn MAIN_(_ac: c_int, _av: *mut *mut std::os::raw::c_char) -> c_int {
    0
}

/// FORTRAN compatibility stub.
pub unsafe fn MAIN__(_ac: c_int, _av: *mut *mut std::os::raw::c_char) -> c_int {
    0
}

/// FORTRAN compatibility stub.
pub unsafe fn __main(_ac: c_int, _av: *mut *mut std::os::raw::c_char) -> c_int {
    0
}
