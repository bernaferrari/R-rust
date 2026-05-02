#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/edit.c — edit() function.
//!
//! Provides the editing entry points used by utils.
//!
//! Android and embedded runtimes do not have a process-wide interactive editor
//! contract. Instead of silently returning `NULL`, unsupported editor calls fail
//! with an R error so callers can recover explicitly.

use std::os::raw::c_int;

use crate::sexp::context::RError;
use crate::sexp::ffi::SEXP;

/// Initialize the edit subsystem.
pub unsafe fn InitEd() {
    // no temp file management needed
}

/// Clean up the edit subsystem.
pub unsafe fn CleanEd() {
    // no temp file to clean
}

fn edit_unavailable() -> ! {
    std::panic::panic_any(RError {
        message: "edit() is not available in the Android/headless runtime".to_string(),
    });
}

/// Edit an R object.
///
/// This is the equivalent of R's `do_edit()` from edit.c.
/// GNU R deparses the object, launches an external editor, and parses the
/// resulting file. That interaction is intentionally outside this embedded
/// runtime; Android callers should provide an editor UI above UniFFI and then
/// submit source text through the parser/evaluator.
pub unsafe fn do_edit(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    edit_unavailable()
}

/// Private edit-files hook for the legacy utils boundary.
pub(crate) unsafe fn R_EditFiles(
    _nfiles: c_int,
    _files: *mut *mut std::os::raw::c_char,
    _editor: *mut std::os::raw::c_char,
) -> c_int {
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_do_edit_reports_headless_policy() {
        let result = std::panic::catch_unwind(|| unsafe {
            do_edit(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        });

        let Err(payload) = result else {
            panic!("expected RError");
        };
        let Some(err) = payload.downcast_ref::<RError>() else {
            panic!("expected RError payload");
        };
        assert!(err.message.contains("Android/headless runtime"));
    }

    #[test]
    fn test_init_ed() {
        unsafe {
            InitEd();
        }
    }
}
