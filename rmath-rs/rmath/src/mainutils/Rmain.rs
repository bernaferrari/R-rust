#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/Rmain.c — main entry point stub.
//!
//! This is a minimal stub since the actual main() is provided by the
//! embedding application or the unix/system module.

use std::os::raw::c_int;

use crate::sexp::instance::with_required_current_instance;

/// Set the running-as-main-program flag.
pub unsafe fn R_SetRunningAsMainProgram(v: c_int) {
    with_required_current_instance(|instance| {
        instance.startup_state.running_as_main_program = v;
    });
}

/// Get the running-as-main-program flag.
pub unsafe fn R_RunningAsMainProgram() -> c_int {
    with_required_current_instance(|instance| instance.startup_state.running_as_main_program)
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

#[cfg(test)]
mod tests {
    use crate::sexp::instance::{RInstance, clear_current_instance, set_current_instance};

    use super::*;

    #[test]
    fn running_as_main_is_session_local() {
        unsafe {
            let mut first = RInstance::new();
            set_current_instance(&mut first);
            R_SetRunningAsMainProgram(1);
            assert_eq!(R_RunningAsMainProgram(), 1);

            let mut second = RInstance::new();
            set_current_instance(&mut second);
            assert_eq!(R_RunningAsMainProgram(), 0);
            R_SetRunningAsMainProgram(2);
            assert_eq!(R_RunningAsMainProgram(), 2);

            set_current_instance(&mut first);
            assert_eq!(R_RunningAsMainProgram(), 1);

            clear_current_instance();
        }
    }
}
