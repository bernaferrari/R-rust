#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/unix/Rembedded.c -- embedded R initialization.
//!
//! Provides `Rf_initEmbeddedR` and `Rf_endEmbeddedR` for embedding
//! R within another application via libR.

use std::os::raw::{c_char, c_int};

use crate::sexp::instance::with_required_current_instance;

// ---------------------------------------------------------------------------
// Stub: Rf_initialize_R, setup_Rmainloop, fpu_setup
// ---------------------------------------------------------------------------

unsafe fn Rf_initialize_R(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
    0
}

unsafe fn setup_Rmainloop() {}

unsafe fn fpu_setup(_start: c_int) {}

// ---------------------------------------------------------------------------
// Stubs for cleanup functions
// ---------------------------------------------------------------------------

unsafe fn R_RunExitFinalizers() {}
unsafe fn CleanEd() {}
unsafe fn KillAllDevices() {}
unsafe fn R_CleanTempDir() {}
unsafe fn PrintWarnings() {}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the embedded R environment.
///
/// This is the main entry point when embedding R within another application
/// by loading libR. The arguments are the command line arguments that would
/// be passed to the regular standalone R.
///
/// Returns 1 on success.
pub unsafe fn Rf_initEmbeddedR(argc: c_int, argv: *mut *mut c_char) -> c_int {
    unsafe {
        Rf_initialize_R(argc, argv);
        with_required_current_instance(|instance| {
            instance.startup_state.interactive = 1;
            instance.eval_state.interactive = 1;
        });
        setup_Rmainloop();
        1
    }
}

/// End the embedded R session.
///
/// Call with fatal != 0 for emergency bail out.
/// Performs cleanup: run exit finalizers, clean editor state,
/// kill graphics devices, clean temp directory, print warnings.
pub unsafe fn Rf_endEmbeddedR(fatal: c_int) {
    unsafe {
        R_RunExitFinalizers();
        CleanEd();
        if fatal == 0 {
            KillAllDevices();
        }
        R_CleanTempDir();
        if fatal == 0 {
            PrintWarnings();
        }
        fpu_setup(0);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_embedded_r_runs() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let argv: &mut [*mut c_char] = &mut [];
            let result = Rf_initEmbeddedR(0, argv.as_mut_ptr());
            assert_eq!(result, 1);
            with_required_current_instance(|instance| {
                assert_eq!(instance.startup_state.interactive, 1);
                assert_eq!(instance.eval_state.interactive, 1);
            });
        }
    }

    #[test]
    fn test_end_embedded_r_runs() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            Rf_endEmbeddedR(0);
        }
    }

    #[test]
    fn test_end_embedded_r_fatal_runs() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            Rf_endEmbeddedR(1);
        }
    }
}
